//! Serde types for the `/api/memory-cloud*` wire contract.
//!
//! These mirror `client/src/features/visualisation/memoryCloud/types.ts`
//! field for field (camelCase on the wire). The TypeScript file is the
//! hand-maintained source for the client; `cargo run --bin generate_types`
//! only emits the settings interfaces and cannot derive these. The
//! `wire_matches_typescript_contract` test pins every field name.

use serde::{Deserialize, Serialize};

/// Wire format version of [`MemoryCloudSnapshot`].
pub const SNAPSHOT_VERSION: u8 = 1;

/// One sampled memory entry. Row order matches `positions` and the blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudMeta {
    /// `memory_entries.id`.
    pub id: String,
    /// `memory_entries.key`.
    pub key: String,
    /// `memory_entries.namespace`.
    pub namespace: String,
    /// `memory_entries.source_type` (empty when null).
    pub source_type: String,
    /// `updated_at` as Unix milliseconds.
    pub updated_at: i64,
}

/// Per-namespace sampling outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudStratum {
    /// Namespace name.
    pub namespace: String,
    /// Rows with an embedding in the sidecar.
    pub total: u64,
    /// Rows included in this snapshot.
    pub sampled: u64,
}

/// `GET /api/memory-cloud`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudSnapshot {
    /// Always [`SNAPSHOT_VERSION`].
    pub version: u8,
    /// Opaque id; changes whenever the sample is rebuilt.
    pub snapshot_id: String,
    /// Unix milliseconds.
    pub generated_at: i64,
    /// Embedding dimension.
    pub dim: usize,
    /// Number of sampled rows.
    pub count: usize,
    /// `3 * count` PCA coordinates scaled to roughly ±100.
    pub positions: Vec<f32>,
    /// One entry per row.
    pub metadata: Vec<MemoryCloudMeta>,
    /// Distinct namespaces present in the sample, sorted.
    pub namespaces: Vec<String>,
    /// Distinct source types present in the sample, sorted.
    pub source_types: Vec<String>,
    /// Sampling outcome per namespace.
    pub strata: Vec<MemoryCloudStratum>,
    /// Namespace patterns withheld by server policy.
    pub excluded_namespaces: Vec<String>,
    /// Relative URL of this snapshot's vectors blob.
    pub vectors_url: String,
}

/// `POST /api/memory-cloud/query` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudQueryRequest {
    /// Query text.
    pub text: String,
    /// Result count, 1..=50 (default 10). Signed so out-of-range values reach
    /// validation and get a precise error rather than a decode failure.
    #[serde(default)]
    pub k: Option<i64>,
    /// Restrict the sidecar search to one namespace.
    #[serde(default)]
    pub namespace: Option<String>,
}

/// One result from the sidecar's own HNSW search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudHit {
    /// `memory_entries.id`.
    pub id: String,
    /// `memory_entries.key`.
    pub key: String,
    /// `memory_entries.namespace`.
    pub namespace: String,
    /// `memory_entries.source_type` (empty when null).
    pub source_type: String,
    /// Cosine similarity, 1 = identical.
    pub score: f32,
    /// Flattened value, at most [`crate::snippet::SNIPPET_MAX_CHARS`] chars.
    pub snippet: String,
    /// Row index in the current snapshot, or `None` when not sampled.
    pub sample_index: Option<usize>,
}

/// Echo of the embedded query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryEcho {
    /// Validated query text.
    pub text: String,
    /// L2-normalised query embedding.
    pub vector: Vec<f32>,
}

/// Sidecar search results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarResults {
    /// Hits, nearest first.
    pub results: Vec<MemoryCloudHit>,
    /// Wall-clock milliseconds of the sidecar query.
    pub took_ms: f64,
    /// How the sidecar produced `results`.
    pub method: SearchMethod,
}

/// How the sidecar produced a result list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMethod {
    /// The sidecar's HNSW index: what an agent's `memory_search` sees.
    Hnsw,
    /// An exact scan. Used for namespace-restricted queries (the index
    /// post-filters and would return too few rows) and for a global query
    /// whose HNSW candidates were mostly excluded, leaving fewer than k rows.
    Exact,
}

/// `POST /api/memory-cloud/query` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudQueryResponse {
    /// Snapshot the `sampleIndex` values refer to.
    pub snapshot_id: String,
    /// Embedding model used for the query.
    pub embed_model: String,
    /// The embedded query.
    pub query: QueryEcho,
    /// The sidecar's answer.
    pub sidecar: SidecarResults,
}

/// Index-versus-exact recall probe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudRecallProbe {
    /// Neighbours per query.
    pub k: usize,
    /// Number of probe queries.
    pub probes: usize,
    /// Mean recall@k of the HNSW index against an exact scan.
    pub recall: f64,
    /// Mean milliseconds per index query.
    pub index_ms: f64,
    /// Mean milliseconds per exact query.
    pub exact_ms: f64,
    /// Unix milliseconds.
    pub measured_at: i64,
}

/// Why the sidecar is not serving, as a fixed category.
///
/// Deliberately closed: driver and connection detail stays in the server
/// log and never reaches a client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarIssue {
    /// `RUVECTOR_PG_CONNINFO` is unset or unparsable.
    NotConfigured,
    /// No snapshot build has finished yet.
    Building,
    /// The latest snapshot build could not reach or query the sidecar.
    Unreachable,
}

/// Sidecar part of [`MemoryCloudHealth`], as of the latest snapshot build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarHealth {
    /// Whether the latest snapshot build reached the sidecar.
    pub reachable: bool,
    /// `pg_extension.extversion` for `ruvector`, read during that build.
    pub extension_version: Option<String>,
    /// Why the sidecar is not serving; `None` when it is.
    pub error: Option<SidecarIssue>,
}

/// Embedder part of [`MemoryCloudHealth`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedderHealth {
    /// Model name.
    pub model: String,
    /// Whether the embedder answered on the latest refresh cycle.
    pub reachable: bool,
}

/// Per-namespace counts in [`MemoryCloudHealth`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceHealth {
    /// Namespace name.
    pub namespace: String,
    /// All rows.
    pub total: u64,
    /// Rows with an embedding.
    pub embedded: u64,
    /// Rows in the current snapshot.
    pub sampled: u64,
}

/// `GET /api/memory-cloud/health`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryCloudHealth {
    /// Current snapshot, if built.
    pub snapshot_id: Option<String>,
    /// Current snapshot's build time, Unix milliseconds.
    pub generated_at: Option<i64>,
    /// Sidecar status.
    pub sidecar: SidecarHealth,
    /// Embedder status.
    pub embedder: EmbedderHealth,
    /// Counts for every non-excluded namespace.
    pub namespaces: Vec<NamespaceHealth>,
    /// Latest recall probe, `None` until one completes.
    pub recall_probe: Option<MemoryCloudRecallProbe>,
}

/// JSON body of every 4xx/5xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Human-readable reason.
    pub error: String,
}

impl ErrorBody {
    /// Wrap a message.
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn keys(v: &Value) -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    /// The headset's query parser (`xr-client/rust/src/memory_query.rs`)
    /// reads the same fixture in `tests/memory_query_parity.rs`. This half
    /// pins the fixture to these types: it must round-trip field for field,
    /// so a renamed, added or removed field fails here before the headset
    /// silently stops reading it.
    #[test]
    fn query_response_matches_the_headset_fixture() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../xr-client/rust/tests/fixtures/memory_query_response.json");
        let text =
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let fixture: Value = serde_json::from_str(&text).unwrap();
        let resp: MemoryCloudQueryResponse = serde_json::from_str(&text).unwrap();
        // via text: f32 fields serialise in their shortest f32 form
        let back: Value = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(
            back, fixture,
            "fixture drifted from MemoryCloudQueryResponse"
        );
        assert!(resp
            .sidecar
            .results
            .iter()
            .any(|h| h.sample_index.is_none()));
        assert!(resp
            .sidecar
            .results
            .iter()
            .any(|h| h.sample_index.is_some()));
    }

    #[test]
    fn wire_matches_typescript_contract() {
        let snap = MemoryCloudSnapshot {
            version: SNAPSHOT_VERSION,
            snapshot_id: "abc".into(),
            generated_at: 1,
            dim: 384,
            count: 1,
            positions: vec![0.0, 1.0, 2.0],
            metadata: vec![MemoryCloudMeta {
                id: "i".into(),
                key: "k".into(),
                namespace: "n".into(),
                source_type: "s".into(),
                updated_at: 2,
            }],
            namespaces: vec!["n".into()],
            source_types: vec!["s".into()],
            strata: vec![MemoryCloudStratum {
                namespace: "n".into(),
                total: 3,
                sampled: 1,
            }],
            excluded_namespaces: vec!["personal-context".into()],
            vectors_url: "/api/memory-cloud/vectors?snapshot=abc".into(),
        };
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(
            keys(&v),
            [
                "count",
                "dim",
                "excludedNamespaces",
                "generatedAt",
                "metadata",
                "namespaces",
                "positions",
                "snapshotId",
                "sourceTypes",
                "strata",
                "vectorsUrl",
                "version"
            ]
        );
        assert_eq!(
            keys(&v["metadata"][0]),
            ["id", "key", "namespace", "sourceType", "updatedAt"]
        );
        assert_eq!(keys(&v["strata"][0]), ["namespace", "sampled", "total"]);

        let hit = MemoryCloudHit {
            id: "i".into(),
            key: "k".into(),
            namespace: "n".into(),
            source_type: "s".into(),
            score: 0.5,
            snippet: "x".into(),
            sample_index: None,
        };
        let resp = MemoryCloudQueryResponse {
            snapshot_id: "abc".into(),
            embed_model: "m".into(),
            query: QueryEcho {
                text: "q".into(),
                vector: vec![1.0],
            },
            sidecar: SidecarResults {
                results: vec![hit],
                took_ms: 1.5,
                method: SearchMethod::Exact,
            },
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(keys(&v), ["embedModel", "query", "sidecar", "snapshotId"]);
        assert_eq!(keys(&v["query"]), ["text", "vector"]);
        assert_eq!(keys(&v["sidecar"]), ["method", "results", "tookMs"]);
        assert_eq!(v["sidecar"]["method"], "exact");
        assert_eq!(serde_json::to_value(SearchMethod::Hnsw).unwrap(), "hnsw");
        assert_eq!(
            keys(&v["sidecar"]["results"][0]),
            [
                "id",
                "key",
                "namespace",
                "sampleIndex",
                "score",
                "snippet",
                "sourceType"
            ]
        );
        assert_eq!(v["sidecar"]["results"][0]["sampleIndex"], Value::Null);

        let health = MemoryCloudHealth {
            snapshot_id: None,
            generated_at: None,
            sidecar: SidecarHealth {
                reachable: false,
                extension_version: None,
                error: Some(SidecarIssue::Unreachable),
            },
            embedder: EmbedderHealth {
                model: "m".into(),
                reachable: true,
            },
            namespaces: vec![NamespaceHealth {
                namespace: "n".into(),
                total: 2,
                embedded: 1,
                sampled: 0,
            }],
            recall_probe: Some(MemoryCloudRecallProbe {
                k: 10,
                probes: 20,
                recall: 0.9,
                index_ms: 1.0,
                exact_ms: 2.0,
                measured_at: 3,
            }),
        };
        let v = serde_json::to_value(&health).unwrap();
        assert_eq!(
            keys(&v),
            [
                "embedder",
                "generatedAt",
                "namespaces",
                "recallProbe",
                "sidecar",
                "snapshotId"
            ]
        );
        assert_eq!(
            keys(&v["sidecar"]),
            ["error", "extensionVersion", "reachable"]
        );
        assert_eq!(keys(&v["embedder"]), ["model", "reachable"]);
        assert_eq!(v["sidecar"]["error"], "unreachable");
        for (issue, text) in [
            (SidecarIssue::NotConfigured, "not_configured"),
            (SidecarIssue::Building, "building"),
            (SidecarIssue::Unreachable, "unreachable"),
        ] {
            assert_eq!(serde_json::to_value(issue).unwrap(), text);
        }
        assert_eq!(
            keys(&v["namespaces"][0]),
            ["embedded", "namespace", "sampled", "total"]
        );
        assert_eq!(
            keys(&v["recallProbe"]),
            ["exactMs", "indexMs", "k", "measuredAt", "probes", "recall"]
        );
    }

    /// The client's wire fixture (`wire.test.ts` checks it against the
    /// TypeScript types) must survive a round trip through these structs
    /// unchanged: a field the structs lack is dropped and a field they add is
    /// missing from the fixture, so either drift fails the equality.
    #[test]
    fn client_fixture_round_trips() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../client/src/features/visualisation/memoryCloud/__tests__/fixtures/wire.json"
        ))
        .unwrap();
        fn round_trip<T: serde::de::DeserializeOwned + Serialize>(v: &Value) -> Value {
            serde_json::to_value(serde_json::from_value::<T>(v.clone()).unwrap()).unwrap()
        }
        /// Deserialises a fixture value as `T` and serialises it back.
        type RoundTrip = fn(&Value) -> Value;
        let cases: [(&str, RoundTrip); 5] = [
            ("snapshot", round_trip::<MemoryCloudSnapshot>),
            ("queryRequest", round_trip::<MemoryCloudQueryRequest>),
            ("queryResponse", round_trip::<MemoryCloudQueryResponse>),
            ("health", round_trip::<MemoryCloudHealth>),
            ("error", round_trip::<ErrorBody>),
        ];
        for (name, rt) in cases {
            let v = &fixture[name];
            assert!(v.is_object(), "fixture lacks {name}");
            assert_eq!(&rt(v), v, "{name} drifted from wire.rs");
        }
    }

    #[test]
    fn query_request_fields_are_optional() {
        let r: MemoryCloudQueryRequest = serde_json::from_value(json!({"text": "hi"})).unwrap();
        assert_eq!(r.k, None);
        assert_eq!(r.namespace, None);
        let r: MemoryCloudQueryRequest =
            serde_json::from_value(json!({"text": "hi", "k": -3, "namespace": "p"})).unwrap();
        assert_eq!(r.k, Some(-3));
    }
}
