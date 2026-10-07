//! Presence WebSocket client for `/ws/presence`.
//!
//! Protocol (server-initiated, authoritative source `src/handlers/presence_handler.rs`):
//! ```text
//! 1. server -> client  {"type":"challenge","nonce":<64hex>,"ts":<u64>}
//! 2. client -> server  {"type":"auth","did":"did:nostr:<64hex>",
//!                       "signature":<128hex>,"room_id":<urn>,
//!                       "metadata":{"display_name":<str>,"model_uri":<null|str>}}
//! 3. server -> client  {"type":"joined","room_id":<urn>,"avatar_id":<urn>,
//!                       "members":[{did,display_name,model_uri,local_id}]}
//! ```
//! Then bidirectional binary 0x43 pose traffic plus text room events
//! (`avatar_joined` / `avatar_left`). The challenge signature is
//! `schnorr(SHA256(nonce || ts.to_le_bytes()))` per `signer::NostrSigner`.
//!
//! Inbound multi-avatar broadcast ("sibling") frames differ from the single
//! `wire::encode` layout — see [`decode_sibling_frame`].

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;
#[cfg(not(test))]
use tracing::warn;

use visionclaw_xr_presence::wire::{encode, OPCODE_AVATAR_POSE};
use visionclaw_xr_presence::{
    AvatarId, AvatarMetadata, Did, PoseFrame, RoomId, Transform, WireError,
};

use crate::ports::{Signer, SignerError, TransportError, WsMessage, WsTransport};

#[cfg(not(test))]
use godot::prelude::*;

#[derive(Debug, Clone, Error)]
pub enum PresenceError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("signer: {0}")]
    Signer(#[from] SignerError),
    #[error("wire: {0}")]
    Wire(#[from] WireError),
    #[error("protocol failure: {0}")]
    Protocol(String),
    #[error("server rejected handshake: {0}")]
    Rejected(String),
}

// --- wire message types (match server serde tags exactly) --------------------

/// One peer in the `joined` roster / `avatar_joined` event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemberDescriptor {
    pub did: String,
    pub display_name: String,
    #[serde(default)]
    pub model_uri: Option<String>,
    /// Server-assigned stable id echoed in every sibling pose frame. `None`
    /// until the server announces it (see task: local_id↔avatar mapping).
    #[serde(default)]
    pub local_id: Option<u32>,
}

/// Every text frame the server can send on `/ws/presence`. `challenge` and
/// `joined` are the handshake; `avatar_joined` / `avatar_left` are live room
/// events delivered after join.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Challenge {
        nonce: String,
        ts: u64,
    },
    Joined {
        room_id: String,
        avatar_id: String,
        members: Vec<MemberDescriptor>,
    },
    AvatarJoined {
        avatar_id: String,
        did: String,
        display_name: String,
        #[serde(default)]
        local_id: Option<u32>,
    },
    AvatarLeft {
        avatar_id: String,
        #[serde(default)]
        did: Option<String>,
    },
}

/// The single `auth` reply the client sends after a `challenge`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Auth {
        did: String,
        signature: String,
        room_id: String,
        metadata: ClientMetadata,
    },
}

#[derive(Debug, Clone, Serialize)]
struct ClientMetadata {
    display_name: String,
    model_uri: Option<String>,
}

/// Result of a successful handshake.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinedState {
    pub avatar_id: String,
    pub members: Vec<MemberDescriptor>,
}

// --- sibling (multi-avatar broadcast) frame ---------------------------------

/// One peer pose inside a sibling broadcast frame, keyed by the server's
/// `local_id` (the URN is delivered separately via room events).
#[derive(Debug, Clone, PartialEq)]
pub struct SiblingPose {
    pub local_id: u32,
    pub frame: PoseFrame,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SiblingBatch {
    pub broadcast_seq: u64,
    pub room_id_u32: u32,
    pub poses: Vec<SiblingPose>,
}

const SLOT_HEAD: u8 = 0b001;
const SLOT_LEFT: u8 = 0b010;
const SLOT_RIGHT: u8 = 0b100;
const SIBLING_HEADER: usize = 1 + 8 + 4 + 2; // opcode + seq + room_u32 + count

/// Decode the server's multi-avatar broadcast frame.
///
/// Layout (little-endian, `src/actors/presence_actor.rs::build_broadcast_frame`):
/// ```text
/// [u8 opcode=0x43][u64 broadcast_seq][u32 room_id][u16 user_count]
/// per user: [u32 local_id][u64 timestamp_us][u8 mask][{28}*popcount(mask) transforms]
/// ```
/// `mask` bit0=head bit1=left_hand bit2=right_hand; head is always present.
pub fn decode_sibling_frame(bytes: &[u8]) -> Result<SiblingBatch, PresenceError> {
    if bytes.len() < SIBLING_HEADER {
        return Err(PresenceError::Protocol(format!(
            "sibling frame too short: {} < {}",
            bytes.len(),
            SIBLING_HEADER
        )));
    }
    if bytes[0] != OPCODE_AVATAR_POSE {
        return Err(PresenceError::Protocol(format!(
            "sibling opcode 0x{:02x}, expected 0x{:02x}",
            bytes[0], OPCODE_AVATAR_POSE
        )));
    }
    let broadcast_seq = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
    let room_id_u32 = u32::from_le_bytes(bytes[9..13].try_into().unwrap());
    let user_count = u16::from_le_bytes(bytes[13..15].try_into().unwrap()) as usize;

    let mut cursor = SIBLING_HEADER;
    let mut poses = Vec::with_capacity(user_count);
    for _ in 0..user_count {
        if cursor + 4 + 8 + 1 > bytes.len() {
            return Err(PresenceError::Protocol(
                "sibling user header truncated".into(),
            ));
        }
        let local_id = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        let timestamp_us = u64::from_le_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        let mask = bytes[cursor];
        cursor += 1;
        if mask & SLOT_HEAD == 0 || mask & !(SLOT_HEAD | SLOT_LEFT | SLOT_RIGHT) != 0 {
            return Err(PresenceError::Protocol(format!(
                "bad sibling mask 0x{mask:02x}"
            )));
        }
        let count = (mask & SLOT_HEAD != 0) as usize
            + (mask & SLOT_LEFT != 0) as usize
            + (mask & SLOT_RIGHT != 0) as usize;
        let need = count * Transform::WIRE_SIZE;
        if cursor + need > bytes.len() {
            return Err(PresenceError::Protocol(
                "sibling transforms truncated".into(),
            ));
        }
        let head_raw = read_transform(&bytes[cursor..cursor + Transform::WIRE_SIZE]);
        cursor += Transform::WIRE_SIZE;
        let left_hand = if mask & SLOT_LEFT != 0 {
            let t = read_transform(&bytes[cursor..cursor + Transform::WIRE_SIZE]);
            cursor += Transform::WIRE_SIZE;
            sanitize_transform(t)
        } else {
            None
        };
        let right_hand = if mask & SLOT_RIGHT != 0 {
            let t = read_transform(&bytes[cursor..cursor + Transform::WIRE_SIZE]);
            cursor += Transform::WIRE_SIZE;
            sanitize_transform(t)
        } else {
            None
        };
        // Head drives the whole avatar transform; a poisoned head is unusable, so
        // drop the entire pose rather than render an avatar at NaN/off-world.
        let head = match sanitize_transform(head_raw) {
            Some(h) => h,
            None => continue,
        };
        poses.push(SiblingPose {
            local_id,
            frame: PoseFrame {
                timestamp_us,
                head,
                left_hand,
                right_hand,
            },
        });
    }
    Ok(SiblingBatch {
        broadcast_seq,
        room_id_u32,
        poses,
    })
}

/// Hard-reject bound (metres) for a pose position component.
const POSE_LIMIT_M: f32 = 10_000.0;
/// Sane world half-extent (metres); finite positions are clamped here.
const POSE_CLAMP_M: f32 = 400.0;
/// A quaternion within this tolerance of unit length is accepted as-is;
/// otherwise it is renormalised (or rejected if degenerate).
const QUAT_UNIT_TOL: f32 = 1e-2;
/// Minimum quaternion norm treated as non-degenerate.
const QUAT_MIN_NORM: f32 = 1e-3;

/// Validate/repair an inbound transform. Returns `None` when a position
/// component is non-finite or beyond [`POSE_LIMIT_M`], or the quaternion is
/// non-finite/degenerate. Finite positions are clamped to [`POSE_CLAMP_M`] and a
/// non-unit (but healthy) quaternion is renormalised so avatar bases stay valid.
fn sanitize_transform(mut t: Transform) -> Option<Transform> {
    for c in t.position.iter() {
        if !c.is_finite() || c.abs() > POSE_LIMIT_M {
            return None;
        }
    }
    for c in t.position.iter_mut() {
        *c = c.clamp(-POSE_CLAMP_M, POSE_CLAMP_M);
    }
    let q = t.rotation;
    let norm_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !norm_sq.is_finite() || norm_sq < QUAT_MIN_NORM * QUAT_MIN_NORM {
        return None;
    }
    let norm = norm_sq.sqrt();
    if (norm - 1.0).abs() > QUAT_UNIT_TOL {
        for c in t.rotation.iter_mut() {
            *c /= norm;
        }
    }
    Some(t)
}

fn read_transform(slice: &[u8]) -> Transform {
    let mut position = [0f32; 3];
    let mut rotation = [0f32; 4];
    for (i, chunk) in slice[..12].as_chunks::<4>().0.iter().enumerate() {
        position[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for (i, chunk) in slice[12..28].as_chunks::<4>().0.iter().enumerate() {
        rotation[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Transform { position, rotation }
}

// --- handshake client --------------------------------------------------------

pub struct PresenceClient<T: WsTransport, S: Signer> {
    transport: Arc<T>,
    signer: Arc<S>,
    room: RoomId,
    avatar: Option<AvatarId>,
}

impl<T: WsTransport, S: Signer> PresenceClient<T, S> {
    pub fn new(transport: Arc<T>, signer: Arc<S>, room: RoomId) -> Self {
        Self {
            transport,
            signer,
            room,
            avatar: None,
        }
    }

    /// Run the server-initiated challenge -> auth -> joined handshake.
    pub async fn handshake(
        &mut self,
        display_name: String,
        model_uri: Option<String>,
    ) -> Result<JoinedState, PresenceError> {
        // 1. challenge
        let (nonce, ts) = match self.recv_server_message().await? {
            ServerMessage::Challenge { nonce, ts } => (nonce, ts),
            other => {
                return Err(PresenceError::Protocol(format!(
                    "expected challenge, got {other:?}"
                )))
            }
        };
        let nonce_bytes = decode_nonce(&nonce)?;

        // 2. auth
        let did = self.signer.did()?;
        let signed = self.signer.sign_challenge(&nonce_bytes, ts)?;
        let auth = ClientMessage::Auth {
            did: did.to_string(),
            signature: signed.signature_hex,
            room_id: self.room.as_str().to_owned(),
            metadata: ClientMetadata {
                display_name,
                model_uri,
            },
        };
        let auth_json = serde_json::to_string(&auth)
            .map_err(|e| PresenceError::Protocol(format!("encode auth: {e}")))?;
        self.transport.send_text(auth_json).await?;

        // 3. joined
        match self.recv_server_message().await? {
            ServerMessage::Joined {
                avatar_id, members, ..
            } => {
                self.avatar = Some(
                    AvatarId::parse(avatar_id.clone())
                        .map_err(|e| PresenceError::Protocol(format!("joined avatar_id: {e}")))?,
                );
                info!(room = %self.room, members = members.len(), "presence joined");
                Ok(JoinedState { avatar_id, members })
            }
            other => Err(PresenceError::Rejected(format!(
                "expected joined, got {other:?}"
            ))),
        }
    }

    async fn recv_server_message(&self) -> Result<ServerMessage, PresenceError> {
        match self.transport.recv().await? {
            WsMessage::Text(t) => serde_json::from_str(&t)
                .map_err(|e| PresenceError::Protocol(format!("decode server msg: {e}"))),
            WsMessage::Binary(_) => Err(PresenceError::Protocol(
                "expected text handshake frame, got binary".into(),
            )),
            WsMessage::Close => Err(PresenceError::Rejected(
                "server closed during handshake".into(),
            )),
        }
    }

    pub async fn send_pose(&self, frame: &PoseFrame) -> Result<(), PresenceError> {
        let avatar = self.avatar.as_ref().ok_or_else(|| {
            PresenceError::Protocol("send_pose before successful handshake".into())
        })?;
        let bytes = encode(frame, &self.room, avatar)?;
        self.transport.send_binary(bytes).await?;
        Ok(())
    }

    pub fn avatar_id(&self) -> Option<&AvatarId> {
        self.avatar.as_ref()
    }
}

fn decode_nonce(hex_str: &str) -> Result<[u8; 32], PresenceError> {
    let raw = hex::decode(hex_str.trim())
        .map_err(|e| PresenceError::Protocol(format!("nonce hex: {e}")))?;
    let arr: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| PresenceError::Protocol(format!("nonce len {} != 32", raw.len())))?;
    Ok(arr)
}

pub fn current_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

pub fn into_avatar_metadata(member: &MemberDescriptor) -> Result<AvatarMetadata, PresenceError> {
    let did = Did::parse(member.did.clone())
        .map_err(|e| PresenceError::Protocol(format!("bad DID in member: {e}")))?;
    Ok(AvatarMetadata {
        did,
        display_name: member.display_name.clone(),
        model_uri: member.model_uri.clone(),
    })
}

// --- Godot node: inbox + poll + outbound pose channel ------------------------

/// Network -> main-thread events for `/ws/presence`. The async pump can never
/// touch Godot objects (not `Send`); it pushes these, and `poll()` drains them
/// on the scene-tree thread to emit signals. Plain transport data type (no Godot
/// deps) so the transport module can use it under `cfg(test)`.
pub enum PresenceInbound {
    Connected,
    Disconnected,
    Joined {
        avatar_id: String,
        members: Vec<MemberDescriptor>,
    },
    AvatarJoined {
        local_id: Option<u32>,
        avatar_id: String,
        did: String,
        display_name: String,
    },
    AvatarLeft {
        avatar_id: String,
    },
    Pose(SiblingBatch),
    Kicked {
        reason: String,
    },
}

#[cfg(not(test))]
use std::collections::{HashMap, VecDeque};
#[cfg(not(test))]
use std::sync::Mutex;

#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct PresenceClientNode {
    inbox: Arc<Mutex<VecDeque<PresenceInbound>>>,
    outbound: Option<tokio::sync::mpsc::UnboundedSender<PoseFrame>>,
    handle: Option<crate::transport::ConnHandle>,
    /// Maps the server's per-pose `local_id` to the avatar URN announced at join.
    local_to_avatar: HashMap<u32, String>,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl PresenceClientNode {
    #[signal]
    fn avatar_joined(did: GString, display_name: GString, avatar_id: GString);

    #[signal]
    fn avatar_left(avatar_id: GString);

    #[signal]
    fn avatar_pose_updated(
        avatar_id: GString,
        head_pos: Vector3,
        head_rot: Quaternion,
        has_left: bool,
        has_right: bool,
        left_pos: Vector3,
        left_rot: Quaternion,
        right_pos: Vector3,
        right_rot: Quaternion,
    );

    #[signal]
    fn presence_kicked(reason: GString);

    #[signal]
    fn connection_changed(connected: bool);

    #[func]
    fn create() -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            inbox: Arc::new(Mutex::new(VecDeque::new())),
            outbound: None,
            handle: None,
            local_to_avatar: HashMap::new(),
            base,
        })
    }

    /// Connect to `/ws/presence`, run the challenge/auth/joined handshake with a
    /// freshly generated (or provided) Nostr identity, and start pumping pose
    /// traffic. `secret_hex` empty => ephemeral identity. Non-blocking.
    #[func]
    fn join(
        &mut self,
        url: GString,
        room_urn: GString,
        display_name: GString,
        secret_hex: GString,
    ) {
        let (handle, tx) = crate::transport::spawn_presence(
            url.to_string(),
            room_urn.to_string(),
            display_name.to_string(),
            secret_hex.to_string(),
            self.inbox.clone(),
        );
        self.handle = Some(handle);
        self.outbound = Some(tx);
    }

    #[func]
    fn close(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
        self.outbound = None;
    }

    /// Queue a local pose for transmission as a 0x43 frame. No-op before join.
    /// The flat Vector3/Quaternion arg list mirrors the XR tracker outputs the
    /// GDScript caller already holds, avoiding a per-frame Dictionary allocation.
    #[allow(clippy::too_many_arguments)]
    #[func]
    fn send_pose(
        &mut self,
        head_pos: Vector3,
        head_rot: Quaternion,
        left_pos: Vector3,
        left_rot: Quaternion,
        right_pos: Vector3,
        right_rot: Quaternion,
        has_left: bool,
        has_right: bool,
    ) {
        let Some(tx) = self.outbound.as_ref() else {
            return;
        };
        let frame = PoseFrame {
            timestamp_us: current_micros(),
            head: transform_of(head_pos, head_rot),
            left_hand: has_left.then(|| transform_of(left_pos, left_rot)),
            right_hand: has_right.then(|| transform_of(right_pos, right_rot)),
        };
        let _ = tx.send(frame);
    }

    /// Drain queued network events on the scene-tree thread and emit signals.
    /// Call once per frame.
    #[func]
    fn poll(&mut self) {
        let drained: Vec<PresenceInbound> = {
            let Ok(mut q) = self.inbox.lock() else {
                return;
            };
            q.drain(..).collect()
        };
        for ev in drained {
            self.dispatch(ev);
        }
    }

    /// Decode an explicit sibling frame (e.g. captured fixture) and emit poses.
    #[func]
    fn ingest_pose_bytes(&mut self, bytes: PackedByteArray) {
        match decode_sibling_frame(bytes.as_slice()) {
            Ok(batch) => self.emit_batch(&batch),
            Err(e) => warn!(err = %e, "presence sibling decode failed"),
        }
    }
}

#[cfg(not(test))]
impl PresenceClientNode {
    fn dispatch(&mut self, ev: PresenceInbound) {
        match ev {
            PresenceInbound::Connected => {
                self.base_mut()
                    .emit_signal("connection_changed", &[Variant::from(true)]);
            }
            PresenceInbound::Disconnected => {
                self.base_mut()
                    .emit_signal("connection_changed", &[Variant::from(false)]);
            }
            PresenceInbound::Joined { members, .. } => {
                for m in &members {
                    if let Some(id) = m.local_id {
                        self.local_to_avatar
                            .insert(id, member_avatar_urn(m).unwrap_or_default());
                    }
                }
            }
            PresenceInbound::AvatarJoined {
                local_id,
                avatar_id,
                did,
                display_name,
            } => {
                if let Some(id) = local_id {
                    self.local_to_avatar.insert(id, avatar_id.clone());
                }
                self.base_mut().emit_signal(
                    "avatar_joined",
                    &[
                        Variant::from(GString::from(did)),
                        Variant::from(GString::from(display_name)),
                        Variant::from(GString::from(avatar_id)),
                    ],
                );
            }
            PresenceInbound::AvatarLeft { avatar_id } => {
                self.local_to_avatar.retain(|_, v| v != &avatar_id);
                self.base_mut()
                    .emit_signal("avatar_left", &[Variant::from(GString::from(avatar_id))]);
            }
            PresenceInbound::Pose(batch) => self.emit_batch(&batch),
            PresenceInbound::Kicked { reason } => {
                self.base_mut()
                    .emit_signal("presence_kicked", &[Variant::from(GString::from(reason))]);
            }
        }
    }

    fn emit_batch(&mut self, batch: &SiblingBatch) {
        for pose in &batch.poses {
            let avatar_id = self
                .local_to_avatar
                .get(&pose.local_id)
                .cloned()
                .unwrap_or_else(|| pose.local_id.to_string());
            let head = &pose.frame.head;
            let pos = Vector3::new(head.position[0], head.position[1], head.position[2]);
            let q = head.rotation;
            // Articulated hands: forward decoded left/right transforms so remote
            // avatars render real hand poses. Absent hands send identity + the
            // has_* flag false, which the avatar script treats as "no hand".
            let (has_left, left_pos, left_rot) = hand_signal_parts(&pose.frame.left_hand);
            let (has_right, right_pos, right_rot) = hand_signal_parts(&pose.frame.right_hand);
            self.base_mut().emit_signal(
                "avatar_pose_updated",
                &[
                    Variant::from(GString::from(avatar_id)),
                    Variant::from(pos),
                    Variant::from(Quaternion::new(q[0], q[1], q[2], q[3])),
                    Variant::from(has_left),
                    Variant::from(has_right),
                    Variant::from(left_pos),
                    Variant::from(left_rot),
                    Variant::from(right_pos),
                    Variant::from(right_rot),
                ],
            );
        }
    }
}

/// Flatten an optional hand transform into the `(has, pos, rot)` triple the
/// `avatar_pose_updated` signal carries. `None` → identity + `false`.
#[cfg(not(test))]
fn hand_signal_parts(hand: &Option<Transform>) -> (bool, Vector3, Quaternion) {
    match hand {
        Some(t) => (
            true,
            Vector3::new(t.position[0], t.position[1], t.position[2]),
            Quaternion::new(t.rotation[0], t.rotation[1], t.rotation[2], t.rotation[3]),
        ),
        None => (false, Vector3::ZERO, Quaternion::new(0.0, 0.0, 0.0, 1.0)),
    }
}

#[cfg(not(test))]
fn transform_of(pos: Vector3, rot: Quaternion) -> Transform {
    Transform {
        position: [pos.x, pos.y, pos.z],
        rotation: [rot.x, rot.y, rot.z, rot.w],
    }
}

#[cfg(not(test))]
fn member_avatar_urn(m: &MemberDescriptor) -> Option<String> {
    Did::parse(m.did.clone())
        .ok()
        .map(|did| AvatarId::from_did(&did).as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fakes::{FakeSigner, FakeWsTransport};

    fn sample_room() -> RoomId {
        RoomId::parse("urn:visionclaw:room:sha256-12-deadbeefcafe").unwrap()
    }

    fn challenge_json() -> String {
        serde_json::json!({
            "type": "challenge",
            "nonce": "00".repeat(32),
            "ts": 1_700_000_000_000_000u64
        })
        .to_string()
    }

    fn joined_json(avatar_hex: &str) -> String {
        serde_json::json!({
            "type": "joined",
            "room_id": sample_room().as_str(),
            "avatar_id": format!("urn:visionclaw:avatar:{avatar_hex}"),
            "members": []
        })
        .to_string()
    }

    #[tokio::test]
    async fn handshake_challenge_auth_joined() {
        let (transport, inbox) = FakeWsTransport::new();
        let transport = Arc::new(transport);
        let signer = Arc::new(FakeSigner::new());
        let avatar_hex = signer.pubkey_hex();
        let mut client = PresenceClient::new(transport.clone(), signer, sample_room());

        inbox.send(WsMessage::Text(challenge_json())).await.unwrap();
        inbox
            .send(WsMessage::Text(joined_json(&avatar_hex)))
            .await
            .unwrap();

        let state = client.handshake("alice".into(), None).await.unwrap();
        assert_eq!(
            state.avatar_id,
            format!("urn:visionclaw:avatar:{avatar_hex}")
        );
        assert!(client.avatar_id().is_some());

        let sent = transport.sent_text.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&sent[0]).unwrap();
        assert_eq!(v["type"], "auth");
        assert_eq!(v["room_id"], sample_room().as_str());
        assert_eq!(v["metadata"]["display_name"], "alice");
        assert!(v["metadata"]["model_uri"].is_null());
    }

    #[tokio::test]
    async fn handshake_rejects_non_challenge_first() {
        let (transport, inbox) = FakeWsTransport::new();
        let transport = Arc::new(transport);
        let signer = Arc::new(FakeSigner::new());
        let mut client = PresenceClient::new(transport, signer, sample_room());

        inbox
            .send(WsMessage::Text(joined_json(&"a".repeat(64))))
            .await
            .unwrap();
        let err = client.handshake("alice".into(), None).await.unwrap_err();
        assert!(matches!(err, PresenceError::Protocol(_)));
    }

    #[tokio::test]
    async fn handshake_binary_first_errors() {
        let (transport, inbox) = FakeWsTransport::new();
        let transport = Arc::new(transport);
        let signer = Arc::new(FakeSigner::new());
        let mut client = PresenceClient::new(transport, signer, sample_room());
        inbox.send(WsMessage::Binary(vec![0x43, 0])).await.unwrap();
        let err = client.handshake("alice".into(), None).await.unwrap_err();
        assert!(matches!(err, PresenceError::Protocol(_)));
    }

    #[tokio::test]
    async fn send_pose_before_handshake_errors() {
        let (transport, _inbox) = FakeWsTransport::new();
        let client = PresenceClient::new(
            Arc::new(transport),
            Arc::new(FakeSigner::new()),
            sample_room(),
        );
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform::identity(),
            left_hand: None,
            right_hand: None,
        };
        let err = client.send_pose(&frame).await.unwrap_err();
        assert!(matches!(err, PresenceError::Protocol(_)));
    }

    #[tokio::test]
    async fn send_pose_after_handshake_writes_0x43() {
        let (transport, inbox) = FakeWsTransport::new();
        let transport = Arc::new(transport);
        let signer = Arc::new(FakeSigner::new());
        let avatar_hex = signer.pubkey_hex();
        let mut client = PresenceClient::new(transport.clone(), signer, sample_room());

        inbox.send(WsMessage::Text(challenge_json())).await.unwrap();
        inbox
            .send(WsMessage::Text(joined_json(&avatar_hex)))
            .await
            .unwrap();
        client.handshake("alice".into(), None).await.unwrap();

        let frame = PoseFrame {
            timestamp_us: 9000,
            head: Transform {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        client.send_pose(&frame).await.unwrap();
        let sent = transport.sent_binary.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0][0], OPCODE_AVATAR_POSE);
    }

    fn build_sibling(poses: &[(u32, PoseFrame)]) -> Vec<u8> {
        let mut out = vec![OPCODE_AVATAR_POSE];
        out.extend_from_slice(&7u64.to_le_bytes()); // broadcast_seq
        out.extend_from_slice(&0xAABBCCDDu32.to_le_bytes()); // room_id
        out.extend_from_slice(&(poses.len() as u16).to_le_bytes());
        for (local_id, f) in poses {
            out.extend_from_slice(&local_id.to_le_bytes());
            out.extend_from_slice(&f.timestamp_us.to_le_bytes());
            let mut mask = SLOT_HEAD;
            if f.left_hand.is_some() {
                mask |= SLOT_LEFT;
            }
            if f.right_hand.is_some() {
                mask |= SLOT_RIGHT;
            }
            out.push(mask);
            push_transform(&mut out, &f.head);
            if let Some(t) = &f.left_hand {
                push_transform(&mut out, t);
            }
            if let Some(t) = &f.right_hand {
                push_transform(&mut out, t);
            }
        }
        out
    }

    fn push_transform(out: &mut Vec<u8>, t: &Transform) {
        for v in t.position {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in t.rotation {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }

    #[test]
    fn sibling_single_head_only() {
        let frame = PoseFrame {
            timestamp_us: 42,
            head: Transform {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(5, frame.clone())]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert_eq!(batch.broadcast_seq, 7);
        assert_eq!(batch.room_id_u32, 0xAABBCCDD);
        assert_eq!(batch.poses.len(), 1);
        assert_eq!(batch.poses[0].local_id, 5);
        assert_eq!(batch.poses[0].frame, frame);
    }

    #[test]
    fn sibling_multi_user_mixed_masks() {
        let t = Transform {
            position: [0.5, 1.7, -0.3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let a = PoseFrame {
            timestamp_us: 1,
            head: t,
            left_hand: Some(t),
            right_hand: None,
        };
        let b = PoseFrame {
            timestamp_us: 2,
            head: t,
            left_hand: Some(t),
            right_hand: Some(t),
        };
        let bytes = build_sibling(&[(1, a.clone()), (2, b.clone())]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert_eq!(batch.poses.len(), 2);
        assert_eq!(batch.poses[0].frame, a);
        assert_eq!(batch.poses[1].frame, b);
    }

    #[test]
    fn sibling_zero_users() {
        let bytes = build_sibling(&[]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert!(batch.poses.is_empty());
    }

    #[test]
    fn sibling_rejects_bad_opcode() {
        let mut bytes = build_sibling(&[]);
        bytes[0] = 0x42;
        assert!(matches!(
            decode_sibling_frame(&bytes),
            Err(PresenceError::Protocol(_))
        ));
    }

    #[test]
    fn sibling_rejects_truncated() {
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform::identity(),
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        let truncated = &bytes[..bytes.len() - 10];
        assert!(matches!(
            decode_sibling_frame(truncated),
            Err(PresenceError::Protocol(_))
        ));
    }

    #[test]
    fn sibling_drops_pose_with_nan_head() {
        let bad = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [f32::NAN, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let good = PoseFrame {
            timestamp_us: 2,
            head: Transform {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, bad), (2, good.clone())]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert_eq!(batch.poses.len(), 1, "NaN-head pose must be dropped");
        assert_eq!(batch.poses[0].local_id, 2);
    }

    #[test]
    fn sibling_drops_head_beyond_world_limit() {
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [POSE_LIMIT_M * 2.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        assert!(decode_sibling_frame(&bytes).unwrap().poses.is_empty());
    }

    #[test]
    fn sibling_clamps_head_position() {
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [POSE_CLAMP_M + 100.0, -5000.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert_eq!(batch.poses[0].frame.head.position[0], POSE_CLAMP_M);
        assert_eq!(batch.poses[0].frame.head.position[1], -POSE_CLAMP_M);
    }

    #[test]
    fn sibling_renormalises_nonunit_head_quaternion() {
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 2.0], // norm 2, well outside tolerance
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        let q = batch.poses[0].frame.head.rotation;
        let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "quaternion must be renormalised");
        assert!((q[3] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn sibling_drops_pose_with_degenerate_head_quaternion() {
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 0.0], // zero quaternion → no valid basis
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        assert!(decode_sibling_frame(&bytes).unwrap().poses.is_empty());
    }

    #[test]
    fn sibling_drops_bad_hand_but_keeps_pose() {
        let bad_hand = Transform {
            position: [f32::INFINITY, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let frame = PoseFrame {
            timestamp_us: 1,
            head: Transform {
                position: [1.0, 1.0, 1.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: Some(bad_hand),
            right_hand: None,
        };
        let bytes = build_sibling(&[(1, frame)]);
        let batch = decode_sibling_frame(&bytes).unwrap();
        assert_eq!(batch.poses.len(), 1, "valid head keeps the pose");
        assert!(
            batch.poses[0].frame.left_hand.is_none(),
            "invalid hand must be dropped to None"
        );
    }

    #[test]
    fn decode_nonce_roundtrip() {
        let n = decode_nonce(&"ab".repeat(32)).unwrap();
        assert_eq!(n, [0xab; 32]);
    }

    #[test]
    fn decode_nonce_wrong_len() {
        assert!(decode_nonce("abcd").is_err());
        assert!(decode_nonce("zz").is_err());
    }

    #[test]
    fn server_message_parses_room_events() {
        let j: ServerMessage = serde_json::from_str(
            r#"{"type":"avatar_joined","avatar_id":"urn:visionclaw:avatar:aa","did":"did:nostr:bb","display_name":"bob","local_id":4}"#,
        )
        .unwrap();
        assert_eq!(
            j,
            ServerMessage::AvatarJoined {
                avatar_id: "urn:visionclaw:avatar:aa".into(),
                did: "did:nostr:bb".into(),
                display_name: "bob".into(),
                local_id: Some(4),
            }
        );
        let l: ServerMessage = serde_json::from_str(
            r#"{"type":"avatar_left","avatar_id":"urn:visionclaw:avatar:aa"}"#,
        )
        .unwrap();
        assert_eq!(
            l,
            ServerMessage::AvatarLeft {
                avatar_id: "urn:visionclaw:avatar:aa".into(),
                did: None,
            }
        );
    }

    #[test]
    fn into_avatar_metadata_valid() {
        let member = MemberDescriptor {
            did: format!("did:nostr:{}", "b".repeat(64)),
            display_name: "bob".into(),
            model_uri: Some("https://example.com/model.glb".into()),
            local_id: Some(3),
        };
        let meta = into_avatar_metadata(&member).unwrap();
        assert_eq!(meta.display_name, "bob");
        assert_eq!(meta.model_uri, Some("https://example.com/model.glb".into()));
    }

    #[test]
    fn into_avatar_metadata_bad_did() {
        let member = MemberDescriptor {
            did: "not-a-did".into(),
            display_name: "evil".into(),
            model_uri: None,
            local_id: None,
        };
        assert!(matches!(
            into_avatar_metadata(&member),
            Err(PresenceError::Protocol(_))
        ));
    }
}
