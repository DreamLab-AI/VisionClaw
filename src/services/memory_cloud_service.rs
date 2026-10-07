//! Live memory cloud service: snapshot cache, sidecar queries and the recall
//! probe behind `/api/memory-cloud*`.
//!
//! The pure parts (allocation, PCA, codecs, snippets, validation, recall
//! maths) live in the `visionclaw-memory-cloud` crate. This module owns the
//! I/O: a read-only Postgres pool to the RuVector sidecar, the
//! OpenAI-compatible embedder, and the background task that rebuilds the
//! stratified snapshot every `MEMORY_CLOUD_REFRESH_SECS` and then measures the
//! sidecar's HNSW recall against an exact scan.
//!
//! The connection string comes from `RUVECTOR_PG_CONNINFO` only. Every
//! session is opened with `default_transaction_read_only=on`, so a bug here
//! cannot write to the shared memory store.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use log::{info, warn};
use serde::Deserialize;
use tokio::sync::{watch, RwLock};
use tokio_postgres::NoTls;

use visionclaw_memory_cloud::config::{MemoryCloudConfig, NamespacePatterns};
use visionclaw_memory_cloud::recall::{mean, recall_at_k, recall_at_k_by_distance};
use visionclaw_memory_cloud::sampling::{allocate, NamespaceCount, SamplingPolicy};
use visionclaw_memory_cloud::snapshot::{build_snapshot, probe_rows, BuiltSnapshot, SampledRow};
use visionclaw_memory_cloud::snippet::{snippet, SNIPPET_MAX_CHARS};
use visionclaw_memory_cloud::validate::ValidatedQuery;
use visionclaw_memory_cloud::vector::{format_ruvector_literal, l2_normalise};
use visionclaw_memory_cloud::wire::{
    EmbedderHealth, MemoryCloudHealth, MemoryCloudHit, MemoryCloudMeta, MemoryCloudQueryResponse,
    MemoryCloudRecallProbe, NamespaceHealth, QueryEcho, SearchMethod, SidecarHealth, SidecarIssue,
    SidecarResults,
};

/// Environment variable holding the sidecar's libpq connection string.
pub const CONNINFO_ENV: &str = "RUVECTOR_PG_CONNINFO";
/// Name of the sidecar's HNSW index on `memory_entries.embedding`.
pub const HNSW_INDEX: &str = "idx_memory_embedding_hnsw";

/// How long a request waits for the very first snapshot.
const FIRST_SNAPSHOT_WAIT: Duration = Duration::from_secs(45);
/// Back-off after a failed build.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(30);
const EMBED_TIMEOUT: Duration = Duration::from_secs(15);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const POOL_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_QUERIES: usize = 20;
const PROBE_K: usize = 10;

/// Session options applied to every pooled connection.
const SESSION_OPTIONS: &str = "-c default_transaction_read_only=on -c statement_timeout=30000";

const COUNTS_SQL: &str = "SELECT namespace, count(*)::bigint, count(embedding)::bigint \
     FROM memory_entries GROUP BY namespace";

/// Random rows per namespace. Each lateral branch walks the namespace's
/// btree index and sorts only ids; embeddings are detoasted for the chosen
/// rows alone.
const SAMPLE_SQL: &str = "SELECT e.id, e.key, e.namespace, coalesce(e.source_type, ''), \
            coalesce((extract(epoch FROM e.updated_at) * 1000)::bigint, 0), \
            e.embedding::text \
     FROM unnest($1::text[], $2::int4[]) AS q(ns, n) \
     CROSS JOIN LATERAL ( \
         SELECT m.id FROM memory_entries m \
         WHERE m.namespace = q.ns AND m.embedding IS NOT NULL \
         ORDER BY random() LIMIT q.n \
     ) s \
     JOIN memory_entries e ON e.id = s.id \
     ORDER BY e.namespace, e.id";

/// Global top-k by cosine distance (`<=>`, served by the `ruvector_cosine_ops`
/// HNSW index). `$2`/`$3` carry the exclusion policy.
const SEARCH_SQL: &str = "SELECT id, key, namespace, coalesce(source_type, ''), \
            (1 - (embedding <=> ($1::text)::ruvector))::float8, value \
     FROM memory_entries \
     WHERE embedding IS NOT NULL \
       AND NOT (namespace = ANY($2::text[])) \
       AND NOT (namespace LIKE ANY($3::text[])) \
     ORDER BY embedding <=> ($1::text)::ruvector \
     LIMIT $4";

/// Namespace-restricted top-k, **exact**. The HNSW index filters after it
/// has gathered its candidates, so a namespace filter on the index path
/// returned 1 of 5 requested hits for `project-state` (measured 2026-10-07).
/// The `+ 0` makes the sort key unmatchable by the index, so the planner
/// walks `idx_memory_namespace` and sorts (18 ms for `project-state`,
/// ~290 ms for the 162k-row `ruvnet-kb`).
const SEARCH_NAMESPACE_SQL: &str = "SELECT id, key, namespace, coalesce(source_type, ''), \
            (1 - (embedding <=> ($1::text)::ruvector))::float8, value \
     FROM memory_entries \
     WHERE embedding IS NOT NULL AND namespace = $2::text \
     ORDER BY (embedding <=> ($1::text)::ruvector) + 0 \
     LIMIT $3";

/// Ids-only top-k used by the recall probe (identical predicate and order).
const PROBE_SQL: &str =
    "SELECT id, (embedding <=> ($1::text)::ruvector)::float8 FROM memory_entries \
     WHERE embedding IS NOT NULL \
       AND NOT (namespace = ANY($2::text[])) \
       AND NOT (namespace LIKE ANY($3::text[])) \
     ORDER BY embedding <=> ($1::text)::ruvector \
     LIMIT $4";

/// Distance tolerance for tie-aware recall (f32 storage, f64 maths).
const TIE_EPS: f64 = 1e-6;

/// Planner switches that force the exact (sequential-scan) path.
const EXACT_SCAN_SETTINGS: &str =
    "SET LOCAL enable_indexscan = off; SET LOCAL enable_bitmapscan = off;";

/// Why a memory-cloud operation could not complete.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MemoryCloudError {
    /// `RUVECTOR_PG_CONNINFO` is unset or unparsable.
    #[error("{0}")]
    Unconfigured(String),
    /// The sidecar could not be reached or a query failed.
    #[error("RuVector sidecar unavailable: {0}")]
    Sidecar(String),
    /// The embedder failed or returned something unusable.
    #[error("embedder unavailable: {0}")]
    Embedder(String),
    /// No snapshot yet and the first build has not finished in time.
    #[error("the memory cloud snapshot is still being built; retry shortly")]
    Building,
}

impl MemoryCloudError {
    /// The fixed message a client may see. The `Display` form carries driver
    /// and connection detail for the server log only.
    pub fn public_message(&self) -> &'static str {
        match self {
            Self::Unconfigured(_) => "memory store not configured",
            Self::Sidecar(_) => "memory store unavailable",
            Self::Embedder(_) => "embedding service unavailable",
            Self::Building => "the memory cloud snapshot is still being built; retry shortly",
        }
    }
}

/// Timings and counts from one snapshot build.
#[derive(Debug, Clone, Default)]
pub struct BuildTimings {
    /// `GROUP BY namespace` count query.
    pub counts_ms: f64,
    /// Stratified random sample query (rows + embeddings).
    pub sample_ms: f64,
    /// Parse, normalise, PCA, blob and JSON encoding.
    pub project_ms: f64,
    /// Rows returned by the sample query.
    pub rows: usize,
    /// Rows dropped while building.
    pub skipped: usize,
}

/// A snapshot with its pre-encoded payloads.
#[derive(Debug)]
pub struct ServedSnapshot {
    /// Snapshot, normalised vectors and id map.
    pub built: BuiltSnapshot,
    /// `MemoryCloudSnapshot` serialised once.
    pub json: Bytes,
    /// Little-endian f32 vectors blob.
    pub blob: Bytes,
    /// How the build went.
    pub timings: BuildTimings,
    /// `(namespace, total, embedded)` for every non-excluded namespace, as
    /// counted by this build; health serves these instead of re-counting.
    pub namespace_counts: Vec<(String, u64, u64)>,
    /// `pg_extension.extversion` for `ruvector`, read by this build.
    pub extension_version: Option<String>,
}

impl ServedSnapshot {
    /// The snapshot's opaque id.
    pub fn id(&self) -> &str {
        &self.built.snapshot.snapshot_id
    }
}

/// One recall-probe query.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeQuery {
    /// Namespace of the sampled row used as the query.
    pub namespace: String,
    /// Rows the index path returned (fewer than k means filtering starved it).
    pub index_rows: usize,
    /// Best cosine distance on the index path (`None` when it returned nothing).
    pub index_best: Option<f64>,
    /// Best cosine distance on the exact path; ~0 because the query is a stored row.
    pub exact_best: Option<f64>,
    /// Tie-aware recall@k (distance-based); this feeds the wire `recall`.
    pub recall: f64,
    /// Id-overlap recall@k; understates recall when embeddings are duplicated.
    pub id_recall: f64,
    /// Index query wall time.
    pub index_ms: f64,
    /// Exact query wall time.
    pub exact_ms: f64,
}

/// Outcome of one recall probe, with per-query detail and plan checks.
#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    /// The wire summary.
    pub summary: MemoryCloudRecallProbe,
    /// Per probe query.
    pub per_query: Vec<ProbeQuery>,
    /// `EXPLAIN` of the index query names the HNSW index.
    pub index_plan_uses_hnsw: bool,
    /// `EXPLAIN` of the exact query is a sequential scan without the index.
    pub exact_plan_is_seq_scan: bool,
}

#[derive(Debug, Clone, Default)]
struct BuildState {
    snapshot: Option<Arc<ServedSnapshot>>,
    last_error: Option<String>,
    attempts: u64,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
}

/// Process-wide memory cloud state; one instance shared by every worker.
pub struct MemoryCloudService {
    config: MemoryCloudConfig,
    pool: Result<Pool, String>,
    http: reqwest::Client,
    state_tx: watch::Sender<BuildState>,
    probe: RwLock<Option<ProbeOutcome>>,
    refresher_started: AtomicBool,
    /// Embedder reachability from the latest refresh cycle or query.
    embedder_ok: AtomicBool,
}

impl MemoryCloudService {
    /// Build from the process environment.
    pub fn from_env() -> Arc<Self> {
        Self::new(
            MemoryCloudConfig::from_lookup(|k| std::env::var(k).ok()),
            std::env::var(CONNINFO_ENV).ok(),
        )
    }

    /// Build from explicit configuration. The pool connects lazily, so this
    /// never blocks and never fails; a missing or unparsable connection
    /// string turns every data endpoint into a 503.
    pub fn new(config: MemoryCloudConfig, conninfo: Option<String>) -> Arc<Self> {
        let pool = match conninfo.filter(|c| !c.trim().is_empty()) {
            None => Err(format!(
                "{CONNINFO_ENV} is not set; the live memory cloud is disabled"
            )),
            Some(conninfo) => build_pool(&conninfo),
        };
        if let Err(e) = &pool {
            warn!("[MemoryCloud] {e}");
        }
        let http = reqwest::Client::builder()
            .timeout(EMBED_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let (state_tx, _) = watch::channel(BuildState::default());
        Arc::new(Self {
            config,
            pool,
            http,
            state_tx,
            probe: RwLock::new(None),
            refresher_started: AtomicBool::new(false),
            embedder_ok: AtomicBool::new(false),
        })
    }

    /// The resolved configuration.
    pub fn config(&self) -> &MemoryCloudConfig {
        &self.config
    }

    fn pool(&self) -> Result<&Pool, MemoryCloudError> {
        self.pool
            .as_ref()
            .map_err(|e| MemoryCloudError::Unconfigured(e.clone()))
    }

    async fn client(&self) -> Result<deadpool_postgres::Object, MemoryCloudError> {
        self.pool()?
            .get()
            .await
            .map_err(|e| MemoryCloudError::Sidecar(e.to_string()))
    }

    /// The current snapshot, starting the background refresher on first use
    /// and waiting up to 45 s for the first build.
    pub async fn snapshot(self: &Arc<Self>) -> Result<Arc<ServedSnapshot>, MemoryCloudError> {
        self.pool()?;
        self.ensure_refresher();
        let mut rx = self.state_tx.subscribe();
        let wait = async {
            loop {
                {
                    let state = rx.borrow_and_update();
                    if let Some(snap) = &state.snapshot {
                        return Ok(Arc::clone(snap));
                    }
                    if let Some(err) = &state.last_error {
                        return Err(MemoryCloudError::Sidecar(err.clone()));
                    }
                }
                if rx.changed().await.is_err() {
                    return Err(MemoryCloudError::Building);
                }
            }
        };
        tokio::time::timeout(FIRST_SNAPSHOT_WAIT, wait)
            .await
            .unwrap_or(Err(MemoryCloudError::Building))
    }

    /// The current snapshot if one has been built, without waiting.
    pub fn current_snapshot(&self) -> Option<Arc<ServedSnapshot>> {
        self.state_tx.borrow().snapshot.clone()
    }

    /// The latest recall probe, if any.
    pub async fn latest_probe(&self) -> Option<ProbeOutcome> {
        self.probe.read().await.clone()
    }

    fn ensure_refresher(self: &Arc<Self>) {
        if self.refresher_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            // A snapshot published by an explicit `rebuild()` keeps its full
            // refresh interval before the first background rebuild.
            if let Some(snap) = this.current_snapshot() {
                let age_ms =
                    chrono::Utc::now().timestamp_millis() - snap.built.snapshot.generated_at;
                let interval_ms = (this.config.refresh_secs * 1000) as i64;
                if age_ms < interval_ms {
                    tokio::time::sleep(Duration::from_millis((interval_ms - age_ms) as u64)).await;
                }
            }
            loop {
                let reachable = this.embedder_reachable().await;
                this.embedder_ok.store(reachable, Ordering::Relaxed);
                let pause = match this.rebuild().await {
                    Ok(snap) => {
                        match this.run_recall_probe(&snap).await {
                            Ok(outcome) => {
                                info!(
                                    "[MemoryCloud] recall probe: recall@{}={:.3} over {} queries, index {:.1} ms, exact {:.1} ms, hnsw plan={}, exact seq-scan={}",
                                    outcome.summary.k,
                                    outcome.summary.recall,
                                    outcome.summary.probes,
                                    outcome.summary.index_ms,
                                    outcome.summary.exact_ms,
                                    outcome.index_plan_uses_hnsw,
                                    outcome.exact_plan_is_seq_scan
                                );
                                *this.probe.write().await = Some(outcome);
                            }
                            Err(e) => warn!("[MemoryCloud] recall probe failed: {e}"),
                        }
                        Duration::from_secs(this.config.refresh_secs)
                    }
                    Err(e) => {
                        warn!("[MemoryCloud] snapshot build failed: {e}");
                        RETRY_AFTER_FAILURE
                    }
                };
                tokio::time::sleep(pause).await;
            }
        });
    }

    /// Build a fresh snapshot and publish it. A failure leaves any previous
    /// snapshot in service and records the error for waiting requests.
    pub async fn rebuild(&self) -> Result<Arc<ServedSnapshot>, MemoryCloudError> {
        let result = self.build().await;
        self.state_tx.send_modify(|state| {
            state.attempts += 1;
            match &result {
                Ok(snap) => {
                    state.snapshot = Some(Arc::clone(snap));
                    state.last_error = None;
                }
                Err(e) => state.last_error = Some(e.to_string()),
            }
        });
        result
    }

    async fn build(&self) -> Result<Arc<ServedSnapshot>, MemoryCloudError> {
        let client = self.client().await?;
        let sidecar = |e: tokio_postgres::Error| MemoryCloudError::Sidecar(e.to_string());

        let extension_version: Option<String> = client
            .query_opt(
                "SELECT extversion FROM pg_extension WHERE extname = 'ruvector'",
                &[],
            )
            .await
            .map_err(sidecar)?
            .map(|r| r.get(0));

        let t = Instant::now();
        let raw_counts: Vec<(String, u64, u64)> = client
            .query(COUNTS_SQL, &[])
            .await
            .map_err(sidecar)?
            .iter()
            .map(|r| {
                (
                    r.get::<_, String>(0),
                    u64::try_from(r.get::<_, i64>(1)).unwrap_or(0),
                    u64::try_from(r.get::<_, i64>(2)).unwrap_or(0),
                )
            })
            .collect();
        let counts_ms = ms(t);
        let counts: Vec<NamespaceCount> = raw_counts
            .iter()
            .map(|(namespace, _, embedded)| NamespaceCount {
                namespace: namespace.clone(),
                embedded: *embedded,
            })
            .collect();
        let namespace_counts = visible_counts(raw_counts, &self.config.excluded);

        let allocations = allocate(
            &SamplingPolicy::with_total(self.config.sample_total),
            &counts,
            &self.config.excluded,
        );
        let namespaces: Vec<String> = allocations.iter().map(|a| a.namespace.clone()).collect();
        let quotas: Vec<i32> = allocations
            .iter()
            .map(|a| i32::try_from(a.quota).unwrap_or(i32::MAX))
            .collect();

        let t = Instant::now();
        let rows: Vec<SampledRow> = client
            .query(SAMPLE_SQL, &[&namespaces, &quotas])
            .await
            .map_err(sidecar)?
            .iter()
            .map(|r| SampledRow {
                meta: MemoryCloudMeta {
                    id: r.get(0),
                    key: r.get(1),
                    namespace: r.get(2),
                    source_type: r.get(3),
                    updated_at: r.get(4),
                },
                embedding: r.get(5),
            })
            .collect();
        let sample_ms = ms(t);
        drop(client);

        let row_count = rows.len();
        let excluded = self.config.excluded.clone();
        let generated_at = chrono::Utc::now().timestamp_millis();
        let t = Instant::now();
        let (built, json) = tokio::task::spawn_blocking(move || {
            let mut built = build_snapshot(rows, &allocations, &excluded, generated_at)
                .map_err(|e| MemoryCloudError::Sidecar(format!("projection failed: {e}")))?;
            let json = serde_json::to_vec(&built.snapshot)
                .map_err(|e| MemoryCloudError::Sidecar(format!("encoding failed: {e}")))?;
            // The blob moves into `Bytes`; keep `vectors` for probe queries.
            let blob = std::mem::take(&mut built.blob);
            Ok::<_, MemoryCloudError>((built, (Bytes::from(json), Bytes::from(blob))))
        })
        .await
        .map_err(|e| MemoryCloudError::Sidecar(format!("build task failed: {e}")))??;
        let project_ms = ms(t);

        let timings = BuildTimings {
            counts_ms,
            sample_ms,
            project_ms,
            rows: row_count,
            skipped: built.skipped,
        };
        info!(
            "[MemoryCloud] snapshot {} built: {} rows over {} namespaces (skipped {}); counts {:.0} ms, sample {:.0} ms, projection {:.0} ms",
            built.snapshot.snapshot_id,
            built.snapshot.count,
            built.snapshot.namespaces.len(),
            built.skipped,
            counts_ms,
            sample_ms,
            project_ms
        );
        Ok(Arc::new(ServedSnapshot {
            built,
            json: json.0,
            blob: json.1,
            timings,
            namespace_counts,
            extension_version,
        }))
    }

    /// Embed `text` with the configured model and L2-normalise the result.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, MemoryCloudError> {
        let url = format!("{}/embeddings", self.config.embed_url);
        let body = serde_json::json!({ "model": self.config.embed_model, "input": [text] });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| MemoryCloudError::Embedder(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(MemoryCloudError::Embedder(format!(
                "{url} answered {status}"
            )));
        }
        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| MemoryCloudError::Embedder(format!("unexpected response: {e}")))?;
        let mut vector = parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| MemoryCloudError::Embedder("response held no embedding".into()))?;
        l2_normalise(&mut vector)
            .map_err(|e| MemoryCloudError::Embedder(format!("unusable embedding: {e}")))?;
        Ok(vector)
    }

    /// Embed the query and run the sidecar's own HNSW top-k.
    pub async fn query(
        self: &Arc<Self>,
        q: ValidatedQuery,
    ) -> Result<MemoryCloudQueryResponse, MemoryCloudError> {
        let snap = self.snapshot().await?;
        let embedded = self.embed(&q.text).await;
        self.embedder_ok.store(embedded.is_ok(), Ordering::Relaxed);
        let vector = embedded?;
        let dim = snap.built.snapshot.dim;
        if dim != 0 && vector.len() != dim {
            return Err(MemoryCloudError::Embedder(format!(
                "model {} returned {} dimensions; the sidecar stores {dim}",
                self.config.embed_model,
                vector.len()
            )));
        }
        let literal = format_ruvector_literal(&vector);
        let exact = self.config.excluded.sql_exact();
        let likes = self.config.excluded.sql_like_prefixes();
        let limit = q.k as i64;

        let mut client = self.client().await?;
        let sidecar = |e: tokio_postgres::Error| MemoryCloudError::Sidecar(e.to_string());
        let t = Instant::now();
        let (rows, method) = match &q.namespace {
            // The index post-filters, so a namespace scan is always exact.
            Some(ns) => (
                client
                    .query(SEARCH_NAMESPACE_SQL, &[&literal, ns, &limit])
                    .await
                    .map_err(sidecar)?,
                SearchMethod::Exact,
            ),
            None => {
                let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
                    [&literal, &exact, &likes, &limit];
                let rows = client.query(SEARCH_SQL, &params).await.map_err(sidecar)?;
                if rows.len() >= q.k {
                    (rows, SearchMethod::Hnsw)
                } else {
                    // The index's candidates were mostly in excluded
                    // namespaces and the post-filter left fewer than k: the
                    // same query as an exact scan fills k.
                    let tx = client.transaction().await.map_err(sidecar)?;
                    tx.batch_execute(EXACT_SCAN_SETTINGS)
                        .await
                        .map_err(sidecar)?;
                    let rows = tx.query(SEARCH_SQL, &params).await.map_err(sidecar)?;
                    tx.rollback().await.map_err(sidecar)?;
                    info!(
                        "[MemoryCloud] HNSW returned fewer than k={} rows after exclusions; answered by exact scan",
                        q.k
                    );
                    (rows, SearchMethod::Exact)
                }
            }
        };
        let took_ms = ms(t);

        let results = rows
            .iter()
            .map(|r| {
                let id: String = r.get(0);
                let value: Option<serde_json::Value> = r.get(5);
                MemoryCloudHit {
                    sample_index: snap.built.index_of.get(&id).copied(),
                    id,
                    key: r.get(1),
                    namespace: r.get(2),
                    source_type: r.get(3),
                    score: r.get::<_, f64>(4) as f32,
                    snippet: value
                        .as_ref()
                        .map(|v| snippet(v, SNIPPET_MAX_CHARS))
                        .unwrap_or_default(),
                }
            })
            .collect();

        Ok(MemoryCloudQueryResponse {
            snapshot_id: snap.id().to_string(),
            embed_model: self.config.embed_model.clone(),
            query: QueryEcho {
                text: q.text,
                vector,
            },
            sidecar: SidecarResults {
                results,
                took_ms,
                method,
            },
        })
    }

    /// Compare the sidecar's HNSW top-k with an exact sequential scan for up
    /// to 20 sampled rows used as queries.
    pub async fn run_recall_probe(
        &self,
        snap: &ServedSnapshot,
    ) -> Result<ProbeOutcome, MemoryCloudError> {
        let dim = snap.built.snapshot.dim;
        let picks = probe_rows(snap.built.snapshot.count, PROBE_QUERIES);
        let exact_ns = self.config.excluded.sql_exact();
        let likes = self.config.excluded.sql_like_prefixes();
        let k = PROBE_K as i64;
        let sidecar = |e: tokio_postgres::Error| MemoryCloudError::Sidecar(e.to_string());

        let mut client = self.client().await?;
        let mut per_query = Vec::with_capacity(picks.len());
        let mut index_plan_uses_hnsw = true;
        let mut exact_plan_is_seq_scan = true;

        for (n, &row) in picks.iter().enumerate() {
            let literal = format_ruvector_literal(&snap.built.vectors[row * dim..(row + 1) * dim]);
            let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
                [&literal, &exact_ns, &likes, &k];

            if n == 0 {
                let plan = explain(&client, &params).await.map_err(sidecar)?;
                index_plan_uses_hnsw = plan.contains(HNSW_INDEX);
            }
            let t = Instant::now();
            let approx = id_distance_rows(client.query(PROBE_SQL, &params).await.map_err(sidecar)?);
            let index_ms = ms(t);

            let tx = client.transaction().await.map_err(sidecar)?;
            tx.batch_execute(EXACT_SCAN_SETTINGS)
                .await
                .map_err(sidecar)?;
            if n == 0 {
                let plan = explain(&tx, &params).await.map_err(sidecar)?;
                exact_plan_is_seq_scan = plan.contains("Seq Scan") && !plan.contains(HNSW_INDEX);
            }
            let t = Instant::now();
            let exact = id_distance_rows(tx.query(PROBE_SQL, &params).await.map_err(sidecar)?);
            let exact_ms = ms(t);
            tx.rollback().await.map_err(sidecar)?;

            let ids = |v: &[(String, f64)]| v.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
            let dists = |v: &[(String, f64)]| v.iter().map(|(_, d)| *d).collect::<Vec<_>>();
            per_query.push(ProbeQuery {
                namespace: snap.built.snapshot.metadata[row].namespace.clone(),
                index_rows: approx.len(),
                index_best: approx.first().map(|(_, d)| *d),
                exact_best: exact.first().map(|(_, d)| *d),
                recall: recall_at_k_by_distance(&dists(&approx), &dists(&exact), PROBE_K, TIE_EPS),
                id_recall: recall_at_k(&ids(&approx), &ids(&exact), PROBE_K),
                index_ms,
                exact_ms,
            });
        }

        if !index_plan_uses_hnsw {
            warn!("[MemoryCloud] recall probe: the index query did not use {HNSW_INDEX}");
        }
        if !exact_plan_is_seq_scan {
            warn!("[MemoryCloud] recall probe: the exact query was not a sequential scan");
        }
        let col = |f: fn(&ProbeQuery) -> f64| {
            mean(&per_query.iter().map(f).collect::<Vec<_>>()).unwrap_or(0.0)
        };
        Ok(ProbeOutcome {
            summary: MemoryCloudRecallProbe {
                k: PROBE_K,
                probes: per_query.len(),
                recall: col(|q| q.recall),
                index_ms: col(|q| q.index_ms),
                exact_ms: col(|q| q.exact_ms),
                measured_at: chrono::Utc::now().timestamp_millis(),
            },
            per_query,
            index_plan_uses_hnsw,
            exact_plan_is_seq_scan,
        })
    }

    /// Health from cached state only: the latest snapshot build's counts,
    /// extension version and outcome, the embedder reachability recorded on
    /// the refresh cycle, and the cached recall probe. Never touches the
    /// network, so it cannot be used to load the sidecar or the embedder.
    /// Starts the background refresher when the store is configured.
    pub async fn health(self: &Arc<Self>) -> MemoryCloudHealth {
        let configured = self.pool.is_ok();
        if configured {
            self.ensure_refresher();
        }
        let (snap, failed) = {
            let state = self.state_tx.borrow();
            (state.snapshot.clone(), state.last_error.is_some())
        };
        let issue = if !configured {
            Some(SidecarIssue::NotConfigured)
        } else if failed {
            Some(SidecarIssue::Unreachable)
        } else if snap.is_none() {
            Some(SidecarIssue::Building)
        } else {
            None
        };
        let namespaces = snap
            .as_ref()
            .map(|s| {
                let sampled_of = |ns: &str| {
                    s.built
                        .snapshot
                        .strata
                        .iter()
                        .find(|st| st.namespace == ns)
                        .map(|st| st.sampled)
                        .unwrap_or(0)
                };
                s.namespace_counts
                    .iter()
                    .map(|(namespace, total, embedded)| NamespaceHealth {
                        sampled: sampled_of(namespace),
                        namespace: namespace.clone(),
                        total: *total,
                        embedded: *embedded,
                    })
                    .collect()
            })
            .unwrap_or_default();
        MemoryCloudHealth {
            snapshot_id: snap.as_ref().map(|s| s.id().to_string()),
            generated_at: snap.as_ref().map(|s| s.built.snapshot.generated_at),
            sidecar: SidecarHealth {
                reachable: issue.is_none(),
                extension_version: snap.as_ref().and_then(|s| s.extension_version.clone()),
                error: issue,
            },
            embedder: EmbedderHealth {
                model: self.config.embed_model.clone(),
                reachable: self.embedder_ok.load(Ordering::Relaxed),
            },
            namespaces,
            recall_probe: self.latest_probe().await.map(|p| p.summary),
        }
    }

    async fn embedder_reachable(&self) -> bool {
        let url = format!("{}/models", self.config.embed_url);
        matches!(
            tokio::time::timeout(HEALTH_TIMEOUT, self.http.get(&url).send()).await,
            Ok(Ok(resp)) if resp.status().is_success()
        )
    }
}

/// Non-excluded namespace counts, largest first.
fn visible_counts(
    mut counts: Vec<(String, u64, u64)>,
    excluded: &NamespacePatterns,
) -> Vec<(String, u64, u64)> {
    counts.retain(|(ns, _, _)| !excluded.matches(ns));
    counts.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    counts
}

fn build_pool(conninfo: &str) -> Result<Pool, String> {
    let mut pg = tokio_postgres::Config::from_str(conninfo)
        .map_err(|e| format!("{CONNINFO_ENV} is not a valid connection string: {e}"))?;
    // A key=value string parses even when cut short, e.g. an unquoted .env
    // value split at its first space leaves only `host=…`; fail here, by name,
    // rather than later as an opaque "unreachable".
    let missing: Vec<&str> = [
        ("user", pg.get_user().is_none()),
        ("dbname", pg.get_dbname().is_none()),
    ]
    .into_iter()
    .filter_map(|(k, absent)| absent.then_some(k))
    .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{CONNINFO_ENV} has no {}; if it is a key=value string in .env, quote the whole value",
            missing.join(" or ")
        ));
    }
    pg.connect_timeout(POOL_TIMEOUT)
        .application_name("visionclaw-memory-cloud")
        .options(SESSION_OPTIONS);
    let manager = Manager::from_config(
        pg,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    Pool::builder(manager)
        .max_size(4)
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(POOL_TIMEOUT))
        .create_timeout(Some(POOL_TIMEOUT))
        .recycle_timeout(Some(POOL_TIMEOUT))
        .build()
        .map_err(|e| format!("could not create the sidecar pool: {e}"))
}

async fn explain<C: deadpool_postgres::GenericClient>(
    client: &C,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<String, tokio_postgres::Error> {
    let rows = client
        .query(format!("EXPLAIN {PROBE_SQL}").as_str(), params)
        .await?;
    Ok(rows
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn id_distance_rows(rows: Vec<tokio_postgres::Row>) -> Vec<(String, f64)> {
    rows.iter().map(|r| (r.get(0), r.get(1))).collect()
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MemoryCloudConfig {
        MemoryCloudConfig::from_lookup(|_| None)
    }

    #[tokio::test]
    async fn conninfo_truncated_at_first_space_is_rejected_with_a_quoting_hint() {
        // An unquoted .env value split at its spaces arrives as host only.
        let svc = MemoryCloudService::new(cfg(), Some("host=ruvector-postgres".into()));
        match svc.snapshot().await {
            Err(MemoryCloudError::Unconfigured(msg)) => {
                assert!(msg.contains("user"), "{msg}");
                assert!(msg.contains("quote"), "{msg}");
            }
            other => panic!("expected Unconfigured, got {other:?}"),
        }
    }

    #[test]
    fn conninfo_with_user_and_dbname_builds_a_pool() {
        assert!(build_pool(
            "host=ruvector-postgres port=5432 dbname=ruvector user=ruvector_reader password=x"
        )
        .is_ok());
        assert!(
            build_pool("postgresql://ruvector_reader:x@ruvector-postgres:5432/ruvector").is_ok()
        );
    }

    #[actix_rt::test]
    async fn unset_conninfo_is_unconfigured_not_a_panic() {
        let svc = MemoryCloudService::new(cfg(), None);
        match svc.snapshot().await {
            Err(MemoryCloudError::Unconfigured(msg)) => assert!(msg.contains(CONNINFO_ENV)),
            other => panic!("expected Unconfigured, got {other:?}"),
        }
        let health = svc.health().await;
        assert!(!health.sidecar.reachable);
        assert_eq!(health.sidecar.error, Some(SidecarIssue::NotConfigured));
        assert!(health.snapshot_id.is_none());
    }

    #[actix_rt::test]
    async fn health_is_served_from_cache_without_network_calls() {
        // Port 1 refuses connections; a per-request probe of either the
        // sidecar or the embedder would show up as failed network work.
        let config = MemoryCloudConfig::from_lookup(|k| {
            (k == "MEMORY_CLOUD_EMBED_URL").then(|| "http://127.0.0.1:1/v1".to_string())
        });
        let svc = MemoryCloudService::new(
            config,
            Some("host=127.0.0.1 port=1 user=reader dbname=ruvector".into()),
        );

        let t = Instant::now();
        let health = svc.health().await;
        assert!(
            t.elapsed() < Duration::from_millis(50),
            "health hit the network"
        );
        assert!(!health.sidecar.reachable);
        assert!(matches!(
            health.sidecar.error,
            Some(SidecarIssue::Building | SidecarIssue::Unreachable)
        ));
        assert!(!health.embedder.reachable);
        assert!(health.namespaces.is_empty());

        // A failed build is reported by category only; the driver detail
        // stays in the error value for the log.
        let err = svc.rebuild().await.expect_err("port 1 refuses");
        assert!(matches!(err, MemoryCloudError::Sidecar(_)));
        let health = svc.health().await;
        assert_eq!(health.sidecar.error, Some(SidecarIssue::Unreachable));
        assert_eq!(health.sidecar.extension_version, None);
    }

    #[test]
    fn public_messages_never_carry_detail() {
        let detail = "password authentication failed for user \"ruvector\" at 10.0.0.5";
        for err in [
            MemoryCloudError::Unconfigured(detail.into()),
            MemoryCloudError::Sidecar(detail.into()),
            MemoryCloudError::Embedder(detail.into()),
            MemoryCloudError::Building,
        ] {
            let public = err.public_message();
            assert!(!public.contains("ruvector") && !public.contains("10.0.0.5"));
            assert!(
                err.to_string().contains("10.0.0.5") || matches!(err, MemoryCloudError::Building)
            );
        }
    }

    #[test]
    fn malformed_conninfo_is_reported() {
        let err = build_pool("host=x port=notaport").unwrap_err();
        assert!(err.contains(CONNINFO_ENV));
    }

    #[test]
    fn visible_counts_hides_excluded_and_sorts() {
        let excluded = NamespacePatterns::parse_list("personal-context,secret/*");
        let got = visible_counts(
            vec![
                ("a".into(), 5, 5),
                ("personal-context".into(), 32, 32),
                ("secret/x".into(), 9, 9),
                ("b".into(), 50, 40),
            ],
            &excluded,
        );
        assert_eq!(got, vec![("b".into(), 50, 40), ("a".into(), 5, 5)]);
    }
}
