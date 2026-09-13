// src/services/liveness_harness.rs
//! LivenessHarness — the sprint-wide live-traffic observer (RES-a, ADR-130 D3).
//!
//! A canary registers a wire; the harness records a `CanaryFired` only when real
//! traffic crosses that wire. It is NOT a synthetic prober — a green ping never
//! stands in for an observation (DDD invariant 5). The harness backs three HTTP
//! surfaces (`register`/`observe`/`status`, see
//! [`crate::handlers::liveness_harness_handler`]) and drives the KG-backend
//! watchdog below.
//!
//! ## KG watchdog
//!
//! VisionClaw's own server IS the KG backend (port 4000). [`run_kg_watchdog`]
//! is a tokio interval task that self-polls `/api/health` and drives the
//! `kg_backend_up` gauge held on the harness ([`LivenessHarness::kg_backend_up`]).
//! Every state transition — including the first `unknown → up`, which proves the
//! watchdog is live, and any later `up → down` on backend loss — fires
//! [`CANARY_KG`]. Fail-open: a poll failure marks the backend down (and fires),
//! it never panics or blocks the runtime.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use log::{error, info, warn};

use crate::adapters::sqlite_canary_repository::{
    CanaryRegistration, CanaryStatus, Result as CanaryResult, SqliteCanaryRepository,
};

/// The KG-backend liveness canary the watchdog fires on gauge transitions.
pub const CANARY_KG: &str = "CANARY-VC-RESA-KG";

/// The broker case-queue round-trip canary (REC-2 / D3). Fires from the
/// enrichment-decision path when a queued case reaches a decision over live
/// traffic (`broker:new_case` → `broker:case_decided`).
pub const CANARY_REC2_CASE: &str = "CANARY-VC-REC2-CASE";

/// Freshness window for the staleness rule (WP-11): a fire older than this
/// re-arms its canary. 30 days per the canon default.
pub const FRESHNESS_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// The P0-wave canaries this repository seeds at start-up (PRD-023 canary
/// table). Tuple: `(canary_id, description, kind, wave)`. Registration is
/// idempotent, so re-seeding on every boot is safe.
pub const P0_CANARIES: &[(&str, &str, &str, &str)] = &[
    (
        "CANARY-VC-COM14-DID",
        "Selected node addressable by a verified did:nostr (Schnorr challenge at selection)",
        "standing",
        "P0",
    ),
    (
        CANARY_REC2_CASE,
        "broker:new_case then broker:case_decided on the multiplexed graph socket",
        "standing",
        "P0",
    ),
    (
        "CANARY-VC-D5-WS",
        "WS status dot transitions to disconnected on a real socket drop",
        "one-shot",
        "P0",
    ),
    (
        "CANARY-VC-M1-HUD",
        "Godot avatar renders a verified DID badge in an xr-runtime session",
        "standing",
        "P0",
    ),
    (
        CANARY_KG,
        "kg_backend_up gauge transition on the watchdog self-poll of /api/health",
        "standing",
        "P0",
    ),
    (
        "CANARY-VC-REC1-ROUTE",
        "Route dump shows no unauthenticated ontology ingest route (auth gates hold)",
        "one-shot",
        "P0",
    ),
];

/// The governed-voice-loop canary (COM-15 / V1 / D6 / M5, PRD-023 WP-5). Fires
/// on the live end-to-end: a spoken command bound to the selected agent's
/// `did:nostr` → a signed 31402 accepted by agentbox `/v1/voice-intent` → a
/// PocketTts TTS acknowledgement. Standing (P1).
pub const CANARY_COM15_PTT: &str = "CANARY-VC-COM15-PTT";

/// The steering-surface canary (D2, PRD-023 WP-3). Fires when a steer action
/// (`/bots/submit-task` or `/bots/interrupt`) is invoked from a mounted
/// per-agent panel — the route handler observes it as live traffic, so a fire
/// means node selection opened a working steering control. Standing (P1).
pub const CANARY_D2_STEER: &str = "CANARY-VC-D2-STEER";

/// The swarm-observability canary (D8, PRD-023 WP-3). Fires when the aggregate
/// swarm dashboard mounts with live poll data — the client observes it over
/// `POST /api/canary/observe/{id}`. One-shot (P1).
pub const CANARY_D8_OBS: &str = "CANARY-VC-D8-OBS";

/// The embodiment-join canary (D1, PRD-023 WP-2). Fires when a live agent action
/// reaches the client data path — a non-empty `/api/bots/agents` roster and/or a
/// decoded `0x23` `AGENT_ACTION` beam frame. Observed by the sprint-end
/// `scripts/canary/d1-beam-check.sh` against a running stack. Standing (P1).
pub const CANARY_D1_BEAM: &str = "CANARY-VC-D1-BEAM";

/// The contextual-transaction-cost canary (REC-3, PRD-023 WP-7). Fires from the
/// `/wss/agent-events` ingest when an envelope carrying a populated typed CTC
/// field ([`crate::agent_events::AgentActionEnvelope::has_ctc`]) crosses the
/// wire. One-shot (P1) — the schema contract is proven once a live DAG emits it.
pub const CANARY_REC3_CTC: &str = "CANARY-VC-REC3-CTC";

/// The four-KPI-dashboard canary (REC-4, PRD-023 WP-8). Fires each time the KPI
/// compute service persists a snapshot computed from real source events
/// (agent-events volume, enrichment decision outcomes). Standing (P1).
pub const CANARY_REC4_KPI: &str = "CANARY-VC-REC4-KPI";

/// The ontology class-count canary (RES-d, PRD-023 WP-12). Fires when the
/// class-count route returns a live count read from Oxigraph — the source the
/// canon `DriftCounter` consumes. One-shot (P1).
pub const CANARY_RESD_COUNT: &str = "CANARY-VC-RESD-COUNT";

/// The voice conversational-repair canary (V3, PRD-023 WP-10). Fires when the
/// confidence gate holds a low-confidence / under-specified spoken command and
/// speaks a clarification instead of dispatching — a clarification turn observed
/// on live traffic (`speech_socket_handler::process_governed_voice`). One-shot
/// (P2): the repair loop is proven once a real utterance is held and clarified.
pub const CANARY_V3_REPAIR: &str = "CANARY-VC-V3-REPAIR";

/// The insight-ingestion-loop canary (REC-10, PRD-023 WP-12). Fires when the
/// loop-trace read returns a loop that closed once end to end —
/// `ontology_propose` → broker decision → merged enrichment with monotonic
/// per-stage timestamps, so Mesh Velocity (insight-to-integration time) is
/// computable. One-shot (P2): the loop is proven once it closes across the mesh.
pub const CANARY_REC10_LOOP: &str = "CANARY-VC-REC10-LOOP";

/// The data-moat unified-trace canary (REC-11, PRD-023 WP-12). Fires when the
/// `GET /api/trace` query returns a trace that joins at least two live source
/// kinds under one `did:nostr` (agent-events / hook-trajectory + broker
/// decisions; pod git-marks incorporated when a `--features git` pod supplies
/// them). One-shot (P2): the join is proven once it spans two live sources.
pub const CANARY_REC11_TRACE: &str = "CANARY-VC-REC11-TRACE";

/// The MR-targeting canary (M4, PRD-023 WP-9). Fires when a controller or
/// head-gaze ray resolves a non-origin agent-node selection in the
/// agentbox/xr-runtime Monado sidecar — observed over `POST
/// /api/canary/observe/{id}` from the on-device session. One-shot (P2).
pub const CANARY_M4_RAY: &str = "CANARY-VC-M4-RAY";

/// The MR-intervention canary (COM-18 / M2, PRD-023 WP-9). Fires when the
/// in-headset intervention panel emits a signed NIP-98 decide (kind
/// 31402/31403) accepted by the shared `POST /api/broker/cases/{id}/decide`
/// route — the server observes it as live traffic. Standing (P2).
pub const CANARY_COM18_INTERV: &str = "CANARY-VC-COM18-INTERV";

/// The P1-wave canaries this repository seeds at start-up (PRD-023 canary
/// table). Idempotent, so re-seeding on every boot is safe. Kept separate from
/// [`P0_CANARIES`] so each wave's rows stay legible; more P1 rows land as their
/// items close.
pub const P1_CANARIES: &[(&str, &str, &str, &str)] = &[
    (
        CANARY_COM15_PTT,
        "Spoken command bound to the selected agent → signed 31402 accepted by \
         /v1/voice-intent → PocketTts TTS acknowledgement",
        "standing",
        "P1",
    ),
    (
        CANARY_D2_STEER,
        "Steer action (/bots/submit-task or /bots/interrupt) invoked from a \
         mounted per-agent panel",
        "standing",
        "P1",
    ),
    (
        CANARY_D8_OBS,
        "Aggregate swarm-observability dashboard mounted with live poll data",
        "one-shot",
        "P1",
    ),
    (
        CANARY_D1_BEAM,
        "0x23 AGENT_ACTION beam frame / non-empty /api/bots/agents roster — live \
         agent activity reaching the client data path",
        "standing",
        "P1",
    ),
    (
        CANARY_REC3_CTC,
        "Agent-events envelope carrying a populated typed CTC field \
         (token_count / handoff_id / verification — agentbox canonical names)",
        "one-shot",
        "P1",
    ),
    (
        CANARY_REC4_KPI,
        "KPI snapshot computed from real source events (agent-events volume, \
         enrichment decision outcomes), persisted and pushed to the dashboard",
        "standing",
        "P1",
    ),
    (
        CANARY_RESD_COUNT,
        "Class-count route returns a live ontology class count read from Oxigraph \
         (the canon DriftCounter source)",
        "one-shot",
        "P1",
    ),
];

/// The P2-wave canaries this repository seeds at start-up (PRD-023 canary
/// table). Idempotent. Kept separate from the earlier waves so each wave's rows
/// stay legible; more P2 rows land as their items close.
///
/// The third M-item canary, `CANARY-VC-M1-HUD` (the in-headset verified-DID
/// badge render), is intentionally **not** listed here: the PRD-023 canary
/// table assigns it to wave **P0**, so it is seeded by [`P0_CANARIES`]. Wave
/// assignment is a canon-register property (ADR-130 "What This ADR Does Not
/// Decide"); duplicating M1-HUD into P2 would contradict it. The full M-item
/// set is therefore M1-HUD (P0) + M4-RAY + COM18-INTERV (P2), all seeded.
pub const P2_CANARIES: &[(&str, &str, &str, &str)] = &[
    (
        CANARY_M4_RAY,
        "Controller or head-gaze ray resolves a non-origin agent-node selection \
         in the xr-runtime Monado sidecar",
        "one-shot",
        "P2",
    ),
    (
        CANARY_COM18_INTERV,
        "In-headset intervention panel emits a signed NIP-98 decide \
         (31402/31403) accepted by /api/broker/cases/{id}/decide",
        "standing",
        "P2",
    ),
    (
        CANARY_V3_REPAIR,
        "Confidence gate holds a low-confidence / under-specified spoken command \
         and speaks a clarification instead of dispatching (a repair turn)",
        "one-shot",
        "P2",
    ),
    (
        CANARY_REC10_LOOP,
        "Insight loop closed once end to end: ontology_propose → broker decision \
         → merged enrichment with monotonic per-stage timestamps (Mesh Velocity \
         computable)",
        "one-shot",
        "P2",
    ),
    (
        CANARY_REC11_TRACE,
        "GET /api/trace returns a trace joining ≥2 live source kinds under one \
         did:nostr (agent-events + broker decisions; pod git-marks when present)",
        "one-shot",
        "P2",
    ),
];

// KG gauge tri-state (an atomic 3-valued gauge, mirroring the AtomicUsize
// counter idiom used for `active_connections`).
const KG_UNKNOWN: u8 = 0;
const KG_UP: u8 = 1;
const KG_DOWN: u8 = 2;

fn kg_label(state: u8) -> &'static str {
    match state {
        KG_UP => "up",
        KG_DOWN => "down",
        _ => "unknown",
    }
}

/// The git SHA the running binary is bound to. Runtime `VISIONCLAW_GIT_SHA`
/// overrides the build-time value embedded by `build.rs`; falls back to
/// `"unknown"`. Used to bind a canary fire to the commit it fired at.
pub fn current_sha() -> String {
    std::env::var("VISIONCLAW_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| option_env!("VISIONCLAW_GIT_SHA").map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The central live-traffic observer. Cheap to clone via `Arc`; the KG gauge is
/// a lock-free atomic so the watchdog and the status handler share it directly.
pub struct LivenessHarness {
    repo: Arc<SqliteCanaryRepository>,
    kg_state: AtomicU8,
}

impl LivenessHarness {
    /// Build a harness over an opened canary store.
    pub fn new(repo: Arc<SqliteCanaryRepository>) -> Self {
        Self {
            repo,
            kg_state: AtomicU8::new(KG_UNKNOWN),
        }
    }

    /// Register (idempotently) a canary declaration from any repository.
    pub async fn register(&self, reg: &CanaryRegistration) -> CanaryResult<()> {
        self.repo.register(reg).await
    }

    /// Record a fire on a registered canary from observed live traffic. Binds
    /// the fire to [`current_sha`] and the wall clock.
    pub async fn observe(&self, canary_id: &str, evidence: &str) -> CanaryResult<i64> {
        self.repo
            .observe(canary_id, evidence, &current_sha(), now_ms())
            .await
    }

    /// Per-canary status applying the 30-day/SHA staleness rule.
    pub async fn status(&self) -> CanaryResult<Vec<CanaryStatus>> {
        self.repo
            .all_status(&current_sha(), now_ms(), FRESHNESS_WINDOW_MS)
            .await
    }

    /// Idempotently seed the P0-wave canaries so the watchdog's target exists
    /// and the harness is immediately queryable at boot.
    pub async fn seed_p0_canaries(&self) -> CanaryResult<()> {
        let sha = current_sha();
        let at = now_ms();
        for (id, description, kind, wave) in P0_CANARIES {
            self.register(&CanaryRegistration {
                canary_id: (*id).to_string(),
                description: (*description).to_string(),
                kind: (*kind).to_string(),
                owner_repo: "visionclaw".to_string(),
                wave: Some((*wave).to_string()),
                sha_at_registration: sha.clone(),
                registered_at_ms: at,
            })
            .await?;
        }
        info!(
            "[liveness] seeded {} P0 canaries at sha={}",
            P0_CANARIES.len(),
            sha
        );
        Ok(())
    }

    /// Idempotently seed the P1-wave canaries (COM-15 governed voice loop, and
    /// any later P1 rows). Registration is idempotent; a live fire is recorded
    /// separately via [`Self::observe`] on the standing wire.
    pub async fn seed_p1_canaries(&self) -> CanaryResult<()> {
        let sha = current_sha();
        let at = now_ms();
        for (id, description, kind, wave) in P1_CANARIES {
            self.register(&CanaryRegistration {
                canary_id: (*id).to_string(),
                description: (*description).to_string(),
                kind: (*kind).to_string(),
                owner_repo: "visionclaw".to_string(),
                wave: Some((*wave).to_string()),
                sha_at_registration: sha.clone(),
                registered_at_ms: at,
            })
            .await?;
        }
        info!(
            "[liveness] seeded {} P1 canaries at sha={}",
            P1_CANARIES.len(),
            sha
        );
        Ok(())
    }

    /// Idempotently seed the P2-wave canaries (MR copresence: M4 targeting ray,
    /// COM-18/M2 in-headset intervention; V3 voice repair; REC-10 insight loop;
    /// REC-11 unified trace). Registration is idempotent; a live fire is
    /// recorded separately via [`Self::observe`] when the live wire crosses —
    /// for the MR rows that is the on-device xr-runtime session. The M1-HUD
    /// canary rides [`P0_CANARIES`] per the register's wave assignment (see
    /// [`P2_CANARIES`]).
    pub async fn seed_p2_canaries(&self) -> CanaryResult<()> {
        let sha = current_sha();
        let at = now_ms();
        for (id, description, kind, wave) in P2_CANARIES {
            self.register(&CanaryRegistration {
                canary_id: (*id).to_string(),
                description: (*description).to_string(),
                kind: (*kind).to_string(),
                owner_repo: "visionclaw".to_string(),
                wave: Some((*wave).to_string()),
                sha_at_registration: sha.clone(),
                registered_at_ms: at,
            })
            .await?;
        }
        info!(
            "[liveness] seeded {} P2 canaries at sha={}",
            P2_CANARIES.len(),
            sha
        );
        Ok(())
    }

    /// The `kg_backend_up` gauge: `None` until the first poll, then `Some(true)`
    /// / `Some(false)`.
    pub fn kg_backend_up(&self) -> Option<bool> {
        match self.kg_state.load(Ordering::SeqCst) {
            KG_UP => Some(true),
            KG_DOWN => Some(false),
            _ => None,
        }
    }

    /// Drive the gauge from a watchdog observation. On a state TRANSITION only,
    /// fires [`CANARY_KG`] with the transition as evidence and returns `true`.
    /// A repeat of the current state is a no-op (returns `false`) — the canary
    /// fires on observed change, not on every tick.
    pub async fn record_kg_state(&self, up: bool) -> bool {
        let new = if up { KG_UP } else { KG_DOWN };
        let prev = self.kg_state.swap(new, Ordering::SeqCst);
        if prev == new {
            return false;
        }
        let evidence = format!(
            "kg_backend_up: {} -> {} (watchdog self-poll /api/health)",
            kg_label(prev),
            kg_label(new)
        );
        if let Err(e) = self.observe(CANARY_KG, &evidence).await {
            warn!("[liveness] failed to record {CANARY_KG} fire: {e}");
        }
        info!("[liveness] {evidence}");
        true
    }
}

/// Run the KG-backend watchdog: self-poll `/api/health` every `period` and
/// drive the `kg_backend_up` gauge, firing [`CANARY_KG`] on every transition.
/// Never returns under normal operation; fail-open on every poll error.
pub async fn run_kg_watchdog(harness: Arc<LivenessHarness>, self_url: String, period: Duration) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("[liveness] KG watchdog HTTP client build failed, watchdog disabled: {e}");
            return;
        }
    };
    info!(
        "[liveness] KG watchdog self-polling {}/api/health every {:?}",
        self_url.trim_end_matches('/'),
        period
    );
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let up = probe_kg(&client, &self_url).await;
        harness.record_kg_state(up).await;
    }
}

/// One health probe. `true` iff `/api/health` answers 2xx and its `status`
/// field is not `"unhealthy"`. A connection failure, timeout, non-2xx, or an
/// unparseable body is backend loss (`false`) — see #12b: a probe must not
/// report UP on any error it cannot positively interpret as healthy.
async fn probe_kg(client: &reqwest::Client, base_url: &str) -> bool {
    let url = format!("{}/api/health", base_url.trim_end_matches('/'));
    match client.get(&url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return false;
            }
            // `Ok(body)` = 2xx + parseable JSON; `Err(())` = 2xx but the body
            // could not be parsed as JSON. The verdict logic lives in the pure
            // `health_verdict` helper so it is unit-testable without a live
            // server (#12b).
            let body = resp.json::<serde_json::Value>().await.map_err(|_| ());
            health_verdict(true, body)
        }
        Err(_) => false,
    }
}

/// Pure health-verdict logic, split out of [`probe_kg`] so the fail-closed
/// behaviour is testable without an HTTP round-trip.
///
/// * `is_success` — did the transport succeed with a 2xx status?
/// * `body` — `Ok` with the parsed JSON, or `Err(())` if the 2xx body could not
///   be parsed.
///
/// Returns `true` (UP) only when the endpoint positively looks healthy:
/// * a 2xx whose `status` field is present and not `"unhealthy"`, or
/// * a 2xx with valid JSON but no `status` field (a terse healthy response —
///   the 2xx itself is the liveness signal).
///
/// #12b: everything else is DOWN. A non-2xx, or a 2xx with an unparseable body,
/// is treated as backend loss so an outage cannot latch `kg_backend_up` UP.
/// ADR-136 documents fail-open only for the watchdog *loop* (it must never
/// panic/exit — see [`run_kg_watchdog`]), NOT for the per-probe verdict.
fn health_verdict(is_success: bool, body: Result<serde_json::Value, ()>) -> bool {
    if !is_success {
        return false;
    }
    match body {
        Ok(v) => v
            .get("status")
            .and_then(|s| s.as_str())
            .map(|s| s != "unhealthy")
            .unwrap_or(true),
        Err(()) => false,
    }
}

#[cfg(test)]
mod probe_tests {
    use super::health_verdict;
    use serde_json::json;

    #[test]
    fn healthy_status_is_up() {
        assert!(health_verdict(true, Ok(json!({ "status": "healthy" }))));
    }

    #[test]
    fn unhealthy_status_is_down() {
        assert!(!health_verdict(true, Ok(json!({ "status": "unhealthy" }))));
    }

    #[test]
    fn terse_2xx_without_status_field_is_up() {
        assert!(health_verdict(true, Ok(json!({ "uptime": 42 }))));
    }

    #[test]
    fn non_2xx_is_down() {
        // Even with a healthy-looking body, a non-2xx transport is DOWN.
        assert!(!health_verdict(false, Ok(json!({ "status": "healthy" }))));
    }

    #[test]
    fn unparseable_2xx_body_is_down() {
        // #12b: the previous behaviour returned UP here, latching the gauge
        // UP during a partial outage. Must now be DOWN.
        assert!(!health_verdict(true, Err(())));
    }
}
