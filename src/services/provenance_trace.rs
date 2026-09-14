//! Unified provenance trace — the data-moat consolidation (REC-11, PRD-023 WP-12).
//!
//! REC-11 consolidates the ecosystem's provenance into ONE queryable trace, with
//! VisionClaw leading. Per ADR-130 this is a **query layer over the stores that
//! already exist**, not a new store: it reads
//!   * **agent-events / hook-trajectory** rows (the durable projection of the
//!     `/wss/agent-events` wire, `kpi_agent_events`; agentbox emits the CTC
//!     fields since P1), keyed on `did:nostr`; and
//!   * **broker decisions** (`enrichment_decisions`), keyed on the deciding
//!     `did:nostr` (`owner_did`) with the PROV-O activity URN;
//! and it JOINS them on the `did:nostr` `agent_did` attribution the solid-pod
//! provenance-trace contract fixes as the shared key
//! (`solid-pod-rs/.../reference/provenance-trace-contract.md` §2.3).
//!
//! A third source — **pod git-marks** from solid-pod-rs — is *default-off*: that
//! contract's write hook is a no-op shim unless the pod server is built
//! `--features git`, so a default pod records zero marks. This trace therefore
//! **tolerates absent sources and reports which were present**: [`PodProvenanceMark`]
//! consumes the contract's `marks[]` element shape so the join incorporates pod
//! marks WHEN a pod supplies them, and lists `pod_git_mark` under
//! `sources_absent` when it does not. Nothing here assumes on-by-default pod
//! provenance (the contract's head caveat).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adapters::sqlite_enrichment_repository::{
    ProvenanceDecisionRow, SqliteEnrichmentRepository,
};
use crate::adapters::sqlite_kpi_repository::{AgentTrajectoryRow, SqliteKpiRepository};
use crate::services::intent_match::intent_match;

/// The three source kinds the trace can join.
pub const SOURCE_AGENT_EVENT: &str = "agent_event";
pub const SOURCE_BROKER_DECISION: &str = "broker_decision";
pub const SOURCE_POD_GIT_MARK: &str = "pod_git_mark";

/// Default trace window: 30 days, matching the KPI rolling window.
pub const TRACE_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

// ---------------------------------------------------------------------------
// Pod git-mark — the solid-pod ADR-060 contract shape (consumed, not produced)
// ---------------------------------------------------------------------------

/// One pod git-mark, mirroring the `marks[]` element of solid-pod-rs
/// `GET /{pod}/_prov/` (provenance-trace-contract §2.3). Consumed by the join;
/// the pod side is VisionClaw-led-but-default-off, so a live trace normally sees
/// none of these until a `--features git` pod supplies them.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PodProvenanceMark {
    pub resource: String,
    /// `commit.agent_did` — the `did:nostr` of the NIP-98 writer; the join key.
    pub agent_did: Option<String>,
    pub commit_sha: String,
    /// `commit.committed_at` (RFC-3339); parsed to epoch ms for ordering.
    pub committed_at: String,
    #[serde(default)]
    pub anchored: bool,
}

impl PodProvenanceMark {
    /// Parse one element of the contract's `marks[]` array. Tolerant of the
    /// nested `commit` object the contract returns.
    pub fn from_contract_json(v: &serde_json::Value) -> Option<Self> {
        let commit = v.get("commit")?;
        Some(Self {
            resource: v
                .get("resource")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            agent_did: commit
                .get("agent_did")
                .and_then(|s| s.as_str())
                .filter(|s| s.starts_with("did:nostr:"))
                .map(|s| s.to_string()),
            commit_sha: commit
                .get("sha")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            committed_at: commit
                .get("committed_at")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            anchored: v.get("anchored").and_then(|b| b.as_bool()).unwrap_or(false),
        })
    }

    fn at_ms(&self) -> i64 {
        chrono::DateTime::parse_from_rfc3339(&self.committed_at)
            .map(|d| d.timestamp_millis())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Trace shapes
// ---------------------------------------------------------------------------

/// One normalised provenance record, across any source kind.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TraceRecord {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_did: Option<String>,
    /// The source-native reference: an activity/handoff URN, or a commit sha.
    pub reference: String,
    /// The source-native kind: a decision outcome, an action name, "git-mark".
    pub kind: String,
    pub at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// FR5.2 (EXP-AC-005) — the agent's DECLARED intent for this act, verbatim
    /// from the envelope, or absent when it declared none. Agent-event records
    /// only; other source kinds carry no declaration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Did the act match the declaration? `Some(true)`/`Some(false)` only when an
    /// intent was declared; `null` means "no claim was made", which is NOT the
    /// same as "the claim failed". See [`crate::services::intent_match`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_match: Option<bool>,
}

/// A cross-source correlation under one `did:nostr` — the actual join. Present
/// only for an identity that appears in **two or more distinct source kinds**.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TraceJoin {
    pub agent_did: String,
    /// The distinct source kinds this identity appears in (≥2).
    pub sources: Vec<&'static str>,
    pub record_count: usize,
}

/// The unified provenance trace.
#[derive(Debug, Clone, Serialize)]
pub struct ProvenanceTrace {
    /// Source kinds that were queried/available for this trace.
    pub sources_present: Vec<&'static str>,
    /// Source kinds not available (e.g. `pod_git_mark` on a default-off pod).
    pub sources_absent: Vec<&'static str>,
    /// Every normalised record, newest first.
    pub records: Vec<TraceRecord>,
    /// Cross-source joins keyed on `did:nostr` (identities spanning ≥2 kinds).
    pub joins: Vec<TraceJoin>,
    /// The number of distinct source kinds that contributed at least one record.
    pub distinct_source_kinds: usize,
    /// The widest cross-source join: the greatest number of distinct source
    /// kinds unified under a single `did:nostr`. ≥2 means the trace genuinely
    /// joined multiple live source kinds (the REC-11 canary predicate).
    pub max_join_span: usize,
    pub total_records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<i64>,
}

impl ProvenanceTrace {
    /// True when the trace joined at least two live source kinds under one
    /// `did:nostr` — the REC-11 acceptance and the `CANARY-VC-REC11-TRACE`
    /// fire predicate.
    pub fn joins_multiple_source_kinds(&self) -> bool {
        self.max_join_span >= 2
    }
}

// ---------------------------------------------------------------------------
// Pure join
// ---------------------------------------------------------------------------

fn trajectory_reference(t: &AgentTrajectoryRow) -> String {
    t.handoff_id
        .clone()
        .or_else(|| t.target_urn.clone())
        .or_else(|| t.source_urn.clone())
        .unwrap_or_else(|| format!("agent-event:{}", t.event_id))
}

/// Assemble the unified trace from the three source projections. Pure over the
/// input rows so the join contract is unit-testable without the stores.
/// `pod_source_available` reports whether the pod source was queried at all —
/// `false` places `pod_git_mark` in `sources_absent` (default-off pod).
pub fn build_trace(
    trajectories: &[AgentTrajectoryRow],
    decisions: &[ProvenanceDecisionRow],
    pod_marks: &[PodProvenanceMark],
    pod_source_available: bool,
    since_ms: Option<i64>,
) -> ProvenanceTrace {
    let mut records: Vec<TraceRecord> = Vec::new();

    for t in trajectories {
        records.push(TraceRecord {
            source: SOURCE_AGENT_EVENT,
            agent_did: t.agent_did.clone(),
            reference: trajectory_reference(t),
            kind: t
                .action_type_name
                .clone()
                .unwrap_or_else(|| "action".to_string()),
            at_ms: t.observed_at_ms,
            detail: t.verification.clone().map(|v| format!("verification={v}")),
            intent: t.intent.clone(),
            intent_match: intent_match(
                t.intent.as_deref(),
                t.action_type_name.as_deref(),
                t.target_urn.as_deref(),
            ),
        });
    }
    for d in decisions {
        records.push(TraceRecord {
            source: SOURCE_BROKER_DECISION,
            agent_did: d.owner_did.clone(),
            reference: d.activity_urn.clone(),
            kind: d.outcome.clone(),
            at_ms: d.decided_at_ms,
            detail: Some(format!("case={} attributed={}", d.case_id, d.attributed)),
            intent: None,
            intent_match: None,
        });
    }
    for m in pod_marks {
        records.push(TraceRecord {
            source: SOURCE_POD_GIT_MARK,
            agent_did: m.agent_did.clone(),
            reference: m.commit_sha.clone(),
            kind: "git-mark".to_string(),
            at_ms: m.at_ms(),
            detail: Some(format!("resource={} anchored={}", m.resource, m.anchored)),
            intent: None,
            intent_match: None,
        });
    }

    // Sources present = the stores actually queried. The two SQLite-backed
    // sources always are; the pod source only when supplied.
    let mut sources_present: Vec<&'static str> = vec![SOURCE_AGENT_EVENT, SOURCE_BROKER_DECISION];
    let mut sources_absent: Vec<&'static str> = Vec::new();
    if pod_source_available {
        sources_present.push(SOURCE_POD_GIT_MARK);
    } else {
        sources_absent.push(SOURCE_POD_GIT_MARK);
    }

    // Distinct source kinds that actually contributed a record.
    let mut kinds_seen: Vec<&'static str> = Vec::new();
    for r in &records {
        if !kinds_seen.contains(&r.source) {
            kinds_seen.push(r.source);
        }
    }
    let distinct_source_kinds = kinds_seen.len();

    // Group by did:nostr and emit a join for any identity spanning ≥2 kinds.
    let mut by_agent: std::collections::BTreeMap<String, (Vec<&'static str>, usize)> =
        std::collections::BTreeMap::new();
    for r in &records {
        let Some(did) = r.agent_did.as_ref() else {
            continue;
        };
        let entry = by_agent
            .entry(did.clone())
            .or_insert_with(|| (Vec::new(), 0));
        if !entry.0.contains(&r.source) {
            entry.0.push(r.source);
        }
        entry.1 += 1;
    }
    let mut joins: Vec<TraceJoin> = by_agent
        .into_iter()
        .filter(|(_, (sources, _))| sources.len() >= 2)
        .map(|(agent_did, (sources, record_count))| TraceJoin {
            agent_did,
            sources,
            record_count,
        })
        .collect();
    joins.sort_by(|a, b| b.sources.len().cmp(&a.sources.len()));
    let max_join_span = joins.iter().map(|j| j.sources.len()).max().unwrap_or(0);

    // Newest first for the wire.
    records.sort_by(|a, b| b.at_ms.cmp(&a.at_ms));
    let total_records = records.len();

    ProvenanceTrace {
        sources_present,
        sources_absent,
        records,
        joins,
        distinct_source_kinds,
        max_join_span,
        total_records,
        since_ms,
    }
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Reads the two live SQLite-backed sources and assembles the unified trace. The
/// pod source is default-off in this deployment (planned, VisionClaw-led) so it
/// is reported absent; a future build supplies pod marks via [`Self::with_pod_marks`].
pub struct ProvenanceTraceService {
    enrichment_repo: Arc<SqliteEnrichmentRepository>,
    kpi_repo: Arc<SqliteKpiRepository>,
}

impl ProvenanceTraceService {
    pub fn new(
        enrichment_repo: Arc<SqliteEnrichmentRepository>,
        kpi_repo: Arc<SqliteKpiRepository>,
    ) -> Self {
        Self {
            enrichment_repo,
            kpi_repo,
        }
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    /// Build the unified trace over `[now − window, now]`, optionally filtered to
    /// one `did:nostr`. The pod source is absent (default-off); the join reports
    /// it under `sources_absent` and still unifies the two live sources.
    pub async fn query(
        &self,
        window_ms: i64,
        agent_did: Option<&str>,
    ) -> Result<ProvenanceTrace, String> {
        let cutoff = Self::now_ms().saturating_sub(window_ms.max(0));
        let mut trajectories = self
            .kpi_repo
            .trajectories_since(cutoff)
            .await
            .map_err(|e| format!("trajectory read failed: {e}"))?;
        let mut decisions = self
            .enrichment_repo
            .provenance_decisions_since(cutoff)
            .await
            .map_err(|e| format!("decision read failed: {e}"))?;

        if let Some(did) = agent_did {
            trajectories.retain(|t| t.agent_did.as_deref() == Some(did));
            decisions.retain(|d| d.owner_did.as_deref() == Some(did));
        }

        // Pod git-marks: default-off (planned, VisionClaw-led). Absent here.
        Ok(build_trace(
            &trajectories,
            &decisions,
            &[],
            false,
            Some(cutoff),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trajectory(did: Option<&str>, at: i64, handoff: Option<&str>) -> AgentTrajectoryRow {
        AgentTrajectoryRow {
            event_id: 1,
            source_agent_id: 7,
            action_type: 1,
            action_type_name: Some("update".into()),
            agent_did: did.map(str::to_string),
            source_urn: None,
            target_urn: None,
            handoff_id: handoff.map(str::to_string),
            token_count: Some(1234),
            verification: Some("pass".into()),
            intent: None,
            observed_at_ms: at,
        }
    }

    fn decision(did: Option<&str>, at: i64, activity: &str) -> ProvenanceDecisionRow {
        ProvenanceDecisionRow {
            case_id: "case-1".into(),
            owner_did: did.map(str::to_string),
            activity_urn: activity.into(),
            proposal_urn: None,
            outcome: "approve".into(),
            attributed: did.is_some(),
            decided_at_ms: at,
        }
    }

    #[test]
    fn joins_two_live_source_kinds_under_one_did() {
        // A shared did:nostr appears in BOTH the agent-events and the broker-
        // decision source ⇒ a real cross-source join spanning two live kinds.
        let did = "did:nostr:aaaa";
        let traj = vec![trajectory(
            Some(did),
            2_000,
            Some("urn:agentbox:activity:chain-1"),
        )];
        let dec = vec![decision(
            Some(did),
            3_000,
            "urn:visionclaw:execution:sha256-12-abc",
        )];
        let trace = build_trace(&traj, &dec, &[], false, None);

        assert!(trace.sources_present.contains(&SOURCE_AGENT_EVENT));
        assert!(trace.sources_present.contains(&SOURCE_BROKER_DECISION));
        // Pod is default-off ⇒ absent, tolerated, and reported.
        assert!(trace.sources_absent.contains(&SOURCE_POD_GIT_MARK));
        assert_eq!(trace.distinct_source_kinds, 2);
        assert!(
            trace.joins_multiple_source_kinds(),
            "did:nostr joins ≥2 kinds"
        );
        assert_eq!(trace.joins.len(), 1);
        assert_eq!(trace.joins[0].agent_did, did);
        assert_eq!(trace.joins[0].sources.len(), 2);
        assert_eq!(trace.joins[0].record_count, 2);
        // Records newest first.
        assert_eq!(trace.records[0].at_ms, 3_000);
        assert_eq!(trace.total_records, 2);
    }

    #[test]
    fn agent_event_records_carry_intent_and_its_verdict() {
        // FR5.2 (EXP-AC-005): /api/trace reports the declaration AND whether it
        // held — three-valued, with `null` reserved for "no claim was made".
        let mut declared = trajectory(Some("did:nostr:aaaa"), 1_000, None);
        declared.intent = Some("update urn:kg:node-7".into());
        declared.action_type_name = Some("graph_update".into());
        declared.target_urn = Some("urn:kg:node-7".into());

        let mut diverged = trajectory(Some("did:nostr:aaaa"), 2_000, None);
        diverged.intent = Some("update urn:kg:node-7".into());
        diverged.action_type_name = Some("graph_update".into());
        diverged.target_urn = Some("urn:kg:node-99".into());

        let silent = trajectory(Some("did:nostr:aaaa"), 3_000, None);

        let trace = build_trace(&[declared, diverged, silent], &[], &[], false, None);
        // Newest first: silent, diverged, declared.
        assert_eq!(trace.records[0].intent, None);
        assert_eq!(trace.records[0].intent_match, None, "no claim ⇒ no verdict");
        assert_eq!(trace.records[1].intent_match, Some(false));
        assert_eq!(trace.records[2].intent_match, Some(true));
        assert_eq!(
            trace.records[2].intent.as_deref(),
            Some("update urn:kg:node-7"),
            "the declaration is reported verbatim"
        );
    }

    #[test]
    fn non_agent_sources_declare_no_intent() {
        let dec = vec![decision(Some("did:nostr:aaaa"), 1_000, "urn:x")];
        let trace = build_trace(&[], &dec, &[], false, None);
        assert_eq!(trace.records[0].intent, None);
        assert_eq!(trace.records[0].intent_match, None);
    }

    #[test]
    fn single_source_does_not_join() {
        // Only agent-events under this did ⇒ no cross-source join.
        let traj = vec![trajectory(Some("did:nostr:bbbb"), 1_000, None)];
        let trace = build_trace(&traj, &[], &[], false, None);
        assert_eq!(trace.distinct_source_kinds, 1);
        assert!(!trace.joins_multiple_source_kinds());
        assert!(trace.joins.is_empty());
    }

    #[test]
    fn anonymous_records_never_join() {
        // Records with no did:nostr are surfaced but cannot join (no shared key).
        let traj = vec![trajectory(None, 1_000, None)];
        let dec = vec![decision(
            None,
            2_000,
            "urn:visionclaw:execution:sha256-12-xyz",
        )];
        let trace = build_trace(&traj, &dec, &[], false, None);
        assert_eq!(trace.total_records, 2);
        assert!(trace.joins.is_empty());
        assert_eq!(trace.max_join_span, 0);
    }

    #[test]
    fn pod_source_incorporated_when_available() {
        // When a --features git pod DOES supply marks, the pod kind is present and
        // a shared did:nostr can span all three sources.
        let did = "did:nostr:cccc";
        let traj = vec![trajectory(Some(did), 1_000, None)];
        let dec = vec![decision(
            Some(did),
            2_000,
            "urn:visionclaw:execution:sha256-12-c",
        )];
        let pod = vec![PodProvenanceMark {
            resource: "/npub1x/alice/notes/foo.ttl".into(),
            agent_did: Some(did.into()),
            commit_sha: "a".repeat(40),
            committed_at: "2026-06-13T10:12:30Z".into(),
            anchored: false,
        }];
        let trace = build_trace(&traj, &dec, &pod, true, None);
        assert!(trace.sources_present.contains(&SOURCE_POD_GIT_MARK));
        assert!(trace.sources_absent.is_empty());
        assert_eq!(trace.distinct_source_kinds, 3);
        assert_eq!(trace.max_join_span, 3, "one did unifies all three sources");
    }

    #[test]
    fn pod_mark_parses_from_contract_json() {
        // The solid-pod GET /{pod}/_prov/ marks[] element shape (contract §2.3).
        let v = serde_json::json!({
            "resource": "/npub1/alice/notes/foo.ttl",
            "commit": {
                "sha": "0".repeat(40),
                "parent": null,
                "agent_did": "did:nostr:dddd",
                "committer": "alice",
                "subject": "add note",
                "committed_at": "2026-06-13T10:12:30Z"
            },
            "prov_ttl": "@prefix prov: ...",
            "anchored": false
        });
        let m = PodProvenanceMark::from_contract_json(&v).expect("parse");
        assert_eq!(m.agent_did.as_deref(), Some("did:nostr:dddd"));
        assert_eq!(m.commit_sha.len(), 40);
        assert!(m.at_ms() > 0, "RFC-3339 committed_at parses to epoch ms");
    }
}
