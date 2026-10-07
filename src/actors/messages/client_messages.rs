//! Client-domain messages: WebSocket client registration, broadcast,
//! authentication, filtering, initial graph load, and position streaming.
//!
//! Domain-safe types have been moved to `visionclaw_actors::messages::client_messages`.
//! This file re-exports them and defines the webxr-internal types that cannot move.

// ---------------------------------------------------------------------------
// Re-export domain-safe types from the domain crate
// ---------------------------------------------------------------------------

pub use visionclaw_actors::messages::client_messages::{
    AuthenticateClient, BroadcastMessage, BroadcastNodePositions, ClientBroadcastAck,
    ForcePositionBroadcast, GetClientCount, InitialClientSync, RelayToUserSessions,
    SendToClientBinary, SendToClientText, UnregisterClient, UpdateClientFilter,
};

// ---------------------------------------------------------------------------
// Webxr-internal types (cannot move to domain crate)
// ---------------------------------------------------------------------------

use actix::prelude::*;

use crate::utils::socket_flow_messages::{InitialEdgeData, InitialNodeData};

/// Erased recipient bundle for a single WebSocket client session.
///
/// Using `Recipient<M>` (actix's type-erased mailbox pointer) instead of
/// `Addr<SocketFlowServer>` breaks the backwards dependency:
///   ClientCoordinatorActor (domain) → SocketFlowServer (delivery layer)
///
/// The coordinator only needs to send three message types to each client.
/// Storing typed `Recipient`s instead of a concrete `Addr` means the actor
/// crate has no `use crate::handlers::*` import.
#[derive(Clone)]
pub struct ClientRecipients {
    pub binary: actix::Recipient<SendToClientBinary>,
    pub text: actix::Recipient<SendToClientText>,
    pub initial_load: actix::Recipient<SendInitialGraphLoad>,
}

impl std::fmt::Debug for ClientRecipients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientRecipients")
            .field("binary", &"Recipient<SendToClientBinary>")
            .field("text", &"Recipient<SendToClientText>")
            .field("initial_load", &"Recipient<SendInitialGraphLoad>")
            .finish()
    }
}

/// Register a new WebSocket client with the client manager.
/// ADR-090 A6-S4: uses `ClientRecipients` (type-erased) instead of
/// `Addr<SocketFlowServer>` so the actor crate has no handler dependency.
#[derive(Message)]
#[rtype(result = "Result<usize, String>")]
pub struct RegisterClient {
    pub recipients: ClientRecipients,
}

/// Broadcast positions to all connected clients.
/// Blocked: references `utils::socket_flow_messages::BinaryNodeDataClient`.
#[derive(Message)]
#[rtype(result = "()")]
pub struct BroadcastPositions {
    pub positions: Vec<crate::utils::socket_flow_messages::BinaryNodeDataClient>,
}

/// Set the graph service supervisor address in client manager.
/// Blocked: references `Addr<actors::GraphServiceSupervisor>`.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetGraphServiceAddress {
    pub addr: actix::Addr<crate::actors::GraphServiceSupervisor>,
}

/// WebSocket protocol: Initial graph load.
/// Blocked: references `utils::socket_flow_messages::{InitialNodeData, InitialEdgeData}`.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendInitialGraphLoad {
    pub nodes: Vec<InitialNodeData>,
    pub edges: Vec<InitialEdgeData>,
}

/// WebSocket protocol: Streamed position updates indexed by node ID.
/// Blocked: field types from socket_flow_messages context.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendPositionUpdate {
    pub node_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
}

/// Broadcast a fully-encoded `0x23 AGENT_ACTION` binary frame to every connected
/// WebSocket client (ADR-059 §4, Phase 2b — the agent-embodiment beam render).
///
/// The payload is the complete wire frame as produced by
/// [`crate::utils::binary_protocol::AgentActionEvent::encode`] (a 1-byte
/// `MessageType::AgentAction` tag + 15-byte LE header + variable metadata
/// payload). Pre-encoding upstream keeps `ClientCoordinatorActor` purely a fan-out
/// stage: its handler reuses the exact same per-client `send_binary` dispatch loop
/// as `BroadcastNodePositions` (`ClientManager::broadcast_to_all`) without knowing
/// anything about the agent-event schema.
///
/// Webxr-internal (carries `Vec<u8>` and is dispatched only inside the webxr
/// `ClientCoordinatorActor`), so it lives here rather than in the domain crate.
#[derive(Message)]
#[rtype(result = "()")]
pub struct BroadcastAgentActionFrame(pub Vec<u8>);
