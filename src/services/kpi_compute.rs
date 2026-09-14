//! KpiComputeService — the REC-4 four-KPI compute engine (ADR-043 resurrection,
//! ADR-130 Decision 5).
//!
//! ADR-043 named four organisational KPIs. Two compute now, from sources that
//! already exist, without new instrumentation at the emit site:
//!
//!   * **Augmentation Ratio** — agent-action volume ÷ ACSP escalation volume.
//!     Numerator: the count of `/wss/agent-events` envelopes observed in the
//!     rolling window (the passive hub tap in [`run_agent_event_tap`]).
//!     Denominator: the count of broker/enrichment decisions in the window (the
//!     ACSP escalation store, `enrichment_decisions`). A higher ratio means more
//!     autonomous agent work per human/broker escalation.
//!
//!   * **Trust Variance** — the dispersion of decision outcomes over the rolling
//!     window, as a Gini-Simpson index (`1 − Σ pᵢ²`) of the outcome categories,
//!     normalised to `[0, 1]`. Low dispersion ⇒ decisions cluster on one outcome
//!     (stable trust); high dispersion ⇒ outcomes scatter (volatile trust). This
//!     is the v1 outcome-category proxy for ADR-043's "rolling variance in
//!     decision quality / override rates".
//!
//! The other two (Mesh Velocity, HITL Precision) have no source event yet — the
//! REC-10 insight loop and the WP-4 case-queue HITL flag supply them later — so
//! the dashboard renders them honestly as "awaiting data source", never faked.
//!
//! Each compute persists a [`KpiSnapshotRow`] with its lineage (WP-8 AC3) and
//! fires `CANARY-VC-REC4-KPI` as observed live traffic.

use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, warn};
use serde::Serialize;

use crate::adapters::sqlite_enrichment_repository::{
    status_for_outcome, DecidedCaseRow, KpiDecisionRow, SqliteEnrichmentRepository,
};
use crate::adapters::sqlite_kpi_repository::{
    AgentTrajectoryRow, NewAgentTrajectory, NewKpiSnapshot, SqliteKpiRepository,
};
use crate::services::intent_match::intent_match;
use crate::services::liveness_harness::{LivenessHarness, CANARY_REC4_KPI};

/// The rolling window for both computed KPIs: 30 days (ADR-043 / the "30-day
/// rolling" Trust-Variance spec).
pub const KPI_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Sample size at which a KPI reaches full confidence. Below it, confidence
/// scales linearly with the sample count so a value computed from three events
/// is not presented with the authority of one computed from three hundred.
pub const FULL_CONFIDENCE_SAMPLE: u64 = 30;

/// The canonical KPI ids (the `kpi` column and the dashboard tile keys).
pub const KPI_AUGMENTATION_RATIO: &str = "augmentation_ratio";
pub const KPI_TRUST_VARIANCE: &str = "trust_variance";
pub const KPI_MESH_VELOCITY: &str = "mesh_velocity";
pub const KPI_HITL_PRECISION: &str = "hitl_precision";

/// The named derivation of HITL Precision, reported on the tile either way.
const HITL_SOURCE: &str = "warranted ÷ decided over human-decided enrichment cases \
     (outcome ≠ requested action, or amend/delegate, or intent_match == false); \
     system:whelk-gate outcomes excluded";

/// Cap on per-decision lineage rows written for one Trust-Variance snapshot, so a
/// pathologically large window cannot bloat the lineage table. The aggregate
/// per-outcome-category rows are always written in full.
const MAX_DECISION_LINEAGE_ROWS: usize = 1000;

// ---------------------------------------------------------------------------
// Pure computation (unit-tested against fixture rows)
// ---------------------------------------------------------------------------

/// Linear confidence in `[0, 1]` from a sample count.
pub fn sample_confidence(sample: u64) -> f64 {
    (sample as f64 / FULL_CONFIDENCE_SAMPLE as f64).min(1.0)
}

/// Augmentation Ratio = agent-action volume ÷ ACSP escalation volume.
///
/// Returns `(value, confidence)`. With zero escalations the ratio is undefined,
/// so it reports `(0.0, 0.0)` — a value with no confidence — rather than an
/// infinite or NaN number the dashboard would have to special-case.
pub fn augmentation_ratio(agent_volume: u64, escalation_volume: u64) -> (f64, f64) {
    if escalation_volume == 0 {
        return (0.0, 0.0);
    }
    let value = agent_volume as f64 / escalation_volume as f64;
    let sample = agent_volume.saturating_add(escalation_volume);
    (value, sample_confidence(sample))
}

/// Trust Variance = normalised Gini-Simpson dispersion of decision outcomes.
///
/// Returns `(value, confidence, sample_count)`. `value` is `0.0` when every
/// decision shares one outcome (no dispersion) and `1.0` at maximum spread
/// across the observed categories. Normalisation divides the raw Gini-Simpson
/// index by its theoretical maximum `1 − 1/k` (k = distinct outcomes) so the
/// figure is comparable regardless of how many categories appear.
pub fn trust_variance(outcomes: &[String]) -> (f64, f64, u64) {
    let n = outcomes.len();
    if n == 0 {
        return (0.0, 0.0, 0);
    }
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for o in outcomes {
        *counts.entry(o.as_str()).or_insert(0) += 1;
    }
    let total = n as f64;
    let sum_sq: f64 = counts
        .values()
        .map(|c| {
            let p = *c as f64 / total;
            p * p
        })
        .sum();
    let gini = 1.0 - sum_sq;
    let k = counts.len();
    let normalised = if k <= 1 {
        0.0
    } else {
        gini / (1.0 - 1.0 / k as f64)
    };
    (
        normalised.clamp(0.0, 1.0),
        sample_confidence(n as u64),
        n as u64,
    )
}

// ---------------------------------------------------------------------------
// HITL Precision (FR5.3/FR5.4, EXP-AC-005)
// ---------------------------------------------------------------------------

/// The reserved non-DID actor for outcomes produced by the EL++ consistency
/// gate (PRD "Identity" §: `system:whelk-gate`).
///
/// DDD invariant 8 — **system actors are not humans** — hangs off this constant:
/// a gate rejection is excluded from every human-reviewer series. Counting one
/// as a human override would inflate HITL Precision with work no human did and
/// scatter Trust Variance with dispersion no human produced.
pub const SYSTEM_WHELK_GATE: &str = "system:whelk-gate";

/// Is this decider a system actor rather than a human reviewer?
pub fn is_system_actor(decided_by: Option<&str>) -> bool {
    decided_by
        .map(|d| d.trim().eq_ignore_ascii_case(SYSTEM_WHELK_GATE))
        .unwrap_or(false)
}

/// The outcome series for a human-only KPI (Trust Variance). Rows resolved by a
/// system actor are dropped; an unattributed row is kept (an unattributed 31403
/// is still a human's, just not a claimed one).
pub fn human_decision_outcomes(rows: &[KpiDecisionRow]) -> Vec<String> {
    rows.iter()
        .filter(|r| !is_system_actor(r.decided_by.as_deref()))
        .map(|r| r.outcome.clone())
        .collect()
}

/// One decided case as HITL Precision reads it: what was asked, what was
/// answered, by whom, and whether the agent's declared intent held.
#[derive(Debug, Clone, PartialEq)]
pub struct DecidedCase {
    pub case_id: String,
    /// The human's (or gate's) outcome.
    pub outcome: String,
    /// The action the proposing agent requested.
    pub requested_action: String,
    /// The deciding identity; `system:whelk-gate` for a gate outcome.
    pub decided_by: Option<String>,
    /// True when some trajectory row for this case recorded `intent_match ==
    /// Some(false)` — the agent did not do what it declared.
    pub intent_mismatch: bool,
}

impl DecidedCase {
    /// Was escalating this case to a human WARRANTED? (DDD §3 "warranted
    /// escalation".) True when the human changed the outcome, did work only a
    /// human can do (`amend`/`delegate`), or the agent's act diverged from its
    /// declared intent. An approval that simply ratifies the request is NOT
    /// warranted — that is the escalation this KPI exists to find.
    pub fn is_warranted(&self) -> bool {
        if self.intent_mismatch {
            return true;
        }
        let outcome = self.outcome.trim().to_ascii_lowercase();
        if outcome.starts_with("amend") || outcome.starts_with("delegate") {
            return true;
        }
        // Compare OUTCOME FAMILIES, not spellings: "approved" answers a
        // requested "approve" — the human agreed, in a different word.
        status_for_outcome(&self.outcome) != status_for_outcome(&self.requested_action)
    }
}

/// HITL Precision with its denominator, always reported.
///
/// `value` is `None` — never `0.0` — when `decided == 0`: a ratio over an empty
/// denominator is undefined, and the dashboard must say so rather than render a
/// number (EXP-AC-005 counter-example "HITL precision reported as a value with
/// `decided == 0`").
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HitlPrecision {
    pub value: Option<f64>,
    pub warranted: u64,
    pub decided: u64,
}

/// Warranted ÷ decided over human-decided cases.
///
/// Cases resolved by a system actor are excluded from BOTH terms (DDD invariant
/// 8), so a window containing only gate rejections reports no value at all
/// rather than a precision no human earned.
pub fn hitl_precision(cases: &[DecidedCase]) -> HitlPrecision {
    let human: Vec<&DecidedCase> = cases
        .iter()
        .filter(|c| !is_system_actor(c.decided_by.as_deref()))
        .collect();
    let decided = human.len() as u64;
    let warranted = human.iter().filter(|c| c.is_warranted()).count() as u64;
    let value = if decided == 0 {
        None
    } else {
        Some(warranted as f64 / decided as f64)
    };
    HitlPrecision {
        value,
        warranted,
        decided,
    }
}

/// Does `urn` name `case_id` as a WHOLE delimited segment?
///
/// URNs are `:`/`/`-delimited, so the case id must occupy one or more complete
/// segments, not merely appear inside one. A plain `contains` would let
/// `vc-elev-foo` match a URN naming `vc-elev-foo-bar`, and — because these URNs
/// come off the agent-event envelope and are therefore agent-controlled — would
/// let an agent attach a deliberately mismatched intent to ANOTHER agent's case
/// and mark its escalation warranted. The boundary check closes that: an
/// embedded id must still start and end on a delimiter.
fn urn_names_case(urn: &str, case_id: &str) -> bool {
    let is_delim = |c: char| c == ':' || c == '/';
    urn.match_indices(case_id).any(|(i, _)| {
        let before_ok = i == 0 || urn[..i].chars().next_back().is_some_and(is_delim);
        let end = i + case_id.len();
        let after_ok = end == urn.len() || urn[end..].chars().next().is_some_and(is_delim);
        before_ok && after_ok
    })
}

/// Does this trajectory row belong to `case_id`?
///
/// The trace carries no foreign key onto cases; the correlation is the case id
/// appearing in the row's own URNs (the elevation actor mints a case id that
/// rides the target/source URN and the CTC handoff chain). Matched on whole
/// delimited segments — see [`urn_names_case`] for why a substring will not do.
pub fn trajectory_names_case(t: &AgentTrajectoryRow, case_id: &str) -> bool {
    if case_id.is_empty() {
        return false;
    }
    [
        t.target_urn.as_deref(),
        t.source_urn.as_deref(),
        t.handoff_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|u| urn_names_case(u, case_id))
}

/// Join decided cases to the trajectory rows that name them, stamping
/// `intent_mismatch` where an agent's declared intent did not hold.
pub fn decided_cases_with_intent(
    rows: &[DecidedCaseRow],
    trajectories: &[AgentTrajectoryRow],
) -> Vec<DecidedCase> {
    rows.iter()
        .map(|r| DecidedCase {
            case_id: r.case_id.clone(),
            outcome: r.outcome.clone(),
            requested_action: r.requested_action.clone(),
            decided_by: r.decided_by.clone(),
            intent_mismatch: trajectories.iter().any(|t| {
                trajectory_names_case(t, &r.case_id)
                    && intent_match(
                        t.intent.as_deref(),
                        t.action_type_name.as_deref(),
                        t.target_urn.as_deref(),
                    ) == Some(false)
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Summary shape (the GET /api/kpi/summary payload)
// ---------------------------------------------------------------------------

/// One dashboard tile. `status` is `"computed"` for a live KPI or
/// `"awaiting_data_source"` for one with no source event yet — the latter
/// carries only the named source, never a fabricated value.
#[derive(Debug, Clone, Serialize)]
pub struct KpiTile {
    pub kpi: String,
    pub label: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numerator: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denominator: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_days: Option<i64>,
    /// The named source event stream. Always present — for computed KPIs it
    /// documents the derivation; for awaiting KPIs it names what is missing.
    pub source: &'static str,
}

impl KpiTile {
    fn awaiting(kpi: &str, label: &str, source: &'static str) -> Self {
        Self {
            kpi: kpi.to_string(),
            label: label.to_string(),
            status: "awaiting_data_source",
            value: None,
            confidence: None,
            unit: None,
            numerator: None,
            denominator: None,
            sample_count: None,
            snapshot_id: None,
            window_days: None,
            source,
        }
    }
}

/// The full four-KPI summary the dashboard renders.
#[derive(Debug, Clone, Serialize)]
pub struct KpiSummary {
    pub tiles: Vec<KpiTile>,
    pub computed_at_ms: i64,
    pub window_days: i64,
    pub sha: String,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Computes the two live KPIs from real source events, persists snapshots with
/// lineage, and reports the four-tile summary. Cheap to clone via `Arc`.
pub struct KpiComputeService {
    kpi_repo: Arc<SqliteKpiRepository>,
    enrichment_repo: Arc<SqliteEnrichmentRepository>,
    harness: Arc<LivenessHarness>,
}

impl KpiComputeService {
    pub fn new(
        kpi_repo: Arc<SqliteKpiRepository>,
        enrichment_repo: Arc<SqliteEnrichmentRepository>,
        harness: Arc<LivenessHarness>,
    ) -> Self {
        Self {
            kpi_repo,
            enrichment_repo,
            harness,
        }
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    /// Compute both live KPIs over `[now - KPI_WINDOW_MS, now]` from real source
    /// events, persist a snapshot with lineage for each, fire `CANARY-VC-REC4-KPI`,
    /// and return the four-tile summary. This is the read path for
    /// `GET /api/kpi/summary`: a read computes fresh and persists, so the stored
    /// series always traces to the events that produced it.
    pub async fn compute_and_persist(&self) -> Result<KpiSummary, String> {
        let now = Self::now_ms();
        let window_start = now.saturating_sub(KPI_WINDOW_MS);
        let sha = crate::services::liveness_harness::current_sha();

        // --- source reads --------------------------------------------------
        let agent_volume =
            self.kpi_repo
                .count_agent_events_since(window_start)
                .await
                .map_err(|e| format!("agent-event volume read failed: {e}"))? as u64;

        let decisions = self
            .enrichment_repo
            .decisions_since(window_start)
            .await
            .map_err(|e| format!("decision read failed: {e}"))?;
        let escalation_volume = decisions.len() as u64;

        // FR5.3: HITL Precision reads DECIDED CASES (one row per case) joined to
        // the trajectory rows that name them, so an agent whose act diverged
        // from its declared intent marks its case as a warranted escalation.
        let decided_rows = self
            .enrichment_repo
            .decided_cases_since(window_start)
            .await
            .map_err(|e| format!("decided-case read failed: {e}"))?;
        let trajectories = self
            .kpi_repo
            .trajectories_since(window_start)
            .await
            .map_err(|e| format!("trajectory read failed: {e}"))?;
        let decided_cases = decided_cases_with_intent(&decided_rows, &trajectories);
        let hitl = hitl_precision(&decided_cases);

        // --- Augmentation Ratio -------------------------------------------
        let (ar_value, ar_conf) = augmentation_ratio(agent_volume, escalation_volume);
        let ar_snapshot = NewKpiSnapshot {
            kpi: KPI_AUGMENTATION_RATIO.into(),
            value: ar_value,
            confidence: ar_conf,
            numerator: Some(agent_volume as f64),
            denominator: Some(escalation_volume as f64),
            sample_count: (agent_volume + escalation_volume) as i64,
            window_start_ms: window_start,
            window_end_ms: now,
            computed_at_ms: now,
            sha: sha.clone(),
        };
        let ar_lineage = vec![
            (
                "agent_event_volume".to_string(),
                "wss/agent-events window count".to_string(),
                Some(agent_volume as f64),
            ),
            (
                "acsp_escalation".to_string(),
                "enrichment_decisions window count".to_string(),
                Some(escalation_volume as f64),
            ),
        ];
        let ar_id = self
            .kpi_repo
            .insert_snapshot_with_lineage(&ar_snapshot, &ar_lineage)
            .await
            .map_err(|e| format!("augmentation-ratio persist failed: {e}"))?;

        // --- Trust Variance -----------------------------------------------
        // FR5.4 / DDD invariant 8: the human-outcome series only. A Whelk-gate
        // rejection is not a human decision and must not scatter this index.
        let outcomes: Vec<String> = human_decision_outcomes(&decisions);
        let (tv_value, tv_conf, tv_sample) = trust_variance(&outcomes);

        // Lineage: one row per distinct outcome category (with its count) and one
        // row per contributing decision (its activity URN), so the value traces
        // back to the decision events (WP-8 AC3).
        let mut tv_lineage: Vec<(String, String, Option<f64>)> = Vec::new();
        let mut category_counts: HashMap<&str, f64> = HashMap::new();
        for outcome in &outcomes {
            *category_counts.entry(outcome.as_str()).or_insert(0.0) += 1.0;
        }
        for (category, count) in &category_counts {
            tv_lineage.push((
                "outcome_category".to_string(),
                (*category).to_string(),
                Some(*count),
            ));
        }
        for d in decisions
            .iter()
            .filter(|d| !is_system_actor(d.decided_by.as_deref()))
            .take(MAX_DECISION_LINEAGE_ROWS)
        {
            tv_lineage.push((
                "enrichment_decision".to_string(),
                d.activity_urn.clone(),
                Some(1.0),
            ));
        }
        let tv_snapshot = NewKpiSnapshot {
            kpi: KPI_TRUST_VARIANCE.into(),
            value: tv_value,
            confidence: tv_conf,
            numerator: None,
            denominator: None,
            sample_count: tv_sample as i64,
            window_start_ms: window_start,
            window_end_ms: now,
            computed_at_ms: now,
            sha: sha.clone(),
        };
        let tv_id = self
            .kpi_repo
            .insert_snapshot_with_lineage(&tv_snapshot, &tv_lineage)
            .await
            .map_err(|e| format!("trust-variance persist failed: {e}"))?;

        // --- fire the standing REC-4 canary on observed live traffic ------
        let evidence = format!(
            "KPI snapshots persisted: augmentation_ratio={ar_value:.3} (id={ar_id}, \
             agent_volume={agent_volume}, escalations={escalation_volume}), \
             trust_variance={tv_value:.3} (id={tv_id}, sample={tv_sample})"
        );
        if let Err(e) = self.harness.observe(CANARY_REC4_KPI, &evidence).await {
            warn!("[kpi] failed to record {CANARY_REC4_KPI} fire: {e}");
        } else {
            debug!("[kpi] {CANARY_REC4_KPI} fired: {evidence}");
        }

        // --- assemble the four-tile summary -------------------------------
        let ar_tile = KpiTile {
            kpi: KPI_AUGMENTATION_RATIO.to_string(),
            label: "Augmentation Ratio".to_string(),
            status: "computed",
            value: Some(ar_value),
            confidence: Some(ar_conf),
            unit: Some("ratio"),
            numerator: Some(agent_volume as f64),
            denominator: Some(escalation_volume as f64),
            sample_count: Some((agent_volume + escalation_volume) as i64),
            snapshot_id: Some(ar_id),
            window_days: Some(KPI_WINDOW_MS / (24 * 60 * 60 * 1000)),
            source: "agent-action volume (/wss/agent-events) ÷ ACSP escalation volume (enrichment_decisions)",
        };
        let tv_tile = KpiTile {
            kpi: KPI_TRUST_VARIANCE.to_string(),
            label: "Trust Variance".to_string(),
            status: "computed",
            value: Some(tv_value),
            confidence: Some(tv_conf),
            unit: Some("index"),
            numerator: None,
            denominator: None,
            sample_count: Some(tv_sample as i64),
            snapshot_id: Some(tv_id),
            window_days: Some(KPI_WINDOW_MS / (24 * 60 * 60 * 1000)),
            source: "Gini-Simpson dispersion of enrichment_decisions outcomes (30-day rolling)",
        };
        let mesh_tile = KpiTile::awaiting(
            KPI_MESH_VELOCITY,
            "Mesh Velocity",
            "REC-10 insight-loop timestamps (ontology_propose → broker decision → merged enrichment)",
        );
        // --- HITL Precision ------------------------------------------------
        // FR5.3: a real computation over decided cases, persisted with the same
        // lineage discipline as the other live KPIs. With a zero denominator the
        // tile reports `no_decided_cases` and carries NO value — never a number.
        let hitl_tile = if hitl.decided == 0 {
            KpiTile {
                kpi: KPI_HITL_PRECISION.to_string(),
                label: "HITL Precision".to_string(),
                status: "no_decided_cases",
                value: None,
                confidence: None,
                unit: Some("ratio"),
                numerator: Some(hitl.warranted as f64),
                denominator: Some(0.0),
                sample_count: Some(0),
                snapshot_id: None,
                window_days: Some(KPI_WINDOW_MS / (24 * 60 * 60 * 1000)),
                source: HITL_SOURCE,
            }
        } else {
            let hitl_value = hitl.value.unwrap_or(0.0);
            let hitl_conf = sample_confidence(hitl.decided);
            let hitl_snapshot = NewKpiSnapshot {
                kpi: KPI_HITL_PRECISION.into(),
                value: hitl_value,
                confidence: hitl_conf,
                numerator: Some(hitl.warranted as f64),
                denominator: Some(hitl.decided as f64),
                sample_count: hitl.decided as i64,
                window_start_ms: window_start,
                window_end_ms: now,
                computed_at_ms: now,
                sha: sha.clone(),
            };
            let mut hitl_lineage: Vec<(String, String, Option<f64>)> = Vec::new();
            for c in decided_cases
                .iter()
                .filter(|c| !is_system_actor(c.decided_by.as_deref()))
                .take(MAX_DECISION_LINEAGE_ROWS)
            {
                hitl_lineage.push((
                    if c.is_warranted() {
                        "warranted_escalation".to_string()
                    } else {
                        "unwarranted_escalation".to_string()
                    },
                    c.case_id.clone(),
                    Some(1.0),
                ));
            }
            let hitl_id = self
                .kpi_repo
                .insert_snapshot_with_lineage(&hitl_snapshot, &hitl_lineage)
                .await
                .map_err(|e| format!("hitl-precision persist failed: {e}"))?;
            KpiTile {
                kpi: KPI_HITL_PRECISION.to_string(),
                label: "HITL Precision".to_string(),
                status: "computed",
                value: Some(hitl_value),
                confidence: Some(hitl_conf),
                unit: Some("ratio"),
                numerator: Some(hitl.warranted as f64),
                denominator: Some(hitl.decided as f64),
                sample_count: Some(hitl.decided as i64),
                snapshot_id: Some(hitl_id),
                window_days: Some(KPI_WINDOW_MS / (24 * 60 * 60 * 1000)),
                source: HITL_SOURCE,
            }
        };

        Ok(KpiSummary {
            tiles: vec![ar_tile, tv_tile, mesh_tile, hitl_tile],
            computed_at_ms: now,
            window_days: KPI_WINDOW_MS / (24 * 60 * 60 * 1000),
            sha,
        })
    }

    /// Lineage rows for a persisted snapshot (`GET /api/kpi/lineage/{id}`).
    pub async fn lineage_for(
        &self,
        snapshot_id: i64,
    ) -> Result<Vec<crate::adapters::sqlite_kpi_repository::KpiLineageRow>, String> {
        self.kpi_repo
            .lineage_for(snapshot_id)
            .await
            .map_err(|e| format!("lineage read failed: {e}"))
    }
}

/// Passive agent-action volume tap. Subscribes to the process-global
/// `/wss/agent-events` hub (the same seam the render actor uses) and records one
/// lightweight volume row per envelope. No change to the emit site or the wire —
/// the volume is read from an existing stream (ADR-130 D5). Never returns;
/// fail-open on a lagged/closed channel.
pub async fn run_agent_event_tap(kpi_repo: Arc<SqliteKpiRepository>) {
    let mut rx = crate::agent_events::hub::subscribe();
    log::info!("[kpi] agent-event volume tap subscribed to the agent-events hub");
    loop {
        match rx.recv().await {
            Ok(env) => {
                let observed_at_ms = chrono::Utc::now().timestamp_millis();
                // REC-11: derive the did:nostr attribution from the envelope's
                // pubkey (a valid x-only hex ⇒ did:nostr:<pubkey>), falling back
                // to a source_urn that is already a did:nostr. Anonymous frames
                // record a NULL agent_did and still count toward volume.
                let agent_did = env
                    .pubkey
                    .as_deref()
                    .filter(|pk| crate::uri::is_pubkey_hex(pk))
                    .and_then(|pk| crate::uri::did_nostr(pk).ok())
                    .or_else(|| {
                        env.source_urn
                            .as_deref()
                            .filter(|u| u.starts_with("did:nostr:"))
                            .map(|u| u.to_string())
                    });
                let trajectory = NewAgentTrajectory {
                    event_id: env.id,
                    source_agent_id: env.source_agent_id,
                    action_type: env.action_type,
                    action_type_name: Some(env.action_type_name.clone()),
                    agent_did,
                    source_urn: env.source_urn.clone(),
                    target_urn: env.target_urn.clone(),
                    handoff_id: env.handoff_id.clone(),
                    token_count: env.token_count,
                    verification: env.verification.clone(),
                    // FR5.1 (EXP-AC-005): the DECLARED intent, exactly as the
                    // producer wrote it. `None` persists as NULL — the tap never
                    // invents one, so `intent_match` stays a real comparison.
                    intent: env.intent.clone(),
                    observed_at_ms,
                };
                if let Err(e) = kpi_repo.record_agent_trajectory(&trajectory).await {
                    warn!("[kpi] failed to record agent-event trajectory: {e}");
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                // Volume is a count; a dropped frame under backpressure only
                // undercounts slightly. Resync on the next frame.
                debug!("[kpi] agent-event tap lagged {n} frames");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                warn!("[kpi] agent-event hub closed; volume tap stopping");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augmentation_ratio_divides_volume_by_escalations() {
        // 42 agent actions, 12 escalations ⇒ 3.5.
        let (value, conf) = augmentation_ratio(42, 12);
        assert!((value - 3.5).abs() < 1e-9);
        assert!(
            (conf - 1.0).abs() < 1e-9,
            "54 samples ≥ 30 ⇒ full confidence"
        );
    }

    #[test]
    fn augmentation_ratio_zero_escalations_is_zero_confidence() {
        let (value, conf) = augmentation_ratio(100, 0);
        assert_eq!(value, 0.0);
        assert_eq!(conf, 0.0, "an undefined ratio carries no confidence");
    }

    #[test]
    fn augmentation_ratio_low_sample_scales_confidence() {
        let (value, conf) = augmentation_ratio(3, 3);
        assert!((value - 1.0).abs() < 1e-9);
        assert!(
            (conf - 6.0 / 30.0).abs() < 1e-9,
            "6 samples ⇒ 0.2 confidence"
        );
    }

    #[test]
    fn trust_variance_uniform_outcome_is_zero() {
        let outcomes = vec!["approve".to_string(); 10];
        let (value, _conf, sample) = trust_variance(&outcomes);
        assert_eq!(value, 0.0, "all one outcome ⇒ no dispersion");
        assert_eq!(sample, 10);
    }

    #[test]
    fn trust_variance_even_split_is_maximal() {
        let mut outcomes = vec!["approve".to_string(); 5];
        outcomes.extend(vec!["reject".to_string(); 5]);
        let (value, _conf, sample) = trust_variance(&outcomes);
        assert!(
            (value - 1.0).abs() < 1e-9,
            "even 50/50 split ⇒ normalised 1.0"
        );
        assert_eq!(sample, 10);
    }

    #[test]
    fn trust_variance_three_way_even_is_maximal() {
        let mut outcomes = vec!["approve".to_string(); 4];
        outcomes.extend(vec!["reject".to_string(); 4]);
        outcomes.extend(vec!["amend".to_string(); 4]);
        let (value, _conf, sample) = trust_variance(&outcomes);
        // Even spread across k=3 categories ⇒ normalised to 1.0.
        assert!((value - 1.0).abs() < 1e-9);
        assert_eq!(sample, 12);
    }

    #[test]
    fn trust_variance_skewed_is_between_zero_and_one() {
        // 9 approve, 1 reject ⇒ some but low dispersion.
        let mut outcomes = vec!["approve".to_string(); 9];
        outcomes.push("reject".to_string());
        let (value, _conf, _sample) = trust_variance(&outcomes);
        assert!(
            value > 0.0 && value < 0.5,
            "skewed split ⇒ low dispersion, got {value}"
        );
    }

    // -----------------------------------------------------------------
    // FR5.3/FR5.4 (EXP-AC-005): HITL Precision over real decided cases, and
    // the exclusion of the Whelk gate's non-human outcomes.
    // -----------------------------------------------------------------

    fn decided(case_id: &str, outcome: &str, requested: &str) -> DecidedCase {
        DecidedCase {
            case_id: case_id.into(),
            outcome: outcome.into(),
            requested_action: requested.into(),
            decided_by: Some("aaaa".into()),
            intent_mismatch: false,
        }
    }

    #[test]
    fn hitl_precision_with_no_decided_cases_is_a_value_of_none() {
        let p = hitl_precision(&[]);
        assert_eq!(p.decided, 0);
        assert_eq!(p.warranted, 0);
        assert_eq!(p.value, None, "never a number over a zero denominator");
    }

    #[test]
    fn an_outcome_matching_the_request_is_not_warranted() {
        let p = hitl_precision(&[decided("c1", "approve", "approve")]);
        assert_eq!(p.decided, 1);
        assert_eq!(p.warranted, 0);
        assert_eq!(p.value, Some(0.0));
    }

    #[test]
    fn an_outcome_differing_from_the_request_is_warranted() {
        let p = hitl_precision(&[decided("c1", "reject", "approve")]);
        assert_eq!(p.warranted, 1);
        assert_eq!(p.decided, 1);
        assert_eq!(p.value, Some(1.0));
    }

    #[test]
    fn outcome_spellings_of_one_family_are_not_an_override() {
        // "approved" answers a requested "approve": the human agreed.
        let p = hitl_precision(&[decided("c1", "approved", "approve")]);
        assert_eq!(p.warranted, 0);
    }

    #[test]
    fn amend_and_delegate_are_always_warranted() {
        let p = hitl_precision(&[decided("c1", "amend", "amend"), decided("c2", "delegate", "delegate")]);
        assert_eq!(p.warranted, 2, "amend and delegate are human work by definition");
        assert_eq!(p.decided, 2);
    }

    #[test]
    fn an_intent_mismatch_makes_an_agreeing_decision_warranted() {
        let mut c = decided("c1", "approve", "approve");
        c.intent_mismatch = true;
        let p = hitl_precision(&[c]);
        assert_eq!(p.warranted, 1, "the agent did not do what it declared");
    }

    #[test]
    fn whelk_gate_rejections_are_not_human_decisions() {
        let mut gate = decided("c1", "reject", "approve");
        gate.decided_by = Some(SYSTEM_WHELK_GATE.to_string());
        let human = decided("c2", "approve", "approve");
        let p = hitl_precision(&[gate, human]);
        assert_eq!(p.decided, 1, "only the human case counts toward the denominator");
        assert_eq!(p.warranted, 0);
        assert_eq!(p.value, Some(0.0));
    }

    #[test]
    fn a_gate_only_window_reports_no_value_rather_than_zero() {
        let mut gate = decided("c1", "reject", "approve");
        gate.decided_by = Some(SYSTEM_WHELK_GATE.to_string());
        let p = hitl_precision(&[gate]);
        assert_eq!(p.decided, 0);
        assert_eq!(p.value, None);
    }

    #[test]
    fn precision_is_warranted_over_decided() {
        let cases = vec![
            decided("c1", "reject", "approve"),
            decided("c2", "approve", "approve"),
            decided("c3", "approve", "approve"),
            decided("c4", "approve", "approve"),
        ];
        let p = hitl_precision(&cases);
        assert_eq!((p.warranted, p.decided), (1, 4));
        assert!((p.value.unwrap() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn trust_variance_human_series_excludes_the_whelk_gate() {
        let rows = vec![
            KpiDecisionRow {
                outcome: "reject".into(),
                activity_urn: "urn:1".into(),
                decided_at_ms: 1,
                decided_by: Some(SYSTEM_WHELK_GATE.into()),
            },
            KpiDecisionRow {
                outcome: "approve".into(),
                activity_urn: "urn:2".into(),
                decided_at_ms: 2,
                decided_by: Some("aaaa".into()),
            },
            KpiDecisionRow {
                outcome: "approve".into(),
                activity_urn: "urn:3".into(),
                decided_at_ms: 3,
                decided_by: None,
            },
        ];
        let human = human_decision_outcomes(&rows);
        assert_eq!(human, vec!["approve".to_string(), "approve".to_string()]);
        // Without the exclusion this series would show spurious dispersion.
        let (value, _conf, sample) = trust_variance(&human);
        assert_eq!(sample, 2);
        assert_eq!(value, 0.0, "two agreeing humans ⇒ no dispersion");
    }

    #[test]
    fn a_case_is_correlated_only_on_whole_urn_segments() {
        // deepsec `other-metric-integrity`: these URNs are agent-controlled, so
        // a substring match would let an agent attach a mismatched intent to
        // ANOTHER case and mark that case's escalation warranted.
        assert!(urn_names_case("urn:visionclaw:case:vc-elev-foo", "vc-elev-foo"));
        assert!(urn_names_case("vc-elev-foo", "vc-elev-foo"));
        assert!(urn_names_case("urn:vc-elev-foo:step-2", "vc-elev-foo"));
        assert!(urn_names_case("a/vc-elev-foo/b", "vc-elev-foo"));

        assert!(!urn_names_case("urn:visionclaw:case:vc-elev-foo-bar", "vc-elev-foo"));
        assert!(!urn_names_case("urn:visionclaw:case:xvc-elev-foo", "vc-elev-foo"));
        assert!(!urn_names_case("urn:visionclaw:case:other", "vc-elev-foo"));
    }

    #[test]
    fn an_intent_mismatch_on_a_neighbouring_case_does_not_bleed_across() {
        let row = |target: &str, intent: &str| AgentTrajectoryRow {
            event_id: 1,
            source_agent_id: 1,
            action_type: 1,
            action_type_name: Some("graph_update".into()),
            agent_did: None,
            source_urn: None,
            target_urn: Some(target.into()),
            handoff_id: None,
            token_count: None,
            verification: None,
            intent: Some(intent.into()),
            observed_at_ms: 1,
        };
        // A trajectory naming `vc-elev-foo-bar` must not mark `vc-elev-foo`.
        let trajectories = vec![row(
            "urn:visionclaw:case:vc-elev-foo-bar",
            "delete urn:visionclaw:case:vc-elev-foo-bar",
        )];
        let rows = vec![DecidedCaseRow {
            case_id: "vc-elev-foo".into(),
            outcome: "approve".into(),
            requested_action: "approve".into(),
            decided_by: Some("aaaa".into()),
            decided_at_ms: 1,
        }];
        let joined = decided_cases_with_intent(&rows, &trajectories);
        assert!(!joined[0].intent_mismatch);
        assert_eq!(hitl_precision(&joined).warranted, 0);
    }

    #[test]
    fn an_intent_mismatch_on_the_cases_own_trajectory_does_count() {
        let trajectories = vec![AgentTrajectoryRow {
            event_id: 1,
            source_agent_id: 1,
            action_type: 1,
            action_type_name: Some("graph_update".into()),
            agent_did: None,
            source_urn: None,
            target_urn: Some("urn:visionclaw:case:vc-elev-foo".into()),
            handoff_id: None,
            token_count: None,
            verification: None,
            intent: Some("delete urn:visionclaw:case:vc-elev-other".into()),
            observed_at_ms: 1,
        }];
        let rows = vec![DecidedCaseRow {
            case_id: "vc-elev-foo".into(),
            outcome: "approve".into(),
            requested_action: "approve".into(),
            decided_by: Some("aaaa".into()),
            decided_at_ms: 1,
        }];
        let joined = decided_cases_with_intent(&rows, &trajectories);
        assert!(joined[0].intent_mismatch, "the agent did not do what it declared");
        assert_eq!(hitl_precision(&joined).warranted, 1);
    }

    #[test]
    fn is_system_actor_recognises_only_the_reserved_gate_identity() {
        assert!(is_system_actor(Some(SYSTEM_WHELK_GATE)));
        assert!(is_system_actor(Some("SYSTEM:WHELK-GATE")));
        assert!(!is_system_actor(Some("did:nostr:aaaa")));
        assert!(!is_system_actor(None));
    }

    #[test]
    fn trust_variance_empty_is_zero() {
        let (value, conf, sample) = trust_variance(&[]);
        assert_eq!(value, 0.0);
        assert_eq!(conf, 0.0);
        assert_eq!(sample, 0);
    }
}
