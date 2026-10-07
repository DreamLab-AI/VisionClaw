//! Visual regression FIXTURES (PRD-QE-002 §4.8 — numerical, not pixel-diff).
//!
//! These tests pin the deterministic numerical outputs that drive what the
//! Godot scene renders: per-node LOD assignments and per-joint avatar
//! transform matrices. They are the unit-level counterpart to the pixel-diff
//! visual regression suite that runs nightly.
//!
//! The point: if the numbers drift, downstream pixels drift. By gating the
//! numbers per-PR, we catch material drift before the nightly visual diff.

use visionclaw_xr_gdext::lod::{classify, LodLevel, LodPolicyState};

// -- fixture: avatar camera + node positions for LOD assignment --------------

/// Snapshot 1: a 6-node star around origin at fixed distances.
/// If anyone changes the LOD thresholds or the squared-distance maths, this
/// fixture catches it because the assignments will change.
#[test]
fn lod_fixture_star_at_origin() {
    let camera = [0.0_f32, 0.0, 0.0];
    let avatars = vec![
        [1.0, 0.0, 0.0],  // 1m → High
        [0.0, 4.99, 0.0], // 4.99m → High
        [0.0, 0.0, 5.0],  // 5m → Medium (boundary)
        [10.0, 0.0, 0.0], // 10m → Medium
        [0.0, 15.0, 0.0], // 15m → Low (boundary)
        [0.0, 0.0, 30.0], // 30m → Culled (boundary)
    ];
    let mut state = LodPolicyState::new();
    let levels = state.classify_avatars(camera, &avatars).to_vec();
    let expected = vec![
        LodLevel::High,
        LodLevel::High,
        LodLevel::Medium,
        LodLevel::Medium,
        LodLevel::Low,
        LodLevel::Culled,
    ];
    assert_eq!(levels, expected, "star fixture LOD assignment drifted");
}

/// Snapshot 2: camera offset 5m from origin — distances shift by 5m.
#[test]
fn lod_fixture_offset_camera() {
    let camera = [5.0_f32, 0.0, 0.0];
    let avatars = vec![
        [5.0, 0.0, 0.0],  // 0m → High
        [10.0, 0.0, 0.0], // 5m → Medium
        [25.0, 0.0, 0.0], // 20m → Low
        [40.0, 0.0, 0.0], // 35m → Culled
    ];
    let mut state = LodPolicyState::new();
    let levels = state.classify_avatars(camera, &avatars).to_vec();
    assert_eq!(
        levels,
        vec![
            LodLevel::High,
            LodLevel::Medium,
            LodLevel::Low,
            LodLevel::Culled,
        ]
    );
}

/// Snapshot 3: per-distance fixture table — used as a reference table for
/// the camera-relative renderer. Any change to thresholds breaks this.
#[test]
fn lod_fixture_distance_table() {
    let cases = [
        (0.0_f32, LodLevel::High),
        (1.0, LodLevel::High),
        (4.999, LodLevel::High),
        (5.0, LodLevel::Medium),
        (10.0, LodLevel::Medium),
        (14.999, LodLevel::Medium),
        (15.0, LodLevel::Low),
        (29.999, LodLevel::Low),
        (30.0, LodLevel::Culled),
        (100.0, LodLevel::Culled),
    ];
    for (d, expected) in cases {
        let actual = classify(d);
        assert_eq!(
            actual, expected,
            "drift at d = {d}: got {actual:?}, want {expected:?}"
        );
    }
}

// -- fixture: avatar joint transforms ----------------------------------------

use visionclaw_xr_presence::types::{PoseFrame, Transform};

/// Snapshot 4: a fixed avatar pose produces a fixed set of position floats
/// when fed through the wire codec.
#[test]
fn avatar_transform_fixture_round_trip() {
    use visionclaw_xr_presence::wire::{decode, encode};
    use visionclaw_xr_presence::{AvatarId, Did, RoomId};

    let frame = PoseFrame {
        timestamp_us: 1_700_000_000_000_000,
        head: Transform {
            position: [0.5, 1.7, -0.3],
            // 90° yaw: [0, sin(45°), 0, cos(45°)].
            rotation: [
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
        },
        left_hand: Some(Transform {
            position: [-0.4, 1.2, -0.5],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }),
        right_hand: Some(Transform {
            position: [0.4, 1.2, -0.5],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }),
    };
    let room = RoomId::parse("urn:visionclaw:room:sha256-12-feedface0001").expect("fixture room");
    let did = Did::parse(format!("did:nostr:{}", "0".repeat(64))).expect("fixture did");
    let avatar = AvatarId::from_did(&did);

    let bytes = encode(&frame, &room, &avatar).expect("encode");
    let decoded = decode(&bytes).expect("decode");

    // The fixture is the round-trip: any drift in the wire codec changes
    // these per-joint floats and breaks the assertion.
    assert_eq!(decoded.frame.head.position, [0.5, 1.7, -0.3]);
    assert_eq!(
        decoded.frame.head.rotation[1],
        std::f32::consts::FRAC_1_SQRT_2
    );
    assert_eq!(
        decoded.frame.left_hand.expect("left").position,
        [-0.4, 1.2, -0.5]
    );
    assert_eq!(
        decoded.frame.right_hand.expect("right").position,
        [0.4, 1.2, -0.5]
    );
}

/// Snapshot 5: identity transform survives a round-trip with bit-exact
/// floats — sanity baseline for downstream renderers that depend on the
/// identity-pose initial state.
#[test]
fn avatar_transform_fixture_identity() {
    use visionclaw_xr_presence::wire::{decode, encode};
    use visionclaw_xr_presence::{AvatarId, Did, RoomId};

    let frame = PoseFrame {
        timestamp_us: 1,
        head: Transform::identity(),
        left_hand: None,
        right_hand: None,
    };
    let room = RoomId::parse("urn:visionclaw:room:sha256-12-feedface0002").expect("fixture room");
    let did = Did::parse(format!("did:nostr:{}", "f".repeat(64))).expect("fixture did");
    let avatar = AvatarId::from_did(&did);

    let bytes = encode(&frame, &room, &avatar).expect("encode");
    let decoded = decode(&bytes).expect("decode");

    assert_eq!(decoded.frame.head.position, [0.0, 0.0, 0.0]);
    assert_eq!(decoded.frame.head.rotation, [0.0, 0.0, 0.0, 1.0]);
    assert!(decoded.frame.left_hand.is_none());
    assert!(decoded.frame.right_hand.is_none());
}
