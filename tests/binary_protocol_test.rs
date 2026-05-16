//! Phase 3 (ADR-02) — integration tests for the V3 binary protocol +
//! `BroadcastActor` state machine.
//!
//! Tests organised per WORKTREE-PLAN.md §7 Agent B (tester) tasks:
//!   - v3_frame_roundtrip_5000_nodes         — T-07 (PRD-02 §6 wire size)
//!   - v3_frame_rejects_corruption           — T-07
//!   - heartbeat_fires_in_settled_state_*    — T-09 BDD-1 (PRD-02 A8)
//!   - register_client_sends_immediate_*     — T-12 (PRD-02 §3)
//!   - frame_id_monotonic_per_connection     — ADR-02 D7
//!   - layout_started_resets_frame_ids       — ADR-02 D2
//!   - drop_on_backpressure_increments_*     — T-11 (PRD-02 A3)
//!   - state_machine_transition_table        — WORKTREE-PLAN §2

use std::sync::{Arc, Mutex};
use std::time::Duration;

use actix::prelude::*;
use webxr::actors::messages::{
    BroadcastState, GetBroadcastActorStatus, GetPositionFrameSnapshot, OnLayoutDestabilised,
    OnLayoutSettled, OnLayoutStarted, PositionFrameSnapshot, PositionRow,
    RegisterBroadcastClient, SendToClientBinary,
};
use webxr::actors::{BroadcastActor, BroadcastConfig};
use webxr::protocol::v3_frame::{BinaryV3Frame, NodeRow, V3DecodeError, V3_MAGIC};

// ----------------------------------------------------------------------------
// Test doubles
// ----------------------------------------------------------------------------

/// In-memory stub for the canonical snapshot source. Avoids booting
/// `GraphStateActor` and the `KnowledgeGraphRepository` mock surface.
/// The held snapshot can be swapped between sends to simulate GPU pushes.
struct StubSnapshotSource {
    snapshot: Arc<Mutex<Arc<PositionFrameSnapshot>>>,
}

impl StubSnapshotSource {
    fn new(initial: Arc<PositionFrameSnapshot>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(initial)),
        }
    }
}

impl Actor for StubSnapshotSource {
    type Context = Context<Self>;
}

impl Handler<GetPositionFrameSnapshot> for StubSnapshotSource {
    type Result = Result<Arc<PositionFrameSnapshot>, String>;
    fn handle(
        &mut self,
        _msg: GetPositionFrameSnapshot,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        Ok(Arc::clone(&self.snapshot.lock().unwrap()))
    }
}

/// Recording mock client. Captures every binary frame received.
struct RecordingClient {
    frames: Vec<Vec<u8>>,
}

impl RecordingClient {
    fn new() -> Self {
        Self { frames: Vec::new() }
    }
}

impl Actor for RecordingClient {
    type Context = Context<Self>;
}

impl Handler<SendToClientBinary> for RecordingClient {
    type Result = ();
    fn handle(&mut self, msg: SendToClientBinary, _ctx: &mut Self::Context) {
        self.frames.push(msg.0);
    }
}

#[derive(Message)]
#[rtype(result = "Vec<Vec<u8>>")]
struct DumpFrames;

impl Handler<DumpFrames> for RecordingClient {
    type Result = MessageResult<DumpFrames>;
    fn handle(&mut self, _msg: DumpFrames, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.frames.clone())
    }
}

/// Slow client that blocks its event loop for `delay` on every send.
/// With a small mailbox capacity this guarantees `SendError::Full`
/// on subsequent rapid sends, exercising the ADR-02 D3 drop path.
struct SlowClient {
    delay: Duration,
}
impl Actor for SlowClient {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1);
    }
}
impl Handler<SendToClientBinary> for SlowClient {
    type Result = ();
    fn handle(&mut self, _msg: SendToClientBinary, _ctx: &mut Self::Context) {
        std::thread::sleep(self.delay);
    }
}

fn sample_snapshot(epoch: u64, count: usize) -> Arc<PositionFrameSnapshot> {
    let rows: Vec<PositionRow> = (0..count)
        .map(|i| PositionRow {
            node_id: (i + 1) as u32,
            x: i as f32,
            y: i as f32 + 0.5,
            z: i as f32 + 1.0,
            vx: 0.01 * i as f32,
            vy: 0.02 * i as f32,
            vz: 0.03 * i as f32,
        })
        .collect();
    Arc::new(PositionFrameSnapshot {
        epoch,
        node_count: count as u32,
        rows,
    })
}

// ----------------------------------------------------------------------------
// Wire-format tests (T-07)
// ----------------------------------------------------------------------------

#[test]
fn v3_frame_roundtrip_5000_nodes() {
    let rows: Vec<NodeRow> = (0..5000)
        .map(|i| {
            NodeRow::new(
                (i as u32) + 1,
                [i as f32, i as f32 * 0.5, -(i as f32)],
                [0.01, 0.02, 0.03],
            )
        })
        .collect();
    let frame = BinaryV3Frame::new(7, rows.clone());
    let bytes = frame.encode();
    assert_eq!(bytes.len(), 140_012, "PRD-02 §6 wire size");
    assert_eq!(&bytes[0..4], b"V3F0");

    let decoded = BinaryV3Frame::decode(&bytes).expect("decode");
    assert_eq!(decoded.magic, V3_MAGIC);
    assert_eq!(decoded.frame_id, 7);
    assert_eq!(decoded.node_count, 5000);
    assert_eq!(decoded.nodes.len(), 5000);
    assert_eq!(decoded.nodes, rows);
}

#[test]
fn v3_frame_rejects_corruption() {
    let frame = BinaryV3Frame::new(0, vec![NodeRow::new(1, [0.0; 3], [0.0; 3])]);
    let mut bytes = frame.encode();

    let mut bad_magic = bytes.clone();
    bad_magic[0] = 0;
    assert!(matches!(
        BinaryV3Frame::decode(&bad_magic),
        Err(V3DecodeError::BadMagic { .. })
    ));

    bytes.truncate(8);
    assert!(matches!(
        BinaryV3Frame::decode(&bytes),
        Err(V3DecodeError::LengthMismatch { .. } | V3DecodeError::TooShort(_))
    ));
}

// ----------------------------------------------------------------------------
// State-machine and timer tests (T-09)
// ----------------------------------------------------------------------------

/// PRD-02 A8 — heartbeat fires on wall clock, not physics ticks.
#[actix_rt::test]
async fn heartbeat_fires_in_settled_state_without_physics_events() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 3)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_millis(400),
        active_poll_interval: Duration::from_millis(100),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    let status_t0 = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(status_t0.state, BroadcastState::Settled);
    assert_eq!(status_t0.heartbeats_fired_total, 0);

    actix_rt::time::sleep(Duration::from_millis(1100)).await;

    let status_t1 = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(status_t1.state, BroadcastState::Settled);
    assert!(
        status_t1.heartbeats_fired_total >= 2,
        "expected ≥2 heartbeats in 1.1 s with 400 ms interval, got {}",
        status_t1.heartbeats_fired_total
    );
}

/// PRD-02 §3 + T-12 — `RegisterBroadcastClient` triggers an immediate frame.
#[actix_rt::test]
async fn register_client_sends_immediate_snapshot() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 1)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_secs(30),
        active_poll_interval: Duration::from_secs(30),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    let client = RecordingClient::new().start();
    bcast
        .send(RegisterBroadcastClient {
            client_id: 1,
            recipient: client.clone().recipient(),
        })
        .await
        .unwrap();

    actix_rt::time::sleep(Duration::from_millis(200)).await;

    let frames = client.send(DumpFrames).await.unwrap();
    assert!(
        !frames.is_empty(),
        "expected ≥1 immediate frame after RegisterBroadcastClient"
    );

    let decoded = BinaryV3Frame::decode(&frames[0]).expect("decode");
    assert_eq!(decoded.magic, V3_MAGIC);
    assert_eq!(decoded.frame_id, 0, "first frame on a new connection");
    assert_eq!(decoded.node_count, 1);
    assert_eq!(decoded.nodes[0].node_id, 1);
}

/// ADR-02 D7 — frame_id is monotonic per connection.
#[actix_rt::test]
async fn frame_id_monotonic_per_connection() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 2)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_millis(150),
        active_poll_interval: Duration::from_millis(50),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    let client = RecordingClient::new().start();
    bcast
        .send(RegisterBroadcastClient {
            client_id: 1,
            recipient: client.clone().recipient(),
        })
        .await
        .unwrap();

    actix_rt::time::sleep(Duration::from_millis(600)).await;

    let frames = client.send(DumpFrames).await.unwrap();
    assert!(
        frames.len() >= 3,
        "expected ≥3 frames over 600 ms with 150 ms heartbeat, got {}",
        frames.len()
    );

    let ids: Vec<u32> = frames
        .iter()
        .map(|f| BinaryV3Frame::decode(f).unwrap().frame_id)
        .collect();
    assert_eq!(ids[0], 0, "first frame_id on a connection must be 0");
    for window in ids.windows(2) {
        assert!(
            window[1] == window[0].wrapping_add(1),
            "frame_id not monotonic: {:?}",
            ids
        );
    }
}

/// ADR-02 D2 — OnLayoutStarted resets per-client frame_id to 0 and moves to
/// ACTIVE.
#[actix_rt::test]
async fn layout_started_resets_frame_ids_and_transitions() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 1)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_millis(80),
        active_poll_interval: Duration::from_millis(50),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    let client = RecordingClient::new().start();
    bcast
        .send(RegisterBroadcastClient {
            client_id: 1,
            recipient: client.clone().recipient(),
        })
        .await
        .unwrap();

    // Let frame_id increment past 0.
    actix_rt::time::sleep(Duration::from_millis(300)).await;
    let pre_reset = client.send(DumpFrames).await.unwrap();
    let pre_count = pre_reset.len();
    assert!(pre_count >= 2, "need ≥2 frames before reset, got {pre_count}");

    bcast.send(OnLayoutStarted).await.unwrap();
    actix_rt::time::sleep(Duration::from_millis(200)).await;

    let post = client.send(DumpFrames).await.unwrap();
    assert!(post.len() > pre_count, "no new frames after LayoutStarted");
    let reset_frame = &post[pre_count];
    let decoded = BinaryV3Frame::decode(reset_frame).unwrap();
    assert_eq!(
        decoded.frame_id, 0,
        "LayoutStarted should have reset frame_id to 0"
    );

    let status = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(status.state, BroadcastState::Active);
}

/// WORKTREE-PLAN §2 — full transition table:
///   SETTLED → ACTIVE (OnLayoutStarted, OnLayoutDestabilised)
///   ACTIVE  → SETTLED (OnLayoutSettled)
#[actix_rt::test]
async fn state_machine_transition_table() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 1)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_secs(30),
        active_poll_interval: Duration::from_secs(30),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    // Initial state: SETTLED.
    let s = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(s.state, BroadcastState::Settled);

    // SETTLED + OnLayoutStarted → ACTIVE.
    bcast.send(OnLayoutStarted).await.unwrap();
    let s = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(s.state, BroadcastState::Active);

    // ACTIVE + OnLayoutSettled → SETTLED.
    bcast.send(OnLayoutSettled).await.unwrap();
    let s = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(s.state, BroadcastState::Settled);

    // SETTLED + OnLayoutDestabilised → ACTIVE.
    bcast.send(OnLayoutDestabilised).await.unwrap();
    let s = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert_eq!(s.state, BroadcastState::Active);
}

/// PRD-02 A3 — frame dropped (not queued) when client mailbox is full.
#[actix_rt::test]
async fn drop_on_backpressure_increments_counter() {
    let source = StubSnapshotSource::new(sample_snapshot(1, 10)).start();
    let cfg = BroadcastConfig {
        heartbeat_interval: Duration::from_millis(20),
        active_poll_interval: Duration::from_millis(20),
        backpressure_threshold_bytes: 64 * 1024,
    };
    let bcast = BroadcastActor::new(source.recipient(), cfg).start();

    let slow = SlowClient {
        delay: Duration::from_millis(80),
    }
    .start();
    bcast
        .send(RegisterBroadcastClient {
            client_id: 1,
            recipient: slow.recipient(),
        })
        .await
        .unwrap();

    // 800 ms of 20 ms ticks = ~40 attempts; the slow client can only consume
    // ~10 (1 message per 80 ms). The remaining ~30 must be dropped.
    actix_rt::time::sleep(Duration::from_millis(800)).await;
    let status = bcast.send(GetBroadcastActorStatus).await.unwrap();
    assert!(
        status.frames_dropped_total > 0,
        "expected drops under saturated client mailbox, got {}",
        status.frames_dropped_total
    );
}
