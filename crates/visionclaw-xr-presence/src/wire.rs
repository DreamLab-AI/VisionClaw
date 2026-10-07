use bytes::{BufMut, Bytes, BytesMut};

use crate::error::WireError;
use crate::types::{AvatarId, PoseFrame, RoomId, Transform};

/// Avatar pose frame opcode. Sibling of the existing 0x42 graph position
/// frame under ADR-061's single-binary-protocol umbrella. Registered in
/// `docs/binary-protocol.md` and `docs/xr-godot-system-architecture.md` §7.1.
pub const OPCODE_AVATAR_POSE: u8 = 0x43;

const ROOM_HASH_BYTES: usize = 16;
const AVATAR_ID_LEN_BYTES: usize = 1;
const TIMESTAMP_BYTES: usize = 8;
const TRANSFORM_MASK_BYTES: usize = 1;
const HEADER_FIXED: usize = 1 + 2 + ROOM_HASH_BYTES;
pub const MAX_AVATAR_ID_WIRE_LEN: usize = 255;

const SLOT_HEAD: u8 = 0b001;
const SLOT_LEFT: u8 = 0b010;
const SLOT_RIGHT: u8 = 0b100;

/// Encode a pose frame into the on-wire layout for opcode 0x43.
///
/// Layout (little-endian):
/// ```text
/// [u8  opcode = 0x43]
/// [u16 frame_len_LE]            // bytes that follow this field
/// [u8;16 room_id_hash]
/// [u8  avatar_id_len]
/// [u8;N avatar_id_utf8]
/// [u64 timestamp_us_LE]
/// [u8  transform_mask]          // bit0=head bit1=left_hand bit2=right_hand
/// [{u8;28} transforms...]       // present slots in head, left, right order
/// ```
///
/// `transform_mask` is a slot-presence bitmask rather than a plain count so
/// `(left_hand=None, right_hand=Some)` and `(left_hand=Some, right_hand=None)`
/// round-trip distinguishably.
pub fn encode(frame: &PoseFrame, room: &RoomId, avatar: &AvatarId) -> Result<Bytes, WireError> {
    let avatar_bytes = avatar.as_str().as_bytes();
    if avatar_bytes.len() > MAX_AVATAR_ID_WIRE_LEN {
        return Err(WireError::AvatarIdTooLong {
            len: avatar_bytes.len(),
            max: MAX_AVATAR_ID_WIRE_LEN,
        });
    }

    let mut mask = SLOT_HEAD;
    if frame.left_hand.is_some() {
        mask |= SLOT_LEFT;
    }
    if frame.right_hand.is_some() {
        mask |= SLOT_RIGHT;
    }
    let count = frame.transform_count();

    let body_len = ROOM_HASH_BYTES
        + AVATAR_ID_LEN_BYTES
        + avatar_bytes.len()
        + TIMESTAMP_BYTES
        + TRANSFORM_MASK_BYTES
        + (count as usize) * Transform::WIRE_SIZE;

    let mut buf = BytesMut::with_capacity(1 + 2 + body_len);
    buf.put_u8(OPCODE_AVATAR_POSE);
    buf.put_u16_le(body_len as u16);
    buf.put_slice(&room.wire_hash());
    buf.put_u8(avatar_bytes.len() as u8);
    buf.put_slice(avatar_bytes);
    buf.put_u64_le(frame.timestamp_us);
    buf.put_u8(mask);
    write_transform(&mut buf, &frame.head);
    if let Some(t) = &frame.left_hand {
        write_transform(&mut buf, t);
    }
    if let Some(t) = &frame.right_hand {
        write_transform(&mut buf, t);
    }

    Ok(buf.freeze())
}

/// Decoded view: the raw room hash and avatar URN are returned alongside the
/// pose so the caller can verify them against authenticated session state.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    pub room_hash: [u8; 16],
    pub avatar_id: String,
    pub frame: PoseFrame,
}

pub fn decode(bytes: &[u8]) -> Result<DecodedFrame, WireError> {
    if bytes.len() < HEADER_FIXED + AVATAR_ID_LEN_BYTES + TIMESTAMP_BYTES + TRANSFORM_MASK_BYTES {
        return Err(WireError::TooShort {
            need: HEADER_FIXED + AVATAR_ID_LEN_BYTES + TIMESTAMP_BYTES + TRANSFORM_MASK_BYTES,
            got: bytes.len(),
        });
    }

    let opcode = bytes[0];
    if opcode != OPCODE_AVATAR_POSE {
        return Err(WireError::BadOpcode {
            found: opcode,
            expected: OPCODE_AVATAR_POSE,
        });
    }

    let frame_len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
    if bytes.len() < 1 + 2 + frame_len {
        return Err(WireError::LengthMismatch {
            declared: frame_len,
            actual: bytes.len().saturating_sub(3),
        });
    }

    // The outer check (line 93) guards the *whole message* minimum, but a frame
    // can declare a `frame_len` too small to hold the fixed prefix while still
    // satisfying `bytes.len() >= 3 + frame_len`. Without this guard a hostile
    // peer sending `frame_len = 0` makes `body` empty and the fixed-offset
    // slices below panic (copy_from_slice out-of-range), aborting the room
    // actor — a remote DoS. Reject any body that cannot hold room_hash + the
    // id-length byte + timestamp + transform mask. (Close-out adversarial
    // finding: wire-decode panic on truncated-but-consistent frame.)
    if frame_len < ROOM_HASH_BYTES + AVATAR_ID_LEN_BYTES + TIMESTAMP_BYTES + TRANSFORM_MASK_BYTES {
        return Err(WireError::TooShort {
            need: ROOM_HASH_BYTES + AVATAR_ID_LEN_BYTES + TIMESTAMP_BYTES + TRANSFORM_MASK_BYTES,
            got: frame_len,
        });
    }

    let body = &bytes[3..3 + frame_len];
    let mut cursor = 0usize;

    let mut room_hash = [0u8; 16];
    room_hash.copy_from_slice(&body[cursor..cursor + ROOM_HASH_BYTES]);
    cursor += ROOM_HASH_BYTES;

    let id_len = body[cursor] as usize;
    cursor += 1;
    if cursor + id_len > body.len() {
        return Err(WireError::LengthMismatch {
            declared: frame_len,
            actual: body.len(),
        });
    }
    let avatar_id = std::str::from_utf8(&body[cursor..cursor + id_len])
        .map_err(|_| WireError::AvatarIdNotUtf8)?
        .to_owned();
    cursor += id_len;

    if cursor + TIMESTAMP_BYTES + TRANSFORM_MASK_BYTES > body.len() {
        return Err(WireError::LengthMismatch {
            declared: frame_len,
            actual: body.len(),
        });
    }
    let mut ts_buf = [0u8; 8];
    ts_buf.copy_from_slice(&body[cursor..cursor + TIMESTAMP_BYTES]);
    let timestamp_us = u64::from_le_bytes(ts_buf);
    cursor += TIMESTAMP_BYTES;

    let mask = body[cursor];
    cursor += 1;
    if mask & SLOT_HEAD == 0 || mask & !(SLOT_HEAD | SLOT_LEFT | SLOT_RIGHT) != 0 {
        return Err(WireError::BadTransformCount { count: mask });
    }
    let count = (mask & SLOT_HEAD != 0) as u8
        + (mask & SLOT_LEFT != 0) as u8
        + (mask & SLOT_RIGHT != 0) as u8;

    let needed_bytes = (count as usize) * Transform::WIRE_SIZE;
    if cursor + needed_bytes > body.len() {
        return Err(WireError::LengthMismatch {
            declared: frame_len,
            actual: body.len(),
        });
    }

    let head = read_transform(&body[cursor..cursor + Transform::WIRE_SIZE]);
    cursor += Transform::WIRE_SIZE;
    let left_hand = if mask & SLOT_LEFT != 0 {
        let t = read_transform(&body[cursor..cursor + Transform::WIRE_SIZE]);
        cursor += Transform::WIRE_SIZE;
        Some(t)
    } else {
        None
    };
    let right_hand = if mask & SLOT_RIGHT != 0 {
        Some(read_transform(&body[cursor..cursor + Transform::WIRE_SIZE]))
    } else {
        None
    };

    Ok(DecodedFrame {
        room_hash,
        avatar_id,
        frame: PoseFrame {
            timestamp_us,
            head,
            left_hand,
            right_hand,
        },
    })
}

fn write_transform(buf: &mut BytesMut, t: &Transform) {
    for v in t.position {
        buf.put_f32_le(v);
    }
    for v in t.rotation {
        buf.put_f32_le(v);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Did;
    use proptest::prelude::*;

    fn sample_room() -> RoomId {
        RoomId::parse("urn:visionclaw:room:sha256-12-deadbeefcafe").unwrap()
    }

    fn sample_avatar() -> AvatarId {
        let did = Did::parse(format!("did:nostr:{}", "f".repeat(64))).unwrap();
        AvatarId::from_did(&did)
    }

    #[test]
    fn round_trip_head_only() {
        let frame = PoseFrame {
            timestamp_us: 1_700_000_000_000_000,
            head: Transform {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            left_hand: None,
            right_hand: None,
        };
        let bytes = encode(&frame, &sample_room(), &sample_avatar()).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.frame, frame);
        assert_eq!(decoded.avatar_id, sample_avatar().as_str());
        assert_eq!(decoded.room_hash, sample_room().wire_hash());
    }

    #[test]
    fn round_trip_two_hands() {
        let t = Transform {
            position: [0.5, 1.7, -0.3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let frame = PoseFrame {
            timestamp_us: 42,
            head: t,
            left_hand: Some(t),
            right_hand: Some(t),
        };
        let bytes = encode(&frame, &sample_room(), &sample_avatar()).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.frame, frame);
    }

    #[test]
    fn rejects_wrong_opcode() {
        let mut bytes = encode(
            &PoseFrame {
                timestamp_us: 0,
                head: Transform::identity(),
                left_hand: None,
                right_hand: None,
            },
            &sample_room(),
            &sample_avatar(),
        )
        .unwrap()
        .to_vec();
        bytes[0] = 0x42;
        assert!(matches!(decode(&bytes), Err(WireError::BadOpcode { .. })));
    }

    #[test]
    fn rejects_truncated_frame_len_without_panic() {
        // A structurally-valid-length message (>= 29 bytes) that declares a
        // frame_len too small to hold the fixed prefix must be rejected, not
        // panic. Regression for the wire-decode DoS: 29 zero bytes with opcode
        // 0x43 and frame_len = 0 previously panicked at copy_from_slice.
        let mut bytes = vec![0u8; 29];
        bytes[0] = OPCODE_AVATAR_POSE;
        // bytes[1..3] = 0 → frame_len = 0
        assert!(matches!(decode(&bytes), Err(WireError::TooShort { .. })));
    }

    proptest! {
        #[test]
        fn proptest_round_trip(
            ts in any::<u64>(),
            hp in proptest::array::uniform3(-100f32..100.0),
            hr in proptest::array::uniform4(-1f32..1.0),
            include_left in any::<bool>(),
            include_right in any::<bool>(),
        ) {
            let head = Transform { position: hp, rotation: hr };
            let other = Transform { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0] };
            let frame = PoseFrame {
                timestamp_us: ts,
                head,
                left_hand: include_left.then_some(other),
                right_hand: include_right.then_some(other),
            };
            let bytes = encode(&frame, &sample_room(), &sample_avatar()).unwrap();
            let decoded = decode(&bytes).unwrap();
            prop_assert_eq!(decoded.frame, frame);
        }
    }
}
