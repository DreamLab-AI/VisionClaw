//! tokio-tungstenite transport: the only place real sockets are opened.
//!
//! Two pumps run on the shared [`crate::runtime`]: [`spawn_graph_stream`] for the
//! `/wss` V3 graph position stream (large topology frames parse on the blocking
//! pool, [`classify_text_off_runtime`]) and [`spawn_presence`] for the `/ws/presence`
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
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    client_async_with_config, connect_async_with_config, MaybeTlsStream, WebSocketStream,
};

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
use crate::liveness::{Liveness, LivenessAction, LivenessConfig, ReadClock, ReadProgress};
use crate::ports::{TransportError, WsMessage, WsTransport};
use crate::presence::{decode_sibling_frame, PresenceClient, PresenceInbound, ServerMessage};
use crate::runtime::runtime;
use crate::signer::NostrSigner;

const GRAPH_REQUEST_INITIAL: &str = r#"{"type":"requestInitialData"}"#;

/// Build the same signed upgrade a browser uses. Credentials never enter a URL.
fn graph_upgrade_request(
    url: &str,
    secret: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let scheme = match request.uri().scheme_str() {
        Some("ws") => "http",
        Some("wss") => "https",
        _ => return Err("graph socket requires ws or wss".into()),
    };
    let authority = request
        .uri()
        .authority()
        .ok_or("missing socket authority")?;
    let origin = format!("{scheme}://{authority}");
    let signed_url = format!(
        "{origin}{}",
        request.uri().path_and_query().map_or("/", |p| p.as_str())
    );
    request
        .headers_mut()
        .insert("Origin", origin.parse().map_err(|_| "invalid Origin")?);
    let protocols = if secret.trim().is_empty() {
        "visionclaw".to_string()
    } else {
        let signer = build_signer(secret).map_err(|e| e.to_string())?;
        let header = signer.nip98_http_authorization(&signed_url, "GET");
        let carrier = header
            .strip_prefix("Nostr ")
            .ok_or("invalid signer header")?
            .trim_end_matches('=')
            .replace('+', "-")
            .replace('/', "_");
        format!("visionclaw, nostr.{carrier}")
    };
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        protocols.parse().map_err(|_| "invalid signed protocol")?,
    );
    Ok(request)
}

/// A message to the graph socket. Text carries the server-authoritative interactions (drag, pin,
/// topology refetch); Binary carries the position acknowledgement (ADR-2141), which the server
/// answers with this client's next chunk.
#[derive(Debug, Clone)]
pub enum Outbound {
    Text(String),
    Binary(Vec<u8>),
}

type WsConn = WebSocketStream<MaybeTlsStream<TcpStream>>;
/// The graph socket: plain TCP under a [`ReadClock`], so the pump's read deadline
/// counts bytes, not whole messages.
type GraphConn = WebSocketStream<ReadClock<TcpStream>>;

/// Open the graph socket over a [`ReadClock`]-wrapped TCP stream. This build has no
/// TLS backend (tokio-tungstenite without a TLS feature), so `wss` is refused here
/// exactly as `connect_async` would refuse it.
async fn connect_graph(
    request: tokio_tungstenite::tungstenite::http::Request<()>,
    progress: Arc<ReadProgress>,
) -> Result<
    (
        GraphConn,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    String,
> {
    let uri = request.uri();
    if uri.scheme_str() != Some("ws") {
        return Err("graph socket: only ws:// is supported by this build (no TLS backend)".into());
    }
    let host = uri.host().ok_or("missing socket host")?.to_owned();
    let port = uri.port_u16().unwrap_or(80);
    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| e.to_string())?;
    progress.touch(std::time::Instant::now());
    client_async_with_config(request, ReadClock::new(tcp, progress), Some(ws_config()))
        .await
        .map_err(|e| e.to_string())
}

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

/// Connect to `/wss`, request the topology, and push every frame into `inbox` for
/// `BinaryProtocolClient::poll` to decode. There is no position subscription: the server streams
/// every registered session its positions and waits for this client's acknowledgements.
/// Returns the outbound channel used for server-authoritative interactions (node drag/pin) and
/// the position acknowledgements. A configured identity signs the HTTP upgrade;
/// release servers authenticate before opening the graph socket.
pub fn spawn_graph_stream(
    url: String,
    nostr_secret_hex: String,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
) -> (ConnHandle, mpsc::UnboundedSender<Outbound>) {
    spawn_graph_stream_with(url, nostr_secret_hex, inbox, LivenessConfig::default())
}

/// [`spawn_graph_stream`] with explicit liveness timings (tests inject short ones).
/// The pump pings every `liveness.ping_every` and, once nothing at all has been read
/// for `liveness.deadline`, closes the socket and reports `Disconnected`, so the
/// scene's backoff redials a half-open connection on its own (audit D2).
pub fn spawn_graph_stream_with(
    url: String,
    nostr_secret_hex: String,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
    liveness: LivenessConfig,
) -> (ConnHandle, mpsc::UnboundedSender<Outbound>) {
    let (tx, rx) = mpsc::unbounded_channel::<Outbound>();
    let handle = runtime().spawn(async move {
        graph_pump(url, nostr_secret_hex, inbox, rx, liveness).await;
    });
    (ConnHandle { handle }, tx)
}

async fn graph_pump(
    url: String,
    nostr_secret_hex: String,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
    outbound: mpsc::UnboundedReceiver<Outbound>,
    liveness: LivenessConfig,
) {
    let request = match graph_upgrade_request(&url, &nostr_secret_hex) {
        Ok(request) => request,
        Err(error) => {
            error!("graph stream: invalid upgrade request: {error}");
            push_graph(&inbox, GraphInbound::Disconnected);
            return;
        }
    };
    let progress = ReadProgress::new(std::time::Instant::now());
    match connect_graph(request, progress.clone()).await {
        Ok((ws, response)) => {
            let (mut sink, mut stream) = ws.split();
            if sink
                .send(Message::Text(GRAPH_REQUEST_INITIAL.to_owned()))
                .await
                .is_err()
            {
                error!("graph stream: failed to send the topology request");
            } else {
                // Only now, once the topology request is on the wire, is
                // the stream usable — surface Connected here (not on bare TCP
                // connect) so the scene's reconnect FSM never treats a half-open
                // socket as live (#3d).
                push_graph(&inbox, GraphInbound::Connected);
                // Codescape's first-message auth predates signed subprotocols.
                // A server selecting visionclaw has authenticated at upgrade;
                // otherwise retain the signed first-message consumer contract.
                if response.headers().get("Sec-WebSocket-Protocol").is_none()
                    && !nostr_secret_hex.trim().is_empty()
                {
                    match build_signer(&nostr_secret_hex) {
                        Ok(signer) => {
                            if sink
                                .send(Message::Text(signer.nip98_authenticate_json(&url, "GET")))
                                .await
                                .is_err()
                            {
                                warn!("graph stream: failed to send first-message authentication");
                            }
                        }
                        Err(error) => warn!(%error, "graph stream: signer unavailable"),
                    }
                }
                pump_graph_messages(
                    &mut sink,
                    &mut stream,
                    &inbox,
                    outbound,
                    &progress,
                    liveness,
                )
                .await;
            }
        }
        Err(e) => error!(err = %e, url = %url, "graph stream connect failed"),
    }
    push_graph(&inbox, GraphInbound::Disconnected);
}

async fn pump_graph_messages(
    sink: &mut SplitSink<GraphConn, Message>,
    stream: &mut SplitStream<GraphConn>,
    inbox: &Arc<Mutex<VecDeque<GraphInbound>>>,
    mut outbound: mpsc::UnboundedReceiver<Outbound>,
    progress: &ReadProgress,
    liveness: LivenessConfig,
) {
    let mut live = Liveness::new(liveness, std::time::Instant::now());
    loop {
        let wake = match live.next(std::time::Instant::now(), progress.last_read()) {
            LivenessAction::Expired => {
                warn!(
                    deadline_ms = liveness.deadline.as_millis() as u64,
                    "graph stream: nothing read within the deadline; closing"
                );
                break;
            }
            LivenessAction::Ping => {
                if let Err(e) = sink.send(Message::Ping(Vec::new())).await {
                    warn!(err = %e, "graph stream ping send failed");
                    break;
                }
                continue;
            }
            LivenessAction::Wait(at) => tokio::time::Instant::from_std(at),
        };
        tokio::select! {
            _ = tokio::time::sleep_until(wake) => {}
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Binary(b))) => push_graph(inbox, GraphInbound::Frame(b)),
                    Some(Ok(Message::Text(t))) => {
                        // initialGraphLoad → Topology; every other JSON envelope
                        // (broker:new_case, broker:case_decided, …) is forwarded
                        // verbatim as Text for the scene layer to route by `type`.
                        push_graph(inbox, classify_text_off_runtime(t).await);
                        // A large parse keeps the pump from reading: that time is ours,
                        // not the peer's silence.
                        progress.touch(std::time::Instant::now());
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
                    Some(out) => {
                        let msg = match out {
                            Outbound::Text(t) => Message::Text(t),
                            Outbound::Binary(b) => Message::Binary(b),
                        };
                        if let Err(e) = sink.send(msg).await {
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

/// Text frames at least this large are classified on the blocking pool (see
/// [`classify_text_off_runtime`]). Envelopes below it (acks, broker events) parse in
/// microseconds and stay inline.
pub const PARSE_OFFLOAD_BYTES: usize = 64 * 1024;

/// Classify one `/wss` text frame without holding an async worker for long.
///
/// A full `initialGraphLoad` (14.6 MB at 13k nodes / 145k edges) takes ~0.4 s to parse. The
/// shared runtime has two async workers (`runtime.rs`), so two connections reloading at once
/// used to occupy both and stall every other socket on the runtime for the whole parse (measured
/// 378 ms on a third stream, `tests/two_clients.rs` scenario 2). Large frames now parse on
/// tokio's blocking pool. The pump awaits the result before reading its next message, so the
/// topology still lands in the inbox ahead of the position frames sent after it.
pub async fn classify_text_off_runtime(text: String) -> GraphInbound {
    if text.len() < PARSE_OFFLOAD_BYTES {
        return crate::binary_protocol::classify_graph_text(&text);
    }
    match tokio::task::spawn_blocking(move || crate::binary_protocol::classify_graph_text(&text))
        .await
    {
        Ok(ev) => ev,
        Err(e) => {
            // Only a panic inside the parser (or runtime shutdown) lands here. The frame is
            // lost either way; say so rather than forward a half-state.
            warn!(err = %e, "graph stream: text frame classification failed");
            GraphInbound::Text(String::new())
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

    #[tokio::test]
    // The handshake callback's error type is tungstenite's, not ours.
    #[allow(clippy::result_large_err)]
    async fn graph_upgrade_supplies_origin_and_signed_http_url_before_connecting() {
        use base64::Engine;
        use nostr::JsonUtil;
        let signer = NostrSigner::generate();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap();
        let url = format!("ws://{host}/wss");
        let expected = format!("http://{host}/wss");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    assert_eq!(request.headers()["Origin"], format!("http://{host}"));
                    let protocols = request.headers()["Sec-WebSocket-Protocol"]
                        .to_str()
                        .unwrap();
                    let carrier = protocols
                        .split(", ")
                        .find_map(|p| p.strip_prefix("nostr."))
                        .unwrap();
                    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(carrier)
                        .unwrap();
                    let event = nostr::Event::from_json(json).unwrap();
                    event.verify().unwrap();
                    let tags = serde_json::to_value(&event).unwrap()["tags"].clone();
                    assert!(tags
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tag| tag == &serde_json::json!(["u", expected])));
                    assert!(tags
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tag| tag == &serde_json::json!(["method", "GET"])));
                    Ok(response)
                },
            )
            .await
            .unwrap();
        });
        let request = graph_upgrade_request(&url, &signer.secret_hex()).unwrap();
        connect_async_with_config(request, Some(ws_config()), false)
            .await
            .unwrap();
        server.await.unwrap();
        assert!(graph_upgrade_request("ws://localhost/wss", "malformed").is_err());
    }

    #[tokio::test]
    async fn small_and_large_text_frames_classify_the_same_way() {
        let small = r#"{"type":"broker:new_case","caseId":"c1"}"#.to_owned();
        match classify_text_off_runtime(small.clone()).await {
            GraphInbound::Text(t) => assert_eq!(t, small),
            _ => panic!("a broker envelope is forwarded as text"),
        }
        let nodes: Vec<String> = (0..4000)
            .map(|i| format!(r#"{{"id":{i},"label":"node {i}","node_type":"page"}}"#))
            .collect();
        let big = format!(
            r#"{{"type":"initialGraphLoad","nodes":[{}],"edges":[{{"source_id":1,"target_id":2}}]}}"#,
            nodes.join(",")
        );
        assert!(
            big.len() >= PARSE_OFFLOAD_BYTES,
            "exercises the blocking-pool path"
        );
        match classify_text_off_runtime(big).await {
            GraphInbound::Topology { edges, metas } => {
                assert_eq!(edges.len(), 1);
                assert_eq!(metas.len(), 4000);
            }
            _ => panic!("a large initialGraphLoad is still a topology"),
        }
    }

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
