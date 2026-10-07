//! tokio-tungstenite transport: the only place real sockets are opened.
//!
//! Two pumps run on the shared [`crate::runtime`]: [`spawn_graph_stream`] for the
//! `/wss` V3 graph position stream and [`spawn_presence`] for the `/ws/presence`
//! avatar pose channel. Both push `Send`-safe events into a main-thread inbox
//! (`Arc<Mutex<VecDeque<_>>>`) because Godot objects are not `Send`; the gdext
//! classes drain that inbox in their per-frame `poll()`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};

/// Websocket receive limits. The default tungstenite cap is 16 MiB, but a
/// full-graph initialGraphLoad (13k nodes / 145k edges) is ~28 MB as one text
/// frame — under the default it kills the socket ("Message too long") in a
/// permanent connect→sync→die loop. 256 MiB leaves ample headroom.
fn ws_config() -> WebSocketConfig {
    WebSocketConfig {
        max_message_size: Some(256 * 1024 * 1024),
        max_frame_size: Some(256 * 1024 * 1024),
        ..WebSocketConfig::default()
    }
}
use tracing::{error, warn};

use visionclaw_xr_presence::{PoseFrame, RoomId};

use crate::binary_protocol::GraphInbound;
use crate::ports::{TransportError, WsMessage, WsTransport};
use crate::presence::{decode_sibling_frame, PresenceClient, PresenceInbound, ServerMessage};
use crate::runtime::runtime;
use crate::signer::NostrSigner;

const GRAPH_REQUEST_INITIAL: &str = r#"{"type":"requestInitialData"}"#;
const GRAPH_SUBSCRIBE: &str =
    r#"{"type":"subscribe_position_updates","data":{"interval":60,"binary":true}}"#;

type WsConn = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Owns a spawned pump task; dropping does not stop it — call [`Self::abort`].
pub struct ConnHandle {
    handle: tokio::task::JoinHandle<()>,
}

impl ConnHandle {
    pub fn abort(&self) {
        self.handle.abort();
    }
}

// --- graph stream (/wss V3) --------------------------------------------------

/// Connect to `/wss`, subscribe to binary position updates, and push every
/// frame into `inbox` for `BinaryProtocolClient::poll` to decode. Returns the
/// outbound text-message channel used for server-authoritative interactions
/// (node drag/pin). When `nostr_secret_hex` is non-empty, a NIP-98 `authenticate`
/// message is sent after subscribe so the server accepts mutating messages.
pub fn spawn_graph_stream(
    url: String,
    nostr_secret_hex: String,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
) -> (ConnHandle, mpsc::UnboundedSender<String>) {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let handle = runtime().spawn(async move {
        graph_pump(url, nostr_secret_hex, inbox, rx).await;
    });
    (ConnHandle { handle }, tx)
}

async fn graph_pump(
    url: String,
    nostr_secret_hex: String,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
    outbound: mpsc::UnboundedReceiver<String>,
) {
    match connect_async_with_config(url.clone(), Some(ws_config()), false).await {
        Ok((ws, _resp)) => {
            let (mut sink, mut stream) = ws.split();
            if sink
                .send(Message::Text(GRAPH_REQUEST_INITIAL.to_owned()))
                .await
                .is_err()
                || sink
                    .send(Message::Text(GRAPH_SUBSCRIBE.to_owned()))
                    .await
                    .is_err()
            {
                error!("graph stream: failed to send subscribe control frames");
            } else {
                // Only now, once the subscribe control frames are on the wire, is
                // the stream usable — surface Connected here (not on bare TCP
                // connect) so the scene's reconnect FSM never treats a half-open
                // socket as live (#3d).
                push_graph(&inbox, GraphInbound::Connected);
                // NIP-98 session auth: unlocks server-authoritative interactions
                // (node drag/pin) for this connection. Anonymous read-only
                // streaming still works without it.
                if !nostr_secret_hex.trim().is_empty() {
                    match build_signer(&nostr_secret_hex)
                        .map(|s| s.nip98_authenticate_json(&url, "GET"))
                    {
                        Ok(auth_msg) => {
                            if sink.send(Message::Text(auth_msg)).await.is_err() {
                                warn!("graph stream: failed to send authenticate message");
                            }
                        }
                        Err(e) => {
                            warn!(err = %e, "graph stream: signer init failed; staying anonymous")
                        }
                    }
                }
                pump_graph_messages(&mut sink, &mut stream, &inbox, outbound).await;
            }
        }
        Err(e) => error!(err = %e, url = %url, "graph stream connect failed"),
    }
    push_graph(&inbox, GraphInbound::Disconnected);
}

async fn pump_graph_messages(
    sink: &mut SplitSink<WsConn, Message>,
    stream: &mut SplitStream<WsConn>,
    inbox: &Arc<Mutex<VecDeque<GraphInbound>>>,
    mut outbound: mpsc::UnboundedReceiver<String>,
) {
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => push_graph(inbox, GraphInbound::Frame(b)),
                    Some(Ok(Message::Text(t))) => {
                        // initialGraphLoad → Topology; every other JSON envelope
                        // (broker:new_case, broker:case_decided, …) is forwarded
                        // verbatim as Text for the scene layer to route by `type`.
                        push_graph(inbox, crate::binary_protocol::classify_graph_text(&t));
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        warn!(err = %e, "graph stream recv error");
                        break;
                    }
                }
            }
            out = outbound.recv() => {
                match out {
                    Some(text) => {
                        if let Err(e) = sink.send(Message::Text(text)).await {
                            warn!(err = %e, "graph stream outbound send failed");
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

fn push_graph(inbox: &Arc<Mutex<VecDeque<GraphInbound>>>, ev: GraphInbound) {
    if let Ok(mut q) = inbox.lock() {
        q.push_back(ev);
    }
}

// --- presence stream (/ws/presence 0x43) -------------------------------------

/// Connect to `/ws/presence`, run the challenge/auth/joined handshake with a
/// Nostr identity (`secret_hex` empty => ephemeral), then bridge inbound room
/// events / sibling pose frames into `inbox` and outbound local poses onto the
/// returned channel.
pub fn spawn_presence(
    url: String,
    room_urn: String,
    display_name: String,
    secret_hex: String,
    inbox: Arc<Mutex<VecDeque<PresenceInbound>>>,
) -> (ConnHandle, mpsc::UnboundedSender<PoseFrame>) {
    let (tx, rx) = mpsc::unbounded_channel::<PoseFrame>();
    let handle = runtime().spawn(async move {
        presence_pump(url, room_urn, display_name, secret_hex, inbox, rx).await;
    });
    (ConnHandle { handle }, tx)
}

async fn presence_pump(
    url: String,
    room_urn: String,
    display_name: String,
    secret_hex: String,
    inbox: Arc<Mutex<VecDeque<PresenceInbound>>>,
    mut outbound: mpsc::UnboundedReceiver<PoseFrame>,
) {
    let room = match RoomId::parse(room_urn.clone()) {
        Ok(r) => r,
        Err(e) => {
            error!(err = %e, room = %room_urn, "presence: bad room urn");
            push_presence(&inbox, PresenceInbound::Disconnected);
            return;
        }
    };
    let signer = match build_signer(&secret_hex) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!(err = %e, "presence: signer init failed");
            push_presence(&inbox, PresenceInbound::Disconnected);
            return;
        }
    };

    match connect_async_with_config(url.clone(), Some(ws_config()), false).await {
        Ok((ws, _resp)) => {
            let transport = Arc::new(TungsteniteWsTransport::new(ws));
            let mut client = PresenceClient::new(transport.clone(), signer, room);
            match client.handshake(display_name, None).await {
                Ok(joined) => {
                    // Emit Connected only after the join handshake completes, so
                    // the reconnect FSM never counts a pre-join socket as live
                    // (#3d).
                    push_presence(&inbox, PresenceInbound::Connected);
                    push_presence(
                        &inbox,
                        PresenceInbound::Joined {
                            avatar_id: joined.avatar_id,
                            members: joined.members,
                        },
                    );
                    presence_event_loop(&transport, &client, &inbox, &mut outbound).await;
                }
                Err(e) => error!(err = %e, "presence handshake failed"),
            }
        }
        Err(e) => error!(err = %e, url = %url, "presence connect failed"),
    }
    push_presence(&inbox, PresenceInbound::Disconnected);
}

async fn presence_event_loop(
    transport: &Arc<TungsteniteWsTransport>,
    client: &PresenceClient<TungsteniteWsTransport, NostrSigner>,
    inbox: &Arc<Mutex<VecDeque<PresenceInbound>>>,
    outbound: &mut mpsc::UnboundedReceiver<PoseFrame>,
) {
    loop {
        tokio::select! {
            incoming = transport.recv() => {
                match incoming {
                    Ok(WsMessage::Binary(b)) => match decode_sibling_frame(&b) {
                        Ok(batch) => push_presence(inbox, PresenceInbound::Pose(batch)),
                        Err(e) => warn!(err = %e, "presence sibling decode failed"),
                    },
                    Ok(WsMessage::Text(t)) => handle_room_event(&t, inbox),
                    Ok(WsMessage::Close) => break,
                    Err(e) => {
                        warn!(err = %e, "presence recv error");
                        break;
                    }
                }
            }
            pose = outbound.recv() => {
                match pose {
                    Some(frame) => {
                        if let Err(e) = client.send_pose(&frame).await {
                            warn!(err = %e, "presence send_pose failed");
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

fn handle_room_event(text: &str, inbox: &Arc<Mutex<VecDeque<PresenceInbound>>>) {
    match serde_json::from_str::<ServerMessage>(text) {
        Ok(ServerMessage::AvatarJoined {
            avatar_id,
            did,
            display_name,
            local_id,
        }) => push_presence(
            inbox,
            PresenceInbound::AvatarJoined {
                local_id,
                avatar_id,
                did,
                display_name,
            },
        ),
        Ok(ServerMessage::AvatarLeft { avatar_id, .. }) => {
            push_presence(inbox, PresenceInbound::AvatarLeft { avatar_id })
        }
        Ok(_) => {} // stray challenge/joined after handshake — ignore
        Err(e) => warn!(err = %e, "presence room event decode failed"),
    }
}

fn build_signer(secret_hex: &str) -> Result<NostrSigner, String> {
    if secret_hex.trim().is_empty() {
        Ok(NostrSigner::generate())
    } else {
        NostrSigner::from_secret_hex(secret_hex).map_err(|e| e.to_string())
    }
}

fn push_presence(inbox: &Arc<Mutex<VecDeque<PresenceInbound>>>, ev: PresenceInbound) {
    if let Ok(mut q) = inbox.lock() {
        q.push_back(ev);
    }
}

// --- WsTransport over a tungstenite socket -----------------------------------

/// Real WebSocket transport. The read half holds an async mutex across `recv`
/// (only the pump calls it, never concurrently); the write half is a separate
/// async mutex so outbound poses and auto-pongs serialise without blocking reads.
pub struct TungsteniteWsTransport {
    sink: AsyncMutex<SplitSink<WsConn, Message>>,
    stream: AsyncMutex<SplitStream<WsConn>>,
}

impl TungsteniteWsTransport {
    pub fn new(ws: WsConn) -> Self {
        let (sink, stream) = ws.split();
        Self {
            sink: AsyncMutex::new(sink),
            stream: AsyncMutex::new(stream),
        }
    }
}

#[async_trait]
impl WsTransport for TungsteniteWsTransport {
    async fn send_binary(&self, payload: Bytes) -> Result<(), TransportError> {
        self.sink
            .lock()
            .await
            .send(Message::Binary(payload.to_vec()))
            .await
            .map_err(|e| TransportError::Send(e.to_string()))
    }

    async fn send_text(&self, payload: String) -> Result<(), TransportError> {
        self.sink
            .lock()
            .await
            .send(Message::Text(payload))
            .await
            .map_err(|e| TransportError::Send(e.to_string()))
    }

    async fn recv(&self) -> Result<WsMessage, TransportError> {
        let mut stream = self.stream.lock().await;
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(t))) => return Ok(WsMessage::Text(t)),
                Some(Ok(Message::Binary(b))) => return Ok(WsMessage::Binary(b)),
                Some(Ok(Message::Ping(p))) => {
                    let _ = self.sink.lock().await.send(Message::Pong(p)).await;
                }
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => continue,
                Some(Ok(Message::Close(_))) | None => return Ok(WsMessage::Close),
                Some(Err(e)) => return Err(TransportError::Recv(e.to_string())),
            }
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.sink
            .lock()
            .await
            .send(Message::Close(None))
            .await
            .map_err(|e| TransportError::Send(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Signer;

    #[test]
    fn build_signer_empty_is_ephemeral() {
        let a = build_signer("").unwrap();
        let b = build_signer("").unwrap();
        assert_ne!(a.pubkey_hex(), b.pubkey_hex());
    }

    #[test]
    fn build_signer_from_hex_is_stable() {
        let s1 = build_signer("").unwrap();
        let hex = s1.secret_hex();
        let s2 = build_signer(&hex).unwrap();
        assert_eq!(s1.pubkey_hex(), s2.pubkey_hex());
    }

    #[test]
    fn build_signer_rejects_bad_hex() {
        assert!(build_signer("nothex").is_err());
    }
}
