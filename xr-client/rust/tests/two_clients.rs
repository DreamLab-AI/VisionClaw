//! P3-pre preflight (XR phase-1 contract §2.1, ADR-2159): two graph connections in one process.
//!
//! Phase 1 holds one `BinaryProtocolClient` per content source (VisionClaw and Codescape) at the
//! same time. Before anything is wired into `graph_scene.gd`, this test shows the transport layer
//! tolerates that. It starts two local WebSocket servers on the shared tokio runtime
//! (`runtime.rs`) and drives two graph streams through `transport::spawn_graph_stream`, the exact
//! path `BinaryProtocolClient::connect_to_url` takes. The test thread stands in for Godot's main
//! thread: it runs a 90 Hz `poll()`-style loop that drains each client's inbox and, per frame,
//! runs `FreshnessGate::admit_and_ack` and sends the `0x34` acknowledgement on that client's own
//! outbound channel, which is `BinaryProtocolClient::emit_frame` step for step.
//! (`BinaryProtocolClient` itself is a `GodotClass` and cannot be constructed without an engine.)
//!
//! Each server waits for `requestInitialData`, sends an `initialGraphLoad`, then 200 V5 position
//! frames paced by acknowledgements as ADR-2141 specifies (frame k+1 only after the ack for k).
//! Server "A" sends a production-size topology (13 000 nodes, 145 000 edges, the figure the
//! transport's message-size note cites) both at the start and again mid-stream, as a
//! `graphUpdated` refetch would. That parse runs synchronously on a tokio worker inside
//! the graph pump, and the shared runtime has two workers, so it is the real way one connection
//! could stall the other. Server "B" sends a small topology and streams throughout.
//!
//! Asserted:
//! - both clients receive every frame (200 each, every one accepted, none rejected);
//! - both send `0x34` acks with independent sequences: each server checks every ack names the
//!   sequence it just sent and the record count of that frame, and the two sequence spaces are
//!   disjoint (A from 1, B from 1 000 000);
//! - neither stalls the other by more than 100 ms: the maximum gap between consecutive frames
//!   drained for each client, and each server's maximum send→ack round trip, are both ≤ 100 ms,
//!   and the test checks that B was mid-stream on both sides of A's mid-stream topology;
//! - the main-thread loop drains both within budget: per-tick pump work (drain, decode, gate,
//!   ack for both clients) has p99 ≤ 2 ms and max ≤ 11.1 ms (one 90 Hz frame). Ticks that
//!   receive a topology are reported separately: that one-off cost is per topology, not per
//!   connection, and the real client adds its render-store load on top.
//!
//! A client's own topology parse delays its own next frame (it is parsed in its pump before the
//! frame behind it is forwarded). That is not one client stalling the other, so the gap and
//! round trip spanning a client's own topology are reported but kept out of the 100 ms checks.
//!
//! The measured figures are printed (run with `--nocapture`).
//!
//! # Process-wide state in `xr-client/rust/src` and whether two instances tolerate it
//!
//! A grep over `src/` for `static`, `OnceLock`, `OnceCell`, `Once`, `lazy_static`, `Lazy<`,
//! `thread_local!`, `static mut` and atomics finds exactly four items; there are no
//! `thread_local!`s, `static mut`s or global atomics.
//!
//! | Item | Verdict |
//! |---|---|
//! | `runtime.rs:12` `RUNTIME: OnceLock<Runtime>` | **Shared by design.** Both clients' pumps (and presence) run on its two workers. Its capacity under a large topology parse is exactly what this test measures. |
//! | `lib.rs:78` `INIT: Once` (tracing subscriber) | **Tolerant.** A once-guard around installing the global subscriber; instance count is irrelevant. |
//! | `thread_cpu.rs:33` `ORIGIN: OnceLock<Instant>` | **Tolerant.** Non-unix fallback only; a read-only epoch after first init. |
//! | `hulls.rs:266` `DIRS: OnceLock<Vec<[f64; 3]>>` | **Tolerant.** Immutable Fibonacci-sphere directions, identical for every caller. |
//!
//! Every other piece of client state (inbox, outbound channel, `FreshnessGate`, render store,
//! settings, refetch gate) is a field of the `BinaryProtocolClient` instance, so two instances
//! share nothing but the runtime.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

use visionclaw_xr_gdext::binary_protocol::{
    FreshnessGate, GraphInbound, NODE_RECORD_BYTES, OPCODE_BROADCAST_ACK, PROTOCOL_V5,
};
use visionclaw_xr_gdext::runtime::runtime;
use visionclaw_xr_gdext::transport::{spawn_graph_stream, ConnHandle, Outbound};

const FRAMES: u64 = 200;
const NODES_PER_FRAME: u32 = 1_000;
/// Production-size topology (transport.rs `ws_config` note: 13k nodes / 145k edges).
const BIG_NODES: u32 = 13_000;
const BIG_EDGES: u32 = 145_000;
const SMALL_NODES: u32 = 200;
const SMALL_EDGES: u32 = 400;
/// Server A re-sends its topology after this many frames (a `graphUpdated` refetch).
const A_REFETCH_AT: u64 = FRAMES / 2;
const SEQ_BASE_A: u64 = 1;
const SEQ_BASE_B: u64 = 1_000_000;
/// Node id ranges, disjoint so a cross-wired frame would be visible.
const ID_BASE_A: u32 = 0;
const ID_BASE_B: u32 = 100_000;

const STALL_LIMIT: Duration = Duration::from_millis(100);
/// 90 Hz headset frame.
const TICK: Duration = Duration::from_micros(11_111);
const POLL_P99_LIMIT: Duration = Duration::from_millis(2);
const POLL_MAX_LIMIT: Duration = TICK;
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const DEADLINE: Duration = Duration::from_secs(60);

fn topology_json(nodes: u32, edges: u32, id_base: u32, tag: &str) -> String {
    let nodes: Vec<serde_json::Value> = (0..nodes)
        .map(|i| {
            serde_json::json!({
                "id": id_base + i,
                "metadata_id": format!("{tag}-page-{i}"),
                "label": format!("{tag} node {i}"),
                "node_type": "page",
                "metadata": {
                    "source_domain": "preflight",
                    "file_size": 1024 + i,
                    "quality_score": 0.5,
                },
            })
        })
        .collect();
    let n = nodes.len() as u32;
    let edges: Vec<serde_json::Value> = (0..edges)
        .map(|i| {
            serde_json::json!({
                "id": format!("e{i}"),
                "source_id": id_base + (i % n),
                "target_id": id_base + ((i * 7 + 1) % n),
                "weight": 1.0,
                "edge_type": "links_to",
            })
        })
        .collect();
    serde_json::json!({
        "type": "initialGraphLoad",
        "nodes": nodes,
        "edges": edges,
        "timestamp": 0,
    })
    .to_string()
}

/// `[0x05][u64 seq][N × 52-byte V3 records]`, little-endian.
fn v5_frame(seq: u64, id_base: u32, t: f32) -> Vec<u8> {
    let mut v = Vec::with_capacity(9 + NODES_PER_FRAME as usize * NODE_RECORD_BYTES);
    v.push(PROTOCOL_V5);
    v.extend_from_slice(&seq.to_le_bytes());
    for i in 0..NODES_PER_FRAME {
        let id = id_base + i;
        v.extend_from_slice(&id.to_le_bytes());
        for c in [i as f32 * 0.01, t, -(i as f32) * 0.01] {
            v.extend_from_slice(&c.to_le_bytes());
        }
        for _ in 0..3 {
            v.extend_from_slice(&0f32.to_le_bytes());
        }
        v.extend_from_slice(&0f32.to_le_bytes()); // sssp_distance
        v.extend_from_slice(&(-1i32).to_le_bytes()); // sssp_parent
        v.extend_from_slice(&0u32.to_le_bytes()); // cluster_id
        v.extend_from_slice(&0f32.to_le_bytes()); // anomaly
        v.extend_from_slice(&(i % 8).to_le_bytes()); // community
        v.extend_from_slice(&0.5f32.to_le_bytes()); // centrality
    }
    v
}

struct ServerPlan {
    name: &'static str,
    topology: Arc<String>,
    refetch_at: Option<u64>,
    seq_base: u64,
    id_base: u32,
}

#[derive(Debug, Default)]
struct ServerLog {
    frames_sent: u64,
    acks: Vec<u64>,
    topologies_sent: u32,
    max_ack_rtt: Duration,
    /// Largest round trip of a frame sent straight after this server's own topology (the client
    /// parses that topology first), kept out of `max_ack_rtt`.
    after_topology_ack_rtt: Duration,
    /// Wall instants (since the shared epoch) of the first and last frame sent.
    first_send: Duration,
    last_send: Duration,
}

async fn serve(listener: TcpListener, plan: ServerPlan, epoch: Instant) -> Result<ServerLog, String> {
    let (tcp, _) = listener.accept().await.map_err(|e| e.to_string())?;
    let ws = tokio_tungstenite::accept_async(tcp)
        .await
        .map_err(|e| format!("{}: ws accept: {e}", plan.name))?;
    let (mut sink, mut stream) = ws.split();
    // requestInitialData first (transport::GRAPH_REQUEST_INITIAL).
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(t))) if t.contains("requestInitialData") => break,
            Some(Ok(_)) => continue,
            other => return Err(format!("{}: no requestInitialData: {other:?}", plan.name)),
        }
    }
    let mut log = ServerLog::default();
    sink.send(Message::Text(plan.topology.as_str().to_owned()))
        .await
        .map_err(|e| e.to_string())?;
    log.topologies_sent += 1;
    for k in 0..FRAMES {
        if plan.refetch_at == Some(k) {
            sink.send(Message::Text(plan.topology.as_str().to_owned()))
                .await
                .map_err(|e| e.to_string())?;
            log.topologies_sent += 1;
        }
        let seq = plan.seq_base + k;
        let frame = v5_frame(seq, plan.id_base, k as f32);
        let sent_at = Instant::now();
        if k == 0 {
            log.first_send = sent_at - epoch;
        }
        log.last_send = sent_at - epoch;
        sink.send(Message::Binary(frame)).await.map_err(|e| e.to_string())?;
        log.frames_sent += 1;
        // ADR-2141: the next chunk is built only once this one is acknowledged.
        let ack = loop {
            let msg = tokio::time::timeout(ACK_TIMEOUT, stream.next())
                .await
                .map_err(|_| format!("{}: no ack for seq {seq} within {ACK_TIMEOUT:?}", plan.name))?;
            match msg {
                Some(Ok(Message::Binary(b))) => break b,
                Some(Ok(Message::Text(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                    continue
                }
                other => return Err(format!("{}: stream ended awaiting ack {seq}: {other:?}", plan.name)),
            }
        };
        let rtt = sent_at.elapsed();
        if k == 0 || plan.refetch_at == Some(k) {
            log.after_topology_ack_rtt = log.after_topology_ack_rtt.max(rtt);
        } else {
            log.max_ack_rtt = log.max_ack_rtt.max(rtt);
        }
        if ack.len() != 21 || ack[0] != OPCODE_BROADCAST_ACK {
            return Err(format!("{}: malformed ack {:?}", plan.name, &ack[..ack.len().min(4)]));
        }
        let ack_seq = u64::from_le_bytes(ack[1..9].try_into().unwrap());
        let ack_nodes = u32::from_le_bytes(ack[9..13].try_into().unwrap());
        if ack_seq != seq {
            return Err(format!("{}: ack names seq {ack_seq}, expected {seq}", plan.name));
        }
        if ack_nodes != NODES_PER_FRAME {
            return Err(format!("{}: ack counts {ack_nodes} nodes, expected {NODES_PER_FRAME}", plan.name));
        }
        log.acks.push(ack_seq);
    }
    let _ = sink.send(Message::Close(None)).await;
    Ok(log)
}

/// One client as `BinaryProtocolClient` holds it: inbox, outbound sender, freshness gate.
struct Client {
    name: &'static str,
    inbox: Arc<Mutex<VecDeque<GraphInbound>>>,
    handle: ConnHandle,
    tx: UnboundedSender<Outbound>,
    gate: FreshnessGate,
    id_base: u32,
    connected: bool,
    frames: u64,
    topologies: u32,
    topology_ticks: Vec<Duration>,
    frame_times: Vec<Duration>,
    foreign_ids: u64,
}

impl Client {
    fn connect(name: &'static str, url: String, id_base: u32) -> Self {
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let (handle, tx) = spawn_graph_stream(url, String::new(), inbox.clone());
        Self {
            name,
            inbox,
            handle,
            tx,
            gate: FreshnessGate::new(),
            id_base,
            connected: false,
            frames: 0,
            topologies: 0,
            topology_ticks: Vec::new(),
            frame_times: Vec::new(),
            foreign_ids: 0,
        }
    }

    /// `BinaryProtocolClient::poll` + `emit_frame`, minus the Godot signals.
    fn poll(&mut self, now: Duration) {
        let drained: Vec<GraphInbound> = {
            let mut q = self.inbox.lock().expect("inbox lock");
            q.drain(..).collect()
        };
        for ev in drained {
            match ev {
                GraphInbound::Connected => self.connected = true,
                GraphInbound::Disconnected => {
                    self.gate.reconnect();
                    self.connected = false;
                }
                GraphInbound::Topology { edges, metas } => {
                    assert!(!edges.is_empty() && !metas.is_empty(), "{}: empty topology", self.name);
                    self.topologies += 1;
                    self.topology_ticks.push(now);
                }
                GraphInbound::Text(t) => panic!("{}: unexpected text frame {t:.80}", self.name),
                GraphInbound::Frame(bytes) => {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let (_, updates, ack) = self
                        .gate
                        .admit_and_ack(&bytes, now_ms)
                        .unwrap_or_else(|e| panic!("{}: decode failed: {e}", self.name));
                    let updates = updates.unwrap_or_else(|| panic!("{}: frame rejected by gate", self.name));
                    let lo = self.id_base;
                    let hi = self.id_base + NODES_PER_FRAME;
                    self.foreign_ids += updates
                        .iter()
                        .filter(|u| u.node_id < lo || u.node_id >= hi)
                        .count() as u64;
                    self.frames += 1;
                    self.frame_times.push(now);
                    if let Some(ack) = ack {
                        self.tx.send(Outbound::Binary(ack)).expect("outbound open");
                    }
                }
            }
        }
    }

    /// Largest gap between consecutive drained frames, ignoring a gap that spans this client's
    /// own topology drain (its own parse is not the other client stalling it). That excluded gap
    /// is returned second, for the record.
    fn max_gaps(&self) -> (Duration, Duration) {
        let mut cross = Duration::ZERO;
        let mut own = Duration::ZERO;
        for w in self.frame_times.windows(2) {
            let gap = w[1] - w[0];
            if self.topology_ticks.iter().any(|&t| t > w[0] && t <= w[1]) {
                own = own.max(gap);
            } else {
                cross = cross.max(gap);
            }
        }
        (cross, own)
    }
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

#[test]
fn two_graph_clients_share_the_runtime_without_stalling() {
    let epoch = Instant::now();
    let rt = runtime();
    let big = Arc::new(topology_json(BIG_NODES, BIG_EDGES, ID_BASE_A, "a"));
    let small = Arc::new(topology_json(SMALL_NODES, SMALL_EDGES, ID_BASE_B, "b"));
    let big_mb = big.len() as f64 / 1.0e6;

    let (la, lb) = rt.block_on(async {
        (
            TcpListener::bind("127.0.0.1:0").await.expect("bind a"),
            TcpListener::bind("127.0.0.1:0").await.expect("bind b"),
        )
    });
    let url_a = format!("ws://{}/wss", la.local_addr().unwrap());
    let url_b = format!("ws://{}/wss", lb.local_addr().unwrap());

    let server_a = rt.spawn(serve(
        la,
        ServerPlan {
            name: "A",
            topology: big.clone(),
            refetch_at: Some(A_REFETCH_AT),
            seq_base: SEQ_BASE_A,
            id_base: ID_BASE_A,
        },
        epoch,
    ));
    let server_b = rt.spawn(serve(
        lb,
        ServerPlan {
            name: "B",
            topology: small.clone(),
            refetch_at: None,
            seq_base: SEQ_BASE_B,
            id_base: ID_BASE_B,
        },
        epoch,
    ));

    let mut a = Client::connect("A", url_a, ID_BASE_A);
    let mut b = Client::connect("B", url_b, ID_BASE_B);

    // Main-thread loop at 90 Hz.
    let mut work: Vec<Duration> = Vec::new();
    let mut topo_work: Vec<Duration> = Vec::new();
    let mut next = Instant::now();
    while a.frames < FRAMES || b.frames < FRAMES {
        assert!(
            epoch.elapsed() < DEADLINE,
            "deadline: A {} frames, B {} frames",
            a.frames,
            b.frames
        );
        let now = epoch.elapsed();
        let topo_before = a.topologies + b.topologies;
        let t0 = Instant::now();
        a.poll(now);
        b.poll(now);
        let spent = t0.elapsed();
        // A tick that receives a topology also frees the previous one; the real client then spends
        // further main-thread time loading it into the render store. That one-off cost is per
        // topology, not per connection, so it is reported apart from the streaming budget.
        if a.topologies + b.topologies != topo_before {
            topo_work.push(spent);
        } else {
            work.push(spent);
        }
        next += TICK;
        let wait = next.saturating_duration_since(Instant::now());
        std::thread::sleep(wait);
    }

    let log_a = rt
        .block_on(server_a)
        .expect("server A task")
        .expect("server A");
    let log_b = rt
        .block_on(server_b)
        .expect("server B task")
        .expect("server B");
    // Drain the close events so the pumps end before abort.
    std::thread::sleep(Duration::from_millis(50));
    a.poll(epoch.elapsed());
    b.poll(epoch.elapsed());
    a.handle.abort();
    b.handle.abort();

    work.sort();
    let p50 = percentile(&work, 0.50);
    let p99 = percentile(&work, 0.99);
    let wmax = *work.last().unwrap();
    let (gap_a, own_gap_a) = a.max_gaps();
    let (gap_b, _) = b.max_gaps();
    let sa = a.gate.stats();
    let sb = b.gate.stats();

    println!("P3-pre two_clients: big topology {big_mb:.1} MB ({BIG_NODES} nodes / {BIG_EDGES} edges), small {:.2} MB", small.len() as f64 / 1.0e6);
    println!(
        "  A: frames {} topologies {} accepted {} resync {} dup {} stale {} foreign ids {} | server acks {} first {} last {} max gap {:.1} ms max ack rtt {:.1} ms",
        a.frames, a.topologies, sa.accepted, sa.resyncs, sa.rejected_duplicate, sa.rejected_stale, a.foreign_ids,
        log_a.acks.len(), log_a.acks.first().unwrap(), log_a.acks.last().unwrap(),
        gap_a.as_secs_f64() * 1e3, log_a.max_ack_rtt.as_secs_f64() * 1e3
    );
    println!(
        "  B: frames {} topologies {} accepted {} resync {} dup {} stale {} foreign ids {} | server acks {} first {} last {} max gap {:.1} ms max ack rtt {:.1} ms",
        b.frames, b.topologies, sb.accepted, sb.resyncs, sb.rejected_duplicate, sb.rejected_stale, b.foreign_ids,
        log_b.acks.len(), log_b.acks.first().unwrap(), log_b.acks.last().unwrap(),
        gap_b.as_secs_f64() * 1e3, log_b.max_ack_rtt.as_secs_f64() * 1e3
    );
    println!(
        "  A topology drained at {:?}; B streamed {:?}..{:?}; A's own refetch gap {:.1} ms, A's after-own-topology ack rtt {:.1} ms",
        a.topology_ticks, log_b.first_send, log_b.last_send,
        own_gap_a.as_secs_f64() * 1e3, log_a.after_topology_ack_rtt.as_secs_f64() * 1e3
    );
    println!(
        "  main-thread poll work over {} ticks: p50 {:.3} ms p99 {:.3} ms max {:.3} ms (limits p99 {:?}, max {:?})",
        work.len(),
        p50.as_secs_f64() * 1e3,
        p99.as_secs_f64() * 1e3,
        wmax.as_secs_f64() * 1e3,
        POLL_P99_LIMIT,
        POLL_MAX_LIMIT
    );
    println!(
        "  ticks that received a topology: {:?} ms",
        topo_work.iter().map(|d| (d.as_secs_f64() * 1e4).round() / 10.0).collect::<Vec<_>>()
    );

    // Every frame received, applied and acknowledged with the client's own sequence.
    assert_eq!(a.frames, FRAMES);
    assert_eq!(b.frames, FRAMES);
    assert_eq!(a.foreign_ids + b.foreign_ids, 0, "a frame reached the wrong client");
    assert_eq!(sa.accepted + sa.resyncs, FRAMES);
    assert_eq!(sb.accepted + sb.resyncs, FRAMES);
    assert_eq!(sa.rejected_duplicate + sa.rejected_stale, 0);
    assert_eq!(sb.rejected_duplicate + sb.rejected_stale, 0);
    assert_eq!(a.gate.watermark(), Some(SEQ_BASE_A + FRAMES - 1));
    assert_eq!(b.gate.watermark(), Some(SEQ_BASE_B + FRAMES - 1));
    let want_a: Vec<u64> = (SEQ_BASE_A..SEQ_BASE_A + FRAMES).collect();
    let want_b: Vec<u64> = (SEQ_BASE_B..SEQ_BASE_B + FRAMES).collect();
    assert_eq!(log_a.acks, want_a, "server A saw acks out of its own sequence");
    assert_eq!(log_b.acks, want_b, "server B saw acks out of its own sequence");
    assert_eq!(a.topologies, 2);
    assert_eq!(log_a.topologies_sent, 2);
    assert_eq!(b.topologies, 1);

    // The stress overlapped: B was streaming on both sides of A's mid-stream topology drain.
    let refetch_drain = a.topology_ticks[1];
    assert!(
        b.frame_times.iter().any(|&t| t < refetch_drain) && b.frame_times.iter().any(|&t| t > refetch_drain),
        "A's refetch did not overlap B's stream; the stall check would be vacuous"
    );

    // Neither stalls the other.
    assert!(gap_a <= STALL_LIMIT, "client A stalled {gap_a:?}");
    assert!(gap_b <= STALL_LIMIT, "client B stalled {gap_b:?}");
    assert!(log_a.max_ack_rtt <= STALL_LIMIT, "server A ack rtt {:?}", log_a.max_ack_rtt);
    assert!(log_b.max_ack_rtt <= STALL_LIMIT, "server B ack rtt {:?}", log_b.max_ack_rtt);

    // Main-thread budget.
    assert!(p99 <= POLL_P99_LIMIT, "poll p99 {p99:?}");
    assert!(wmax <= POLL_MAX_LIMIT, "poll max {wmax:?}");
}
