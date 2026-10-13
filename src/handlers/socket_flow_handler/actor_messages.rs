use actix::{Handler, Message};
use log::{debug, error, info, warn};

use super::types::SocketFlowServer;

// Message to set client ID after registration
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct SetClientId(pub usize);

impl Handler<SetClientId> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SetClientId, _ctx: &mut Self::Context) -> Self::Result {
        self.client_id = Some(msg.0);
        info!("[WebSocket] Client assigned ID: {}", msg.0);
        // A session authenticated at the upgrade (NIP-98 Authorization header,
        // or the dev query token) already holds its pubkey, but the coordinator
        // registered it anonymously and was never told — so anything scoped by
        // the coordinator's pubkey (the ADR-2134 same-user relay, per-user
        // filters) missed it until an `authenticate` frame arrived.
        if let Some(pubkey) = self.pubkey.clone() {
            use crate::actors::messages::AuthenticateClient;
            self.client_manager_addr.do_send(AuthenticateClient {
                client_id: msg.0,
                pubkey,
                is_power_user: self.is_power_user,
                ephemeral: false,
            });
        }
    }
}

// Import the actor messages for binary/text send
use crate::actors::messages::{SendToClientBinary, SendToClientText};

impl Handler<SendToClientBinary> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendToClientBinary, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

impl Handler<crate::actors::messages::CloseClientSession> for SocketFlowServer {
    type Result = ();

    fn handle(
        &mut self,
        msg: crate::actors::messages::CloseClientSession,
        ctx: &mut Self::Context,
    ) {
        warn!(
            "[WebSocket] Client {:?} closed by the coordinator: {}",
            self.client_id, msg.reason
        );
        use actix::ActorContext;
        ctx.close(Some(actix_web_actors::ws::CloseReason {
            code: actix_web_actors::ws::CloseCode::Policy,
            description: Some(msg.reason),
        }));
        ctx.stop();
    }
}

impl Handler<SendToClientText> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendToClientText, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

// Handler for initial graph load - sends all nodes and edges as JSON
use crate::actors::messages::SendInitialGraphLoad;

impl Handler<SendInitialGraphLoad> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: SendInitialGraphLoad, ctx: &mut Self::Context) -> Self::Result {
        use crate::utils::socket_flow_messages::Message;

        let initial_load = Message::InitialGraphLoad {
            nodes: msg.nodes,
            edges: msg.edges,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };

        if let Ok(json) = serde_json::to_string(&initial_load) {
            ctx.text(json);
            if let Message::InitialGraphLoad { nodes, edges, .. } = &initial_load {
                info!(
                    "[WebSocket] Sent initial graph load: {} nodes, {} edges",
                    nodes.len(),
                    edges.len()
                );
            }
        } else {
            error!("[WebSocket] Failed to serialize initial graph load message");
        }
    }
}

// ---------------------------------------------------------------------------
// codex round-3: generation-gated variants of the two text sends used by
// `send_full_state_sync`. Unlike the shared `SendToClientText` /
// `SendInitialGraphLoad` (which many other callers use unconditionally), these
// carry the sync generation and are validated at the socket write — closing the
// TOCTOU window between the pre-send check and the actor handling the queued
// message. A superseded frame is dropped with a log.
// ---------------------------------------------------------------------------

/// A text frame tagged with the sync generation it belongs to.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct GenGatedText {
    pub text: String,
    pub generation: u64,
}

impl Handler<GenGatedText> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: GenGatedText, ctx: &mut Self::Context) {
        if !super::types::sync_generation_is_current(&self.sync_generation, msg.generation) {
            debug!("[WebSocket] Dropping stale state_sync text (generation superseded)");
            return;
        }
        ctx.text(msg.text);
    }
}

/// An initial-graph-load frame tagged with the sync generation it belongs to.
#[derive(Message)]
#[rtype(result = "()")]
pub(crate) struct GenGatedInitialGraphLoad {
    pub nodes: Vec<crate::utils::socket_flow_messages::InitialNodeData>,
    pub edges: Vec<crate::utils::socket_flow_messages::InitialEdgeData>,
    pub generation: u64,
}

impl Handler<GenGatedInitialGraphLoad> for SocketFlowServer {
    type Result = ();

    fn handle(&mut self, msg: GenGatedInitialGraphLoad, ctx: &mut Self::Context) {
        use crate::utils::socket_flow_messages::Message;

        if !super::types::sync_generation_is_current(&self.sync_generation, msg.generation) {
            debug!("[WebSocket] Dropping stale InitialGraphLoad (generation superseded)");
            return;
        }

        let initial_load = Message::InitialGraphLoad {
            nodes: msg.nodes,
            edges: msg.edges,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        };

        if let Ok(json) = serde_json::to_string(&initial_load) {
            ctx.text(json);
            if let Message::InitialGraphLoad { nodes, edges, .. } = &initial_load {
                info!(
                    "[WebSocket] Sent initial graph load: {} nodes, {} edges",
                    nodes.len(),
                    edges.len()
                );
            }
        } else {
            error!("[WebSocket] Failed to serialize initial graph load message");
        }
    }
}

// REMOVED (ADR-2054): PushDirective message + Handler<PushDirective>. Zero senders
// tree-wide — `send_pong`/`get_pending_directives` were never called on the live
// ping/pong path (mod.rs's StreamHandler answers pings with `ctx.pong(&msg)`
// directly), so the queued directive could never reach a client.
