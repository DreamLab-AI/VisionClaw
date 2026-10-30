//! P3-pre preflight (XR phase-1 contract §2.1, ADR-2159): two graph connections in one process.
//!
//! Phase 1 holds one `BinaryProtocolClient` per content source (VisionClaw and Codescape) at the
//! same time. This test shows the transport layer tolerates that. It starts local WebSocket
//! servers on the shared tokio runtime (`runtime.rs`) and drives graph streams through
//! `transport::spawn_graph_stream`, the exact path `BinaryProtocolClient::connect_to_url` takes.
//! The test thread stands in for Godot's main thread: it runs a 90 Hz `poll()`-style loop that
//! drains each client's inbox and, per frame, runs `FreshnessGate::admit_and_ack` and sends the
//! `0x34` acknowledgement on that client's own outbound channel, which is
//! `BinaryProtocolClient::emit_frame` step for step. (`BinaryProtocolClient` itself is a
//! `GodotClass` and cannot be constructed without an engine.)
//!
//! Each server waits for `requestInitialData`, sends an `initialGraphLoad`, then V5 position
//! frames paced by acknowledgements as ADR-2141 specifies (frame k+1 only after the ack for k).
//!
//! # Scenario 1: one client reloads while the other streams
//! Server "A" sends a production-size topology (13 000 nodes, 145 000 edges, 14.6 MB: the figure
//! the transport's message-size note cites) at the start and again mid-stream, as a
//! `graphUpdated` refetch would. Server "B" sends a small topology and streams throughout.
//!
//! # Scenario 2: both clients reload a full topology at the same moment
//! A and B both send the 14.6 MB topology at the start, and both re-send it mid-stream behind a
//! shared barrier, so the two parses overlap. A third small "canary" stream (C) runs throughout,
//! ack-paced like the others: it stands for everything else on the runtime (presence, a third
//! source). Before the parse moved off the async workers, two overlapping parses could occupy
//! both of the runtime's two workers and stall everything else for the length of a parse.
//!
//! # Asserted in both scenarios
//! - every client receives every frame (each accepted, none rejected, no foreign node ids);
//! - every client acks with its own sequence: each server checks every ack names the sequence it
//!   just sent and that frame's record count, and the sequence spaces are disjoint;
//! - no client stalls another by more than 100 ms: the largest gap between consecutive frames
//!   drained for each client, and each server's largest send→ack round trip, are ≤ 100 ms
//!   (scenario 2 also holds the canary to this, across both overlapping parses);
//! - the main-thread loop drains everything within budget: per-tick pump work (drain, decode,
//!   gate, ack) has p99 ≤ 2 ms and max ≤ 11.1 ms (one 90 Hz frame). Ticks that receive a
//!   topology are reported separately: that one-off cost is per topology, and the real client
//!   adds its render-store load on top.
//!
//! A client's own topology parse delays its own next frame (the frame behind it is forwarded only
//! after the parse, which keeps topology and frames in order). That is not one client stalling
//! another, so the gap and round trip spanning a client's own topology are reported but kept out
//! of the 100 ms checks.
//!
//! The measured figures are printed (run with `--nocapture`). The two scenarios share one runtime
//! and their timing would interfere, so they are serialised with a lock.
//!
//! # Process-wide state in `xr-client/rust/src` and whether two instances tolerate it
//!
//! A grep over `src/` for `static`, `OnceLock`, `OnceCell`, `Once`, `lazy_static`, `Lazy<`,
//! `thread_local!`, `static mut` and atomics finds exactly four items; there are no
//! `thread_local!`s, `static mut`s or global atomics.
//!
//! | Item | Verdict |
//! |---|---|
//! | `runtime.rs:12` `RUNTIME: OnceLock<Runtime>` | **Shared by design.** Every client's pump (and presence) runs on its two workers; large topology parses run on its blocking pool (`transport.rs`). Its capacity is what both scenarios measure. |
//! | `lib.rs:78` `INIT: Once` (tracing subscriber) | **Tolerant.** A once-guard around installing the global subscriber; instance count is irrelevant. |
//! | `thread_cpu.rs:33` `ORIGIN: OnceLock<Instant>` | **Tolerant.** Non-unix fallback only; a read-only epoch after first init. |
//! | `hulls.rs:266` `DIRS: OnceLock<Vec<[f64; 3]>>` | **Tolerant.** Immutable Fibonacci-sphere directions, identical for every caller. |
//!
//! Every other piece of client state (inbox, outbound channel, `FreshnessGate`, render store and
//! its pack slots, settings, refetch gate) is a field of the `BinaryProtocolClient` instance, so
//! two instances share nothing but the runtime.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc::UnboundedSender, Barrier, Notify};
use tokio_tungstenite::tungstenite::Message;

use visionclaw_xr_gdext::binary_protocol::{
    FreshnessGate, GraphInbound, NODE_RECORD_BYTES, OPCODE_BROADCAST_ACK, PROTOCOL_V5,
};
use visionclaw_xr_gdext::runtime::runtime;
use visionclaw_xr_gdext::transport::{spawn_graph_stream, ConnHandle, Outbound};

const FRAMES: u64 = 200;
const NODES_PER_FRAME: u32 = 1_000;
const CANARY_NODES_PER_FRAME: u32 = 10;
/// Production-size topology (transport.rs `ws_config` note: 13k nodes / 145k edges).
const BIG_NODES: u32 = 13_000;
const BIG_EDGES: u32 = 145_000;
const SMALL_NODES: u32 = 200;
const SMALL_EDGES: u32 = 400;
/// The reloading servers re-send their topology after this many frames (a `graphUpdated` refetch).
const REFETCH_AT: u64 = FRAMES / 2;
const SEQ_BASE_A: u64 = 1;
const SEQ_BASE_B: u64 = 1_000_000;
const SEQ_BASE_C: u64 = 9_000_000;
/// Node id ranges, disjoint so a cross-wired frame would be visible.
const ID_BASE_A: u32 = 0;
const ID_BASE_B: u32 = 100_000;
const ID_BASE_C: u32 = 200_000;

const STALL_LIMIT: Duration = Duration::from_millis(100);
/// 90 Hz headset frame.
const TICK: Duration = Duration::from_micros(11_111);
const POLL_P99_LIMIT: Duration = Duration::from_millis(2);
const POLL_MAX_LIMIT: Duration = TICK;
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const DEADLINE: Duration = Duration::from_secs(60);

static SERIAL: Mutex<()> = Mutex::new(());

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
fn v5_frame(seq: u64, id_base: u32, nodes: u32, t: f32) -> Vec<u8> {
    let mut v = Vec::with_capacity(9 + nodes as usize * NODE_RECORD_BYTES);
    v.push(PROTOCOL_V5);
    v.extend_from_slice(&seq.to_le_bytes());
    for i in 0..nodes {
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
    /// Waited on just before the refetch topology, so two servers send it together.
    refetch_barrier: Option<Arc<Barrier>>,
    seq_base: u64,
    id_base: u32,
    nodes: u32,
    /// `Some(n)`: stop after n frames. `None`: stream until `stop` is notified (the canary).
    frames: Option<u64>,
    stop: Arc<Notify>,
}

#[derive(Debug, Default)]
struct ServerLog {
    acks: Vec<u64>,
    topologies_sent: u32,
    max_ack_rtt: Duration,
    /// Largest round trip of a frame sent straight after this server's own topology (the client
    /// parses that topology first), kept out of `max_ack_rtt`.
    after_topology_ack_rtt: Duration,
    /// Wall instants (since the shared epoch) of the refetch topology send and the last frame.
    refetch_send: Option<Duration>,
    last_send: Duration,
}

async fn send_text(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        Message,
    >,
    text: &str,
) -> Result<(), String> {
    sink.send(Message::Text(text.to_owned())).await.map_err(|e| e.to_string())
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
    send_text(&mut sink, &plan.topology).await?;
    log.topologies_sent += 1;
    let mut k: u64 = 0;
    loop {
        if plan.frames.is_some_and(|n| k >= n) {
            break;
        }
        let mut after_topology = k == 0;
        if plan.refetch_at == Some(k) {
            if let Some(b) = &plan.refetch_barrier {
                b.wait().await;
            }
            log.refetch_send = Some(epoch.elapsed());
            send_text(&mut sink, &plan.topology).await?;
            log.topologies_sent += 1;
            after_topology = true;
        }
        let seq = plan.seq_base + k;
        let frame = v5_frame(seq, plan.id_base, plan.nodes, k as f32);
        let sent_at = Instant::now();
        log.last_send = sent_at - epoch;
        sink.send(Message::Binary(frame)).await.map_err(|e| e.to_string())?;
        // ADR-2141: the next chunk is built only once this one is acknowledged.
        let ack = loop {
            let next = tokio::select! {
                m = tokio::time::timeout(ACK_TIMEOUT, stream.next()) => m,
                _ = plan.stop.notified(), if plan.frames.is_none() => {
                    let _ = sink.send(Message::Close(None)).await;
                    return Ok(log);
                }
            };
            let msg = next.map_err(|_| format!("{}: no ack for seq {seq} within {ACK_TIMEOUT:?}", plan.name))?;
            match msg {
                Some(Ok(Message::Binary(b))) => break b,
                Some(Ok(Message::Text(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                other => return Err(format!("{}: stream ended awaiting ack {seq}: {other:?}", plan.name)),
            }
        };
        let rtt = sent_at.elapsed();
        if after_topology {
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
        if ack_nodes != plan.nodes {
            return Err(format!("{}: ack counts {ack_nodes} nodes, expected {}", plan.name, plan.nodes));
        }
        log.acks.push(ack_seq);
        k += 1;
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
    nodes: u32,
    frames: u64,
    topologies: u32,
    topology_ticks: Vec<Duration>,
    frame_times: Vec<Duration>,
    foreign_ids: u64,
}

impl Client {
    fn connect(name: &'static str, url: String, id_base: u32, nodes: u32) -> Self {
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let (handle, tx) = spawn_graph_stream(url, String::new(), inbox.clone());
        Self {
            name,
            inbox,
            handle,
            tx,
            gate: FreshnessGate::new(),
            id_base,
            nodes,
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
                GraphInbound::Connected => {}
                GraphInbound::Disconnected => self.gate.reconnect(),
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
                    let hi = self.id_base + self.nodes;
                    self.foreign_ids += updates
                        .iter()
                        .filter(|u| u.node_id < lo || u.node_id >= hi)
                        .count() as u64;
                    self.frames += 1;
                    self.frame_times.push(now);
                    if let Some(ack) = ack {
                        // A closed outbound means the pump ended; the server's result says why.
                        let _ = self.tx.send(Outbound::Binary(ack));
                    }
                }
            }
        }
    }

    /// Largest gap between consecutive drained frames, ignoring a gap that spans this client's
    /// own topology drain (its own parse is not another client stalling it). That excluded gap
    /// is returned second, for the record. `window` limits the gaps considered to those that
    /// end inside it.
    fn max_gaps(&self, window: Option<(Duration, Duration)>) -> (Duration, Duration) {
        let mut cross = Duration::ZERO;
        let mut own = Duration::ZERO;
        for w in self.frame_times.windows(2) {
            if let Some((lo, hi)) = window {
                if w[1] < lo || w[1] > hi {
                    continue;
                }
            }
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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

struct Stream {
    plan: ServerPlan,
    finite: bool,
}

struct Outcome {
    clients: Vec<Client>,
    logs: Vec<ServerLog>,
    work: Vec<Duration>,
    topo_work: Vec<Duration>,
}

/// Start one server per stream on the shared runtime, connect a client to each, and run the 90 Hz
/// main-thread loop until every finite stream has delivered all its frames.
fn run(streams: Vec<Stream>, epoch: Instant) -> Outcome {
    let rt = runtime();
    let mut clients = Vec::new();
    let mut servers = Vec::new();
    let mut stops = Vec::new();
    let mut finite = Vec::new();
    for s in streams {
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).expect("bind");
        let url = format!("ws://{}/wss", listener.local_addr().unwrap());
        clients.push(Client::connect(s.plan.name, url, s.plan.id_base, s.plan.nodes));
        stops.push(s.plan.stop.clone());
        finite.push(s.finite);
        servers.push(rt.spawn(serve(listener, s.plan, epoch)));
    }
    let mut work = Vec::new();
    let mut topo_work = Vec::new();
    let mut next = Instant::now();
    loop {
        let done = clients
            .iter()
            .zip(&finite)
            .all(|(c, &f)| !f || c.frames >= FRAMES);
        if done {
            break;
        }
        assert!(
            epoch.elapsed() < DEADLINE,
            "deadline: frames {:?}",
            clients.iter().map(|c| (c.name, c.frames)).collect::<Vec<_>>()
        );
        let now = epoch.elapsed();
        let topo_before: u32 = clients.iter().map(|c| c.topologies).sum();
        let t0 = Instant::now();
        for c in clients.iter_mut() {
            c.poll(now);
        }
        let spent = t0.elapsed();
        if clients.iter().map(|c| c.topologies).sum::<u32>() != topo_before {
            topo_work.push(spent);
        } else {
            work.push(spent);
        }
        next += TICK;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    for s in &stops {
        s.notify_one();
    }
    let logs: Vec<ServerLog> = servers
        .into_iter()
        .map(|h| rt.block_on(h).expect("server task").expect("server"))
        .collect();
    std::thread::sleep(Duration::from_millis(50));
    for c in clients.iter_mut() {
        c.poll(epoch.elapsed());
        c.handle.abort();
    }
    work.sort();
    Outcome { clients, logs, work, topo_work }
}

fn finite_plan(
    name: &'static str,
    topology: Arc<String>,
    refetch_barrier: Option<Arc<Barrier>>,
    seq_base: u64,
    id_base: u32,
) -> Stream {
    Stream {
        plan: ServerPlan {
            name,
            topology,
            refetch_at: Some(REFETCH_AT),
            refetch_barrier,
            seq_base,
            id_base,
            nodes: NODES_PER_FRAME,
            frames: Some(FRAMES),
            stop: Arc::new(Notify::new()),
        },
        finite: true,
    }
}

/// Every check both scenarios share, plus the printed figures.
fn check_common(out: &Outcome, seq_bases: &[u64]) {
    for ((c, log), &base) in out.clients.iter().zip(&out.logs).zip(seq_bases) {
        let s = c.gate.stats();
        let (cross, own) = c.max_gaps(None);
        println!(
            "  {}: frames {} topologies {} accepted {} resync {} dup {} stale {} foreign {} | acks {} max gap {:.1} ms (own-topology gap {:.1} ms) max ack rtt {:.1} ms (after own topology {:.1} ms)",
            c.name, c.frames, c.topologies, s.accepted, s.resyncs, s.rejected_duplicate, s.rejected_stale,
            c.foreign_ids, log.acks.len(), ms(cross), ms(own), ms(log.max_ack_rtt), ms(log.after_topology_ack_rtt)
        );
        assert_eq!(c.foreign_ids, 0, "{}: a frame reached the wrong client", c.name);
        assert_eq!(s.rejected_duplicate + s.rejected_stale, 0, "{}", c.name);
        assert_eq!(s.accepted + s.resyncs, c.frames, "{}", c.name);
        assert_eq!(log.topologies_sent, c.topologies, "{}: every topology arrived", c.name);
        let want: Vec<u64> = (base..base + log.acks.len() as u64).collect();
        assert_eq!(log.acks, want, "{}: acks out of its own sequence", c.name);
        assert!(cross <= STALL_LIMIT, "{} stalled {cross:?}", c.name);
        assert!(log.max_ack_rtt <= STALL_LIMIT, "{} ack rtt {:?}", c.name, log.max_ack_rtt);
    }
    let p50 = percentile(&out.work, 0.50);
    let p99 = percentile(&out.work, 0.99);
    let wmax = out.work.last().copied().unwrap_or_default();
    println!(
        "  main-thread poll work over {} ticks: p50 {:.3} ms p99 {:.3} ms max {:.3} ms; ticks that received a topology: {:?} ms",
        out.work.len(), ms(p50), ms(p99), ms(wmax),
        out.topo_work.iter().map(|d| (ms(*d) * 10.0).round() / 10.0).collect::<Vec<_>>()
    );
    assert!(p99 <= POLL_P99_LIMIT, "poll p99 {p99:?}");
    assert!(wmax <= POLL_MAX_LIMIT, "poll max {wmax:?}");
}

#[test]
fn one_client_reloads_while_the_other_streams() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let big = Arc::new(topology_json(BIG_NODES, BIG_EDGES, ID_BASE_A, "a"));
    let small = Arc::new(topology_json(SMALL_NODES, SMALL_EDGES, ID_BASE_B, "b"));
    let epoch = Instant::now();
    let mut b = finite_plan("B", small, None, SEQ_BASE_B, ID_BASE_B);
    b.plan.refetch_at = None;
    let out = run(vec![finite_plan("A", big.clone(), None, SEQ_BASE_A, ID_BASE_A), b], epoch);
    println!("P3-pre scenario 1: A reloads {:.1} MB twice, B streams", big.len() as f64 / 1e6);
    check_common(&out, &[SEQ_BASE_A, SEQ_BASE_B]);
    let (a, bc) = (&out.clients[0], &out.clients[1]);
    assert_eq!(a.frames, FRAMES);
    assert_eq!(bc.frames, FRAMES);
    assert_eq!(a.topologies, 2);
    assert_eq!(bc.topologies, 1);
    // The stress overlapped: B was streaming on both sides of A's mid-stream topology drain.
    let refetch_drain = a.topology_ticks[1];
    assert!(
        bc.frame_times.iter().any(|&t| t < refetch_drain) && bc.frame_times.iter().any(|&t| t > refetch_drain),
        "A's refetch did not overlap B's stream; the stall check would be vacuous"
    );
}

#[test]
fn both_clients_reload_a_full_topology_at_once() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let big_a = Arc::new(topology_json(BIG_NODES, BIG_EDGES, ID_BASE_A, "a"));
    let big_b = Arc::new(topology_json(BIG_NODES, BIG_EDGES, ID_BASE_B, "b"));
    let small_c = Arc::new(topology_json(SMALL_NODES, SMALL_EDGES, ID_BASE_C, "c"));
    let epoch = Instant::now();
    let barrier = Arc::new(Barrier::new(2));
    let canary = Stream {
        plan: ServerPlan {
            name: "C",
            topology: small_c,
            refetch_at: None,
            refetch_barrier: None,
            seq_base: SEQ_BASE_C,
            id_base: ID_BASE_C,
            nodes: CANARY_NODES_PER_FRAME,
            frames: None,
            stop: Arc::new(Notify::new()),
        },
        finite: false,
    };
    let out = run(
        vec![
            finite_plan("A", big_a.clone(), Some(barrier.clone()), SEQ_BASE_A, ID_BASE_A),
            finite_plan("B", big_b, Some(barrier), SEQ_BASE_B, ID_BASE_B),
            canary,
        ],
        epoch,
    );
    let (la, lb) = (&out.logs[0], &out.logs[1]);
    let (ra, rb) = (la.refetch_send.unwrap(), lb.refetch_send.unwrap());
    let (a, b, c) = (&out.clients[0], &out.clients[1], &out.clients[2]);
    println!(
        "P3-pre scenario 2: A and B both reload {:.1} MB; refetch sends {:.1} ms apart; drains A {:?} B {:?}",
        big_a.len() as f64 / 1e6,
        ms(ra.abs_diff(rb)),
        a.topology_ticks,
        b.topology_ticks
    );
    check_common(&out, &[SEQ_BASE_A, SEQ_BASE_B, SEQ_BASE_C]);
    assert_eq!(a.frames, FRAMES);
    assert_eq!(b.frames, FRAMES);
    assert_eq!(a.topologies, 2);
    assert_eq!(b.topologies, 2);
    // The two refetches really went out together, and the canary streamed across both parses:
    // its gaps inside the window [first refetch send, last refetch drain] are what the shared
    // runtime's capacity decides.
    assert!(ra.abs_diff(rb) <= Duration::from_millis(50), "refetches not simultaneous");
    let lo = ra.min(rb);
    let hi = a.topology_ticks[1].max(b.topology_ticks[1]);
    let (canary_gap, _) = c.max_gaps(Some((lo, hi)));
    let in_window = c.frame_times.iter().filter(|&&t| t >= lo && t <= hi).count();
    println!(
        "  canary across both parses ({:.1}..{:.1} ms): {} frames, max gap {:.1} ms",
        ms(lo), ms(hi), in_window, ms(canary_gap)
    );
    assert!(in_window >= 2, "canary did not stream across the parses; the check would be vacuous");
    assert!(canary_gap <= STALL_LIMIT, "canary stalled {canary_gap:?} while both topologies parsed");
}
