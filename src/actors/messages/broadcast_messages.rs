//! Messages for the Phase 3 `BroadcastActor`.
//!
//! See `docs/migration-sprint/02-binary-protocol/WORKTREE-PLAN.md` §2 for the
//! state-machine these messages drive.

use actix::prelude::*;
use actix::Recipient;

use crate::actors::messages::SendToClientBinary;

/// Opaque identifier for a connected WebSocket client. Matches the `client_id`
/// returned by `ClientManager::register_client()`.
pub type ClientId = usize;

/// Register a new client with the broadcast actor.
///
/// On receipt, `BroadcastActor` immediately sends one V3 position frame to
/// the new client regardless of its current state (ADR-02 §3 "Client
/// connecting cold").
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterBroadcastClient {
    pub client_id: ClientId,
    pub recipient: Recipient<SendToClientBinary>,
}

/// Remove a client from the broadcast registry.
#[derive(Message)]
#[rtype(result = "()")]
pub struct UnregisterBroadcastClient {
    pub client_id: ClientId,
}

/// Physics emitted `LayoutStarted` — a new layout epoch is beginning.
/// Transition: any → ACTIVE. Resets per-client `frame_id` to 0 and emits
/// an immediate snapshot.
#[derive(Message)]
#[rtype(result = "()")]
pub struct OnLayoutStarted;

/// Physics emitted `LayoutSettled` — the layout has converged.
/// Transition: ACTIVE → SETTLED. Cancels the 100ms poll timer and starts
/// the wall-clock heartbeat interval.
#[derive(Message)]
#[rtype(result = "()")]
pub struct OnLayoutSettled;

/// Physics emitted `LayoutDestabilised` — the layout has been perturbed.
/// Transition: SETTLED → ACTIVE. Cancels the heartbeat and starts the
/// 100ms poll timer.
#[derive(Message)]
#[rtype(result = "()")]
pub struct OnLayoutDestabilised;

/// Physics emitted `PhysicsClamped` — informational only; no protocol
/// effect (ADR-02 D2). The broadcast actor logs and stays in its current
/// state.
#[derive(Message)]
#[rtype(result = "()")]
pub struct OnPhysicsClamped;

/// Internal timer message: wall-clock heartbeat fired while in SETTLED
/// state. The actor reads `current_snapshot()`, encodes V3, and emits to
/// all registered clients (with per-client backpressure check).
#[derive(Message)]
#[rtype(result = "()")]
pub struct TriggerHeartbeat;

/// Internal timer message: 100ms poll fired while in ACTIVE state.
#[derive(Message)]
#[rtype(result = "()")]
pub struct BroadcastTick;

/// Graceful shutdown — cancel timers, drain, stop the actor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct ShutdownBroadcastActor;

/// Diagnostic query: return a snapshot of the actor's current observable
/// state. Used by tests and the `/metrics` endpoint.
#[derive(Message)]
#[rtype(result = "BroadcastActorStatus")]
pub struct GetBroadcastActorStatus;

#[derive(Debug, Clone, PartialEq)]
pub enum BroadcastState {
    Active,
    Settled,
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct BroadcastActorStatus {
    pub state: BroadcastState,
    pub client_count: usize,
    /// Total number of V3 frames emitted to all clients combined.
    pub frames_sent_total: u64,
    /// Total V3 frames dropped due to per-client backpressure
    /// (`buffered_amount > 64KiB`). See ADR-02 D3 and PRD-02 A3/A7.
    pub frames_dropped_total: u64,
    /// Total wall-clock heartbeats that fired in SETTLED state.
    pub heartbeats_fired_total: u64,
}
