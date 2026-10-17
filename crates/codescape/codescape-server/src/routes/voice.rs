//! `/ws/voice?repo=&name=&session=`: push-to-talk.
//!
//! Client → server: text `{"type":"start"}` (button down) and `{"type":"stop"}`
//! (button up); binary frames of little-endian f32 mono PCM at 24 kHz.
//!
//! Server → client: text `{"type":"floor",…FloorState}`, `{"type":"listening"}`,
//! `{"type":"query",…QueryResult}`, `{"type":"error","message"}`,
//! `{"type":"tts_start","sample_rate":24000,"format":"s16le"}` then binary
//! s16le PCM chunks, then `{"type":"tts_end"}`.
//!
//! The single floor ([`codescape_collab::FloorArbiter`]) gates the ASR: only
//! the holder streams. Captions (partial and final) are broadcast on
//! `/ws/events`. The final transcript runs through the same executor as typed
//! commands, and the answer is spoken back to the speaker only.

use std::sync::Arc;
use std::time::Duration;

use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{AggregatedMessage, Session};
use codescape_collab::ActorKind;
use codescape_model::events::{CaptionEvent, FloorState, PresenceEvent, PresenceKind, ServerEvent};
use codescape_model::Tier;
use codescape_server::now_ms;
use codescape_server::voice::asr::{f32le_samples, AsrSession, AsrUpdate};
use codescape_server::voice::tts::SAMPLE_RATE;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

use super::run_command;
use super::ws::{
    check_origin, register_actor, source_ip, take_slot, Heartbeat, InFlight, WsQuery, MAX_INBOUND,
};
use crate::engine::RepoEngine;
use crate::queries::{CommandCtx, Effect};
use crate::state::{AppState, FloorCaller};

/// Most pre-roll kept while waiting for the floor (2 s at 24 kHz).
pub const PREROLL_SAMPLES: usize = 2 * 24_000;

/// Buffer audio until the ASR opens, keeping only the newest [`PREROLL_SAMPLES`].
fn push_preroll(buf: &mut Vec<f32>, samples: &[f32]) {
    buf.extend_from_slice(samples);
    if buf.len() > PREROLL_SAMPLES {
        let excess = buf.len() - PREROLL_SAMPLES;
        buf.drain(..excess);
    }
}

/// How long the floor reports `busy` after nemotron refused a stream.
pub const BUSY_COOLDOWN: Duration = Duration::from_secs(5);

/// `GET /ws/voice`.
pub async fn voice(
    req: HttpRequest,
    body: web::Payload,
    st: web::Data<AppState>,
    q: web::Query<WsQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    if let Err(r) = super::verify(&st, &req) {
        return Err(actix_web::error::InternalError::from_response("unauthorized", *r).into());
    }
    check_origin(&st, &req)?;
    let repo = st
        .repo(q.repo.as_deref())
        .ok_or_else(|| actix_web::error::ErrorNotFound("unknown repo"))?;
    let label = q
        .name
        .clone()
        .filter(|n| !n.trim().is_empty() && n.len() <= 128)
        .unwrap_or_else(|| st.anon_label("voice"));
    // One voice connection per actor: refuse a second one before upgrading
    // (looked up, never allocated, so a refused handshake costs nothing).
    if let Some(id) = st.lookup_actor(&label) {
        if st.floor.lock().is_claimed(id) {
            return Err(actix_web::error::ErrorConflict(
                "this name already has an open voice connection",
            ));
        }
    }
    let source = source_ip(&req);
    let session_key = q.session.clone();
    let slot = take_slot(&st)?;
    let (res, session, stream) = actix_ws::handle(&req, body)?;
    let stream = stream
        .max_frame_size(MAX_INBOUND)
        .aggregate_continuations()
        .max_continuation_size(MAX_INBOUND);
    let view = st.view_for(q.session.as_deref(), Tier::L2);
    let st2 = st.clone();
    actix_web::rt::spawn(async move {
        let _slot = slot;
        voice_session(session, stream, st2, repo, label, view, source, session_key).await;
    });
    Ok(res)
}

fn floor_json(state: &FloorState) -> String {
    let mut v = serde_json::to_value(state).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.insert("type".into(), "floor".into());
    }
    v.to_string()
}

async fn speak(session: &mut Session, st: &AppState, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    match st.tts.speak(text).await {
        Ok(mut chunks) => {
            if session
                .text(
                    json!({"type":"tts_start","sample_rate":SAMPLE_RATE,"format":"s16le"})
                        .to_string(),
                )
                .await
                .is_err()
            {
                return;
            }
            while let Some(c) = chunks.next().await {
                match c {
                    Ok(b) if !b.is_empty() => {
                        if session.binary(b).await.is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "TTS stream failed");
                        break;
                    }
                }
            }
            let _ = session.text(json!({"type":"tts_end"}).to_string()).await;
        }
        Err(e) => {
            tracing::warn!(error = %e, "TTS unavailable");
            let _ = session
                .text(
                    json!({"type":"error","message": format!("speech output unavailable: {e}")})
                        .to_string(),
                )
                .await;
        }
    }
}

/// After the upgrade: allocate the actor (rate-limited), claim its floor for this
/// connection, run the loop, and drop the claim on close.
#[allow(clippy::too_many_arguments)]
async fn voice_session(
    mut session: Session,
    inbound: actix_ws::AggregatedMessageStream,
    st: web::Data<AppState>,
    repo: Arc<RepoEngine>,
    label: String,
    view: Arc<crate::state::SharedView>,
    source: Option<std::net::IpAddr>,
    session_key: Option<String>,
) {
    let refuse = |message: String| json!({"type": "error", "message": message}).to_string();
    let actor = match register_actor(&st, &repo, &label, ActorKind::Human, source) {
        Ok(a) => a,
        Err(message) => {
            let _ = session.text(refuse(message)).await;
            let _ = session.close(None).await;
            return;
        }
    };
    let conn = st.conn_id();
    if !st.floor.lock().claim(actor, conn, session_key.as_deref()) {
        let _ = session
            .text(refuse(
                "this name already has an open voice connection".into(),
            ))
            .await;
        let _ = session.close(None).await;
        return;
    }
    voice_loop(session, inbound, st.clone(), repo, actor, conn, label, view).await;
    let changed = st.floor.lock().unclaim(actor, conn, now_ms());
    if let Some(state) = changed {
        st.broadcast_floor(state);
    }
}

#[allow(clippy::too_many_arguments)]
async fn voice_loop(
    mut session: Session,
    mut inbound: actix_ws::AggregatedMessageStream,
    st: web::Data<AppState>,
    repo: Arc<RepoEngine>,
    actor: u32,
    conn: u64,
    label: String,
    view: Arc<crate::state::SharedView>,
) {
    let me = FloorCaller::Voice(conn);
    let mut hb = Heartbeat::new();
    let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (upd_tx, mut upd_rx) = mpsc::channel::<AsrUpdate>(64);
    let mut asr: Option<AsrSession> = None;
    let mut held = false;
    // Audio that arrives between `start` and our own `Held` echo (the ASR is not open yet).
    let mut preroll: Vec<f32> = Vec::new();
    let mut events = repo.events.subscribe();
    let ctx = Arc::new(Mutex::new(CommandCtx {
        actor,
        ..Default::default()
    }));
    // Bind first: a temporary guard would be held across the await.
    let initial = st.floor.lock().state();
    let _ = session.text(floor_json(&initial)).await;

    let release = |st: &AppState| {
        let r = st.floor.lock().release(actor, me, now_ms());
        if let Ok(s) = r {
            st.broadcast_floor(s);
        }
    };

    loop {
        if st.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        tokio::select! {
            msg = inbound.next() => {
                let Some(Ok(msg)) = msg else { break };
                hb.seen();
                match msg {
                    AggregatedMessage::Text(t) => {
                        let ty = serde_json::from_str::<serde_json::Value>(&t).ok().and_then(|v| v.get("type").and_then(|x| x.as_str()).map(str::to_string)).unwrap_or_default();
                        match ty.as_str() {
                            "start" if !held => {
                                held = true;
                                let r = st.floor.lock().request(actor, me, now_ms());
                                // The broadcast comes back on this socket's event subscription, which
                                // echoes it to the client and starts the ASR once we hold the floor.
                                if let Ok(state) = r {
                                    st.broadcast_floor(state);
                                }
                            }
                            "stop" if held => {
                                held = false;
                                match asr.as_ref() {
                                    Some(a) => a.finish().await,
                                    None => {
                                        // A quick tap: the floor may already be ours with audio buffered.
                                        let ours = st.floor.lock().holder() == Some(actor);
                                        if ours && !preroll.is_empty() {
                                            match AsrSession::start(&st.egress, &st.cfg.asr_url, upd_tx.clone()).await {
                                                Ok(a) => {
                                                    a.send_audio(std::mem::take(&mut preroll)).await;
                                                    a.finish().await;
                                                    asr = Some(a);
                                                }
                                                Err(e) => {
                                                    tracing::warn!(error = %e, "ASR session failed to start");
                                                    preroll.clear();
                                                    release(&st);
                                                }
                                            }
                                        } else {
                                            preroll.clear();
                                            release(&st);
                                        }
                                    }
                                }
                            }
                            "start" | "stop" => {}
                            other => tracing::debug!(%other, "voice text ignored"),
                        }
                    }
                    AggregatedMessage::Binary(b) => {
                        if !held {
                            continue;
                        }
                        let samples = f32le_samples(&b);
                        match asr.as_ref() {
                            Some(a) => {
                                if !a.send_audio(samples).await {
                                    asr = None;
                                }
                            }
                            None => push_preroll(&mut preroll, &samples),
                        }
                    }
                    AggregatedMessage::Ping(p) => { if session.pong(&p).await.is_err() { break } }
                    AggregatedMessage::Close(_) => break,
                    AggregatedMessage::Pong(_) => {}
                }
            }
            ev = events.recv() => match ev {
                Ok(ev) => if let ServerEvent::Floor(state) = ev.as_ref() {
                    let mine = matches!(state, FloorState::Held { holder, .. } if *holder == actor);
                    if mine && held && asr.is_none() {
                        match AsrSession::start(&st.egress, &st.cfg.asr_url, upd_tx.clone()).await {
                            Ok(a) => {
                                if !preroll.is_empty() {
                                    a.send_audio(std::mem::take(&mut preroll)).await;
                                }
                                asr = Some(a);
                                let _ = session.text(json!({"type":"listening"}).to_string()).await;
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "ASR session failed to start");
                                let _ = session.text(json!({"type":"error","message": format!("speech recognition unavailable: {e}")}).to_string()).await;
                                held = false;
                                preroll.clear();
                                release(&st);
                            }
                        }
                    } else if !mine {
                        // Hold cap expired (Free), the floor moved on, or the ASR went busy:
                        // finalise what we have so nemotron's single stream is freed.
                        if let Some(a) = asr.as_ref() {
                            a.finish().await;
                            held = false;
                        }
                        preroll.clear();
                    }
                    let _ = session.text(floor_json(state)).await;
                },
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = hb.ticker.tick() => {
                if !hb.beat(&mut session).await { break }
            }
            u = upd_rx.recv() => {
                let Some(u) = u else { continue };
                match u {
                    AsrUpdate::Partial(text) => {
                        repo.emit(ServerEvent::Caption(CaptionEvent { actor, text, partial: true, ts_ms: now_ms() }));
                    }
                    AsrUpdate::Final(text) => {
                        asr = None;
                        release(&st);
                        repo.emit(ServerEvent::Caption(CaptionEvent { actor, text: text.clone(), partial: false, ts_ms: now_ms() }));
                        if text.trim().is_empty() {
                            continue;
                        }
                        if in_flight.swap(true, std::sync::atomic::Ordering::AcqRel) {
                            let _ = session.text(json!({"type":"error","message":"busy: one command at a time"}).to_string()).await;
                            continue;
                        }
                        let done = InFlight(in_flight.clone());
                        let focus = ctx.lock().focus;
                        repo.record_presence(PresenceEvent { actor, actor_label: label.clone(), actor_kind: "human".into(), kind: PresenceKind::Say, node_ids: focus.into_iter().collect(), paths: vec![], text: Some(text.clone()), ts_ms: now_ms() });
                        let (st2, repo2, ctx2, view2, mut sess2) = (st.clone(), repo.clone(), ctx.clone(), view.clone(), session.clone());
                        tokio::spawn(async move {
                            // Held until the answer has been spoken.
                            let _done = done;
                            match run_command(&st2, &repo2, &text, ctx2).await {
                                Ok((r, eff)) => {
                                    if let Effect::Focus(id) = eff {
                                        let file = {
                                            let p = repo2.current();
                                            p.index.get(&id).map(|&i| &p.snapshot.nodes[i]).map(|n| if n.kind == codescape_model::NodeKind::File { n.id } else { n.parent })
                                        };
                                        view2.set_focus(Some(id), file.filter(|&f| f != 0));
                                        repo2.focus_layout(vec![id]);
                                    }
                                    let answer = r.answer.clone();
                                    if let Ok(t) = serde_json::to_string(&ServerEvent::Query(r)) {
                                        let _ = sess2.text(t).await;
                                    }
                                    speak(&mut sess2, &st2, &answer).await;
                                }
                                Err(message) => {
                                    let _ = sess2.text(json!({"type":"error","message": message.clone()}).to_string()).await;
                                    speak(&mut sess2, &st2, &message).await;
                                }
                            }
                        });
                    }
                    AsrUpdate::Busy => {
                        asr = None;
                        held = false;
                        let s = { let mut f = st.floor.lock(); let _ = f.release(actor, me, now_ms()); f.mark_busy(true) };
                        st.broadcast_floor(s);
                        let st2 = st.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(BUSY_COOLDOWN).await;
                            let s = st2.floor.lock().mark_busy(false);
                            st2.broadcast_floor(s);
                        });
                    }
                    AsrUpdate::Failed(e) => {
                        asr = None;
                        held = false;
                        release(&st);
                        let _ = session.text(json!({"type":"error","message": e}).to_string()).await;
                    }
                }
            }
        }
    }
    if let Some(a) = asr.take() {
        a.finish().await;
    }
    // The claim (and with it any hold) is dropped by `voice_session`.
    let _ = session.close(None).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preroll_keeps_the_newest_two_seconds() {
        let mut b = vec![];
        push_preroll(&mut b, &[1.0; 1000]);
        assert_eq!(b.len(), 1000);
        push_preroll(&mut b, &vec![2.0; PREROLL_SAMPLES]);
        assert_eq!(b.len(), PREROLL_SAMPLES);
        assert!(b.iter().all(|&x| x == 2.0), "oldest samples dropped first");
    }

    #[test]
    fn floor_messages_are_typed() {
        let v: serde_json::Value = serde_json::from_str(&floor_json(&FloorState::Held {
            holder: 3,
            queue: vec![],
        }))
        .unwrap();
        assert_eq!(v["type"], "floor");
        assert_eq!(v["state"], "held");
        assert_eq!(v["holder"], 3);
        let v: serde_json::Value = serde_json::from_str(&floor_json(&FloorState::Busy)).unwrap();
        assert_eq!(v["state"], "busy");
    }
}
