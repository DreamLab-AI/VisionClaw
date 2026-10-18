//! WebSocket routes.
//!
//! * `/ws/positions?repo=&session=&tier=`: binary V5 frames at `FRAME_HZ`,
//!   keyframe on join and every 10 s, partial frames otherwise, paced by `0x34`
//!   acks. Text `view`/`focus` client events are accepted here too.
//! * `/ws/events?repo=&session=`: JSON [`ServerEvent`]s out, [`ClientEvent`]s in.
//!   A `session` shared with `/ws/positions` links the view, focus and lag.
//! * `/wss?repo=&tier=`: XR emulation. `initialGraphLoad` text frame first
//!   (file tier plus focus symbols by default), then the same binary stream,
//!   plus `0x23` agent actions. `authenticate`, `requestInitialData` and `ping`
//!   are accepted tolerantly.

use std::sync::Arc;
use std::time::Duration;

use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{AggregatedMessage, Session};
use codescape_collab::ActorKind;
use codescape_model::events::{ClientEvent, PresenceEvent, PresenceKind, ServerEvent};
use codescape_model::{NodeKind, Tier};
use codescape_server::xr::{classify_text, pong, InboundText};
use codescape_server::{now_ms, VERSION};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc};

use super::{run_command, verify};
use crate::engine::RepoEngine;
use crate::frames::PositionStream;
use crate::queries::{CommandCtx, Effect};
use crate::state::{parse_tier, AppState, ConnGuard, FloorCaller, SharedView};

/// Largest inbound WebSocket message.
pub const MAX_INBOUND: usize = 1 << 20;
/// Minimum gap between topology resends on `/wss` after deltas.
pub const TOPOLOGY_RESEND_GAP: Duration = Duration::from_secs(2);

/// Query parameters shared by the socket routes.
#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    /// Repository label.
    pub repo: Option<String>,
    /// Links `/ws/events` and `/ws/positions` of one client.
    pub session: Option<String>,
    /// Initial LOD tier.
    pub tier: Option<String>,
    /// Display name (events, voice).
    pub name: Option<String>,
}

fn file_of(repo: &RepoEngine, id: u32) -> Option<u32> {
    let p = repo.current();
    let n = &p.snapshot.nodes[*p.index.get(&id)?];
    match n.kind {
        NodeKind::File => Some(id),
        NodeKind::Directory => None,
        _ => Some(n.parent).filter(|&p| p != 0),
    }
}

/// Apply a `view`/`focus` client event to a shared view. Returns `true` if handled.
fn apply_view_event(repo: &RepoEngine, view: &SharedView, ev: &ClientEvent) -> bool {
    match ev {
        ClientEvent::View { tier, focus_files } => {
            if let Some(t) = parse_tier(tier) {
                view.set_view(t, focus_files.iter().copied().take(256).collect());
            }
            true
        }
        ClientEvent::Focus { node_id } => {
            let f = (*node_id != 0).then_some(*node_id);
            view.set_focus(f, f.and_then(|id| file_of(repo, id)));
            if let Some(id) = f {
                repo.focus_layout(vec![id]);
            }
            true
        }
        _ => false,
    }
}

/// Refuse a cross-site handshake (403) before any upgrade or allocation.
pub fn check_origin(st: &AppState, req: &HttpRequest) -> Result<(), actix_web::Error> {
    let header = |name| {
        req.headers()
            .get(name)
            .and_then(|h: &actix_web::http::header::HeaderValue| h.to_str().ok())
    };
    let origin = header(actix_web::http::header::ORIGIN);
    // `Host`, or the trusted `X-Forwarded-Host` under --trust-proxy.
    let peer = req.peer_addr().map(|a| a.ip());
    if st
        .origins
        .allows(origin, st.proxy().host(req.headers(), peer))
    {
        return Ok(());
    }
    tracing::warn!(origin = ?origin, path = %req.path(), "WebSocket handshake refused: origin not allowed");
    Err(actix_web::error::InternalError::from_response(
        "origin not allowed",
        HttpResponse::Forbidden().json(serde_json::json!({"error": "origin not allowed"})),
    )
    .into())
}

/// The address per-client limits charge: the TCP peer, or the last
/// `X-Forwarded-For` hop under `--trust-proxy` (never a forwarded header otherwise).
pub fn source_ip(st: &AppState, req: &HttpRequest) -> Option<std::net::IpAddr> {
    st.proxy()
        .client_ip(req.headers(), req.peer_addr().map(|a| a.ip()))
}

/// Most `hello` renames one `/ws/events` connection may make.
pub const MAX_RENAMES: u32 = 4;
/// Marks a connection's command slot free when dropped (also on panic or cancellation).
pub struct InFlight(pub Arc<std::sync::atomic::AtomicBool>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Server ping interval on every socket.
pub const PING_EVERY: Duration = Duration::from_secs(20);
/// A socket silent this long (no message, no pong) is closed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Ping cadence and idle detection for one socket. Any inbound frame,
/// including a pong, counts as life.
pub struct Heartbeat {
    last_inbound: tokio::time::Instant,
    /// Fires every [`PING_EVERY`].
    pub ticker: tokio::time::Interval,
}

impl Heartbeat {
    /// Start now.
    pub fn new() -> Self {
        let mut ticker =
            tokio::time::interval_at(tokio::time::Instant::now() + PING_EVERY, PING_EVERY);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            last_inbound: tokio::time::Instant::now(),
            ticker,
        }
    }
    /// Record inbound traffic.
    pub fn seen(&mut self) {
        self.last_inbound = tokio::time::Instant::now();
    }
    /// On a tick: `false` when the peer has been idle past [`IDLE_TIMEOUT`] (close it),
    /// otherwise ping and report whether the ping was sent.
    pub async fn beat(&mut self, session: &mut Session) -> bool {
        if self.last_inbound.elapsed() >= IDLE_TIMEOUT {
            tracing::debug!("closing idle socket");
            return false;
        }
        session.ping(b"").await.is_ok()
    }
}

/// A WebSocket connection's resources: the upgraded session, its message
/// stream, the repo, and the global connection slot it holds until dropped.
pub struct Opened {
    /// Handshake response.
    pub res: HttpResponse,
    /// Outbound half.
    pub session: Session,
    /// Inbound messages.
    pub stream: actix_ws::AggregatedMessageStream,
    /// Repository served.
    pub repo: Arc<RepoEngine>,
    /// Connection slot; dropping it frees the slot.
    pub slot: ConnGuard,
}

/// Take a connection slot or answer 503.
pub fn take_slot(st: &AppState) -> Result<ConnGuard, actix_web::Error> {
    st.try_connect().ok_or_else(|| {
        actix_web::error::InternalError::from_response(
            "too many connections",
            HttpResponse::ServiceUnavailable()
                .insert_header(("Retry-After", "5"))
                .json(serde_json::json!({"error": "too many connections"})),
        )
        .into()
    })
}

pub(crate) fn open(
    req: &HttpRequest,
    body: web::Payload,
    st: &AppState,
    q: &WsQuery,
) -> Result<Opened, actix_web::Error> {
    if let Err(r) = verify(st, req) {
        return Err(actix_web::error::InternalError::from_response("unauthorized", *r).into());
    }
    check_origin(st, req)?;
    let repo = st
        .repo(q.repo.as_deref())
        .ok_or_else(|| actix_web::error::ErrorNotFound("unknown repo"))?;
    let slot = take_slot(st)?;
    let (res, session, stream) = actix_ws::handle(req, body)?;
    let stream = stream
        .max_frame_size(MAX_INBOUND)
        .aggregate_continuations()
        .max_continuation_size(MAX_INBOUND);
    Ok(Opened {
        res,
        session,
        stream,
        repo,
        slot,
    })
}

/// `GET /ws/positions`.
pub async fn positions(
    req: HttpRequest,
    body: web::Payload,
    st: web::Data<AppState>,
    q: web::Query<WsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let Opened {
        res,
        session,
        stream,
        repo,
        slot,
    } = open(&req, body, &st, &q)?;
    let tier = q.tier.as_deref().and_then(parse_tier).unwrap_or(Tier::L2);
    let view = st.view_for(q.session.as_deref(), tier);
    let hz = st.cfg.frame_hz;
    let shutdown = st.shutdown.clone();
    actix_web::rt::spawn(async move {
        let _slot = slot;
        position_loop(session, stream, repo, view, hz, None, shutdown).await;
    });
    Ok(res)
}

/// `GET /wss` (XR emulation).
pub async fn wss(
    req: HttpRequest,
    body: web::Payload,
    st: web::Data<AppState>,
    q: web::Query<WsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let Opened {
        res,
        session,
        stream,
        repo,
        slot,
    } = open(&req, body, &st, &q)?;
    let tier = q.tier.as_deref().and_then(parse_tier).unwrap_or(Tier::L1);
    let view = Arc::new(SharedView::new(tier));
    let hz = st.cfg.frame_hz;
    let shutdown = st.shutdown.clone();
    // NIP-98 (P6) compares the signed URL with the public origin, never with Host.
    let url = st
        .public
        .url_for(req.headers(), req.peer_addr().map(|a| a.ip()), req.uri());
    let st2 = st.clone();
    actix_web::rt::spawn(async move {
        let _slot = slot;
        position_loop(
            session,
            stream,
            repo,
            view,
            hz,
            Some(XrExtras { st: st2, url }),
            shutdown,
        )
        .await;
    });
    Ok(res)
}

struct XrExtras {
    st: web::Data<AppState>,
    url: String,
}

async fn send_initial_load(
    session: &mut Session,
    repo: &Arc<RepoEngine>,
    stream: &mut PositionStream,
) -> bool {
    let mut ids: Vec<u32> = stream.projection_ids().as_ref().clone();
    ids.sort_unstable();
    // Cached per (revision, projection): sockets with the same view share one body.
    let r = repo.clone();
    let text = match web::block(move || r.initial_load_body(&ids)).await {
        Ok(t) => t,
        Err(_) => return true,
    };
    tracing::debug!(bytes = text.len(), "initialGraphLoad sent");
    stream.force_keyframe();
    session.text(text.as_str().to_owned()).await.is_ok()
}

async fn position_loop(
    mut session: Session,
    mut inbound: actix_ws::AggregatedMessageStream,
    repo: Arc<RepoEngine>,
    view: Arc<SharedView>,
    hz: u32,
    xr: Option<XrExtras>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut stream = PositionStream::new(repo.clone(), view.clone());
    let mut ticker = tokio::time::interval(Duration::from_millis(1000 / u64::from(hz.max(1))));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut actions = repo.actions.subscribe();
    let mut events = repo.events.subscribe();
    let is_xr = xr.is_some();
    let mut topology_dirty = false;
    let mut last_topology = tokio::time::Instant::now();
    let mut hb = Heartbeat::new();
    if is_xr && !send_initial_load(&mut session, &repo, &mut stream).await {
        return;
    }
    loop {
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            msg = inbound.next() => {
                let Some(Ok(msg)) = msg else { break };
                hb.seen();
                match msg {
                    AggregatedMessage::Binary(b) => {
                        if !stream.on_binary(&b) {
                            tracing::debug!(len = b.len(), "unrecognised binary message ignored");
                        }
                    }
                    AggregatedMessage::Text(t) => match classify_text(&t) {
                        InboundText::Authenticate(env) => {
                            if let Some(x) = &xr {
                                match x.st.verifier.verify_ws_envelope(&x.url, &env) {
                                    Ok(p) => tracing::info!(pubkey = ?p.pubkey, verifier = x.st.verifier.name(), "authenticate envelope accepted"),
                                    Err(e) => {
                                        tracing::warn!(error = %e, "authenticate envelope rejected");
                                        let _ = session.close(None).await;
                                        return;
                                    }
                                }
                            }
                        }
                        InboundText::RequestInitialData => {
                            // At most one topology frame per TOPOLOGY_RESEND_GAP; a request
                            // inside the gap is served by the next tick after it.
                            if is_xr {
                                if last_topology.elapsed() >= TOPOLOGY_RESEND_GAP {
                                    last_topology = tokio::time::Instant::now();
                                    topology_dirty = false;
                                    if !send_initial_load(&mut session, &repo, &mut stream).await {
                                        break;
                                    }
                                } else {
                                    topology_dirty = true;
                                }
                            }
                            stream.force_keyframe();
                        }
                        InboundText::Ping(ts) => {
                            if session.text(pong(ts, now_ms()).to_string()).await.is_err() {
                                break;
                            }
                        }
                        InboundText::Client(ev) => {
                            let before = view.get().0.focus_files.len();
                            apply_view_event(&repo, &view, &ev);
                            if is_xr && view.get().0.focus_files.len() != before {
                                topology_dirty = true;
                            }
                        }
                        InboundText::Other(kind) => tracing::debug!(%kind, "text message ignored"),
                    },
                    AggregatedMessage::Ping(p) => {
                        if session.pong(&p).await.is_err() {
                            break;
                        }
                    }
                    AggregatedMessage::Close(_) => break,
                    AggregatedMessage::Pong(_) => {}
                }
            }
            _ = ticker.tick() => {
                if is_xr && topology_dirty && last_topology.elapsed() >= TOPOLOGY_RESEND_GAP {
                    topology_dirty = false;
                    last_topology = tokio::time::Instant::now();
                    if !send_initial_load(&mut session, &repo, &mut stream).await {
                        break;
                    }
                }
                if let Some(frame) = stream.tick(now_ms()) {
                    if session.binary(frame).await.is_err() {
                        break;
                    }
                }
            }
            _ = hb.ticker.tick() => {
                if !hb.beat(&mut session).await {
                    break;
                }
            }
            a = actions.recv(), if is_xr => match a {
                Ok(frame) => {
                    if session.binary(frame.as_ref().clone()).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => tracing::debug!(n, "agent actions lagged"),
                Err(broadcast::error::RecvError::Closed) => break,
            },
            e = events.recv(), if is_xr => match e {
                Ok(ev) => {
                    if matches!(ev.as_ref(), ServerEvent::Delta(_)) {
                        topology_dirty = true;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => topology_dirty = true,
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    tracing::debug!(
        frames = stream.frames_sent,
        xr = is_xr,
        "position socket closed"
    );
    let _ = session.close(None).await;
}

/// `GET /ws/events`.
pub async fn events(
    req: HttpRequest,
    body: web::Payload,
    st: web::Data<AppState>,
    q: web::Query<WsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let Opened {
        res,
        session,
        stream,
        repo,
        slot,
    } = open(&req, body, &st, &q)?;
    let view = st.view_for(q.session.as_deref(), Tier::L2);
    let name = q
        .name
        .clone()
        .filter(|n| !n.trim().is_empty() && n.len() <= 128)
        .unwrap_or_else(|| st.anon_label("guest"));
    let st2 = st.clone();
    let source = source_ip(&st, &req);
    let session_key = q.session.clone();
    actix_web::rt::spawn(async move {
        let _slot = slot;
        events_loop(session, stream, st2, repo, view, name, source, session_key).await;
    });
    Ok(res)
}

fn hello(repo: &RepoEngine) -> ServerEvent {
    let p = repo.current();
    ServerEvent::Hello {
        repo: repo.label.clone(),
        epoch: p.snapshot.epoch,
        revision: p.snapshot.revision,
        nodes: p.snapshot.nodes.len(),
        edges: p.snapshot.edges.len(),
        engine: repo.layout.read().engine.clone(),
        version: VERSION.to_string(),
    }
}

async fn send_event(session: &mut Session, ev: &ServerEvent) -> bool {
    match serde_json::to_string(ev) {
        Ok(t) => session.text(t).await.is_ok(),
        Err(_) => true,
    }
}

/// Allocate (rate-limited per `source`) and register an actor in this repo's registry.
/// Call only after the WebSocket upgrade, so a refused handshake allocates nothing.
pub fn register_actor(
    st: &AppState,
    repo: &RepoEngine,
    label: &str,
    kind: ActorKind,
    source: Option<std::net::IpAddr>,
) -> Result<u32, String> {
    let id = st.actor_id(label, source).map_err(|e| e.to_string())?;
    if !repo.admit_actor(id, label, kind, now_ms()) {
        return Err("too many active collaborators; try again later".into());
    }
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn events_loop(
    mut session: Session,
    mut inbound: actix_ws::AggregatedMessageStream,
    st: web::Data<AppState>,
    repo: Arc<RepoEngine>,
    view: Arc<SharedView>,
    name: String,
    source: Option<std::net::IpAddr>,
    session_key: Option<String>,
) {
    let mut rx = repo.events.subscribe();
    if !send_event(&mut session, &hello(&repo)).await {
        return;
    }
    // Bind first: a temporary guard would be held across the await.
    let floor = st.floor.lock().state();
    let _ = send_event(&mut session, &ServerEvent::Floor(floor)).await;
    let mut label = name;
    let mut kind = ActorKind::Human;
    let mut actor = match register_actor(&st, &repo, &label, kind, source) {
        Ok(a) => Some(a),
        Err(message) => {
            let _ = send_event(&mut session, &ServerEvent::Error { message }).await;
            None
        }
    };
    let mut renames = 0u32;
    let mut hb = Heartbeat::new();
    // One command at a time per connection; cleared by the task when it answers.
    let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ctx = Arc::new(Mutex::new(CommandCtx {
        actor: actor.unwrap_or(0),
        ..Default::default()
    }));
    let (res_tx, mut res_rx) = mpsc::channel::<ServerEvent>(16);
    let mut lag_tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_lag = 0u32;
    loop {
        if st.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            msg = inbound.next() => {
                let Some(Ok(msg)) = msg else { break };
                hb.seen();
                let text = match msg {
                    AggregatedMessage::Text(t) => t,
                    AggregatedMessage::Ping(p) => { if session.pong(&p).await.is_err() { break } continue }
                    AggregatedMessage::Close(_) => break,
                    _ => continue,
                };
                let ev: ClientEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        if !send_event(&mut session, &ServerEvent::Error { message: format!("bad event: {e}") }).await { break }
                        continue;
                    }
                };
                match ev {
                    ClientEvent::Hello { name, kind: k } => {
                        if name.trim().is_empty() || name.len() > 128 || name == label {
                            continue;
                        }
                        if renames >= MAX_RENAMES {
                            if !send_event(&mut session, &ServerEvent::Error { message: format!("at most {MAX_RENAMES} renames per connection") }).await { break }
                            continue;
                        }
                        renames += 1;
                        let k = ActorKind::from_wire(&k);
                        match register_actor(&st, &repo, &name, k, source) {
                            Ok(a) => {
                                label = name;
                                kind = k;
                                actor = Some(a);
                                ctx.lock().actor = a;
                            }
                            Err(message) => {
                                if !send_event(&mut session, &ServerEvent::Error { message }).await { break }
                            }
                        }
                    }
                    ClientEvent::Command { text } => {
                        if in_flight.swap(true, std::sync::atomic::Ordering::AcqRel) {
                            if !send_event(&mut session, &ServerEvent::Error { message: "busy: one command at a time".into() }).await { break }
                            continue;
                        }
                        let done = InFlight(in_flight.clone());
                        let (st, repo, ctx, tx, view) = (st.clone(), repo.clone(), ctx.clone(), res_tx.clone(), view.clone());
                        let (label, actor) = (label.clone(), actor);
                        tokio::spawn(async move {
                            let _done = done;
                            let ev = match run_command(&st, &repo, &text, ctx).await {
                                Ok((r, eff)) => {
                                    if let Effect::Focus(id) = eff {
                                        view.set_focus(Some(id), file_of(&repo, id));
                                        repo.focus_layout(vec![id]);
                                        if let Some(a) = actor {
                                            repo.record_presence(PresenceEvent { actor: a, actor_label: label, actor_kind: kind.as_str().into(), kind: PresenceKind::Focus, node_ids: vec![id], paths: vec![], text: None, ts_ms: now_ms() });
                                        }
                                    }
                                    ServerEvent::Query(r)
                                }
                                Err(message) => ServerEvent::Error { message },
                            };
                            let _ = tx.send(ev).await;
                        });
                    }
                    ev @ (ClientEvent::Focus { .. } | ClientEvent::View { .. }) => {
                        apply_view_event(&repo, &view, &ev);
                        if let ClientEvent::Focus { node_id } = ev {
                            ctx.lock().focus = (node_id != 0).then_some(node_id);
                            if let Some(a) = actor {
                                repo.record_presence(PresenceEvent {
                                    actor: a,
                                    actor_label: label.clone(),
                                    actor_kind: kind.as_str().into(),
                                    kind: PresenceKind::Focus,
                                    node_ids: if node_id == 0 { vec![] } else { vec![node_id] },
                                    paths: vec![],
                                    text: None,
                                    ts_ms: now_ms(),
                                });
                            }
                        }
                    }
                    ClientEvent::Floor { request } => {
                        let Some(a) = actor else { continue };
                        let caller = FloorCaller::Events(session_key.as_deref());
                        let result = {
                            let mut f = st.floor.lock();
                            if request { f.request(a, caller, now_ms()) } else { f.release(a, caller, now_ms()) }
                        };
                        match result {
                            Ok(state) => st.broadcast_floor(state),
                            Err(e) => {
                                if !send_event(&mut session, &ServerEvent::Error { message: e.to_string() }).await { break }
                            }
                        }
                    }
                }
            }
            ev = rx.recv() => match ev {
                Ok(ev) => { if !send_event(&mut session, &ev).await { break } }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    if !send_event(&mut session, &ServerEvent::Lag { behind: n.min(u64::from(u32::MAX)) as u32 }).await { break }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            r = res_rx.recv() => {
                if let Some(ev) = r {
                    if !send_event(&mut session, &ev).await { break }
                }
            }
            _ = hb.ticker.tick() => {
                if !hb.beat(&mut session).await { break }
            }
            _ = lag_tick.tick() => {
                let lag = view.lag.load(std::sync::atomic::Ordering::Relaxed);
                if lag != last_lag {
                    last_lag = lag;
                    if lag > 0 && !send_event(&mut session, &ServerEvent::Lag { behind: lag }).await { break }
                }
            }
        }
    }
    // The floor belongs to the voice connection; closing this socket releases nothing.
    let _ = session.close(None).await;
}
