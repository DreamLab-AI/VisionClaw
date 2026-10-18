//! ADR-2018 consumer freshness and ADR-2019 per-opcode frame policy, exercised
//! in the XR decoder against the *shared* wire fixtures.
//!
//! The fixtures come from the dependency-free `visionclaw-wire` crate, the same
//! one the server encoder tests use, so this deliberately isolated workspace
//! gets identical bytes without taking on the domain crate's dependency tree.
//! The server side proves those fixtures are byte-identical to the real encoder
//! output; this side proves the consumer handles them per the documented policy.

use visionclaw_wire as fx;
use visionclaw_xr_gdext::binary_protocol::{
    decode_agent_action_frame, decode_position_frame, decode_position_frame_with_sequence,
    encode_broadcast_ack, DecodeError, Freshness, FreshnessGate,
};

fn records(n: u32) -> Vec<fx::NodeRecord> {
    (0..n)
        .map(|i| fx::NodeRecord::plain(i + 1, [i as f32, 0.0, 0.0]))
        .collect()
}

// ── ADR-2018: consumer freshness ───────────────────────────────────────────

#[test]
fn v5_sequence_is_now_recoverable_by_the_consumer() {
    // The closeout finding: the decoder skipped these bytes, so the envelope's
    // ordering information never reached the consumer at all.
    let frame = fx::v5_frame(9_000, &records(2));
    let (seq, updates) = decode_position_frame_with_sequence(&frame).expect("valid V5");
    assert_eq!(seq, Some(9_000));
    assert_eq!(updates.len(), 2);

    // A V3 frame makes no ordering claim whatsoever.
    let (seq, updates) =
        decode_position_frame_with_sequence(&fx::v3_frame(&records(2))).expect("valid V3");
    assert_eq!(seq, None);
    assert_eq!(updates.len(), 2);
}

#[test]
fn increasing_sequences_are_accepted_and_advance_the_watermark() {
    let mut gate = FreshnessGate::new();
    for seq in [1u64, 2, 7, 8] {
        let frame = fx::v5_frame(seq, &records(1));
        let (verdict, applied) = gate.admit_frame(&frame).expect("valid");
        assert_eq!(verdict, Freshness::Accept { sequence: seq });
        assert_eq!(applied.expect("records applied").len(), 1);
    }
    assert_eq!(gate.watermark(), Some(8));
    assert_eq!(gate.stats().accepted, 4);
}

#[test]
fn a_duplicate_sequence_is_refused_and_its_records_withheld() {
    let mut gate = FreshnessGate::new();
    let first = fx::v5_frame(5, &records(1));
    assert!(gate
        .admit_frame(&first)
        .unwrap()
        .0
        .is_accepted());

    // Re-delivery of the same sequence adds nothing.
    let (verdict, applied) = gate.admit_frame(&first).expect("valid");
    assert_eq!(verdict, Freshness::RejectDuplicate { sequence: 5 });
    assert!(
        applied.is_none(),
        "a rejected frame must not hand back records"
    );
    assert_eq!(gate.watermark(), Some(5));
    assert_eq!(gate.stats().rejected_duplicate, 1);
}

#[test]
fn a_decreasing_sequence_is_refused_as_stale() {
    let mut gate = FreshnessGate::new();
    gate.admit(Some(100));

    // The exact case the closeout names: a slower/reordered producer hands the
    // consumer older state than it already holds.
    let stale = fx::v5_frame(99, &records(1));
    let (verdict, applied) = gate.admit_frame(&stale).expect("valid");
    assert_eq!(
        verdict,
        Freshness::RejectStale {
            sequence: 99,
            watermark: 100
        }
    );
    assert!(applied.is_none());
    // The watermark never moves backwards outside an explicit resync.
    assert_eq!(gate.watermark(), Some(100));
    assert_eq!(gate.stats().rejected_stale, 1);
}

#[test]
fn a_late_frame_built_before_a_newer_one_never_wins() {
    // Whichever frame is older loses, however it got delayed.
    let mut gate = FreshnessGate::new();
    assert!(gate.admit(Some(10)).is_accepted());
    assert!(gate.admit(Some(11)).is_accepted());
    // A chunk built before seq 11 arrives late and must not win.
    assert_eq!(
        gate.admit(Some(10)),
        Freshness::RejectStale {
            sequence: 10,
            watermark: 11
        }
    );
    assert!(gate.admit(Some(12)).is_accepted());
    assert_eq!(gate.watermark(), Some(12));
}

#[test]
fn reconnect_rebaselines_on_the_first_frame_even_when_the_sequence_restarts() {
    let mut gate = FreshnessGate::new();
    assert!(gate.admit(Some(5_000)).is_accepted());

    // The producer restarted: its sequence space begins again from 1. Under a
    // bare `seq > watermark` rule the client would reject every frame forever.
    gate.reconnect();
    assert!(gate.awaiting_resync());
    let resync = fx::v5_frame(1, &records(3));
    let (verdict, applied) = gate.admit_frame(&resync).expect("valid");
    assert_eq!(
        verdict,
        Freshness::AcceptResync {
            sequence: 1,
            previous: Some(5_000)
        }
    );
    assert_eq!(applied.expect("snapshot applied").len(), 3);
    assert!(!gate.awaiting_resync());
    assert_eq!(gate.watermark(), Some(1));

    // Normal ordering resumes in the new epoch: a re-delivery of the frame we
    // just accepted is a duplicate, and anything older than the new watermark
    // is stale — the pre-reconnect 5_000 no longer has any authority.
    assert_eq!(
        gate.admit(Some(1)),
        Freshness::RejectDuplicate { sequence: 1 }
    );
    assert!(gate.admit(Some(2)).is_accepted());
    assert_eq!(
        gate.admit(Some(1)),
        Freshness::RejectStale {
            sequence: 1,
            watermark: 2
        }
    );
}

#[test]
fn the_first_frame_after_a_reconnect_is_taken_whatever_its_sequence() {
    // Every position frame is a complete snapshot of what the server holds for this client
    // (ADR-2141), so there is nothing to wait for: the first sequenced frame after a reconnect
    // re-baselines, and is applied.
    let mut gate = FreshnessGate::new();
    assert!(gate.admit(Some(40)).is_accepted());
    gate.reconnect();
    let (verdict, applied) = gate
        .admit_frame(&fx::v5_frame(41, &records(1)))
        .expect("valid");
    assert_eq!(verdict, Freshness::AcceptResync { sequence: 41, previous: Some(40) });
    assert_eq!(applied.expect("applied").len(), 1);
    assert!(!gate.awaiting_resync());
}

#[test]
fn the_position_acknowledgement_is_byte_identical_to_the_shared_wire_fixture() {
    // The server decodes the fixture's bytes (src/utils/binary_protocol.rs); this proves the XR
    // encoder produces exactly them, so a chunk's ack is never mis-read.
    for (seq, nodes, ts) in [(1u64, 0u32, 0u64), (48_113, 2500, 1_700_000_000_123), (u64::MAX, u32::MAX, u64::MAX)] {
        assert_eq!(encode_broadcast_ack(seq, nodes, ts), fx::broadcast_ack_frame(seq, nodes, ts));
    }
    assert_eq!(fx::BROADCAST_ACK_BYTES, 21);
    assert_eq!(fx::OPCODE_BROADCAST_ACK, 0x34);
}

#[test]
fn an_unsequenced_v3_frame_never_moves_the_watermark() {
    let mut gate = FreshnessGate::new();
    assert!(gate.admit(Some(70)).is_accepted());

    // V3 is accepted for compatibility but must not be able to unblock a
    // sequence the gate has already refused.
    let (verdict, applied) = gate
        .admit_frame(&fx::v3_frame(&records(2)))
        .expect("valid");
    assert_eq!(verdict, Freshness::AcceptUnsequenced);
    assert_eq!(applied.expect("applied").len(), 2);
    assert_eq!(gate.watermark(), Some(70));
    assert_eq!(
        gate.admit(Some(69)),
        Freshness::RejectStale {
            sequence: 69,
            watermark: 70
        }
    );
}

#[test]
fn the_frozen_52_byte_record_is_preserved_end_to_end() {
    // ADR-2018's other half: freshness work must not disturb the record size.
    let frame = fx::v5_frame(1, &records(4));
    assert_eq!(frame.len(), 1 + 8 + 4 * 52);
    assert_eq!(decode_position_frame(&frame).unwrap().len(), 4);
}

// ── ADR-2019: per-opcode malformed/truncated-frame policy ──────────────────

#[test]
fn position_frames_reject_unknown_versions_whole() {
    // Policy: a position frame is all-or-nothing. An unknown version is refused
    // before the body is looked at, so a future opcode can never be
    // misinterpreted as V3 records.
    for version in [0x00u8, 0x01, 0x02, 0x04, 0x06, 0x23, 0x43, 0x44, 0xFF] {
        let frame = fx::frame_with_unknown_version(version, &records(2));
        match decode_position_frame(&frame) {
            Err(DecodeError::BadVersion { version: got, .. }) => assert_eq!(got, version),
            other => panic!("version {version:#04x} must be rejected, got {other:?}"),
        }
    }
}

#[test]
fn position_frames_reject_a_misaligned_body_whole() {
    // A partial trailing record is not a partially-valid frame: rejecting the
    // whole frame is what stops a torn record reaching the layout.
    for dropped in [1usize, 7, 51] {
        let frame = fx::v3_frame_misaligned(&records(3), dropped);
        match decode_position_frame(&frame) {
            Err(DecodeError::Misaligned { len }) => {
                assert_ne!(len % 52, 0, "fixture must actually be misaligned")
            }
            other => panic!("dropping {dropped} bytes must be Misaligned, got {other:?}"),
        }
    }
}

#[test]
fn a_v5_frame_cut_inside_its_sequence_is_too_short_not_zero_sequence() {
    // The sequence is envelope header, not payload: a truncated header must
    // never decode as "sequence 0", which would corrupt the watermark.
    for kept in 0..8usize {
        let frame = fx::v5_frame_truncated_sequence(kept);
        match decode_position_frame_with_sequence(&frame) {
            Err(DecodeError::TooShort { need, got }) => {
                assert_eq!(need, 9);
                assert_eq!(got, 1 + kept);
            }
            other => panic!("kept {kept} sequence bytes must be TooShort, got {other:?}"),
        }
    }
}

#[test]
fn an_empty_frame_is_too_short() {
    match decode_position_frame(&fx::empty_frame()) {
        Err(DecodeError::TooShort { need: 1, got: 0 }) => {}
        other => panic!("empty frame must be TooShort, got {other:?}"),
    }
}

#[test]
fn the_agent_action_batch_is_a_tolerant_prefix_parse_by_policy() {
    // ADR-2019's scoped guarantee: 0x23 does NOT share the position frame's
    // all-or-nothing rule. Complete events before a truncation are accepted and
    // the incomplete tail is dropped — deliberate, so one torn event does not
    // discard a whole burst of visual activity.
    let events = [
        fx::ActionEvent::plain(11, 21, 1_000),
        fx::ActionEvent::plain(12, 22, 1_001),
        fx::ActionEvent::plain(13, 23, 1_002),
    ];
    let full = fx::agent_action_frame(&events);
    let decoded = decode_agent_action_frame(&full).expect("is a 0x23 frame");
    assert_eq!(decoded.len(), 3);

    // Cut the last event in half: the first two survive.
    let truncated = fx::agent_action_frame_truncated(&events, 8);
    let decoded = decode_agent_action_frame(&truncated).expect("still a 0x23 frame");
    assert_eq!(decoded.len(), 2, "accepted prefix, dropped torn tail");
    assert_eq!(decoded[0].source_agent_id, 11);
    assert_eq!(decoded[1].source_agent_id, 12);
}

#[test]
fn an_overstated_batch_count_cannot_read_past_the_buffer() {
    // A hostile or corrupt count must bound-check, not over-read.
    let events = [fx::ActionEvent::plain(1, 2, 3)];
    let frame = fx::agent_action_frame_overstated_count(&events, 9);
    let decoded = decode_agent_action_frame(&frame).expect("is a 0x23 frame");
    assert_eq!(decoded.len(), 1, "only the events actually present");
}

#[test]
fn a_batch_header_with_no_bodies_yields_no_actions() {
    let frame = fx::agent_action_frame_header_only(4);
    let decoded = decode_agent_action_frame(&frame).expect("is a 0x23 frame");
    assert!(decoded.is_empty());
}

#[test]
fn the_two_codecs_do_not_claim_each_others_frames() {
    // Unknown-tag handling at the demultiplexer: each decoder must decline the
    // other's frame rather than mis-parse it.
    let position = fx::v5_frame(1, &records(1));
    assert!(
        decode_agent_action_frame(&position).is_none(),
        "0x23 decoder must decline a position frame"
    );

    let actions = fx::agent_action_frame(&[fx::ActionEvent::plain(1, 2, 3)]);
    match decode_position_frame(&actions) {
        Err(DecodeError::BadVersion { version: 0x23, .. }) => {}
        other => panic!("position decoder must decline a 0x23 frame, got {other:?}"),
    }
}
