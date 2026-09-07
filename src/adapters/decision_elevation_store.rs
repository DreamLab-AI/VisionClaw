// src/adapters/decision_elevation_store.rs
//! Durable case-state authority for ADR-050 decision elevation (ADR-2101).
//!
//! [`crate::actors::decision_elevation_actor::DecisionElevationActor`] used to
//! keep every open governance case in two in-process `HashMap`s (`pending`
//! awaiting a human decision, `elevating` awaiting a terminal git state). A
//! crash — in particular between the kind-31404 terminal publish and any local
//! bookkeeping — silently lost an open decision, and nothing on restart could
//! tell that a case had ever been opened. ADR-2006's closeout named exactly this
//! gap ("current elevation processing owns pending state"; "the durable
//! case-state authority is still not named").
//!
//! This module names it: the **same** `data/enrichment.sqlite3` store the
//! sibling [`crate::actors::elevation_actor::ElevationActor`] already writes
//! through, reached via [`SqliteEnrichmentRepository`]. Decision-elevation cases
//! are rows in `enrichment_proposals` tagged with [`CATEGORY`]; broker decisions
//! are rows in `enrichment_decisions` written by
//! [`SqliteEnrichmentRepository::record_decision`], so the ADR-2006 signed-event
//! correlation columns (`decision_event_id` / `decision_created_at_s`) and the
//! re-delivery suppression they guard come for free.
//!
//! ## Why reuse the enrichment store rather than a new table
//!
//! A decision-elevation case *is* a governance case with the same shape as an
//! enrichment case: an opaque proposal body, a fine-grained lifecycle status and
//! zero-or-more attributed decisions. Reusing the table means one backup unit
//! (`enrichment.sqlite3` is already in the required-DB manifest), one
//! single-writer connection, one migration lineage, and the ADR-2006 duplicate
//! suppression applying to both elevation paths identically. The [`CATEGORY`]
//! tag keeps the two case families disjoint on read.
//!
//! ## Lifecycle
//!
//! ```text
//! pending ──broker approve──▶ approved ──claim──▶ applying ──PR recorded──▶ elevating ──merged──▶ published
//!    │                            │                        └──closed unmerged▶ abandoned
//!    │                            └──(before claim)──▶ resumed at boot
//!    │                               applying without PR URL requires manual reconciliation
//!    ├──broker reject──▶ rejected            (terminal)
//!    ├──amend/delegate─▶ reviewed            (terminal)
//!    ├──31402 publish failed──▶ publish_failed (terminal)
//!    └──TTL exceeded at boot──▶ expired      (terminal, with a 31404 receipt)
//! ```
//!
//! `approved` / `rejected` / `reviewed` are written atomically by
//! `record_decision` (it derives them through `status_for_outcome`), so the
//! decision row and the case status can never disagree.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adapters::sqlite_enrichment_repository::{
    EnrichmentProposal, Result, SqliteEnrichmentRepository, StoredDecision,
};

/// `enrichment_proposals.category` tag marking a decision-elevation case. Read
/// paths filter on it so decision cases and enrichment cases stay disjoint.
pub const CATEGORY: &str = "decision-elevation";

/// How many rows a reconciliation scan reads. Bounded by `MAX_OPEN_CASES` (16)
/// in practice; the headroom covers terminal rows interleaved by `updated_at`.
pub const RECONCILE_SCAN_LIMIT: i64 = 500;

/// Fine-grained lifecycle statuses. `approved` / `rejected` / `reviewed` are
/// produced by `status_for_outcome` inside `record_decision` and are re-declared
/// here so the whole vocabulary reads in one place.
pub mod status {
    /// Case published to the forum, awaiting a human decision.
    pub const PENDING: &str = "pending";
    /// Broker approved; no application has been claimed yet.
    pub const APPROVED: &str = "approved";
    /// Application claimed; upstream outcome may be unknown. Reconcile manually.
    pub const APPLYING: &str = "applying";
    /// Corpus PR is open and being polled for a terminal git state (GOV-2).
    pub const ELEVATING: &str = "elevating";
    /// PR merged — the decision reached the corpus.
    pub const PUBLISHED: &str = "published";
    /// PR closed unmerged — elevation abandoned.
    pub const ABANDONED: &str = "abandoned";
    /// Broker rejected; the decision stays runtime-only.
    pub const REJECTED: &str = "rejected";
    /// Broker answered amend/delegate; no corpus write follows.
    pub const REVIEWED: &str = "reviewed";
    /// The kind-31402 case publish failed, so no human can ever answer it.
    pub const PUBLISH_FAILED: &str = "publish_failed";
    /// Open past the TTL and timed out at boot reconciliation, with a receipt.
    pub const EXPIRED: &str = "expired";

    /// Statuses that need no further work: the case is closed for good.
    pub fn is_terminal(s: &str) -> bool {
        matches!(
            s,
            PUBLISHED | ABANDONED | REJECTED | REVIEWED | PUBLISH_FAILED | EXPIRED
        )
    }
}

/// One durable decision-elevation case. `case_id` and `status` are columns; the
/// remaining fields ride in `proposal_json` (see [`CaseBody`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionCase {
    pub case_id: String,
    pub decision_urn: String,
    pub file_path: String,
    /// The rendered corpus page. Persisted so an approval that crashed before
    /// the PR opened can be resumed after a restart without re-drafting.
    pub draft: String,
    pub summary: String,
    pub status: String,
    /// Set once the corpus PR is open (status `elevating` onwards).
    pub pr_url: Option<String>,
    /// Unix seconds at which the case was opened. Drives the TTL timeout.
    pub opened_at_s: i64,
}

/// The `proposal_json` body — everything not already carried by a column.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaseBody {
    decision_urn: String,
    file_path: String,
    draft: String,
    summary: String,
    #[serde(default)]
    pr_url: Option<String>,
    #[serde(default)]
    opened_at_s: i64,
}

impl DecisionCase {
    /// A freshly opened case, stamped `pending` at `opened_at_s`.
    pub fn opening(
        case_id: impl Into<String>,
        decision_urn: impl Into<String>,
        file_path: impl Into<String>,
        draft: impl Into<String>,
        summary: impl Into<String>,
        opened_at_s: i64,
    ) -> Self {
        Self {
            case_id: case_id.into(),
            decision_urn: decision_urn.into(),
            file_path: file_path.into(),
            draft: draft.into(),
            summary: summary.into(),
            status: status::PENDING.into(),
            pr_url: None,
            opened_at_s,
        }
    }

    fn to_row(&self) -> std::result::Result<EnrichmentProposal, serde_json::Error> {
        let body = CaseBody {
            decision_urn: self.decision_urn.clone(),
            file_path: self.file_path.clone(),
            draft: self.draft.clone(),
            summary: self.summary.clone(),
            pr_url: self.pr_url.clone(),
            opened_at_s: self.opened_at_s,
        };
        Ok(EnrichmentProposal {
            case_id: self.case_id.clone(),
            category: Some(CATEGORY.to_string()),
            source_iri: Some(self.decision_urn.clone()),
            proposal_json: serde_json::to_value(body)?,
            status: self.status.clone(),
            // `create_or_update` stamps both timestamps itself.
            created_at: 0,
            updated_at: 0,
        })
    }

    /// Rebuild a case from its stored row. `None` when the body is not a
    /// decision-elevation payload (a foreign row, or one written by an older
    /// schema) — the caller skips it rather than failing the whole scan.
    fn from_row(p: &EnrichmentProposal) -> Option<Self> {
        let body: CaseBody = serde_json::from_value(p.proposal_json.clone()).ok()?;
        Some(Self {
            case_id: p.case_id.clone(),
            decision_urn: body.decision_urn,
            file_path: body.file_path,
            draft: body.draft,
            summary: body.summary,
            status: p.status.clone(),
            pr_url: body.pr_url,
            // Fall back to the row's own creation stamp for a body written
            // before `opened_at_s` existed, so the TTL still has an anchor.
            opened_at_s: if body.opened_at_s > 0 {
                body.opened_at_s
            } else {
                p.created_at
            },
        })
    }
}

/// Typed facade over [`SqliteEnrichmentRepository`] for decision-elevation cases.
///
/// Cheap to clone (one `Arc`), and safe to share: `tokio-rusqlite` serialises
/// every call onto the store's own worker thread.
pub struct DecisionElevationStore {
    repo: Arc<SqliteEnrichmentRepository>,
}

impl DecisionElevationStore {
    /// Wrap an already-open enrichment repository (the wiring `AppState` would
    /// use, and what the tests use).
    pub fn new(repo: Arc<SqliteEnrichmentRepository>) -> Self {
        Self { repo }
    }

    /// Open (or create) the enrichment store at `db_path` and wrap it.
    ///
    /// The actor boots before `AppState` hands anything out, so it opens its own
    /// connection to the same file. SQLite in WAL mode (set by the shared
    /// schema) supports multiple connections to one database; every write here
    /// is a short single-statement or single-transaction call.
    pub async fn open(db_path: &Path) -> Result<Self> {
        Ok(Self::new(Arc::new(
            SqliteEnrichmentRepository::open(db_path).await?,
        )))
    }

    /// The `data/enrichment.sqlite3` path, resolved exactly as `AppState` does.
    pub fn default_db_path() -> std::path::PathBuf {
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        Path::new(&data_dir).join("enrichment.sqlite3")
    }

    /// Persist a newly opened case (status `pending`). Written *before* the
    /// kind-31402 publish so the durable record can never be the thing that is
    /// missing; a publish that then fails is closed out with
    /// [`Self::mark_terminal`] and `publish_failed`.
    pub async fn open_case(&self, case: &DecisionCase) -> Result<()> {
        let row = case.to_row().map_err(|e| {
            crate::adapters::sqlite_enrichment_repository::EnrichmentStoreError::Serialization(
                e.to_string(),
            )
        })?;
        self.repo.create_or_update(&row).await
    }

    /// Record a broker decision atomically (INSERT decision + UPDATE case
    /// status, one transaction). Returns the decision row id; a re-delivered
    /// signed event returns the existing id and writes nothing (ADR-2006).
    pub async fn record_decision(&self, d: &StoredDecision) -> Result<i64> {
        self.repo.record_decision(d).await
    }

    /// Claim one approved operation. An `applying` row is an uncertain external
    /// result until its PR URL is durably recorded; boot never retries it blindly.
    pub async fn claim_application(&self, case_id: &str) -> Result<bool> {
        self.repo.claim_application(case_id).await
    }

    /// Stamp the opened corpus PR onto the case and move it to `elevating`.
    /// Re-reads the row so the draft body is preserved verbatim.
    pub async fn mark_elevating(&self, case_id: &str, pr_url: &str) -> Result<()> {
        let Some(mut case) = self.get(case_id).await? else {
            return Ok(()); // unknown case — nothing to stamp (no-op, like set_status)
        };
        case.pr_url = Some(pr_url.to_string());
        case.status = status::ELEVATING.to_string();
        self.open_case(&case).await
    }

    /// Force a terminal status (`published` / `abandoned` / `publish_failed` /
    /// `expired`). A no-op for an unknown case id.
    pub async fn mark_terminal(&self, case_id: &str, status: &str) -> Result<()> {
        self.repo.set_status(case_id, status).await
    }

    /// Fetch one decision-elevation case. `None` for an unknown id or a row
    /// belonging to another case family.
    pub async fn get(&self, case_id: &str) -> Result<Option<DecisionCase>> {
        Ok(self
            .repo
            .get(case_id)
            .await?
            .filter(|p| p.category.as_deref() == Some(CATEGORY))
            .as_ref()
            .and_then(DecisionCase::from_row))
    }

    /// Every non-terminal decision-elevation case, newest-updated first. This is
    /// the boot reconciliation read: what the process must resume or time out.
    pub async fn open_cases(&self) -> Result<Vec<DecisionCase>> {
        Ok(self
            .repo
            .list(None, RECONCILE_SCAN_LIMIT, 0)
            .await?
            .iter()
            .filter(|p| p.category.as_deref() == Some(CATEGORY))
            .filter(|p| !status::is_terminal(&p.status))
            .filter_map(DecisionCase::from_row)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (DecisionElevationStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("enrichment.sqlite3");
        let s = DecisionElevationStore::open(&path).await.expect("open");
        (s, dir)
    }

    fn case(id: &str, at: i64) -> DecisionCase {
        DecisionCase::opening(
            id,
            format!("urn:ngm:kg:decision:{id}"),
            format!("decisions/{id}.md"),
            "# draft page\n\nbody",
            "a significant decision",
            at,
        )
    }

    #[test]
    fn terminal_statuses_are_exactly_the_closed_set() {
        for s in [
            status::PUBLISHED,
            status::ABANDONED,
            status::REJECTED,
            status::REVIEWED,
            status::PUBLISH_FAILED,
            status::EXPIRED,
        ] {
            assert!(status::is_terminal(s), "{s} must be terminal");
        }
        for s in [status::PENDING, status::APPROVED, status::ELEVATING] {
            assert!(!status::is_terminal(s), "{s} must stay open");
        }
    }

    #[tokio::test]
    async fn application_claim_is_conditional_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cases.sqlite3");
        let store = DecisionElevationStore::open(&path).await.unwrap();
        let mut c = case("claim", 1);
        store.open_case(&c).await.unwrap();
        assert!(!store.claim_application("claim").await.unwrap());
        assert!(!store.claim_application("missing").await.unwrap());
        c.status = status::APPROVED.into();
        store.open_case(&c).await.unwrap();
        assert!(store.claim_application("claim").await.unwrap());
        assert!(!store.claim_application("claim").await.unwrap());
        drop(store);
        let reopened = DecisionElevationStore::open(&path).await.unwrap();
        assert!(!reopened.claim_application("claim").await.unwrap());
        assert_eq!(
            reopened.get("claim").await.unwrap().unwrap().status,
            "applying"
        );
    }

    #[tokio::test]
    async fn open_case_round_trips_every_field() {
        let (store, _d) = store().await;
        let c = case("vc-decelev-alpha", 1_700_000_000);
        store.open_case(&c).await.unwrap();

        let got = store
            .get("vc-decelev-alpha")
            .await
            .unwrap()
            .expect("stored");
        assert_eq!(got, c, "the whole case must survive the round trip");
        assert_eq!(got.status, status::PENDING);
        assert!(got.pr_url.is_none());
    }

    #[tokio::test]
    async fn get_ignores_rows_from_another_case_family() {
        let (store, _d) = store().await;
        store
            .repo
            .create_or_update(&EnrichmentProposal {
                case_id: "vc-enrich-1".into(),
                category: Some("enrichment".into()),
                source_iri: None,
                proposal_json: serde_json::json!({"label": "not a decision case"}),
                status: "pending".into(),
                created_at: 0,
                updated_at: 0,
            })
            .await
            .unwrap();

        assert!(store.get("vc-enrich-1").await.unwrap().is_none());
        assert!(store.open_cases().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn transitions_persist_pending_elevating_published() {
        let (store, _d) = store().await;
        let c = case("vc-decelev-beta", 1_700_000_000);
        store.open_case(&c).await.unwrap();

        store
            .mark_elevating("vc-decelev-beta", "https://github.com/o/r/pull/7")
            .await
            .unwrap();
        let got = store.get("vc-decelev-beta").await.unwrap().unwrap();
        assert_eq!(got.status, status::ELEVATING);
        assert_eq!(got.pr_url.as_deref(), Some("https://github.com/o/r/pull/7"));
        assert_eq!(got.draft, c.draft, "the draft survives the PR stamp");

        store
            .mark_terminal("vc-decelev-beta", status::PUBLISHED)
            .await
            .unwrap();
        assert_eq!(
            store.get("vc-decelev-beta").await.unwrap().unwrap().status,
            status::PUBLISHED
        );
        assert!(
            store.open_cases().await.unwrap().is_empty(),
            "a published case is no longer open work"
        );
    }

    #[tokio::test]
    async fn record_decision_sets_case_status_atomically() {
        let (store, _d) = store().await;
        store.open_case(&case("vc-decelev-gamma", 1)).await.unwrap();

        let d = StoredDecision {
            case_id: "vc-decelev-gamma".into(),
            outcome: "approve".into(),
            attributed: false,
            broker_pubkey: None,
            reasoning: Some("looks right".into()),
            writeback_triggered: true,
            writeback_committed: false,
            activity_urn: "urn:ngm:execution:decelev-decide:gamma".into(),
            proposal_urn: None,
            owner_did: None,
            decided_at_ms: 1_700_000_000_000,
            decision_event_id: Some("event-abc".into()),
            decision_created_at_s: Some(1_700_000_000),
        };
        let id = store.record_decision(&d).await.unwrap();

        assert_eq!(
            store.get("vc-decelev-gamma").await.unwrap().unwrap().status,
            status::APPROVED,
            "the decision transaction moves the case to approved"
        );
        // ADR-2006: a re-delivered signed event is recorded once.
        assert_eq!(store.record_decision(&d).await.unwrap(), id);
    }

    #[tokio::test]
    async fn open_cases_survive_a_restart_of_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enrichment.sqlite3");
        {
            let store = DecisionElevationStore::open(&path).await.unwrap();
            store
                .open_case(&case("vc-decelev-pending", 100))
                .await
                .unwrap();
            store
                .open_case(&case("vc-decelev-tracked", 200))
                .await
                .unwrap();
            store
                .mark_elevating("vc-decelev-tracked", "https://github.com/o/r/pull/9")
                .await
                .unwrap();
            store
                .open_case(&case("vc-decelev-done", 300))
                .await
                .unwrap();
            store
                .mark_terminal("vc-decelev-done", status::ABANDONED)
                .await
                .unwrap();
        } // drop every handle — the process "crashes" here

        let store = DecisionElevationStore::open(&path).await.unwrap();
        let mut open = store.open_cases().await.unwrap();
        open.sort_by(|a, b| a.case_id.cmp(&b.case_id));
        assert_eq!(open.len(), 2, "only the terminal case is gone");
        assert_eq!(open[0].case_id, "vc-decelev-pending");
        assert_eq!(open[0].status, status::PENDING);
        assert_eq!(open[1].case_id, "vc-decelev-tracked");
        assert_eq!(open[1].status, status::ELEVATING);
        assert_eq!(
            open[1].pr_url.as_deref(),
            Some("https://github.com/o/r/pull/9")
        );
        assert_eq!(open[1].draft, "# draft page\n\nbody");
    }

    #[tokio::test]
    async fn marking_an_unknown_case_is_a_no_op() {
        let (store, _d) = store().await;
        store.mark_elevating("nope", "https://x/1").await.unwrap();
        store
            .mark_terminal("nope", status::PUBLISHED)
            .await
            .unwrap();
        assert!(store.get("nope").await.unwrap().is_none());
    }
}
