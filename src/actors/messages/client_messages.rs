//! Client-domain messages: WebSocket client registration, broadcast,
//! authentication, filtering, initial graph load, and position streaming.
//!
//! Domain-safe types have been moved to `visionclaw_actors::messages::client_messages`.
//! This file re-exports them and defines the webxr-internal types that cannot move.

// ---------------------------------------------------------------------------
// Re-export domain-safe types from the domain crate
// ---------------------------------------------------------------------------

pub use visionclaw_actors::messages::client_messages::{
    AuthenticateClient, BroadcastMessage, ClientBroadcastAck, GetClientCount, RelayToUserSessions,
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
/// Tells a client session to close (sent with `do_send`, which bypasses a
/// full mailbox). `reason` goes into the WebSocket close frame.
#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct CloseClientSession {
    pub reason: String,
}

/// Closes a client's transport (its TCP connection) from outside the session
/// actor.
///
/// A peer that stops reading stalls actix's HTTP dispatcher on the socket
/// write, and the dispatcher then stops polling the response body that the
/// session actor lives in: the actor is not run, so no message sent to it,
/// `do_send` included, is handled until the peer reads again. Shutting the
/// socket down makes the dispatcher's next I/O fail, which drops the
/// connection and stops the actor. Built from a dup of the connection's fd
/// taken in `HttpServer::on_connect` (see
/// `handlers::socket_flow_handler::transport`).
#[derive(Clone)]
pub struct TransportCloser(std::sync::Arc<dyn Fn() + Send + Sync>);

impl TransportCloser {
    pub fn new(close: impl Fn() + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(close))
    }

    /// Shut the transport down. Idempotent.
    pub fn close(&self) {
        (self.0)()
    }
}

/// Default for [`ClientRecipients::stall_timeout`]: the default
/// `system.websocket.heartbeatTimeout` (60 s).
pub const DEFAULT_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Erased handle bundle for a single WebSocket client session.
///
/// Using `Recipient<M>` (actix's type-erased mailbox pointer) instead of
/// `Addr<SocketFlowServer>` breaks the backwards dependency:
///   ClientCoordinatorActor (domain) → SocketFlowServer (delivery layer)
///
/// Storing typed `Recipient`s instead of a concrete `Addr` means the actor
/// crate has no `use crate::handlers::*` import.
#[derive(Clone)]
pub struct ClientRecipients {
    pub binary: actix::Recipient<SendToClientBinary>,
    pub text: actix::Recipient<SendToClientText>,
    pub initial_load: actix::Recipient<SendInitialGraphLoad>,
    pub close: actix::Recipient<CloseClientSession>,
    /// Closes the TCP connection when the session actor cannot be reached.
    pub transport: Option<TransportCloser>,
    /// How long the client may stay congested (every broadcast finding its
    /// mailbox full) before it is evicted and closed. The socket sets this to
    /// its heartbeat timeout.
    pub stall_timeout: std::time::Duration,
}

impl ClientRecipients {
    pub fn new(
        binary: actix::Recipient<SendToClientBinary>,
        text: actix::Recipient<SendToClientText>,
        initial_load: actix::Recipient<SendInitialGraphLoad>,
        close: actix::Recipient<CloseClientSession>,
    ) -> Self {
        Self {
            binary,
            text,
            initial_load,
            close,
            transport: None,
            stall_timeout: DEFAULT_STALL_TIMEOUT,
        }
    }

    pub fn with_transport(mut self, transport: TransportCloser) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_stall_timeout(mut self, stall_timeout: std::time::Duration) -> Self {
        self.stall_timeout = stall_timeout;
        self
    }
}

impl std::fmt::Debug for ClientRecipients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientRecipients")
            .field("binary", &"Recipient<SendToClientBinary>")
            .field("text", &"Recipient<SendToClientText>")
            .field("initial_load", &"Recipient<SendInitialGraphLoad>")
            .field("close", &"Recipient<CloseClientSession>")
            .field("transport", &self.transport.is_some())
            .field("stall_timeout", &self.stall_timeout)
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

/// One full set of display positions from the physics (every entitled node, absolute
/// coordinates). The client coordinator replaces its cache with it and sends each client the next
/// chunk of what that client has not yet acknowledged (ADR-2141).
#[derive(Message)]
#[rtype(result = "()")]
pub struct PublishPositions {
    pub positions: Vec<crate::utils::socket_flow_messages::BinaryNodeDataClient>,
}

/// The stored stream settings (`physics.streamChunkNodes`, `streamMoveThreshold`,
/// `streamAckTimeoutMs`), applied by the client coordinator to every client's next chunk.
#[derive(Message, Debug, Clone, Copy)]
#[rtype(result = "()")]
pub struct ConfigureStream {
    pub chunk_nodes: u32,
    pub move_threshold: f32,
    pub ack_timeout_ms: u32,
}
