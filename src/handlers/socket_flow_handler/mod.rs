// WebSocket handler for the /wss endpoint.
// Split into submodules for maintainability.

pub mod actor_messages;
pub mod binary_protocol;
pub mod filter_auth;
pub mod heartbeat;
pub mod http_handler;
pub mod message_routing;
pub mod position_updates;
pub mod session_relay;
pub mod transport;
pub mod types;

// Re-export public API (preserves all external imports)
pub use actor_messages::BroadcastPositionUpdate;
// REMOVED (ADR-2054): PushDirective — zero senders tree-wide, see actor_messages.rs.
pub use http_handler::socket_flow_handler;
pub use types::SocketFlowServer;

// StreamHandler glue -- delegates text/binary to submodules
use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, error, info, warn};

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for SocketFlowServer {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if matches!(
            msg,
            Ok(ws::Message::Ping(_)
                | ws::Message::Pong(_)
                | ws::Message::Text(_)
                | ws::Message::Binary(_))
        ) {
            self.heartbeat.inbound(std::time::Instant::now());
        }
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                debug!("[WebSocket] Received standard ping");
                self.last_activity = std::time::Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_activity = std::time::Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                self.handle_text_message(&text, ctx);
            }
            Ok(ws::Message::Binary(data)) => {
                self.handle_binary_message(&data, ctx);
            }
            Ok(ws::Message::Close(reason)) => {
                info!("[WebSocket] Client initiated close: {:?}", reason);
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                warn!("[WebSocket] Received unexpected continuation frame");
            }
            Ok(ws::Message::Nop) => {
                debug!("[WebSocket] Received Nop");
            }
            Err(e) => {
                error!("[WebSocket] Error in WebSocket connection: {}", e);
                let error_msg = serde_json::json!({
                    "type": "error",
                    "message": format!("WebSocket error: {}", e),
                    "recoverable": true
                });
                if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                    ctx.text(msg_str);
                }
            }
        }
    }
}
