//! Phase 3 (ADR-02 §D2) — `BroadcastActor`.
//!
//! Owns the WebSocket position-frame broadcast pipeline. Subscribes to
//! physics domain events (`LayoutStarted`, `LayoutSettled`,
//! `LayoutDestabilised`, `PhysicsClamped`) and runs a three-state machine:
//!
//! - **ACTIVE**   — layout in motion; emits full V3 frames at up to 10 Hz
//!                  (100 ms `ctx.run_interval`).
//! - **SETTLED**  — layout converged; emits wall-clock heartbeats every
//!                  `broadcast_heartbeat_secs` (default 5 s). Independent of
//!                  GPU tick rate (PRD-02 A8).
//! - **SHUTDOWN** — terminal; cancels timers, drops messages.
//!
//! All position reads go through `GraphStateActor::current_snapshot()`
//! (ADR-02 D4 — single source of truth). Backpressure is honoured per
//! ADR-02 D3: drop frame for any client whose mailbox is full; never queue.
//!
//! See `docs/migration-sprint/02-binary-protocol/WORKTREE-PLAN.md` §2 for
//! the state-machine transition table.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use actix::prelude::*;
use log::{debug, info, warn};

use crate::actors::messages::{
    BroadcastActorStatus, BroadcastClientId, BroadcastState, BroadcastTick,
    GetBroadcastActorStatus, GetPositionFrameSnapshot, OnLayoutDestabilised, OnLayoutSettled,
    OnLayoutStarted, OnPhysicsClamped, PositionFrameSnapshot, RegisterBroadcastClient,
    SendToClientBinary, ShutdownBroadcastActor, TriggerHeartbeat, UnregisterBroadcastClient,
};
use crate::actors::GraphStateActor;
use crate::protocol::v3_frame::{BinaryV3Frame, NodeRow};

/// Type-erased snapshot source. Production wiring passes
/// `graph_state_actor.recipient::<GetPositionFrameSnapshot>()`; tests pass
/// a stub actor implementing the same handler. Decoupling here keeps the
/// state-machine test surface free of `KnowledgeGraphRepository` mock
/// boilerplate.
pub type SnapshotSource = Recipient<GetPositionFrameSnapshot>;

// `GraphStateActor` is kept in scope only as the canonical production
// snapshot source; suppress the dead-import warning when this module is
// linked without the full app stack.
#[allow(dead_code)]
fn _gs_marker(_: Addr<GraphStateActor>) {}

/// `BroadcastActor` configuration. Tuned by `--broadcast-heartbeat-secs`
/// CLI flag (default 5 s) — see ADR-02 D2.
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Wall-clock heartbeat cadence while in SETTLED state.
    pub heartbeat_interval: Duration,
    /// Maximum broadcast cadence while in ACTIVE state.
    pub active_poll_interval: Duration,
    /// Per-client backpressure threshold. Frames are dropped for any
    /// client whose `SocketFlowServer` mailbox returns `SendError::Full`.
    /// The 64 KiB constant lives at the WebSocket layer (mailbox capacity);
    /// this field is informational and reported by `GetBroadcastActorStatus`.
    pub backpressure_threshold_bytes: usize,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(5),
            active_poll_interval: Duration::from_millis(100),
            backpressure_threshold_bytes: 64 * 1024,
        }
    }
}

/// Per-client broadcast state tracked by the actor.
struct ClientEntry {
    recipient: Recipient<SendToClientBinary>,
    /// Per-connection monotonic frame id (ADR-02 D7). Resets to 0 on
    /// `OnLayoutStarted`. Wraps at `u32::MAX`.
    frame_id: u32,
}

pub struct BroadcastActor {
    config: BroadcastConfig,
    state: BroadcastState,

    snapshot_source: SnapshotSource,

    clients: HashMap<BroadcastClientId, ClientEntry>,

    /// Cached snapshot epoch from the last successful encode. Lets ACTIVE
    /// ticks skip re-encoding when no new GPU positions have arrived.
    last_encoded_epoch: u64,
    /// Reusable encode buffer to keep ACTIVE-state allocations flat.
    encode_buf: Vec<u8>,

    // Telemetry
    frames_sent_total: u64,
    frames_dropped_total: u64,
    heartbeats_fired_total: u64,

    // Timer handles for cancellation on state transitions.
    active_tick_handle: Option<SpawnHandle>,
    heartbeat_handle: Option<SpawnHandle>,
}

impl BroadcastActor {
    /// Construct a new actor wired to a snapshot source. In production
    /// callers pass `graph_state_addr.recipient()`. Tests can pass any
    /// actor that implements `Handler<GetPositionFrameSnapshot>`.
    pub fn new(snapshot_source: SnapshotSource, config: BroadcastConfig) -> Self {
        Self {
            config,
            state: BroadcastState::Settled,
            snapshot_source,
            clients: HashMap::new(),
            last_encoded_epoch: 0,
            encode_buf: Vec::with_capacity(64 * 1024),
            frames_sent_total: 0,
            frames_dropped_total: 0,
            heartbeats_fired_total: 0,
            active_tick_handle: None,
            heartbeat_handle: None,
        }
    }

    /// Production helper: wire to a real `GraphStateActor` with defaults.
    pub fn with_graph_state(graph_state: Addr<GraphStateActor>) -> Self {
        Self::new(graph_state.recipient(), BroadcastConfig::default())
    }

    /// Convert a snapshot's `PositionRow` array into wire-format `NodeRow`s.
    /// One pass, no per-row allocation thanks to the pre-sized `Vec`.
    fn rows_from_snapshot(snapshot: &PositionFrameSnapshot) -> Vec<NodeRow> {
        let mut out = Vec::with_capacity(snapshot.rows.len());
        for row in &snapshot.rows {
            out.push(NodeRow {
                node_id: row.node_id,
                pos: [row.x, row.y, row.z],
                vel: [row.vx, row.vy, row.vz],
            });
        }
        out
    }

    /// Encode the snapshot once and send to every client with their own
    /// `frame_id`. Drops on backpressure per ADR-02 D3.
    fn fan_out(&mut self, snapshot: Arc<PositionFrameSnapshot>) {
        if self.clients.is_empty() {
            return;
        }
        let rows = Self::rows_from_snapshot(&snapshot);

        // Frame bytes differ per client only in `frame_id` (offset 4..8).
        // Encode once into the shared buffer; clone per send and overwrite
        // the four `frame_id` bytes on the clone. This keeps the hot path
        // to a single Vec allocation per send.
        // (The size variance is zero; only the 4 header bytes differ.)
        let mut dropped_in_round = 0u64;
        let mut sent_in_round = 0u64;

        // We can't borrow `self` mutably for both `encode_buf` and `clients`,
        // so build a template buffer with `frame_id = 0` then write per-client
        // ids into clones.
        BinaryV3Frame::encode_slice(0, &rows, &mut self.encode_buf);
        let template = self.encode_buf.clone();

        for entry in self.clients.values_mut() {
            let mut payload = template.clone();
            // Overwrite the 4-byte `frame_id` (offset 4..8).
            payload[4..8].copy_from_slice(&entry.frame_id.to_le_bytes());

            match entry.recipient.try_send(SendToClientBinary(payload)) {
                Ok(()) => {
                    entry.frame_id = entry.frame_id.wrapping_add(1);
                    sent_in_round += 1;
                }
                Err(SendError::Full(_)) => {
                    // ADR-02 D3 — drop, do not queue. The next tick re-evaluates.
                    dropped_in_round += 1;
                }
                Err(SendError::Closed(_)) => {
                    dropped_in_round += 1;
                    // The client will be reaped on the next Unregister.
                }
            }
        }

        self.frames_sent_total = self.frames_sent_total.saturating_add(sent_in_round);
        self.frames_dropped_total = self
            .frames_dropped_total
            .saturating_add(dropped_in_round);
        self.last_encoded_epoch = snapshot.epoch;
    }

    /// Pull the current snapshot from `GraphStateActor` and fan out to clients.
    fn fetch_and_fan_out(&mut self, ctx: &mut Context<Self>) {
        let source = self.snapshot_source.clone();
        let fut = async move { source.send(GetPositionFrameSnapshot).await };
        let fut = actix::fut::wrap_future::<_, Self>(fut).map(
            |res, act, _ctx| match res {
                Ok(Ok(snapshot)) => act.fan_out(snapshot),
                Ok(Err(e)) => warn!("BroadcastActor: GraphStateActor returned error: {e}"),
                Err(e) => warn!("BroadcastActor: GraphStateActor mailbox error: {e}"),
            },
        );
        ctx.spawn(fut);
    }

    /// Send exactly one snapshot to a single newly-registered client.
    /// Used in `handle(RegisterBroadcastClient)` to satisfy
    /// PRD-02 §3 "Client connecting cold" within 500 ms.
    fn send_immediate_to(
        &mut self,
        client_id: BroadcastClientId,
        ctx: &mut Context<Self>,
    ) {
        let source = self.snapshot_source.clone();
        let fut = async move { source.send(GetPositionFrameSnapshot).await };
        let fut = actix::fut::wrap_future::<_, Self>(fut).map(
            move |res, act, _ctx| {
                let snapshot = match res {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        warn!("BroadcastActor: snapshot fetch failed on register ({e})");
                        return;
                    }
                    Err(e) => {
                        warn!("BroadcastActor: mailbox error on register ({e})");
                        return;
                    }
                };
                let rows = Self::rows_from_snapshot(&snapshot);
                let mut buf = Vec::new();
                let entry = match act.clients.get_mut(&client_id) {
                    Some(e) => e,
                    None => return, // client already unregistered
                };
                BinaryV3Frame::encode_slice(entry.frame_id, &rows, &mut buf);
                match entry.recipient.try_send(SendToClientBinary(buf)) {
                    Ok(()) => {
                        entry.frame_id = entry.frame_id.wrapping_add(1);
                        act.frames_sent_total = act.frames_sent_total.saturating_add(1);
                    }
                    Err(SendError::Full(_)) | Err(SendError::Closed(_)) => {
                        act.frames_dropped_total =
                            act.frames_dropped_total.saturating_add(1);
                    }
                }
            },
        );
        ctx.spawn(fut);
    }

    fn start_active_poll(&mut self, ctx: &mut Context<Self>) {
        if self.active_tick_handle.is_some() {
            return;
        }
        let handle = ctx.run_interval(self.config.active_poll_interval, |_, ctx2| {
            ctx2.address().do_send(BroadcastTick);
        });
        self.active_tick_handle = Some(handle);
    }

    fn cancel_active_poll(&mut self, ctx: &mut Context<Self>) {
        if let Some(h) = self.active_tick_handle.take() {
            ctx.cancel_future(h);
        }
    }

    fn start_heartbeat(&mut self, ctx: &mut Context<Self>) {
        if self.heartbeat_handle.is_some() {
            return;
        }
        let handle = ctx.run_interval(self.config.heartbeat_interval, |_, ctx2| {
            ctx2.address().do_send(TriggerHeartbeat);
        });
        self.heartbeat_handle = Some(handle);
    }

    fn cancel_heartbeat(&mut self, ctx: &mut Context<Self>) {
        if let Some(h) = self.heartbeat_handle.take() {
            ctx.cancel_future(h);
        }
    }

    fn transition_to_active(&mut self, ctx: &mut Context<Self>) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        if !matches!(self.state, BroadcastState::Active) {
            info!("BroadcastActor: → ACTIVE");
            self.state = BroadcastState::Active;
        }
        self.cancel_heartbeat(ctx);
        self.start_active_poll(ctx);
    }

    fn transition_to_settled(&mut self, ctx: &mut Context<Self>) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        if !matches!(self.state, BroadcastState::Settled) {
            info!("BroadcastActor: → SETTLED");
            self.state = BroadcastState::Settled;
        }
        self.cancel_active_poll(ctx);
        self.start_heartbeat(ctx);
    }

    fn transition_to_shutdown(&mut self, ctx: &mut Context<Self>) {
        info!("BroadcastActor: → SHUTDOWN");
        self.state = BroadcastState::Shutdown;
        self.cancel_active_poll(ctx);
        self.cancel_heartbeat(ctx);
    }

    fn reset_frame_ids(&mut self) {
        for entry in self.clients.values_mut() {
            entry.frame_id = 0;
        }
    }
}

impl Actor for BroadcastActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!(
            "BroadcastActor started — initial state SETTLED, heartbeat = {:?}",
            self.config.heartbeat_interval
        );
        self.start_heartbeat(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!(
            "BroadcastActor stopped — frames_sent={}, frames_dropped={}, heartbeats={}",
            self.frames_sent_total, self.frames_dropped_total, self.heartbeats_fired_total
        );
    }
}

// ----------------------------------------------------------------------------
// Physics event handlers
// ----------------------------------------------------------------------------

impl Handler<OnLayoutStarted> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: OnLayoutStarted, ctx: &mut Self::Context) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        // ADR-02 D2: reset frame_ids on epoch transition.
        self.reset_frame_ids();
        self.transition_to_active(ctx);
        // Emit one immediate snapshot so cold clients see the new epoch.
        self.fetch_and_fan_out(ctx);
    }
}

impl Handler<OnLayoutSettled> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: OnLayoutSettled, ctx: &mut Self::Context) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        self.transition_to_settled(ctx);
    }
}

impl Handler<OnLayoutDestabilised> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: OnLayoutDestabilised, ctx: &mut Self::Context) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        self.transition_to_active(ctx);
    }
}

impl Handler<OnPhysicsClamped> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: OnPhysicsClamped, _ctx: &mut Self::Context) {
        warn!("BroadcastActor: PhysicsClamped received (informational only)");
    }
}

// ----------------------------------------------------------------------------
// Timer-driven handlers
// ----------------------------------------------------------------------------

impl Handler<BroadcastTick> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: BroadcastTick, ctx: &mut Self::Context) {
        if !matches!(self.state, BroadcastState::Active) {
            return;
        }
        self.fetch_and_fan_out(ctx);
    }
}

impl Handler<TriggerHeartbeat> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: TriggerHeartbeat, ctx: &mut Self::Context) {
        if !matches!(self.state, BroadcastState::Settled) {
            return;
        }
        self.heartbeats_fired_total = self.heartbeats_fired_total.saturating_add(1);
        debug!(
            "BroadcastActor: heartbeat #{} ({} clients)",
            self.heartbeats_fired_total,
            self.clients.len()
        );
        self.fetch_and_fan_out(ctx);
    }
}

// ----------------------------------------------------------------------------
// Client registration handlers
// ----------------------------------------------------------------------------

impl Handler<RegisterBroadcastClient> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, msg: RegisterBroadcastClient, ctx: &mut Self::Context) {
        if matches!(self.state, BroadcastState::Shutdown) {
            return;
        }
        let client_id = msg.client_id;
        self.clients.insert(
            client_id,
            ClientEntry {
                recipient: msg.recipient,
                frame_id: 0,
            },
        );
        info!(
            "BroadcastActor: client {} registered ({} total)",
            client_id,
            self.clients.len()
        );
        // PRD-02 §3 — emit one frame immediately, regardless of state.
        self.send_immediate_to(client_id, ctx);
    }
}

impl Handler<UnregisterBroadcastClient> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, msg: UnregisterBroadcastClient, _ctx: &mut Self::Context) {
        if self.clients.remove(&msg.client_id).is_some() {
            info!(
                "BroadcastActor: client {} unregistered ({} remaining)",
                msg.client_id,
                self.clients.len()
            );
        }
    }
}

impl Handler<ShutdownBroadcastActor> for BroadcastActor {
    type Result = ();

    fn handle(&mut self, _msg: ShutdownBroadcastActor, ctx: &mut Self::Context) {
        self.transition_to_shutdown(ctx);
        ctx.stop();
    }
}

impl Handler<GetBroadcastActorStatus> for BroadcastActor {
    type Result = MessageResult<GetBroadcastActorStatus>;

    fn handle(
        &mut self,
        _msg: GetBroadcastActorStatus,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        MessageResult(BroadcastActorStatus {
            state: self.state.clone(),
            client_count: self.clients.len(),
            frames_sent_total: self.frames_sent_total,
            frames_dropped_total: self.frames_dropped_total,
            heartbeats_fired_total: self.heartbeats_fired_total,
        })
    }
}

#[cfg(test)]
mod tests {
    //! State-machine smoke tests for `BroadcastActor`.
    //!
    //! Heavier integration tests (heartbeat-while-physics-paused,
    //! backpressure drop, frame round-trip) live in
    //! `tests/binary_protocol_test.rs`.

    use super::*;

    #[test]
    fn config_default_values() {
        let c = BroadcastConfig::default();
        assert_eq!(c.heartbeat_interval, Duration::from_secs(5));
        assert_eq!(c.active_poll_interval, Duration::from_millis(100));
        assert_eq!(c.backpressure_threshold_bytes, 64 * 1024);
    }

    #[test]
    fn rows_from_snapshot_matches_layout() {
        let snapshot = Arc::new(PositionFrameSnapshot {
            epoch: 1,
            node_count: 2,
            rows: vec![
                crate::actors::messages::PositionRow {
                    node_id: 7,
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    vx: 0.1,
                    vy: 0.2,
                    vz: 0.3,
                },
                crate::actors::messages::PositionRow {
                    node_id: 11,
                    x: 4.0,
                    y: 5.0,
                    z: 6.0,
                    vx: 0.4,
                    vy: 0.5,
                    vz: 0.6,
                },
            ],
        });
        let rows = BroadcastActor::rows_from_snapshot(&snapshot);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].node_id, 7);
        assert_eq!(rows[0].pos, [1.0, 2.0, 3.0]);
        assert_eq!(rows[0].vel, [0.1, 0.2, 0.3]);
        assert_eq!(rows[1].node_id, 11);
        assert_eq!(rows[1].pos, [4.0, 5.0, 6.0]);
    }
}
