//! Insight Ingestion Loop v1 — the five-stage loop trace (REC-10, PRD-023 WP-12).
//!
//! PRD-023 (Measurement Data Sources) fixes VisionClaw as the lead for the
//! Insight Ingestion Loop: an `ontology_propose` event lands in the governed
//! write-back queue, becomes a broker case, is decided, and — on an attributed
//! approval — is merged back into the knowledge graph. This module assembles
//! that lifecycle into an ordered five-stage trace with a persisted timestamp at
//! each stage, so **Mesh Velocity** (insight-to-integration time) is computable
//! rather than asserted.
//!
//! The five stages (ADR presentation diagram `03-insight-ingestion-loop`):
//!   1. **propose**            — the `ontology_propose` capture instant.
//!   2. **queued**             — the insight entered the governed write-back
//!      queue (the broker case, P0 kernel + P1 queue).
//!   3. **broker_decision**    — a broker decided the case.
//!   4. **merged_enrichment**  — the fenced Oxigraph `:summary` write landed.
//!   5. **amplification**      — *planned*. The insight propagates/amplifies
//!      across the mesh; labelled `planned`, never a
//!      fabricated value.
//!
//! The assembler is a pure function over one [`LoopTraceRow`]
//! (`crate::adapters::sqlite_enrichment_repository`), so the loop contract is
//! unit-testable without the store or the actor system. Timestamps are read from
//! the persisted stores: the propose/queue instants prefer the finer stamps the
//! `ontology_propose` event puts on the proposal body (`proposed_at_ms` /
//! `queued_at_ms`) and fall back to the proposal row's `created_at` (unix
//! seconds → ms); the decision and merged instants come from the decision row
//! (`decided_at_ms`, `writeback_committed_at_ms`).

use serde::Serialize;

use crate::adapters::sqlite_enrichment_repository::LoopTraceRow;

/// Canonical stage identifiers (the `stage` key and the trace-diagram order).
pub const STAGE_PROPOSE: &str = "propose";
pub const STAGE_QUEUED: &str = "queued";
pub const STAGE_DECISION: &str = "broker_decision";
pub const STAGE_MERGED: &str = "merged_enrichment";
pub const STAGE_AMPLIFICATION: &str = "amplification";

/// One stage of the loop. `status` is one of `complete`, `pending`,
/// `not_applicable` (a rejected proposal never merges), or `planned` (the
/// amplification stage, unbuilt in v1). `at_ms` is the persisted instant the
/// stage reached, or `None` when it has not (pending/planned).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LoopStage {
    pub stage: &'static str,
    pub label: &'static str,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The assembled five-stage trace for one case.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InsightLoopTrace {
    pub case_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Fine-grained proposal status from the queue (`pending`/`approved`/…).
    pub status: String,
    pub stages: Vec<LoopStage>,
    /// True once the merged-enrichment stage completed (the loop closed once end
    /// to end). Amplification being `planned` does not gate closure — v1 closes
    /// on integration into the KG.
    pub loop_closed: bool,
    /// Insight-to-integration time: `merged_at − propose_at`, in ms. `Some` only
    /// for a closed loop (the Mesh Velocity sample this case contributes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_velocity_ms: Option<i64>,
    /// Propose-to-decision latency, in ms, when the case has been decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_decision_ms: Option<i64>,
    /// True when every completed stage's timestamp is non-decreasing — the
    /// loop timeline is monotonic (the REC-10 falsification guard).
    pub monotonic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_urn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_did: Option<String>,
}

/// The stage instants resolved from a row (propose, queued, decision, merged).
/// The propose/queue instants prefer the body stamps, else the proposal-row
/// `created_at` promoted from seconds to ms.
fn stage_instants(row: &LoopTraceRow) -> (i64, i64, Option<i64>, Option<i64>) {
    let created_ms = row.created_at_s.saturating_mul(1000);
    let body_ms = |key: &str| row.proposal_json.get(key).and_then(|v| v.as_i64());
    let propose = body_ms("proposed_at_ms").unwrap_or(created_ms);
    let queued = body_ms("queued_at_ms").unwrap_or(created_ms);
    (
        propose,
        queued,
        row.decided_at_ms,
        row.writeback_committed_at_ms,
    )
}

/// Assemble the five-stage insight-loop trace for one joined row (REC-10). Pure:
/// no I/O, so the loop contract is unit-tested against fixture rows.
pub fn build_trace(row: &LoopTraceRow) -> InsightLoopTrace {
    let (propose_at, queued_at, decision_at, merged_at) = stage_instants(row);

    // Stage 1: propose. A row in the queue means the insight was captured.
    let propose = LoopStage {
        stage: STAGE_PROPOSE,
        label: "Insight proposed",
        status: "complete",
        at_ms: Some(propose_at),
        detail: row
            .proposal_json
            .get("target_path")
            .and_then(|v| v.as_str())
            .map(|s| format!("target={s}")),
    };

    // Stage 2: queued into the governed write-back queue (the broker case).
    let queued = LoopStage {
        stage: STAGE_QUEUED,
        label: "Queued for governance",
        status: "complete",
        at_ms: Some(queued_at),
        detail: row.category.clone().map(|c| format!("category={c}")),
    };

    // Stage 3: broker decision.
    let decision = LoopStage {
        stage: STAGE_DECISION,
        label: "Broker decided",
        status: if decision_at.is_some() {
            "complete"
        } else {
            "pending"
        },
        at_ms: decision_at,
        detail: row.decision_outcome.clone().map(|o| format!("outcome={o}")),
    };

    // Stage 4: merged enrichment (the fenced KG write). A decision that never
    // triggers a write-back (a rejection, or an unattributed approval) has no
    // merge to complete — that is `not_applicable`, not a stuck `pending`.
    let merged_status = match (row.decided_at_ms, row.writeback_triggered, merged_at) {
        (_, _, Some(_)) => "complete",
        (Some(_), Some(true), None) => "pending",
        (Some(_), _, None) => "not_applicable",
        (None, _, None) => "pending",
    };
    let merged = LoopStage {
        stage: STAGE_MERGED,
        label: "Merged into knowledge graph",
        status: merged_status,
        at_ms: merged_at,
        detail: row.activity_urn.clone().map(|u| format!("activity={u}")),
    };

    // Stage 5: amplification — planned (labelled, never a fabricated value).
    let amplification = LoopStage {
        stage: STAGE_AMPLIFICATION,
        label: "Amplified across the mesh",
        status: "planned",
        at_ms: None,
        detail: Some("v1 scope: capture→queue→decide→merge; amplification is planned".into()),
    };

    let loop_closed = merged_at.is_some();
    let mesh_velocity_ms = merged_at.map(|m| m - propose_at);
    let time_to_decision_ms = decision_at.map(|d| d - propose_at);

    // Monotonicity over the completed stages' instants (propose ≤ queued ≤
    // decision ≤ merged, skipping stages with no timestamp).
    let ordered: Vec<i64> = [Some(propose_at), Some(queued_at), decision_at, merged_at]
        .into_iter()
        .flatten()
        .collect();
    let monotonic = ordered.windows(2).all(|w| w[0] <= w[1]);

    InsightLoopTrace {
        case_id: row.case_id.clone(),
        category: row.category.clone(),
        status: row.status.clone(),
        stages: vec![propose, queued, decision, merged, amplification],
        loop_closed,
        mesh_velocity_ms,
        time_to_decision_ms,
        monotonic,
        activity_urn: row.activity_urn.clone(),
        owner_did: row.owner_did.clone(),
    }
}

/// A batch of traces plus the aggregate Mesh Velocity over the closed loops.
#[derive(Debug, Clone, Serialize)]
pub struct InsightLoopSummary {
    pub traces: Vec<InsightLoopTrace>,
    pub total: usize,
    /// Count of loops that closed (reached merged-enrichment) in this batch.
    pub closed_loops: usize,
    /// Mean insight-to-integration time over the closed loops, in ms; `None`
    /// when no loop has closed yet (honest — no fabricated velocity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_velocity_mean_ms: Option<i64>,
    /// The amplification stage is planned across the whole loop v1.
    pub amplification_status: &'static str,
}

/// Summarise a batch of rows into traces + the aggregate Mesh Velocity.
pub fn summarise(rows: &[LoopTraceRow]) -> InsightLoopSummary {
    let traces: Vec<InsightLoopTrace> = rows.iter().map(build_trace).collect();
    let velocities: Vec<i64> = traces.iter().filter_map(|t| t.mesh_velocity_ms).collect();
    let closed_loops = velocities.len();
    let mesh_velocity_mean_ms = if velocities.is_empty() {
        None
    } else {
        Some(velocities.iter().sum::<i64>() / velocities.len() as i64)
    };
    InsightLoopSummary {
        total: traces.len(),
        traces,
        closed_loops,
        mesh_velocity_mean_ms,
        amplification_status: "planned",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(decided: Option<i64>, triggered: Option<bool>, merged: Option<i64>) -> LoopTraceRow {
        LoopTraceRow {
            case_id: "case-loop".into(),
            category: Some("knowledge_enrichment".into()),
            status: "approved".into(),
            proposal_json: json!({
                "target_path": "pages/foo.md",
                "proposed_at_ms": 1_000_i64,
                "queued_at_ms": 2_000_i64,
            }),
            created_at_s: 1,
            updated_at_s: 4,
            decision_outcome: decided.map(|_| "approve".to_string()),
            decided_at_ms: decided,
            writeback_triggered: triggered,
            writeback_committed: merged.map(|_| true),
            writeback_committed_at_ms: merged,
            activity_urn: Some("urn:visionclaw:execution:sha256-12-abcabcabcabc".into()),
            owner_did: Some("did:nostr:aaaa".into()),
        }
    }

    #[test]
    fn closed_loop_has_five_stages_monotonic_and_a_velocity() {
        let t = build_trace(&row(Some(3_000), Some(true), Some(4_000)));
        assert_eq!(t.stages.len(), 5);
        // Stage ids in order.
        let ids: Vec<&str> = t.stages.iter().map(|s| s.stage).collect();
        assert_eq!(
            ids,
            vec![
                STAGE_PROPOSE,
                STAGE_QUEUED,
                STAGE_DECISION,
                STAGE_MERGED,
                STAGE_AMPLIFICATION
            ]
        );
        // First four completed, amplification planned.
        assert_eq!(t.stages[0].status, "complete");
        assert_eq!(t.stages[1].status, "complete");
        assert_eq!(t.stages[2].status, "complete");
        assert_eq!(t.stages[3].status, "complete");
        assert_eq!(t.stages[4].status, "planned");
        assert!(t.loop_closed);
        assert!(t.monotonic, "propose≤queued≤decision≤merged");
        // Mesh Velocity = merged(4000) − propose(1000).
        assert_eq!(t.mesh_velocity_ms, Some(3_000));
        assert_eq!(t.time_to_decision_ms, Some(2_000));
    }

    #[test]
    fn rejection_marks_merge_not_applicable_not_pending() {
        // A decided-but-not-triggered proposal (reject / unattributed) has no
        // merge to reach — that is not_applicable, and the loop is not closed.
        let t = build_trace(&row(Some(3_000), Some(false), None));
        assert_eq!(t.stages[3].status, "not_applicable");
        assert!(!t.loop_closed);
        assert!(t.mesh_velocity_ms.is_none());
        assert_eq!(t.time_to_decision_ms, Some(2_000));
    }

    #[test]
    fn pending_proposal_has_pending_decision_and_no_velocity() {
        let t = build_trace(&row(None, None, None));
        assert_eq!(t.stages[2].status, "pending");
        assert_eq!(t.stages[3].status, "pending");
        assert!(!t.loop_closed);
        assert!(t.mesh_velocity_ms.is_none());
        assert!(t.time_to_decision_ms.is_none());
        // Propose + queued still complete (the insight is in the queue).
        assert!(t.monotonic);
    }

    #[test]
    fn propose_instant_falls_back_to_created_at_when_body_unstamped() {
        let mut r = row(Some(3_000), Some(true), Some(4_000));
        r.proposal_json = json!({ "target_path": "pages/bar.md" }); // no stamps
        r.created_at_s = 2; // → 2_000 ms
        let t = build_trace(&r);
        assert_eq!(
            t.stages[0].at_ms,
            Some(2_000),
            "propose falls back to created_at*1000"
        );
        assert_eq!(
            t.stages[1].at_ms,
            Some(2_000),
            "queued falls back to created_at*1000"
        );
        // created_at(2000) ≤ decided(3000) ≤ merged(4000): still monotonic.
        assert!(t.monotonic);
        assert_eq!(t.mesh_velocity_ms, Some(2_000));
    }

    #[test]
    fn summary_means_velocity_over_closed_loops_only() {
        let rows = vec![
            row(Some(3_000), Some(true), Some(4_000)), // velocity 3000
            row(Some(3_000), Some(true), Some(6_000)), // velocity 5000
            row(None, None, None),                     // open — excluded
        ];
        let s = summarise(&rows);
        assert_eq!(s.total, 3);
        assert_eq!(s.closed_loops, 2);
        assert_eq!(s.mesh_velocity_mean_ms, Some(4_000)); // (3000+5000)/2
        assert_eq!(s.amplification_status, "planned");
    }

    #[test]
    fn empty_batch_reports_no_velocity() {
        let s = summarise(&[]);
        assert_eq!(s.total, 0);
        assert_eq!(s.closed_loops, 0);
        assert!(s.mesh_velocity_mean_ms.is_none());
    }
}
