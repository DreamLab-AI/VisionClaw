//! ADR-2020: the client half of the `0x44` agent co-presence integration.
//!
//! The closeout found the codec present on both sides with no live encode/decode
//! integration anywhere. The server side is now wired in `PresenceActor`; this
//! exercises the consumer against frames built by the **real encoder**, the same
//! function the server calls, so the two halves cannot drift.

use visionclaw_xr_gdext::avatar_state::RemotePresenceStore;
use visionclaw_xr_presence::agent_presence::{
    encode_agent_presence, AgentActivity, AgentPresence, AgentPresenceDelta, AttentionTarget,
    OPCODE_AGENT_PRESENCE,
};

fn presence(a: AgentActivity, gaze: [f32; 3], attn: AttentionTarget) -> AgentPresence {
    AgentPresence::new(a, gaze, attn)
}

const FWD: [f32; 3] = [0.0, 0.0, -1.0];

#[test]
fn a_full_delta_from_the_real_encoder_reconstructs_the_published_state() {
    let published = presence(
        AgentActivity::Working,
        FWD,
        AttentionTarget::GraphNode(4_242),
    );
    let frame = encode_agent_presence(1, &[AgentPresenceDelta::full(7, &published)]).unwrap();
    assert_eq!(frame[0], OPCODE_AGENT_PRESENCE);

    let mut store = RemotePresenceStore::new();
    assert_eq!(store.ingest_frame(&frame).unwrap(), Some(1));

    let got = store.get(7).expect("agent 7 is known");
    assert_eq!(got.state, AgentActivity::Working);
    assert_eq!(got.attention, AttentionTarget::GraphNode(4_242));
    // Node correlation on the client: the attended id is in the 26-bit wire
    // space the position stream uses, so the scene can resolve it.
    assert_eq!(store.attention_node(7), Some(4_242));
    assert_eq!(store.watermark(), Some(1));
}

#[test]
fn elided_fields_are_folded_onto_the_previous_state() {
    // The point of holding state client-side: a delta carrying only `state` must
    // not blank the attention target the agent still holds.
    let first = presence(AgentActivity::Idle, FWD, AttentionTarget::GraphNode(99));
    let second = presence(AgentActivity::Working, FWD, AttentionTarget::GraphNode(99));

    let mut store = RemotePresenceStore::new();
    store
        .ingest_frame(&encode_agent_presence(1, &[AgentPresenceDelta::full(3, &first)]).unwrap())
        .unwrap();

    let delta = AgentPresenceDelta::between(3, &first, &second);
    assert!(delta.gaze_dir.is_none(), "unchanged gaze is elided");
    assert!(delta.attention.is_none(), "unchanged attention is elided");

    store
        .ingest_frame(&encode_agent_presence(2, &[delta]).unwrap())
        .unwrap();

    let got = store.get(3).unwrap();
    assert_eq!(
        got.state,
        AgentActivity::Working,
        "the changed field applied"
    );
    assert_eq!(
        got.attention,
        AttentionTarget::GraphNode(99),
        "the elided field survived"
    );
}

#[test]
fn an_out_of_order_batch_is_refused_whole() {
    // Applying a delta stream out of order reconstructs a state that never
    // existed on the server.
    let a = presence(AgentActivity::Idle, FWD, AttentionTarget::None);
    let b = presence(AgentActivity::Working, FWD, AttentionTarget::None);

    let mut store = RemotePresenceStore::new();
    store
        .ingest_frame(&encode_agent_presence(5, &[AgentPresenceDelta::full(1, &b)]).unwrap())
        .unwrap();
    assert_eq!(store.get(1).unwrap().state, AgentActivity::Working);

    // An older batch arrives late.
    let refused = store
        .ingest_frame(&encode_agent_presence(4, &[AgentPresenceDelta::full(1, &a)]).unwrap())
        .unwrap();
    assert_eq!(refused, None, "an older sequence must be refused");
    assert_eq!(
        store.get(1).unwrap().state,
        AgentActivity::Working,
        "state must not regress"
    );

    // A duplicate is refused too.
    assert_eq!(
        store
            .ingest_frame(&encode_agent_presence(5, &[AgentPresenceDelta::full(1, &a)]).unwrap())
            .unwrap(),
        None
    );
    assert_eq!(store.stale_frames(), 2);
    assert_eq!(store.watermark(), Some(5));
}

#[test]
fn several_agents_are_tracked_independently() {
    let mut store = RemotePresenceStore::new();
    let working = presence(AgentActivity::Working, FWD, AttentionTarget::GraphNode(10));
    let speaking = presence(AgentActivity::Speaking, FWD, AttentionTarget::User);
    let frame = encode_agent_presence(
        1,
        &[
            AgentPresenceDelta::full(1, &working),
            AgentPresenceDelta::full(2, &speaking),
        ],
    )
    .unwrap();

    assert_eq!(store.ingest_frame(&frame).unwrap(), Some(2));
    assert_eq!(store.agent_ids(), vec![1, 2]);
    assert_eq!(store.attention_node(1), Some(10));
    assert_eq!(
        store.attention_node(2),
        None,
        "attending a user, not a node"
    );
    assert_eq!(store.get(2).unwrap().state, AgentActivity::Speaking);
}

#[test]
fn stale_removal_drops_the_agent_the_server_retired() {
    // The server announces retirement on the JSON room-event channel; the scene
    // forwards that here so the avatar stops claiming to attend a node.
    let mut store = RemotePresenceStore::new();
    store
        .ingest_frame(
            &encode_agent_presence(
                1,
                &[AgentPresenceDelta::full(
                    9,
                    &presence(AgentActivity::Working, FWD, AttentionTarget::GraphNode(55)),
                )],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(store.len(), 1);
    assert_eq!(store.attention_node(9), Some(55));

    assert!(store.remove(9), "the retired agent is dropped");
    assert!(store.is_empty());
    assert_eq!(store.attention_node(9), None);
    assert!(!store.remove(9), "removing twice is a no-op");
}

#[test]
fn the_store_declines_frames_belonging_to_a_sibling_opcode() {
    // Unknown-tag handling at the demultiplexer: 0x44 must not claim a 0x43 pose
    // frame or a 0x05 position frame.
    let mut store = RemotePresenceStore::new();
    for opcode in [0x03u8, 0x05, 0x23, 0x43, 0x00, 0xFF] {
        let err = store
            .ingest_frame(&[opcode, 0, 0, 0])
            .expect_err("must decline a sibling opcode");
        assert!(err.contains("not an agent-presence frame"), "got: {err}");
    }
    assert!(store.is_empty());
    assert_eq!(store.watermark(), None, "a declined frame moves nothing");
}

#[test]
fn a_truncated_agent_presence_frame_is_rejected_not_half_applied() {
    let frame = encode_agent_presence(
        1,
        &[AgentPresenceDelta::full(
            1,
            &presence(AgentActivity::Working, FWD, AttentionTarget::GraphNode(3)),
        )],
    )
    .unwrap();

    let mut store = RemotePresenceStore::new();
    for cut in 1..frame.len() {
        let mut truncated = frame[..cut].to_vec();
        // Keep the opcode so the store accepts routing and the codec does the
        // rejecting — that is the boundary under test.
        truncated[0] = OPCODE_AGENT_PRESENCE;
        if store.ingest_frame(&truncated).is_ok() {
            continue; // a prefix that happens to be a complete shorter frame
        }
        assert!(store.is_empty(), "a rejected frame must not half-apply");
    }
}

#[test]
fn an_agent_seen_mid_stream_converges_from_a_neutral_base() {
    // A client that joins after the stream started receives a delta for an agent
    // it has never seen. Folding onto a neutral base keeps it usable until a full
    // delta arrives, rather than dropping the agent entirely.
    let mut store = RemotePresenceStore::new();
    let before = presence(AgentActivity::Idle, FWD, AttentionTarget::None);
    let after = presence(AgentActivity::Working, FWD, AttentionTarget::None);
    let partial = AgentPresenceDelta::between(4, &before, &after);

    assert_eq!(
        store
            .ingest_frame(&encode_agent_presence(1, &[partial]).unwrap())
            .unwrap(),
        Some(1)
    );
    let got = store
        .get(4)
        .expect("agent is tracked despite the partial delta");
    assert_eq!(got.state, AgentActivity::Working);
}
