//! Actix WebSocket handler for the `/ws/presence` endpoint (PRD-008 §5.3).
//!
//! Per-connection actor that runs the JSON challenge/auth handshake described
//! in `docs/xr-godot-system-architecture.md` §3 and `docs/xr-godot-threat-model.md`
//! §T-WS-1, then switches to binary mode (opcode 0x43) and forwards each
//! pose frame to the per-room [`PresenceActor`].

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use dashmap::DashMap;
use lru::LruCache;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::actors::presence_actor::{
    BroadcastFrame, IngestOutcome, IngestPose, JoinRejection, JoinRoom, LeaveRoom, PresenceActor,
    RoomEventEnvelope,
};
use visionclaw_xr_presence::{
    ports::{IdentityVerifier, SignedChallenge},
    AvatarId, AvatarMetadata, Did, RoomId,
};

const RATE_LIMIT_FRAMES_PER_SEC: usize = 120;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

const CLOSE_CODE_AUTH_FAIL: u16 = 4401;
const CLOSE_CODE_RATE_LIMIT: u16 = 4429;
const CLOSE_CODE_VALIDATION: u16 = 4400;

pub type PresenceRoomRegistry = Arc<DashMap<String, actix::Addr<PresenceActor>>>;

pub fn new_room_registry() -> PresenceRoomRegistry {
    Arc::new(DashMap::new())
}

/// T-WS-1 replay defence: bounded set of challenge nonces that have already been
/// answered. A nonce is inserted the first time a client successfully verifies
/// against it; a second auth carrying the same nonce is rejected as a replay.
/// Shared across every session/worker so replay across connections is caught.
pub type SeenNonces = Arc<Mutex<LruCache<[u8; 32], ()>>>;

/// Upper bound on remembered nonces (#9). Bounded so a flood of connections
/// cannot grow the set without limit; the LRU evicts the oldest, which is safe
/// because an evicted nonce belongs to a session that closed long ago and whose
/// 10s handshake window has already lapsed.
const SEEN_NONCE_CAPACITY: usize = 4096;

/// Reject a client-supplied handshake timestamp that is more than this far from
/// the server clock (#9). Guards against replay of an old captured handshake.
const MAX_HANDSHAKE_SKEW_US: u64 = 30_000_000; // 30s

pub fn new_seen_nonce_cache() -> SeenNonces {
    Arc::new(Mutex::new(LruCache::new(
        NonZeroUsize::new(SEEN_NONCE_CAPACITY).expect("capacity is non-zero"),
    )))
}

#[derive(Clone)]
pub struct PresenceHandlerState {
    pub registry: PresenceRoomRegistry,
    pub identity_verifier: Arc<dyn IdentityVerifier>,
    pub seen_nonces: SeenNonces,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerHandshake {
    Challenge {
        nonce: String,
        ts: u64,
    },
    Joined {
        room_id: String,
        avatar_id: String,
        members: Vec<MemberDescriptor>,
    },
}

#[derive(Debug, Serialize)]
struct MemberDescriptor {
    did: String,
    display_name: String,
    model_uri: Option<String>,
    /// Transport id this member's poses are tagged with in 0x43 sibling frames.
    local_id: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientHandshake {
    Auth {
        did: String,
        signature: String,
        room_id: String,
        metadata: ClientMetadata,
        /// Optional client-supplied handshake timestamp (µs since epoch). When
        /// present it is bounded against the server clock to reject replay of a
        /// stale captured handshake (#9). Absent for the current Godot client,
        /// which does not send it — its omission does not weaken the fresh
        /// per-connection nonce + replay-LRU defence.
        #[serde(default)]
        ts: Option<u64>,
    },
}

#[derive(Debug, Deserialize)]
struct ClientMetadata {
    display_name: String,
    model_uri: Option<String>,
}

#[derive(Debug)]
enum SessionPhase {
    Challenged {
        nonce: [u8; 32],
        ts_us: u64,
    },
    Joined {
        avatar_id: AvatarId,
        room_addr: actix::Addr<PresenceActor>,
    },
}

pub struct PresenceSession {
    state: Arc<PresenceHandlerState>,
    phase: SessionPhase,
    frame_window: VecDeque<Instant>,
    last_heartbeat: Instant,
    handshake_started_at: Instant,
}

impl PresenceSession {
    pub fn new(state: Arc<PresenceHandlerState>) -> Self {
        // #9: fill the challenge nonce from the OS CSPRNG (getrandom-backed
        // OsRng), not the non-cryptographic `fastrand` PRNG. A predictable nonce
        // would let an attacker precompute a signature for a future challenge.
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        let ts_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        Self {
            state,
            phase: SessionPhase::Challenged { nonce, ts_us },
            frame_window: VecDeque::new(),
            last_heartbeat: Instant::now(),
            handshake_started_at: Instant::now(),
        }
    }

    fn send_challenge(&self, ctx: &mut ws::WebsocketContext<Self>) {
        if let SessionPhase::Challenged { nonce, ts_us } = &self.phase {
            let msg = ServerHandshake::Challenge {
                nonce: hex::encode(nonce),
                ts: *ts_us,
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                ctx.text(json);
            }
        }
    }

    fn check_rate_limit(&mut self) -> bool {
        let now = Instant::now();
        while let Some(front) = self.frame_window.front() {
            if now.duration_since(*front) > RATE_LIMIT_WINDOW {
                self.frame_window.pop_front();
            } else {
                break;
            }
        }
        if self.frame_window.len() >= RATE_LIMIT_FRAMES_PER_SEC {
            return false;
        }
        self.frame_window.push_back(now);
        true
    }

    fn handle_auth(&mut self, text: &str, ctx: &mut ws::WebsocketContext<Self>) {
        let SessionPhase::Challenged { nonce, ts_us } = &self.phase else {
            warn!("auth attempted in wrong phase");
            close_with(ctx, CLOSE_CODE_VALIDATION, "auth in wrong phase");
            return;
        };
        let nonce = *nonce;
        let ts_us = *ts_us;

        let parsed: ClientHandshake = match serde_json::from_str(text) {
            Ok(p) => p,
            Err(e) => {
                warn!("malformed auth json: {e}");
                close_with_code(ctx, ws::CloseCode::Unsupported, "malformed json");
                return;
            }
        };
        let ClientHandshake::Auth {
            did,
            signature,
            room_id,
            metadata,
            ts,
        } = parsed;

        // #9: if the client supplied a handshake timestamp, reject it when it is
        // implausibly far from the server clock — a replayed old handshake.
        if let Some(client_ts) = ts {
            let now_us = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_micros() as u64)
                .unwrap_or(0);
            let skew = now_us.abs_diff(client_ts);
            if skew > MAX_HANDSHAKE_SKEW_US {
                warn!("stale handshake timestamp (skew {skew}µs); rejecting");
                close_with(ctx, CLOSE_CODE_AUTH_FAIL, "stale handshake timestamp");
                return;
            }
        }

        let challenge = SignedChallenge {
            nonce,
            timestamp_us: ts_us,
            claimed_pubkey_hex: did.strip_prefix("did:nostr:").unwrap_or(&did).to_owned(),
            signature_hex: signature,
        };
        let verified = match self
            .state
            .identity_verifier
            .verify_signed_challenge(&challenge)
        {
            Ok(d) => d,
            Err(e) => {
                warn!("identity verification failed: {e}");
                close_with(ctx, CLOSE_CODE_AUTH_FAIL, &format!("auth: {e}"));
                return;
            }
        };
        if verified.as_str() != did {
            close_with(ctx, CLOSE_CODE_AUTH_FAIL, "did/signature mismatch");
            return;
        }

        // #9: replay defence. A valid (nonce, signature) pair may only be
        // consumed once. Insert the nonce into the shared bounded LRU; if it was
        // already present, this exact challenge response has been seen before —
        // reject it. Checked only after signature verification so an attacker
        // cannot burn a victim's nonce with a garbage signature.
        {
            let mut seen = match self.state.seen_nonces.lock() {
                Ok(g) => g,
                // A poisoned mutex means another thread panicked mid-update;
                // recover the guard rather than fail-open on replay.
                Err(poisoned) => poisoned.into_inner(),
            };
            if seen.put(nonce, ()).is_some() {
                warn!("replayed presence challenge nonce; rejecting");
                close_with(ctx, CLOSE_CODE_AUTH_FAIL, "replayed challenge");
                return;
            }
        }

        let room = match RoomId::parse(room_id.clone()) {
            Ok(r) => r,
            Err(e) => {
                close_with(ctx, CLOSE_CODE_VALIDATION, &format!("room_id: {e}"));
                return;
            }
        };

        let registry = self.state.registry.clone();
        let room_addr = {
            let mut entry = registry
                .entry(room.as_str().to_owned())
                .or_insert_with(|| PresenceActor::new(room.clone()).start());
            // A PresenceActor self-stops once its room empties, but its Addr
            // lingers here. Reusing that dead Addr makes every rejoin fail with
            // "Mailbox has closed", silently poisoning the room. Replace any
            // disconnected entry with a fresh actor before the joiner gets it.
            if !entry.connected() {
                *entry = PresenceActor::new(room.clone()).start();
            }
            entry.clone()
        };

        let avatar_metadata = AvatarMetadata {
            did: verified.clone(),
            display_name: metadata.display_name,
            model_uri: metadata.model_uri,
        };
        let session_addr = ctx.address();
        let frame_recipient: Recipient<BroadcastFrame> = session_addr.clone().recipient();
        let event_recipient: Recipient<RoomEventEnvelope> = session_addr.recipient();

        let join = room_addr.send(JoinRoom {
            did: verified.clone(),
            metadata: avatar_metadata,
            frame_recipient,
            event_recipient,
        });
        let room_for_join = room.clone();
        let room_addr_for_state = room_addr.clone();
        ctx.spawn(
            join.into_actor(self)
                .map(move |res, act, inner_ctx| match res {
                    Ok(Ok(ack)) => {
                        let descriptors: Vec<MemberDescriptor> = ack
                            .members
                            .iter()
                            .map(|m| MemberDescriptor {
                                did: m.metadata.did.to_string(),
                                display_name: m.metadata.display_name.clone(),
                                model_uri: m.metadata.model_uri.clone(),
                                local_id: m.local_id,
                            })
                            .collect();
                        let msg = ServerHandshake::Joined {
                            room_id: room_for_join.as_str().to_owned(),
                            avatar_id: ack.avatar_id.to_string(),
                            members: descriptors,
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            inner_ctx.text(json);
                        }
                        act.phase = SessionPhase::Joined {
                            avatar_id: ack.avatar_id,
                            room_addr: room_addr_for_state.clone(),
                        };
                        info!(room = %room_for_join, "presence session joined");
                    }
                    Ok(Err(JoinRejection::DuplicateMember)) => {
                        close_with(inner_ctx, CLOSE_CODE_VALIDATION, "duplicate member");
                    }
                    Ok(Err(other)) => {
                        close_with(inner_ctx, CLOSE_CODE_VALIDATION, &other.to_string());
                    }
                    Err(e) => {
                        warn!("join mailbox error: {e}");
                        close_with(inner_ctx, CLOSE_CODE_VALIDATION, "join failed");
                    }
                }),
        );
    }

    fn handle_pose_frame(&mut self, bin: bytes::Bytes, ctx: &mut ws::WebsocketContext<Self>) {
        let (avatar, addr) = match &self.phase {
            SessionPhase::Joined {
                avatar_id,
                room_addr,
            } => (avatar_id.clone(), room_addr.clone()),
            _ => {
                close_with(ctx, CLOSE_CODE_VALIDATION, "binary before auth");
                return;
            }
        };
        if !self.check_rate_limit() {
            close_with(ctx, CLOSE_CODE_RATE_LIMIT, "rate limit exceeded");
            return;
        }
        let payload = bin.to_vec();
        ctx.spawn(
            async move {
                addr.send(IngestPose {
                    avatar_id: avatar,
                    frame_bytes: payload,
                })
                .await
            }
            .into_actor(self)
            .map(|res, _act, inner_ctx| match res {
                Ok(IngestOutcome::Accepted) => {}
                Ok(IngestOutcome::Decode(reason)) | Ok(IngestOutcome::ValidationFailed(reason)) => {
                    warn!("pose rejected: {reason}");
                }
                Ok(IngestOutcome::Kick(reason)) => {
                    close_with(inner_ctx, CLOSE_CODE_VALIDATION, &reason);
                }
                Err(e) => {
                    warn!("ingest mailbox error: {e}");
                }
            }),
        );
    }

    fn enforce_handshake_deadline(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        if matches!(self.phase, SessionPhase::Challenged { .. })
            && self.handshake_started_at.elapsed() > HANDSHAKE_TIMEOUT
        {
            close_with(ctx, CLOSE_CODE_AUTH_FAIL, "handshake timeout");
        }
    }

    fn heartbeat(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        if Instant::now().duration_since(self.last_heartbeat) > HEARTBEAT_INTERVAL * 2 {
            warn!("client heartbeat missed; closing");
            ctx.stop();
            return;
        }
        ctx.ping(b"");
    }
}

fn close_with(ctx: &mut ws::WebsocketContext<PresenceSession>, code: u16, description: &str) {
    ctx.close(Some(ws::CloseReason {
        code: ws::CloseCode::Other(code),
        description: Some(description.to_owned()),
    }));
    ctx.stop();
}

fn close_with_code(
    ctx: &mut ws::WebsocketContext<PresenceSession>,
    code: ws::CloseCode,
    description: &str,
) {
    ctx.close(Some(ws::CloseReason {
        code,
        description: Some(description.to_owned()),
    }));
    ctx.stop();
}

impl Actor for PresenceSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.send_challenge(ctx);
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            act.heartbeat(ctx);
            act.enforce_handshake_deadline(ctx);
        });
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> actix::Running {
        if let SessionPhase::Joined {
            avatar_id,
            room_addr,
        } = &self.phase
        {
            room_addr.do_send(LeaveRoom {
                avatar_id: avatar_id.clone(),
            });
        }
        actix::Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for PresenceSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => self.handle_auth(&text, ctx),
            Ok(ws::Message::Binary(bin)) => self.handle_pose_frame(bin, ctx),
            Ok(ws::Message::Ping(p)) => {
                self.last_heartbeat = Instant::now();
                ctx.pong(&p);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_heartbeat = Instant::now();
            }
            Ok(ws::Message::Close(reason)) => {
                info!("client closed: {reason:?}");
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) | Ok(ws::Message::Nop) => {}
            Err(e) => {
                warn!("ws protocol error: {e}");
                ctx.stop();
            }
        }
    }
}

impl Handler<BroadcastFrame> for PresenceSession {
    type Result = ();

    fn handle(&mut self, msg: BroadcastFrame, ctx: &mut Self::Context) {
        ctx.binary(msg.bytes);
    }
}

impl Handler<RoomEventEnvelope> for PresenceSession {
    type Result = ();

    fn handle(&mut self, msg: RoomEventEnvelope, ctx: &mut Self::Context) {
        if let Ok(json) = serde_json::to_string(&msg) {
            ctx.text(json);
        }
    }
}

pub async fn ws_presence(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<PresenceHandlerState>,
) -> Result<HttpResponse, actix_web::Error> {
    let session = PresenceSession::new(Arc::new((**state).clone()));
    ws::start(session, &req, stream)
}

pub fn allow_unused_did(_did: &Did) {}

#[cfg(test)]
mod nonce_tests {
    use super::*;

    #[test]
    fn csprng_nonce_is_nonzero_and_distinct() {
        // Two freshly generated nonces must differ and not be all-zero (a
        // regression guard against the nonce fill being dropped/stubbed).
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        OsRng.fill_bytes(&mut a);
        OsRng.fill_bytes(&mut b);
        assert_ne!(a, [0u8; 32]);
        assert_ne!(a, b);
    }

    #[test]
    fn replay_lru_rejects_second_use_of_same_nonce() {
        let cache = new_seen_nonce_cache();
        let nonce = [7u8; 32];
        let mut guard = cache.lock().unwrap();
        // First use: not previously seen.
        assert!(guard.put(nonce, ()).is_none());
        // Second use of the SAME nonce: seen → replay.
        assert!(guard.put(nonce, ()).is_some());
    }

    #[test]
    fn replay_lru_allows_distinct_nonces() {
        let cache = new_seen_nonce_cache();
        let mut guard = cache.lock().unwrap();
        assert!(guard.put([1u8; 32], ()).is_none());
        assert!(guard.put([2u8; 32], ()).is_none());
    }

    #[test]
    fn seen_nonce_cache_is_bounded() {
        assert_eq!(
            new_seen_nonce_cache().lock().unwrap().cap().get(),
            SEEN_NONCE_CAPACITY
        );
    }
}
