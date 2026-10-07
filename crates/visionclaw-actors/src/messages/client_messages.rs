//! Domain-safe client message types.
//!
//! Blocked in webxr (stay in src/actors/messages/client_messages.rs):
//!   - `RegisterClient`        — refs `Addr<handlers::socket_flow_handler::SocketFlowServer>`
//!   - `BroadcastPositions`    — refs `utils::socket_flow_messages::BinaryNodeDataClient`
//!   - `SetGraphServiceAddress`— refs `Addr<actors::GraphServiceSupervisor>`
//!   - `SendInitialGraphLoad`  — refs `utils::socket_flow_messages::{InitialNodeData, InitialEdgeData}`
//!   - `SendPositionUpdate`    — refs field types from socket_flow_messages
//!
//! Everything below depends only on `actix`, `serde`, `std`.

use actix::prelude::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Client Manager Actor Messages
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct UnregisterClient {
    pub client_id: usize,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct BroadcastNodePositions {
    pub positions: Vec<u8>,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct BroadcastMessage {
    pub message: String,
}

#[derive(Message)]
#[rtype(result = "Result<usize, String>")]
pub struct GetClientCount;

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct ForcePositionBroadcast {
    pub reason: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct InitialClientSync {
    pub client_identifier: String,
    pub trigger_source: String,
}

// Messages for ClientManagerActor to send to individual SocketFlowServer clients
#[derive(Message)]
#[rtype(result = "()")]
pub struct SendToClientBinary(pub Vec<u8>);

#[derive(Message)]
#[rtype(result = "()")]
pub struct SendToClientText(pub String);

/// Deliver a text frame to every OTHER session authenticated as `pubkey`
/// (ADR-2134 same-user relay: the desktop's beat clock and memory route to the
/// same user's headset). Never reaches a session with a different or no
/// pubkey. Returns how many sessions it was handed to.
#[derive(Message)]
#[rtype(result = "usize")]
pub struct RelayToUserSessions {
    pub pubkey: String,
    /// The sending session, excluded from delivery.
    pub exclude_client_id: Option<usize>,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Client authentication
// ---------------------------------------------------------------------------

#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct AuthenticateClient {
    pub client_id: usize,
    pub pubkey: String,
    pub is_power_user: bool,
    /// Whether this client uses an ephemeral (dev-mode) session identity
    pub ephemeral: bool,
}

// ---------------------------------------------------------------------------
// Client filter settings
// ---------------------------------------------------------------------------

#[derive(Message, Clone, Serialize, Deserialize)]
#[rtype(result = "Result<(), String>")]
pub struct UpdateClientFilter {
    pub client_id: usize,
    pub enabled: bool,
    pub quality_threshold: f64,
    pub authority_threshold: f64,
    pub filter_by_quality: bool,
    pub filter_by_authority: bool,
    pub filter_mode: String,
    pub max_nodes: Option<i32>,
    #[serde(default = "default_include_linked_pages")]
    pub include_linked_pages: bool,
}

// ---------------------------------------------------------------------------
// Client broadcast acknowledgement (end-to-end flow control)
// ---------------------------------------------------------------------------

/// Client-originated broadcast acknowledgement for true end-to-end flow control.
/// Sent by WebSocket clients after they process position updates.
#[derive(Message, Debug, Clone)]
#[rtype(result = "()")]
pub struct ClientBroadcastAck {
    /// Sequence ID from the original broadcast
    pub sequence_id: u64,
    /// Number of nodes the client actually processed
    pub nodes_received: u32,
    /// Client receive timestamp (ms since epoch)
    pub timestamp: u64,
    /// Client ID that sent this ACK (set by handler)
    pub client_id: Option<usize>,
}

fn default_include_linked_pages() -> bool {
    false
}
