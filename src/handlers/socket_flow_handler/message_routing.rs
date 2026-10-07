use actix::prelude::*;
use actix_web_actors::ws;
use log::{debug, info, warn};

use crate::utils::socket_flow_messages::PingMessage;

use super::types::SocketFlowServer;

/// Text message dispatch -- routes parsed JSON messages to the appropriate handler.
///
/// Handles: ping, update_physics_params, request_full_snapshot, requestInitialData,
/// enableRandomization, requestBotsGraph, requestBotsPositions, subscribe_position_updates,
/// requestPositionUpdates (legacy), authenticate, filter_update, requestSwarmTelemetry,
/// ontology_* messages.
impl SocketFlowServer {
    pub(crate) fn handle_text_message(&mut self, text: &str, ctx: &mut <Self as Actor>::Context) {
        // Handle plain-text heartbeat before JSON parsing
        if text.trim() == "ping" {
            self.last_activity = std::time::Instant::now();
            ctx.text("pong");
            return;
        }
        // debug, not info: drag updates and relayed beat frames arrive at up to
        // tens of Hz and would flood the log (and copy route ids into it).
        debug!("Received text message: {}", text);
        self.last_activity = std::time::Instant::now();

        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(msg) => match msg.get("type").and_then(|t| t.as_str()) {
                Some("ping") => self.handle_json_ping(&msg, ctx),
                Some("update_physics_params") => {
                    warn!("Client attempted deprecated WebSocket physics update - ignoring");
                    ctx.text(r#"{"type":"error","message":"Physics updates must use REST API: POST /api/analytics/params"}"#);
                }
                Some("request_full_snapshot") => {
                    super::position_updates::handle_request_full_snapshot(self, &msg, ctx);
                }
                Some("requestInitialData") => {
                    super::position_updates::handle_request_initial_data(self, ctx);
                }
                Some("enableRandomization") => {
                    super::position_updates::handle_enable_randomization(&msg);
                }
                Some("requestBotsGraph") => {
                    super::position_updates::handle_request_bots_graph(self, ctx);
                }
                Some("requestBotsPositions") => {
                    super::position_updates::handle_request_bots_positions(self, ctx);
                }
                Some("subscribe_position_updates") => {
                    super::position_updates::handle_subscribe_position_updates(self, &msg, ctx);
                }
                Some("requestPositionUpdates") => {
                    info!("Client requested position updates (legacy format)");
                    let subscription_msg = r#"{"type":"subscribe_position_updates","data":{"interval":60,"binary":true}}"#;
                    <SocketFlowServer as StreamHandler<
                            Result<ws::Message, ws::ProtocolError>,
                        >>::handle(
                            self,
                            Ok(ws::Message::Text(subscription_msg.to_string().into())),
                            ctx,
                        );
                }
                Some("authenticate") => {
                    super::filter_auth::handle_authenticate(self, &msg, ctx);
                }
                Some("filter_update") => {
                    super::filter_auth::handle_filter_update(self, &msg, ctx);
                }
                Some("requestSwarmTelemetry") => {
                    super::position_updates::handle_request_swarm_telemetry(self, ctx);
                }
                Some("ontology_validation") | Some("ontology_validation_report") => {
                    super::filter_auth::handle_ontology_validation(self, &msg, ctx);
                }
                Some("ontology_constraint_update") | Some("ontology_constraint_toggle") => {
                    super::filter_auth::handle_ontology_constraint_update(ctx);
                }
                Some("ontology_reasoning") => {
                    super::filter_auth::handle_ontology_reasoning(self, &msg, ctx);
                }
                Some("nodeDragStart") => {
                    super::position_updates::handle_node_drag_start(self, &msg, ctx);
                }
                Some("nodeDragEnd") => {
                    super::position_updates::handle_node_drag_end(self, &msg, ctx);
                }
                Some("nodeDragUpdate") => {
                    super::position_updates::handle_node_drag_update(self, &msg, ctx);
                }
                Some("nodeUnpin") => {
                    super::position_updates::handle_node_unpin(self, &msg, ctx);
                }
                Some(t @ ("beatClock" | "memoryRoute")) => {
                    if let Some(kind) = super::session_relay::RelayKind::from_type(t) {
                        super::session_relay::handle_relay(self, kind, text.len(), &msg, ctx);
                    }
                }
                _ => {
                    warn!("[WebSocket] Unknown message type: {:?}", msg);
                }
            },
            Err(e) => {
                warn!("[WebSocket] Failed to parse text message: {}", e);
                let error_msg = serde_json::json!({
                    "type": "error",
                    "message": format!("Failed to parse text message: {}", e)
                });
                if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                    ctx.text(msg_str);
                }
            }
        }
    }

    fn handle_json_ping(&mut self, msg: &serde_json::Value, ctx: &mut <Self as Actor>::Context) {
        if let Ok(ping_msg) = serde_json::from_value::<PingMessage>(msg.clone()) {
            let pong = self.handle_ping(ping_msg);
            self.last_activity = std::time::Instant::now();
            if let Ok(response) = serde_json::to_string(&pong) {
                ctx.text(response);
            }
        } else if let Some(text_ping) = msg.as_str() {
            if text_ping == "ping" {
                self.last_activity = std::time::Instant::now();
                ctx.text("pong");
            }
        }
    }
}
