use actix::prelude::*;
use log::{debug, info, trace, warn};

use super::types::{SocketFlowServer, WEBSOCKET_RATE_LIMITER};

/// Opcode of the client -> server position acknowledgement (ADR-2141).
const OPCODE_BROADCAST_ACK: u8 = 0x34;

/// Whether this binary frame is a position acknowledgement.
pub(crate) fn is_stream_ack(data: &[u8]) -> bool {
    data.first() == Some(&OPCODE_BROADCAST_ACK)
}

/// Whether a binary frame counts against the per-IP message rate limit.
///
/// Acknowledgements do NOT. They are the flow control of the position stream: one per chunk the
/// client applied, so their rate is the stream's rate. Counting them against a 300-per-minute
/// limiter (designed for chatty position WRITES) silently dropped acks past the burst, after which
/// the server resent every chunk on its backed-off timeout and, after ten violations, banned the
/// address for ten minutes. An ack is cheap and harmless to over-send: it names a sequence, and an
/// unknown one is a no-op.
pub(crate) fn rate_limit_applies(data: &[u8]) -> bool {
    !is_stream_ack(data)
}

/// Handle incoming binary WebSocket messages: position acknowledgements and voice data. There is
/// no binary position WRITE: clients move nodes with the JSON drag messages (the GPU pin), and the
/// old decode-and-simulate fallback for client-sent positions is gone.
impl SocketFlowServer {
    pub(crate) fn handle_binary_message(
        &mut self,
        data: &[u8],
        ctx: &mut <Self as Actor>::Context,
    ) {
        if rate_limit_applies(data) && !WEBSOCKET_RATE_LIMITER.is_allowed(&self.client_ip) {
            warn!("Binary message rate limit exceeded for client: {}", self.client_ip);
            let error_msg = serde_json::json!({
                "type": "rate_limit_warning",
                "message": "Message rate too high, some messages may be dropped",
                "retry_after": WEBSOCKET_RATE_LIMITER.reset_time(&self.client_ip).as_secs()
            });
            if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                ctx.text(msg_str);
            }
            return;
        }

        self.last_activity = std::time::Instant::now();

        use crate::utils::binary_protocol::{BinaryProtocol, Message as ProtocolMessage};

        match BinaryProtocol::decode_message(data) {
            Ok(ProtocolMessage::VoiceData { audio }) => {
                info!("Received voice data: {} bytes", audio.len());
                let response = serde_json::json!({
                    "type": "voice_ack",
                    "bytes": audio.len(),
                    "message": "Voice data received but not yet processed"
                });
                if let Ok(msg_str) = serde_json::to_string(&response) {
                    ctx.text(msg_str);
                }
            }
            Ok(ProtocolMessage::BroadcastAck {
                sequence_id,
                nodes_received,
                timestamp,
            }) => {
                use crate::actors::messages::ClientBroadcastAck;

                trace!(
                    "Received BroadcastAck: seq={}, nodes={}, timestamp={}",
                    sequence_id,
                    nodes_received,
                    timestamp
                );

                self.client_manager_addr.do_send(ClientBroadcastAck {
                    sequence_id,
                    nodes_received,
                    timestamp,
                    client_id: self.client_id,
                });
            }
            Err(e) => {
                debug!("Unsupported binary message ({} bytes): {}", data.len(), e);
                let error_msg = serde_json::json!({
                    "type": "error",
                    "message": format!("Unsupported binary message: {}", e),
                    "recoverable": true,
                    "details": { "data_length": data.len() }
                });
                if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                    ctx.text(msg_str);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_acknowledgements_are_exempt_from_the_message_rate_limit() {
        let ack = visionclaw_protocol::wire_fixtures::broadcast_ack_frame(48_113, 2500, 1_700_000_000_000);
        assert!(is_stream_ack(&ack));
        assert!(!rate_limit_applies(&ack), "an ack must never be rate limited: it IS the flow control");
    }

    #[test]
    fn other_binary_traffic_is_still_rate_limited() {
        assert!(rate_limit_applies(&[0x02, 1, 2, 3]), "voice data");
        assert!(rate_limit_applies(&[0x03, 0, 0, 0]), "a stray V3 frame");
        assert!(rate_limit_applies(&[]), "an empty frame");
        assert!(!is_stream_ack(&[]));
    }

    /// The failure this guards against, end to end: a sustained ack stream well above the limiter's
    /// per-minute budget keeps being accepted, while the same count of non-ack frames is cut off.
    #[actix::test]
    async fn a_sustained_ack_stream_never_exhausts_the_limiter_but_other_frames_do() {
        use crate::utils::validation::rate_limit::{EndpointRateLimits, RateLimiter};
        let limiter = RateLimiter::new(EndpointRateLimits::socket_flow_updates());
        let ack = visionclaw_protocol::wire_fixtures::broadcast_ack_frame(1, 1, 0);
        let voice = [0x02u8, 1];
        let admitted = |frame: &[u8], ip: &str| (0..2000).filter(|_| !rate_limit_applies(frame) || limiter.is_allowed(ip)).count();
        assert_eq!(admitted(&ack, "10.0.0.1"), 2000, "2000 acks in a burst: all admitted");
        assert!(admitted(&voice, "10.0.0.2") < 400, "the same volume of other frames is limited");
    }
}
