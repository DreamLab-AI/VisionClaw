//! `GET /ws/presence?repo=`: XR avatar presence (the Godot client's
//! `PresenceClientNode::join`).
//!
//! Challenge → auth → joined, then binary `0x43` poses relayed between the
//! members of a room as sibling frames, and `avatar_joined`/`avatar_left` text
//! events. Protocol, bounds and the open-auth policy:
//! [`codescape_server::xr_presence`]. Unknown text messages are ignored.
//!
//! Each XR member is also a human actor in the repo's registry, so desktops list
//! it with the other collaborators. Its label is the claimed pubkey hex (a
//! persisted id), or a per-boot `guest-N` without one. Joining records one
//! `focus` presence event on no node. Poses heat no node: they touch the actor's
//! `last_seen` at most once a second ([`TOUCH_EVERY`]) and re-announce it to
//! desktops at most every [`ANNOUNCE_EVERY`], so 64 headsets cannot flood `/ws/events`.

use std::time::{Duration, Instant};

use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{AggregatedMessage, CloseCode, CloseReason, Session};
use codescape_collab::ActorKind;
use codescape_model::events::{PresenceEvent, PresenceKind};
use codescape_server::now_ms;
use codescape_server::xr_presence::{
    decode_pose, display_name_of, room_of, ClientMessage, ClientMetadata, Identity, Outbound,
    PoseRate, ServerMessage,
};
use futures_util::StreamExt;

use super::ws::{open, register_actor, source_ip, Heartbeat, Opened, WsQuery};
use crate::engine::RepoEngine;
use crate::state::AppState;

/// Time a client has to answer the challenge.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Minimum gap between `last_seen` touches from poses.
pub const TOUCH_EVERY: Duration = Duration::from_secs(1);
/// Minimum gap between desktop re-announcements of a posing XR member.
pub const ANNOUNCE_EVERY: Duration = Duration::from_secs(30);

/// A `focus` presence on no node: "this actor is here", nothing to heat.
fn announce(repo: &RepoEngine, actor: u32, label: &str) {
    repo.record_presence(PresenceEvent {
        actor,
        actor_label: label.to_string(),
        actor_kind: ActorKind::Human.as_str().into(),
        kind: PresenceKind::Focus,
        node_ids: vec![],
        paths: vec![],
        text: None,
        ts_ms: now_ms(),
    });
}

/// `GET /ws/presence`.
pub async fn presence(
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
    let source = source_ip(&st, &req);
    let conn = st.conn_id();
    let st = st.into_inner();
    actix_web::rt::spawn(async move {
        let _slot = slot;
        presence_session(session, stream, st, repo, source, conn).await;
    });
    Ok(res)
}

async fn close(session: Session, code: CloseCode, why: &str) {
    let _ = session
        .close(Some(CloseReason {
            code,
            description: Some(why.to_string()),
        }))
        .await;
}

/// The `auth` fields, or `None` when the socket closed or timed out first.
async fn await_auth(
    session: &mut Session,
    inbound: &mut actix_ws::AggregatedMessageStream,
) -> Option<(Option<String>, Option<String>, ClientMetadata)> {
    let deadline = tokio::time::sleep(AUTH_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            msg = inbound.next() => match msg? .ok()? {
                AggregatedMessage::Text(t) => {
                    // Signature accepted unverified until P6; unknown types are ignored.
                    if let Some(ClientMessage::Auth { did, room_id, metadata, .. }) = ClientMessage::parse(&t) {
                        return Some((did, room_id, metadata.unwrap_or_default()));
                    }
                }
                AggregatedMessage::Ping(p) => session.pong(&p).await.ok()?,
                AggregatedMessage::Close(_) => return None,
                _ => {}
            },
            _ = &mut deadline => return None,
        }
    }
}

async fn presence_session(
    mut session: Session,
    mut inbound: actix_ws::AggregatedMessageStream,
    st: std::sync::Arc<AppState>,
    repo: std::sync::Arc<RepoEngine>,
    source: Option<std::net::IpAddr>,
    conn: u64,
) {
    let challenge = match ServerMessage::challenge() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "presence: no OS randomness for the challenge");
            return close(session, CloseCode::Error, "server error").await;
        }
    };
    if session.text(challenge.to_json()).await.is_err() {
        return;
    }
    let Some((did, room_id, metadata)) = await_auth(&mut session, &mut inbound).await else {
        return close(session, CloseCode::Policy, "no auth").await;
    };
    let Some(room) = room_of(room_id.as_deref()) else {
        return close(session, CloseCode::Policy, "bad room_id").await;
    };
    let who = Identity::resolve(did.as_deref(), &format!("{}:{conn}", now_ms()));
    let label = who
        .pubkey_hex
        .clone()
        .unwrap_or_else(|| st.anon_label("guest"));
    let name = display_name_of(metadata.display_name.as_deref(), &label);
    // Desktop halo; XR presence still works when the registry refuses (rate, capacity).
    let actor = match register_actor(&st, &repo, &label, ActorKind::Human, source) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!(error = %e, "presence: XR member not shown on the desktop");
            None
        }
    };
    let mut member = match st.presence.join(&room, &who, &name, metadata.model_uri) {
        Ok(m) => m,
        Err(e) => return close(session, CloseCode::Policy, &e.to_string()).await,
    };
    tracing::info!(room = %room, local_id = member.local_id, actor = ?actor, "presence joined");
    if session.text(member.joined.to_json()).await.is_err() {
        return;
    }
    if let Some(a) = actor {
        announce(&repo, a, &label);
    }

    let mut rate = PoseRate::new(Instant::now());
    let mut last_touch = Instant::now();
    let mut last_announce = Instant::now();
    let mut hb = Heartbeat::new();
    loop {
        if st.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            msg = inbound.next() => {
                let Some(Ok(msg)) = msg else { break };
                hb.seen();
                match msg {
                    AggregatedMessage::Binary(b) => {
                        let Ok(pose) = decode_pose(&b) else { continue };
                        let now = Instant::now();
                        if !rate.allow_at(now) {
                            continue;
                        }
                        member.relay(&pose);
                        if let Some(a) = actor {
                            if now.duration_since(last_announce) >= ANNOUNCE_EVERY {
                                last_announce = now;
                                last_touch = now;
                                announce(&repo, a, &label);
                            } else if now.duration_since(last_touch) >= TOUCH_EVERY {
                                last_touch = now;
                                repo.admit_actor(a, &label, ActorKind::Human, now_ms());
                            }
                        }
                    }
                    AggregatedMessage::Ping(p) => { if session.pong(&p).await.is_err() { break } }
                    AggregatedMessage::Close(_) => break,
                    _ => {}
                }
            }
            out = member.rx.recv() => match out {
                Some(Outbound::Text(t)) => { if session.text(t).await.is_err() { break } }
                Some(Outbound::Binary(b)) => { if session.binary(b).await.is_err() { break } }
                Some(Outbound::Close(why)) => {
                    drop(member);
                    return close(session, CloseCode::Policy, &why).await;
                }
                None => break,
            },
            _ = hb.ticker.tick() => {
                if !hb.beat(&mut session).await { break }
            }
        }
    }
    // Leaving announces `avatar_left` to the rest of the room.
    drop(member);
    let _ = session.close(None).await;
}
