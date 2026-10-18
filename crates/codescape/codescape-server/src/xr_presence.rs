//! `/ws/presence` for the Godot XR client: the JSON handshake, the room hub and
//! the `0x43` pose codec.
//!
//! The protocol is the one the XR client speaks to VisionClaw
//! (`xr-client/rust/src/presence.rs`), reproduced here byte for byte without
//! depending on any VisionClaw crate:
//!
//! ```text
//! server -> {"type":"challenge","nonce":<64 hex>,"ts":<µs>}
//! client -> {"type":"auth","did":"did:nostr:<64 hex>","signature":<hex>,
//!            "room_id":<urn>,"metadata":{"display_name":..,"model_uri":..}}
//! server -> {"type":"joined","room_id":..,"avatar_id":"urn:visionclaw:avatar:<64 hex>",
//!            "members":[{did,display_name,model_uri,local_id}]}
//! then     <- {"type":"avatar_joined",avatar_id,did,display_name,local_id}
//!          <- {"type":"avatar_left",avatar_id,did}
//!          -> binary 0x43 pose (single-avatar layout, [`decode_pose`])
//!          <- binary 0x43 sibling frame (multi-avatar layout, [`sibling_frame`])
//! ```
//!
//! Authentication is open until P6: the signature is accepted unverified, and
//! the claimed DID is only a label. A missing or malformed DID gets a
//! placeholder derived from the connection ([`Identity::resolve`]), because the
//! client rejects anything but `did:nostr:<64 hex>` and an avatar URN built from it.
//!
//! Bounds: [`MAX_MEMBERS_PER_ROOM`] members per room, [`MAX_ROOMS`] rooms,
//! [`MAX_POSES_PER_SEC`] relayed poses per member ([`PoseRate`]), and a
//! [`MEMBER_QUEUE`]-deep outbound queue per member; anything beyond is dropped.
//! A second join with the same avatar replaces the first (a reconnect must not
//! wait for the stale socket's idle timeout); the replaced socket is closed.
//!
//! # Example
//!
//! ```
//! use codescape_server::xr_presence::{decode_sibling, encode_pose, Identity, Outbound, PresenceHub};
//!
//! let hub = PresenceHub::new();
//! let room = "urn:visionclaw:room:sha256-12-deadbeefcafe";
//! let alice = Identity::resolve(Some(&format!("did:nostr:{}", "a".repeat(64))), "c1");
//! let bob = Identity::resolve(Some(&format!("did:nostr:{}", "b".repeat(64))), "c2");
//! let a = hub.join(room, &alice, "alice", None).unwrap();
//! let mut b = hub.join(room, &bob, "bob", None).unwrap();
//! assert_eq!(hub.members(room), 2);
//!
//! let raw = encode_pose(room, &alice.avatar_id, 7, 0b001, &[0u8; 28]);
//! let pose = codescape_server::xr_presence::decode_pose(&raw).unwrap();
//! assert_eq!(a.relay(&pose), 1);
//! let Ok(Outbound::Binary(frame)) = b.rx.try_recv() else { panic!("bob gets the pose") };
//! assert_eq!(decode_sibling(&frame).unwrap().2, vec![(a.local_id, 7, 0b001)]);
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

/// Avatar pose opcode (shared with VisionClaw's binary protocol).
pub const OPCODE_AVATAR_POSE: u8 = 0x43;
/// Most members one room holds.
pub const MAX_MEMBERS_PER_ROOM: usize = 64;
/// Most rooms open at once.
pub const MAX_ROOMS: usize = 256;
/// Relayed poses per member per second; more are dropped.
pub const MAX_POSES_PER_SEC: u32 = 20;
/// Outbound messages queued per member before new ones are dropped.
pub const MEMBER_QUEUE: usize = 64;
/// Longest accepted `room_id`.
pub const MAX_ROOM_ID_LEN: usize = 256;
/// Longest display name kept (characters).
pub const MAX_DISPLAY_NAME: usize = 64;
/// Room used when the client names none.
pub const DEFAULT_ROOM: &str = "urn:visionclaw:room:sha256-12-000000000000";
/// One transform on the wire: position `[f32; 3]` + rotation `[f32; 4]`.
pub const TRANSFORM_BYTES: usize = 28;

const SLOT_HEAD: u8 = 0b001;
const SLOT_LEFT: u8 = 0b010;
const SLOT_RIGHT: u8 = 0b100;
const AVATAR_PREFIX: &str = "urn:visionclaw:avatar:";
const DID_PREFIX: &str = "did:nostr:";

// --- JSON messages ------------------------------------------------------------

/// One peer in `joined.members`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberDescriptor {
    /// `did:nostr:<64 hex>`.
    pub did: String,
    /// Display name.
    pub display_name: String,
    /// Avatar model, if the client sent one.
    pub model_uri: Option<String>,
    /// Id this member's poses carry in sibling frames.
    pub local_id: u32,
}

/// Every text frame the server sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Handshake step 1.
    Challenge {
        /// 32 random bytes, hex.
        nonce: String,
        /// Microseconds since the Unix epoch.
        ts: u64,
    },
    /// Handshake step 3: the room as it is now, the joiner included.
    Joined {
        /// The room joined.
        room_id: String,
        /// The joiner's avatar URN.
        avatar_id: String,
        /// Everyone in the room.
        members: Vec<MemberDescriptor>,
    },
    /// Someone else joined.
    AvatarJoined {
        /// Their avatar URN.
        avatar_id: String,
        /// Their DID.
        did: String,
        /// Their display name.
        display_name: String,
        /// Id their poses carry.
        local_id: u32,
    },
    /// Someone left.
    AvatarLeft {
        /// Their avatar URN.
        avatar_id: String,
        /// Their DID.
        did: String,
    },
}

impl ServerMessage {
    /// A challenge with a fresh nonce from the OS RNG, stamped now.
    pub fn challenge() -> Result<Self, getrandom::Error> {
        let mut nonce = [0u8; 32];
        getrandom::getrandom(&mut nonce)?;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as u64);
        Ok(Self::Challenge {
            nonce: hex::encode(nonce),
            ts,
        })
    }

    /// The JSON text frame.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// `metadata` in `auth`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ClientMetadata {
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Avatar model URI.
    #[serde(default)]
    pub model_uri: Option<String>,
}

/// Text frames the client sends; anything else is [`ClientMessage::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Handshake step 2. Every field is optional: auth is open until P6.
    Auth {
        /// Claimed DID.
        #[serde(default)]
        did: Option<String>,
        /// Schnorr signature over the challenge (not verified before P6).
        #[serde(default)]
        signature: Option<String>,
        /// Room to join.
        #[serde(default)]
        room_id: Option<String>,
        /// Display name and model.
        #[serde(default)]
        metadata: Option<ClientMetadata>,
    },
    /// Any other `type`.
    #[serde(other)]
    Unknown,
}

impl ClientMessage {
    /// Parse a text frame; `None` for anything that is not a JSON object with a `type`.
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

/// The room a client asked for: its own (trimmed) if usable, [`DEFAULT_ROOM`] if
/// absent, `None` if too long or containing control characters.
pub fn room_of(requested: Option<&str>) -> Option<String> {
    let r = requested.map(str::trim).filter(|r| !r.is_empty());
    match r {
        None => Some(DEFAULT_ROOM.to_string()),
        Some(r) if r.len() <= MAX_ROOM_ID_LEN && !r.chars().any(char::is_control) => {
            Some(r.to_string())
        }
        Some(_) => None,
    }
}

/// A display name cut to [`MAX_DISPLAY_NAME`] characters, control characters removed.
pub fn display_name_of(raw: Option<&str>, fallback: &str) -> String {
    let n: String = raw
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DISPLAY_NAME)
        .collect();
    let n = n.trim();
    if n.is_empty() {
        fallback.to_string()
    } else {
        n.to_string()
    }
}

/// Who a member claims to be, in the shapes the client requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// `did:nostr:<64 lowercase hex>`.
    pub did: String,
    /// The claimed pubkey, when the client sent a well-formed DID.
    pub pubkey_hex: Option<String>,
    /// `urn:visionclaw:avatar:<64 hex>`.
    pub avatar_id: String,
}

impl Identity {
    /// From the claimed DID. A missing or malformed DID becomes a placeholder
    /// derived from `conn_seed` (unique per connection), so two anonymous
    /// clients never collide and nobody can be forced onto another's avatar.
    pub fn resolve(claimed: Option<&str>, conn_seed: &str) -> Self {
        let valid = claimed
            .and_then(|d| d.trim().strip_prefix(DID_PREFIX))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase);
        let hex = valid.clone().unwrap_or_else(|| {
            hex::encode(Sha256::digest(
                format!("codescape-xr-anon:{conn_seed}").as_bytes(),
            ))
        });
        Self {
            did: format!("{DID_PREFIX}{hex}"),
            pubkey_hex: valid,
            avatar_id: format!("{AVATAR_PREFIX}{hex}"),
        }
    }
}

// --- binary codec ---------------------------------------------------------------

/// FNV-1a-64 forwards and backwards over the room URN: the 16-byte room hash in
/// a client pose frame.
pub fn room_wire_hash(room: &str) -> [u8; 16] {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let fold = |seed: u64, it: &mut dyn Iterator<Item = &u8>| {
        it.fold(seed, |h, &b| (h ^ u64::from(b)).wrapping_mul(PRIME))
    };
    let b = room.as_bytes();
    let h1 = fold(0xcbf2_9ce4_8422_2325, &mut b.iter());
    let h2 = fold(0x8422_2325_cbf2_9ce4, &mut b.iter().rev());
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h1.to_le_bytes());
    out[8..].copy_from_slice(&h2.to_le_bytes());
    out
}

/// The `u32` room id in a sibling frame: the first four bytes of [`room_wire_hash`].
pub fn room_u32(room: &str) -> u32 {
    let h = room_wire_hash(room);
    u32::from_le_bytes([h[0], h[1], h[2], h[3]])
}

fn slots(mask: u8) -> Option<usize> {
    if mask & SLOT_HEAD == 0 || mask & !(SLOT_HEAD | SLOT_LEFT | SLOT_RIGHT) != 0 {
        return None;
    }
    Some(mask.count_ones() as usize)
}

/// One decoded client pose.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundPose {
    /// [`room_wire_hash`] of the room the client believes it is in.
    pub room_hash: [u8; 16],
    /// The avatar URN the client signs its poses with.
    pub avatar_id: String,
    /// Client clock, microseconds.
    pub timestamp_us: u64,
    /// Slot mask: bit0 head (always), bit1 left hand, bit2 right hand.
    pub mask: u8,
    /// `popcount(mask)` transforms, [`TRANSFORM_BYTES`] each, all finite.
    pub transforms: Vec<u8>,
}

/// Why a pose frame was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PoseError {
    /// Shorter than its own header or declared length.
    #[error("truncated pose frame")]
    Truncated,
    /// Not `0x43`.
    #[error("not a pose frame")]
    Opcode,
    /// Head slot missing or unknown slot bits.
    #[error("bad slot mask")]
    Mask,
    /// Avatar id is not UTF-8.
    #[error("avatar id is not UTF-8")]
    AvatarId,
    /// NaN or infinite component.
    #[error("non-finite transform")]
    NonFinite,
}

/// Decode a client pose frame:
/// `[0x43][u16 len][16 room hash][u8 n][n avatar id][u64 ts][u8 mask][28 × slots]`.
pub fn decode_pose(b: &[u8]) -> Result<InboundPose, PoseError> {
    const FIXED: usize = 16 + 1 + 8 + 1;
    if b.len() < 3 {
        return Err(PoseError::Truncated);
    }
    if b[0] != OPCODE_AVATAR_POSE {
        return Err(PoseError::Opcode);
    }
    let len = usize::from(u16::from_le_bytes([b[1], b[2]]));
    if len < FIXED || b.len() < 3 + len {
        return Err(PoseError::Truncated);
    }
    let body = &b[3..3 + len];
    let mut room_hash = [0u8; 16];
    room_hash.copy_from_slice(&body[..16]);
    let id_len = usize::from(body[16]);
    let mut c = 17;
    if c + id_len + 9 > body.len() {
        return Err(PoseError::Truncated);
    }
    let avatar_id = std::str::from_utf8(&body[c..c + id_len])
        .map_err(|_| PoseError::AvatarId)?
        .to_string();
    c += id_len;
    let timestamp_us = u64::from_le_bytes(body[c..c + 8].try_into().unwrap_or_default());
    let mask = body[c + 8];
    c += 9;
    let n = slots(mask).ok_or(PoseError::Mask)?;
    let need = n * TRANSFORM_BYTES;
    if c + need > body.len() {
        return Err(PoseError::Truncated);
    }
    let transforms = body[c..c + need].to_vec();
    let finite = transforms
        .as_chunks::<4>()
        .0
        .iter()
        .all(|f| f32::from_le_bytes(*f).is_finite());
    if !finite {
        return Err(PoseError::NonFinite);
    }
    Ok(InboundPose {
        room_hash,
        avatar_id,
        timestamp_us,
        mask,
        transforms,
    })
}

/// Encode a client pose frame (the layout [`decode_pose`] reads). `transforms`
/// must hold `popcount(mask)` transforms; used by tests and the smoke client.
pub fn encode_pose(
    room: &str,
    avatar_id: &str,
    timestamp_us: u64,
    mask: u8,
    transforms: &[u8],
) -> Vec<u8> {
    let id = avatar_id.as_bytes();
    let id = &id[..id.len().min(255)];
    let len = 16 + 1 + id.len() + 8 + 1 + transforms.len();
    let mut v = Vec::with_capacity(3 + len);
    v.push(OPCODE_AVATAR_POSE);
    v.extend_from_slice(&(len.min(usize::from(u16::MAX)) as u16).to_le_bytes());
    v.extend_from_slice(&room_wire_hash(room));
    v.push(id.len() as u8);
    v.extend_from_slice(id);
    v.extend_from_slice(&timestamp_us.to_le_bytes());
    v.push(mask);
    v.extend_from_slice(transforms);
    v
}

/// Encode a sibling frame with one pose:
/// `[0x43][u64 seq][u32 room][u16 1][u32 local_id][u64 ts][u8 mask][28 × slots]`.
pub fn sibling_frame(seq: u64, room: u32, local_id: u32, pose: &InboundPose) -> Bytes {
    let mut v = Vec::with_capacity(15 + 13 + pose.transforms.len());
    v.push(OPCODE_AVATAR_POSE);
    v.extend_from_slice(&seq.to_le_bytes());
    v.extend_from_slice(&room.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&local_id.to_le_bytes());
    v.extend_from_slice(&pose.timestamp_us.to_le_bytes());
    v.push(pose.mask);
    v.extend_from_slice(&pose.transforms);
    Bytes::from(v)
}

/// One entry of a decoded sibling frame: `(local_id, timestamp_us, mask)`.
pub type SiblingEntry = (u32, u64, u8);

/// Decode a sibling frame into `(seq, room, entries)` (tests and the smoke client).
pub fn decode_sibling(b: &[u8]) -> Option<(u64, u32, Vec<SiblingEntry>)> {
    if b.len() < 15 || b[0] != OPCODE_AVATAR_POSE {
        return None;
    }
    let seq = u64::from_le_bytes(b[1..9].try_into().ok()?);
    let room = u32::from_le_bytes(b[9..13].try_into().ok()?);
    let count = u16::from_le_bytes(b[13..15].try_into().ok()?);
    let mut c = 15;
    let mut out = Vec::new();
    for _ in 0..count {
        let head = b.get(c..c + 13)?;
        let local = u32::from_le_bytes(head[..4].try_into().ok()?);
        let ts = u64::from_le_bytes(head[4..12].try_into().ok()?);
        let mask = head[12];
        c += 13 + slots(mask)? * TRANSFORM_BYTES;
        if c > b.len() {
            return None;
        }
        out.push((local, ts, mask));
    }
    Some((seq, room, out))
}

// --- rate -----------------------------------------------------------------------

/// Fixed one-second window: at most [`MAX_POSES_PER_SEC`] poses per member.
#[derive(Debug, Clone)]
pub struct PoseRate {
    window: Instant,
    count: u32,
}

impl PoseRate {
    /// A fresh window starting at `now`.
    pub fn new(now: Instant) -> Self {
        Self {
            window: now,
            count: 0,
        }
    }

    /// Whether a pose arriving at `now` may be relayed.
    pub fn allow_at(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.window) >= Duration::from_secs(1) {
            self.window = now;
            self.count = 0;
        }
        if self.count >= MAX_POSES_PER_SEC {
            return false;
        }
        self.count += 1;
        true
    }
}

// --- hub ------------------------------------------------------------------------

/// A message for one member's socket.
#[derive(Debug, Clone, PartialEq)]
pub enum Outbound {
    /// JSON room event.
    Text(String),
    /// Sibling pose frame.
    Binary(Bytes),
    /// Close the socket with this reason (replaced by a newer join).
    Close(String),
}

/// Why a join was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JoinError {
    /// [`MAX_MEMBERS_PER_ROOM`] reached.
    #[error("room full")]
    RoomFull,
    /// [`MAX_ROOMS`] reached.
    #[error("too many rooms")]
    TooManyRooms,
}

struct Entry {
    local_id: u32,
    desc: MemberDescriptor,
    avatar_id: String,
    tx: mpsc::Sender<Outbound>,
}

#[derive(Default)]
struct Inner {
    rooms: Mutex<HashMap<String, Vec<Entry>>>,
    seq: AtomicU64,
    next_local: AtomicU32,
}

impl Inner {
    fn send_all(room: &[Entry], except: u32, msg: &Outbound) -> usize {
        room.iter()
            .filter(|e| e.local_id != except)
            .filter(|e| e.tx.try_send(msg.clone()).is_ok())
            .count()
    }

    fn left(e: &Entry) -> Outbound {
        Outbound::Text(
            ServerMessage::AvatarLeft {
                avatar_id: e.avatar_id.clone(),
                did: e.desc.did.clone(),
            }
            .to_json(),
        )
    }
}

/// XR avatar rooms. Cheap to clone; clones share the rooms.
#[derive(Clone, Default)]
pub struct PresenceHub(Arc<Inner>);

impl PresenceHub {
    /// No rooms.
    pub fn new() -> Self {
        Self::default()
    }

    /// Join `room`, announcing the joiner to everyone already there. A member
    /// with the same avatar is replaced: it gets [`Outbound::Close`] and the room
    /// sees it leave. The returned [`Membership`] leaves when dropped.
    pub fn join(
        &self,
        room: &str,
        who: &Identity,
        display_name: &str,
        model_uri: Option<String>,
    ) -> Result<Membership, JoinError> {
        let mut rooms = self.0.rooms.lock();
        if !rooms.contains_key(room) && rooms.len() >= MAX_ROOMS {
            return Err(JoinError::TooManyRooms);
        }
        let members = rooms.entry(room.to_string()).or_default();
        if let Some(i) = members.iter().position(|e| e.avatar_id == who.avatar_id) {
            let old = members.swap_remove(i);
            let _ = old
                .tx
                .try_send(Outbound::Close("replaced by a newer connection".into()));
            Inner::send_all(members, old.local_id, &Inner::left(&old));
        }
        if members.len() >= MAX_MEMBERS_PER_ROOM {
            if members.is_empty() {
                rooms.remove(room);
            }
            return Err(JoinError::RoomFull);
        }
        let local_id = loop {
            let id = self
                .0
                .next_local
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1);
            if id != 0 {
                break id;
            }
        };
        let desc = MemberDescriptor {
            did: who.did.clone(),
            display_name: display_name.to_string(),
            model_uri,
            local_id,
        };
        let announce = Outbound::Text(
            ServerMessage::AvatarJoined {
                avatar_id: who.avatar_id.clone(),
                did: who.did.clone(),
                display_name: display_name.to_string(),
                local_id,
            }
            .to_json(),
        );
        Inner::send_all(members, local_id, &announce);
        let (tx, rx) = mpsc::channel(MEMBER_QUEUE);
        members.push(Entry {
            local_id,
            desc,
            avatar_id: who.avatar_id.clone(),
            tx,
        });
        let joined = ServerMessage::Joined {
            room_id: room.to_string(),
            avatar_id: who.avatar_id.clone(),
            members: members.iter().map(|e| e.desc.clone()).collect(),
        };
        Ok(Membership {
            hub: self.clone(),
            room: room.to_string(),
            room_hash: room_wire_hash(room),
            room_u32: room_u32(room),
            local_id,
            avatar_id: who.avatar_id.clone(),
            joined,
            rx,
        })
    }

    /// Members in `room`.
    pub fn members(&self, room: &str) -> usize {
        self.0.rooms.lock().get(room).map_or(0, Vec::len)
    }

    fn leave(&self, room: &str, local_id: u32) {
        let mut rooms = self.0.rooms.lock();
        let Some(members) = rooms.get_mut(room) else {
            return;
        };
        // Already gone when a newer join replaced this member.
        if let Some(i) = members.iter().position(|e| e.local_id == local_id) {
            let e = members.swap_remove(i);
            Inner::send_all(members, local_id, &Inner::left(&e));
        }
        if members.is_empty() {
            rooms.remove(room);
        }
    }
}

/// One member's place in a room; dropping it leaves (and announces `avatar_left`).
pub struct Membership {
    hub: PresenceHub,
    /// Room URN.
    pub room: String,
    room_hash: [u8; 16],
    room_u32: u32,
    /// Id this member's poses carry.
    pub local_id: u32,
    /// This member's avatar URN.
    pub avatar_id: String,
    /// The `joined` reply, roster as of the join.
    pub joined: ServerMessage,
    /// Messages for this member's socket.
    pub rx: mpsc::Receiver<Outbound>,
}

impl Membership {
    /// Relay a pose to the rest of the room as a sibling frame. A pose naming
    /// another room or another avatar is dropped. Returns the members reached.
    pub fn relay(&self, pose: &InboundPose) -> usize {
        if pose.room_hash != self.room_hash || pose.avatar_id != self.avatar_id {
            return 0;
        }
        let seq = self.hub.0.seq.fetch_add(1, Ordering::Relaxed);
        let msg = Outbound::Binary(sibling_frame(seq, self.room_u32, self.local_id, pose));
        let rooms = self.hub.0.rooms.lock();
        rooms
            .get(&self.room)
            .map_or(0, |m| Inner::send_all(m, self.local_id, &msg))
    }
}

impl Drop for Membership {
    fn drop(&mut self) {
        self.hub.leave(&self.room, self.local_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &str = "urn:visionclaw:room:sha256-12-deadbeefcafe";

    // --- a copy of the XR client's wire types (xr-client/rust/src/presence.rs) ---
    // Deserialising our output into these proves the client can parse it.

    #[derive(Debug, Deserialize, PartialEq)]
    struct ClientMember {
        did: String,
        display_name: String,
        #[serde(default)]
        model_uri: Option<String>,
        #[serde(default)]
        local_id: Option<u32>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum ClientSide {
        Challenge {
            nonce: String,
            ts: u64,
        },
        Joined {
            room_id: String,
            avatar_id: String,
            members: Vec<ClientMember>,
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

    /// `AvatarId::parse` / `Did::parse` in `visionclaw-xr-presence`.
    fn client_accepts_urn(s: &str, prefix: &str) -> bool {
        s.strip_prefix(prefix)
            .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    fn did(c: char) -> String {
        format!("did:nostr:{}", c.to_string().repeat(64))
    }

    /// The exact `auth` frame `PresenceClient::handshake` emits (serde field order of
    /// `ClientMessage::Auth` + `ClientMetadata`).
    fn client_auth_json() -> String {
        format!(
            r#"{{"type":"auth","did":"{}","signature":"{}","room_id":"{ROOM}","metadata":{{"display_name":"alice","model_uri":null}}}}"#,
            did('a'),
            "c".repeat(128)
        )
    }

    fn text(o: Outbound) -> ClientSide {
        match o {
            Outbound::Text(t) => serde_json::from_str(&t).unwrap(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn handshake_golden_against_the_client_wire() {
        // 1. challenge: the client hex-decodes the nonce to exactly 32 bytes.
        let ch = ServerMessage::challenge().unwrap().to_json();
        match serde_json::from_str::<ClientSide>(&ch).unwrap() {
            ClientSide::Challenge { nonce, ts } => {
                assert_eq!(hex::decode(nonce).unwrap().len(), 32);
                assert!(ts > 1_700_000_000_000_000, "microseconds");
            }
            other => panic!("{other:?}"),
        }
        // 2. auth, verbatim from the client.
        let auth = ClientMessage::parse(&client_auth_json()).unwrap();
        let ClientMessage::Auth {
            did: d,
            signature,
            room_id,
            metadata,
        } = auth
        else {
            panic!("auth not recognised")
        };
        assert_eq!(signature.unwrap().len(), 128);
        let who = Identity::resolve(d.as_deref(), "conn-1");
        assert_eq!(who.pubkey_hex.as_deref(), Some("a".repeat(64).as_str()));
        let room = room_of(room_id.as_deref()).unwrap();
        let name = display_name_of(metadata.unwrap().display_name.as_deref(), "xr");
        // 3. joined: exact JSON, and parseable by the client with valid URNs.
        let hub = PresenceHub::new();
        let m = hub.join(&room, &who, &name, None).unwrap();
        let joined = m.joined.to_json();
        assert_eq!(
            joined,
            format!(
                r#"{{"type":"joined","room_id":"{ROOM}","avatar_id":"urn:visionclaw:avatar:{a}","members":[{{"did":"did:nostr:{a}","display_name":"alice","model_uri":null,"local_id":{id}}}]}}"#,
                a = "a".repeat(64),
                id = m.local_id
            )
        );
        match serde_json::from_str::<ClientSide>(&joined).unwrap() {
            ClientSide::Joined {
                room_id,
                avatar_id,
                members,
            } => {
                assert_eq!(room_id, ROOM);
                assert!(client_accepts_urn(&avatar_id, AVATAR_PREFIX));
                assert!(client_accepts_urn(&members[0].did, DID_PREFIX));
                assert_eq!(members[0].local_id, Some(m.local_id));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn open_auth_tolerates_missing_and_malformed_fields() {
        let bare = ClientMessage::parse(r#"{"type":"auth"}"#).unwrap();
        assert!(matches!(bare, ClientMessage::Auth { did: None, .. }));
        assert_eq!(
            ClientMessage::parse(r#"{"type":"pose_hint","x":1}"#),
            Some(ClientMessage::Unknown)
        );
        assert_eq!(ClientMessage::parse("not json"), None);
        let a = Identity::resolve(Some("did:nostr:xyz"), "c1");
        let b = Identity::resolve(None, "c2");
        assert!(a.pubkey_hex.is_none() && b.pubkey_hex.is_none());
        assert_ne!(a.avatar_id, b.avatar_id, "placeholders are per connection");
        for who in [a, b] {
            assert!(client_accepts_urn(&who.avatar_id, AVATAR_PREFIX));
            assert!(client_accepts_urn(&who.did, DID_PREFIX));
        }
        let upper = Identity::resolve(Some(&format!("did:nostr:{}", "AB".repeat(32))), "c");
        assert_eq!(upper.pubkey_hex.unwrap(), "ab".repeat(32));
        assert_eq!(room_of(None).unwrap(), DEFAULT_ROOM);
        assert!(room_of(Some(&"r".repeat(MAX_ROOM_ID_LEN + 1))).is_none());
        assert!(room_of(Some("a\nb")).is_none());
        assert_eq!(display_name_of(Some("  "), "xr-1"), "xr-1");
        assert_eq!(
            display_name_of(Some(&"n".repeat(200)), "x").chars().count(),
            MAX_DISPLAY_NAME
        );
    }

    #[test]
    fn two_members_see_each_other_and_poses_relay() {
        let hub = PresenceHub::new();
        let alice = Identity::resolve(Some(&did('a')), "1");
        let bob = Identity::resolve(Some(&did('b')), "2");
        let mut a = hub.join(ROOM, &alice, "alice", None).unwrap();
        let mut b = hub.join(ROOM, &bob, "bob", Some("m.glb".into())).unwrap();
        // Bob's roster lists both; alice hears bob join.
        let ServerMessage::Joined { members, .. } = &b.joined else {
            panic!()
        };
        let names: Vec<&str> = members.iter().map(|m| m.display_name.as_str()).collect();
        assert_eq!(names, ["alice", "bob"]);
        assert_eq!(
            text(a.rx.try_recv().unwrap()),
            ClientSide::AvatarJoined {
                avatar_id: bob.avatar_id.clone(),
                did: bob.did.clone(),
                display_name: "bob".into(),
                local_id: Some(b.local_id),
            }
        );
        assert!(b.rx.try_recv().is_err(), "no self-announcement");

        // Alice's pose reaches bob as a sibling frame keyed by her local_id.
        let mut tf = Vec::new();
        for f in [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0] {
            tf.extend_from_slice(&f.to_le_bytes());
        }
        let raw = encode_pose(ROOM, &alice.avatar_id, 42, SLOT_HEAD, &tf);
        let pose = decode_pose(&raw).unwrap();
        assert_eq!(a.relay(&pose), 1);
        let Outbound::Binary(frame) = b.rx.try_recv().unwrap() else {
            panic!()
        };
        let (_, room, entries) = decode_sibling(&frame).unwrap();
        assert_eq!(room, room_u32(ROOM));
        assert_eq!(entries, vec![(a.local_id, 42, SLOT_HEAD)]);
        assert_eq!(
            &frame[frame.len() - 28..],
            tf.as_slice(),
            "transform bytes intact"
        );
        assert!(a.rx.try_recv().is_err(), "no echo to the sender");

        // A pose claiming bob's avatar, or another room, is not relayed.
        let spoof = decode_pose(&encode_pose(ROOM, &bob.avatar_id, 1, SLOT_HEAD, &tf)).unwrap();
        assert_eq!(a.relay(&spoof), 0);
        let other =
            decode_pose(&encode_pose("urn:x", &alice.avatar_id, 1, SLOT_HEAD, &tf)).unwrap();
        assert_eq!(a.relay(&other), 0);
    }

    #[test]
    fn disconnect_sends_avatar_left_and_empties_the_room() {
        let hub = PresenceHub::new();
        let alice = Identity::resolve(Some(&did('a')), "1");
        let bob = Identity::resolve(Some(&did('b')), "2");
        let mut a = hub.join(ROOM, &alice, "alice", None).unwrap();
        let b = hub.join(ROOM, &bob, "bob", None).unwrap();
        let _ = a.rx.try_recv();
        drop(b);
        assert_eq!(
            text(a.rx.try_recv().unwrap()),
            ClientSide::AvatarLeft {
                avatar_id: bob.avatar_id.clone(),
                did: Some(bob.did.clone()),
            }
        );
        assert_eq!(hub.members(ROOM), 1);
        drop(a);
        assert_eq!(hub.members(ROOM), 0);
        assert!(hub.0.rooms.lock().is_empty(), "empty rooms are freed");
    }

    #[test]
    fn rejoin_replaces_the_stale_member() {
        let hub = PresenceHub::new();
        let alice = Identity::resolve(Some(&did('a')), "1");
        let bob = Identity::resolve(Some(&did('b')), "2");
        let mut b = hub.join(ROOM, &bob, "bob", None).unwrap();
        let mut old = hub.join(ROOM, &alice, "alice", None).unwrap();
        let _ = b.rx.try_recv();
        let new = hub.join(ROOM, &alice, "alice", None).unwrap();
        assert!(matches!(old.rx.try_recv().unwrap(), Outbound::Close(_)));
        assert!(matches!(
            text(b.rx.try_recv().unwrap()),
            ClientSide::AvatarLeft { .. }
        ));
        assert!(matches!(
            text(b.rx.try_recv().unwrap()),
            ClientSide::AvatarJoined { .. }
        ));
        assert_eq!(hub.members(ROOM), 2);
        drop(old);
        assert_eq!(
            hub.members(ROOM),
            2,
            "the replaced member's drop is a no-op"
        );
        assert!(b.rx.try_recv().is_err(), "and announces nothing");
        drop(new);
    }

    #[test]
    fn rooms_and_queues_are_bounded() {
        let hub = PresenceHub::new();
        let mut keep = Vec::new();
        for i in 0..MAX_MEMBERS_PER_ROOM {
            let who = Identity::resolve(None, &format!("c{i}"));
            keep.push(hub.join(ROOM, &who, "x", None).unwrap());
        }
        let extra = Identity::resolve(None, "extra");
        assert_eq!(
            hub.join(ROOM, &extra, "x", None).err(),
            Some(JoinError::RoomFull)
        );
        // The first member's queue fills with join announcements, then drops.
        let pose = decode_pose(&encode_pose(
            ROOM,
            &keep[1].avatar_id,
            0,
            SLOT_HEAD,
            &[0; 28],
        ))
        .unwrap();
        for _ in 0..MEMBER_QUEUE * 2 {
            keep[1].relay(&pose);
        }
        let mut n = 0;
        while keep[0].rx.try_recv().is_ok() {
            n += 1;
        }
        assert_eq!(n, MEMBER_QUEUE, "queue depth is the bound");
    }

    #[test]
    fn pose_rate_is_twenty_per_second() {
        let t0 = Instant::now();
        let mut r = PoseRate::new(t0);
        assert_eq!((0..50).filter(|_| r.allow_at(t0)).count(), 20);
        assert!(!r.allow_at(t0 + Duration::from_millis(999)));
        assert!(r.allow_at(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn hostile_pose_frames_are_refused() {
        let ok = encode_pose(ROOM, "urn:visionclaw:avatar:aa", 1, 0b111, &[0u8; 84]);
        assert!(decode_pose(&ok).is_ok());
        assert_eq!(decode_pose(&ok[..ok.len() - 1]), Err(PoseError::Truncated));
        assert_eq!(decode_pose(&[]), Err(PoseError::Truncated));
        let mut zero_len = ok.clone();
        zero_len[1] = 0;
        zero_len[2] = 0;
        assert_eq!(decode_pose(&zero_len), Err(PoseError::Truncated));
        let mut op = ok.clone();
        op[0] = 0x42;
        assert_eq!(decode_pose(&op), Err(PoseError::Opcode));
        let no_head = encode_pose(ROOM, "a", 1, 0b010, &[0u8; 28]);
        assert_eq!(decode_pose(&no_head), Err(PoseError::Mask));
        let short = encode_pose(ROOM, "a", 1, 0b011, &[0u8; 28]);
        assert_eq!(decode_pose(&short), Err(PoseError::Truncated));
        let mut nan = [0u8; 28];
        nan[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(
            decode_pose(&encode_pose(ROOM, "a", 1, 0b001, &nan)),
            Err(PoseError::NonFinite)
        );
    }

    #[test]
    fn room_hash_matches_the_client_fnv() {
        // Recomputed by hand from visionclaw-xr-presence `RoomId::wire_hash`.
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in ROOM.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        assert_eq!(&room_wire_hash(ROOM)[..8], &h.to_le_bytes());
        assert_eq!(room_u32(ROOM), h as u32);
    }
}
