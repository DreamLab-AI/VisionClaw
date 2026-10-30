use serde::{Deserialize, Serialize};

use crate::error::RoomError;

/// Nostr DID, format `did:nostr:<64-hex>`. Hex pubkey is canonical scope form
/// per CLAUDE.md; bech32 npub is only used at the Nostr relay wire boundary.
/// @ontology urn:ngm:class:did-nostr
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Did(String);

impl Did {
    pub fn parse(s: impl Into<String>) -> Result<Self, RoomError> {
        let s = s.into();
        let rest = s
            .strip_prefix("did:nostr:")
            .ok_or_else(|| RoomError::InvalidDid { did: s.clone() })?;
        if rest.len() != 64 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RoomError::InvalidDid { did: s });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn pubkey_hex(&self) -> &str {
        &self.0[10..]
    }
}

impl std::fmt::Display for Did {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `urn:visionclaw:room:<sha256-12>` per CLAUDE.md content-addressing convention.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoomId(String);

impl RoomId {
    pub fn parse(s: impl Into<String>) -> Result<Self, RoomError> {
        let s = s.into();
        let rest = s
            .strip_prefix("urn:visionclaw:room:")
            .ok_or_else(|| RoomError::InvalidUrn { urn: s.clone() })?;
        if !rest.starts_with("sha256-12-") {
            return Err(RoomError::InvalidUrn { urn: s });
        }
        let hex = &rest[10..];
        if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RoomError::InvalidUrn { urn: s });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Stable 16-byte room hash digest used in the on-wire frame header.
    pub fn wire_hash(&self) -> [u8; 16] {
        let bytes = self.0.as_bytes();
        let mut out = [0u8; 16];
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        out[..8].copy_from_slice(&h.to_le_bytes());
        let mut h2: u64 = 0x84222325cbf29ce4;
        for &b in bytes.iter().rev() {
            h2 ^= b as u64;
            h2 = h2.wrapping_mul(0x100000001b3);
        }
        out[8..].copy_from_slice(&h2.to_le_bytes());
        out
    }
}

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `urn:visionclaw:avatar:<did-hex>` — bound 1:1 with avatar's DID per
/// `ddd-xr-godot-context.md` §7.2.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AvatarId(String);

impl AvatarId {
    pub fn parse(s: impl Into<String>) -> Result<Self, RoomError> {
        let s = s.into();
        let rest = s
            .strip_prefix("urn:visionclaw:avatar:")
            .ok_or_else(|| RoomError::InvalidUrn { urn: s.clone() })?;
        if rest.len() != 64 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RoomError::InvalidUrn { urn: s });
        }
        Ok(Self(s))
    }

    pub fn from_did(did: &Did) -> Self {
        Self(format!("urn:visionclaw:avatar:{}", did.pubkey_hex()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn pubkey_hex(&self) -> &str {
        &self.0[22..]
    }
}

impl std::fmt::Display for AvatarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 28 bytes on the wire: position[3] + rotation[4] floats, little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

impl Transform {
    pub const WIRE_SIZE: usize = 28;

    pub const fn identity() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn quaternion_magnitude(&self) -> f32 {
        let [x, y, z, w] = self.rotation;
        (x * x + y * y + z * z + w * w).sqrt()
    }
}

/// Per `ddd-xr-godot-context.md` §3.5 — `AvatarTransform` triple of head plus
/// optional left/right hand poses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoseFrame {
    pub timestamp_us: u64,
    pub head: Transform,
    pub left_hand: Option<Transform>,
    pub right_hand: Option<Transform>,
}

impl PoseFrame {
    pub fn transform_count(&self) -> u8 {
        1 + self.left_hand.is_some() as u8 + self.right_hand.is_some() as u8
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvatarMetadata {
    pub did: Did,
    pub display_name: String,
    pub model_uri: Option<String>,
}

/// Axis-aligned bounding box, used by [`crate::validate::world_bounds`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    pub const fn symmetric(half_extent_m: f32) -> Self {
        Self {
            min: [-half_extent_m, -half_extent_m, -half_extent_m],
            max: [half_extent_m, half_extent_m, half_extent_m],
        }
    }

    pub fn contains(&self, p: &[f32; 3]) -> bool {
        p[0] >= self.min[0]
            && p[0] <= self.max[0]
            && p[1] >= self.min[1]
            && p[1] <= self.max[1]
            && p[2] >= self.min[2]
            && p[2] <= self.max[2]
    }
}

/// Placeholder hand pose carrier for the anatomy validator. The full MANO
/// joint set (26×7 floats per hand) is out of scope for v1; the validator
/// currently checks the wrist pose only and exposes hooks for future joint
/// data via `joints`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandPose {
    pub wrist: Transform,
    /// TODO(PRD-008-followup): populate with MANO joint angles once the gdext
    /// hand tracker exposes them. Empty in v1.
    pub joints: Vec<[f32; 4]>,
}
