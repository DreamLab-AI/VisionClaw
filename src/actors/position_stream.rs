//! Per-client acknowledged position streaming (ADR-2141).
//!
//! The GPU physics produces a full set of display positions every step. Nothing here decides
//! *when physics runs*; this module decides, for ONE client, *which nodes to send next*:
//!
//! * The server remembers, per client, what that client last **acknowledged** for each node: its
//!   position and the analytics that ride on every record (cluster, community, anomaly,
//!   centrality). A node is a candidate when it is now more than `move_threshold` away from the
//!   acknowledged position, when its analytics differ from the acknowledged ones, or when it has
//!   never been acknowledged. Comparing with the acknowledged
//!   position, not with the previous frame, means slow drift is not lost: it accumulates until
//!   it crosses the threshold and is then sent.
//! * A chunk is the `chunk_nodes` candidates with the largest displacement (never-sent nodes
//!   first, in id order, so a new client's initial sync needs no special case). Chunks are built
//!   fresh from current state with a new sequence number, so a lost chunk is simply rebuilt and
//!   the receiver's monotonic-sequence freshness checks stay valid.
//! * One chunk is in flight per client. The next is built when the client acknowledges the
//!   previous one, or when `ack_timeout` passes without an acknowledgement (the client, or the
//!   frame, was lost). The delivery rate is therefore set by each client's own round trip, and
//!   when every client has the latest positions there are no candidates and nothing is sent.
//!
//! There are no deltas: a chunk carries absolute positions, and every entitled node is eventually
//! delivered and acknowledged.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::utils::socket_flow_messages::BinaryNodeDataClient;

/// The stored stream settings (`physics.streamChunkNodes`, `streamMoveThreshold`,
/// `streamAckTimeoutMs`), validated where they are stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamSettings {
    /// Most nodes in one chunk.
    pub chunk_nodes: usize,
    /// Display-space distance below which a node counts as "unchanged" for a client.
    pub move_threshold: f32,
    /// How long a chunk may go unacknowledged before it is rebuilt and sent again.
    pub ack_timeout: Duration,
}

/// The analytics that ride on each node record: `[cluster, community, anomaly bits, centrality
/// bits]`. Compared bit for bit, so any change is a change.
pub type AnalyticsKey = [u32; 4];

/// What a client holds for one node.
#[derive(Debug, Clone, Copy)]
struct Held {
    pos: [f32; 3],
    key: AnalyticsKey,
}

#[derive(Debug, Clone)]
struct InFlight {
    seq: u64,
    sent_at: Instant,
    nodes: Vec<(u32, Held)>,
}

/// What one client has acknowledged, and the chunk currently on its way.
#[derive(Debug, Clone, Default)]
pub struct ClientStream {
    acked: HashMap<u32, Held>,
    in_flight: Option<InFlight>,
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

impl ClientStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a chunk is on its way and has not yet timed out: nothing new should be sent.
    pub fn is_waiting(&self, now: Instant, ack_timeout: Duration) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|f| now.saturating_duration_since(f.sent_at) < ack_timeout)
    }

    /// True when a chunk has been sent and is neither acknowledged nor timed out.
    pub fn has_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Number of nodes this client has acknowledged a position for.
    pub fn acknowledged_nodes(&self) -> usize {
        self.acked.len()
    }

    /// The node ids to send next, most-displaced first, at most `settings.chunk_nodes`.
    ///
    /// `visible` is the client's entitlement and explicit-filter test: nodes it rejects are never
    /// candidates. Returns an empty list when this client already holds the latest positions.
    pub fn plan(
        &self,
        cache: &HashMap<u32, BinaryNodeDataClient>,
        visible: &dyn Fn(u32) -> bool,
        analytics: &dyn Fn(u32) -> AnalyticsKey,
        settings: &StreamSettings,
    ) -> Vec<u32> {
        let thr2 = settings.move_threshold * settings.move_threshold;
        // (displacement², id); never-acknowledged nodes are infinitely displaced.
        let mut candidates: Vec<(f32, u32)> = Vec::new();
        for (&id, p) in cache {
            if !visible(id) {
                continue;
            }
            let pos = [p.x, p.y, p.z];
            match self.acked.get(&id) {
                None => candidates.push((f32::INFINITY, id)),
                Some(held) => {
                    let d2 = dist2(pos, held.pos);
                    // A zero threshold means "any change"; an identical position is never a change.
                    if d2 > thr2 && d2 > 0.0 {
                        candidates.push((d2, id));
                    } else if analytics(id) != held.key {
                        // Same place, different cluster / anomaly / centrality: still news, but
                        // behind every node that actually moved.
                        candidates.push((0.0, id));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }
        // Largest displacement first; ties (all the never-sent nodes) by id for determinism.
        let order = |a: &(f32, u32), b: &(f32, u32)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1));
        let k = settings.chunk_nodes.max(1);
        if candidates.len() > k {
            candidates.select_nth_unstable_by(k - 1, order);
            candidates.truncate(k);
        }
        candidates.sort_unstable_by(order);
        candidates.into_iter().map(|(_, id)| id).collect()
    }

    /// Record the chunk just sent, replacing any timed-out one.
    pub fn record_sent(
        &mut self,
        seq: u64,
        now: Instant,
        positions: &[BinaryNodeDataClient],
        analytics: &dyn Fn(u32) -> AnalyticsKey,
    ) {
        self.in_flight = Some(InFlight {
            seq,
            sent_at: now,
            nodes: positions
                .iter()
                .map(|p| (p.node_id, Held { pos: [p.x, p.y, p.z], key: analytics(p.node_id) }))
                .collect(),
        });
    }

    /// The client acknowledged chunk `seq`: the positions in it are now what it holds. Returns the
    /// number of nodes acknowledged, or `None` for an unknown or superseded sequence (a late ack
    /// for a chunk that already timed out and was rebuilt), which changes nothing.
    pub fn acknowledge(&mut self, seq: u64) -> Option<usize> {
        if self.in_flight.as_ref().map(|f| f.seq) != Some(seq) {
            return None;
        }
        let f = self.in_flight.take()?;
        let n = f.nodes.len();
        for (id, held) in f.nodes {
            self.acked.insert(id, held);
        }
        Some(n)
    }

    /// Forget nodes that left the graph (or the client's entitlement), so a node that comes back
    /// is sent afresh.
    pub fn retain_nodes(&mut self, keep: &dyn Fn(u32) -> bool) {
        self.acked.retain(|&id, _| keep(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, x: f32) -> BinaryNodeDataClient {
        BinaryNodeDataClient { node_id: id, x, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0 }
    }

    fn cache(n: u32) -> HashMap<u32, BinaryNodeDataClient> {
        (1..=n).map(|i| (i, node(i, i as f32))).collect()
    }

    fn settings(chunk: usize, thr: f32) -> StreamSettings {
        StreamSettings { chunk_nodes: chunk, move_threshold: thr, ack_timeout: Duration::from_millis(500) }
    }

    fn all(_: u32) -> bool {
        true
    }

    fn no_analytics(_: u32) -> AnalyticsKey {
        [0; 4]
    }

    /// Send one chunk and acknowledge it, as a healthy client does.
    fn cycle(s: &mut ClientStream, c: &HashMap<u32, BinaryNodeDataClient>, set: &StreamSettings, seq: &mut u64) -> Vec<u32> {
        let ids = s.plan(c, &all, &no_analytics, set);
        if ids.is_empty() {
            return ids;
        }
        *seq += 1;
        let chunk: Vec<_> = ids.iter().map(|i| c[i]).collect();
        s.record_sent(*seq, Instant::now(), &chunk, &no_analytics);
        assert_eq!(s.acknowledge(*seq), Some(ids.len()));
        ids
    }

    #[test]
    fn a_new_client_receives_every_node_exactly_once_then_the_stream_goes_quiet() {
        let c = cache(25);
        let set = settings(10, 0.25);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        let mut seen = Vec::new();
        for _ in 0..10 {
            let ids = cycle(&mut s, &c, &set, &mut seq);
            if ids.is_empty() {
                break;
            }
            assert!(ids.len() <= 10, "a chunk never exceeds the stored size");
            seen.extend(ids);
        }
        seen.sort_unstable();
        assert_eq!(seen, (1..=25).collect::<Vec<_>>(), "all 25 delivered, none twice");
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "everything acknowledged: nothing to send");
        assert_eq!(s.acknowledged_nodes(), 25);
    }

    #[test]
    fn a_node_is_sent_again_only_when_it_has_moved_past_the_threshold_from_what_was_acknowledged() {
        let mut c = cache(3);
        let set = settings(10, 0.25);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        cycle(&mut s, &c, &set, &mut seq);

        c.insert(2, node(2, 2.0 + 0.1));
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "0.1 < 0.25: not worth sending");
        c.insert(2, node(2, 2.0 + 0.2));
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "drift accumulates against the ACKNOWLEDGED position, still 0.2");
        c.insert(2, node(2, 2.0 + 0.3));
        assert_eq!(s.plan(&c, &all, &no_analytics, &set), vec![2], "0.3 from what the client holds: sent");
    }

    #[test]
    fn the_most_displaced_nodes_go_first_when_a_chunk_cannot_hold_them_all() {
        let mut c = cache(6);
        let set = settings(2, 0.1);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        while !cycle(&mut s, &c, &set, &mut seq).is_empty() {}
        c.insert(1, node(1, 1.0 + 1.0));
        c.insert(4, node(4, 4.0 + 50.0));
        c.insert(6, node(6, 6.0 + 5.0));
        assert_eq!(s.plan(&c, &all, &no_analytics, &set), vec![4, 6], "largest first, capped at the chunk size");
    }

    #[test]
    fn one_chunk_is_in_flight_until_it_is_acknowledged_or_times_out() {
        let c = cache(5);
        let set = settings(10, 0.25);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        assert!(!s.is_waiting(t0, set.ack_timeout));
        let chunk: Vec<_> = s.plan(&c, &all, &no_analytics, &set).iter().map(|i| c[i]).collect();
        s.record_sent(1, t0, &chunk, &no_analytics);
        assert!(s.is_waiting(t0 + Duration::from_millis(100), set.ack_timeout), "waiting for the ack");
        assert!(!s.is_waiting(t0 + Duration::from_millis(501), set.ack_timeout), "timed out: send again");
        // Nothing was acknowledged, so the same nodes are still candidates and are rebuilt.
        assert_eq!(s.plan(&c, &all, &no_analytics, &set).len(), 5);
    }

    #[test]
    fn a_late_ack_for_a_superseded_chunk_changes_nothing() {
        let c = cache(4);
        let set = settings(10, 0.25);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        let chunk: Vec<_> = s.plan(&c, &all, &no_analytics, &set).iter().map(|i| c[i]).collect();
        s.record_sent(1, t0, &chunk, &no_analytics);
        s.record_sent(2, t0 + Duration::from_secs(1), &chunk, &no_analytics); // timed out, rebuilt as seq 2
        assert_eq!(s.acknowledge(1), None, "seq 1 was superseded");
        assert_eq!(s.acknowledged_nodes(), 0);
        assert_eq!(s.acknowledge(2), Some(4));
        assert_eq!(s.acknowledge(2), None, "an ack is consumed once");
    }

    #[test]
    fn entitlement_and_the_explicit_filter_exclude_nodes_entirely() {
        let c = cache(6);
        let set = settings(10, 0.25);
        let hidden = |id: u32| id != 3 && id != 5;
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        let ids = s.plan(&c, &hidden, &no_analytics, &set);
        assert_eq!(ids, vec![1, 2, 4, 6]);
        let chunk: Vec<_> = ids.iter().map(|i| c[i]).collect();
        seq += 1;
        s.record_sent(seq, Instant::now(), &chunk, &no_analytics);
        s.acknowledge(seq);
        assert!(s.plan(&c, &hidden, &no_analytics, &set).is_empty());
        assert!(!s.acked.contains_key(&3) && !s.acked.contains_key(&5), "a hidden node is never recorded");
    }

    #[test]
    fn a_zero_threshold_sends_any_real_change_but_never_an_identical_position() {
        let mut c = cache(2);
        let set = settings(10, 0.0);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        cycle(&mut s, &c, &set, &mut seq);
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "identical positions are not a change");
        c.insert(1, node(1, 1.0 + 1e-4));
        assert_eq!(s.plan(&c, &all, &no_analytics, &set), vec![1]);
    }

    #[test]
    fn a_node_whose_analytics_changed_is_sent_again_even_though_it_did_not_move() {
        let c = cache(4);
        let set = settings(10, 0.25);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        cycle(&mut s, &c, &set, &mut seq);
        // Node 3 is re-clustered; nothing moved.
        let recluster = |id: u32| if id == 3 { [7, 7, 0, 0] } else { [0; 4] };
        assert_eq!(s.plan(&c, &all, &recluster, &set), vec![3], "the new cluster id must reach the client");
        let chunk: Vec<_> = vec![c[&3]];
        seq += 1;
        s.record_sent(seq, Instant::now(), &chunk, &recluster);
        s.acknowledge(seq);
        assert!(s.plan(&c, &all, &recluster, &set).is_empty(), "once acknowledged, it is current again");
    }

    #[test]
    fn analytics_only_changes_queue_behind_nodes_that_actually_moved() {
        let mut c = cache(5);
        let set = settings(2, 0.1);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        while !cycle(&mut s, &c, &set, &mut seq).is_empty() {}
        c.insert(4, node(4, 4.0 + 9.0));
        let changed = |id: u32| if id == 1 { [1, 0, 0, 0] } else { [0; 4] };
        assert_eq!(s.plan(&c, &all, &changed, &set), vec![4, 1], "the mover first, then the analytics change");
    }

    #[test]
    fn a_node_that_returns_after_being_forgotten_is_sent_afresh() {
        let c = cache(3);
        let set = settings(10, 0.25);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        cycle(&mut s, &c, &set, &mut seq);
        s.retain_nodes(&|id| id != 2);
        assert_eq!(s.plan(&c, &all, &no_analytics, &set), vec![2]);
    }

    #[test]
    fn a_reconnecting_client_is_a_new_stream_and_gets_everything_again() {
        let c = cache(8);
        let set = settings(100, 0.25);
        let (mut old, mut seq) = (ClientStream::new(), 0u64);
        cycle(&mut old, &c, &set, &mut seq);
        assert!(old.plan(&c, &all, &no_analytics, &set).is_empty());
        let fresh = ClientStream::new();
        assert_eq!(fresh.plan(&c, &all, &no_analytics, &set).len(), 8, "no memory of the old connection");
    }

    #[test]
    fn planning_9478_nodes_is_cheap_enough_to_run_for_every_client_every_physics_step() {
        let c = cache(9478);
        let set = settings(2500, 0.25);
        let s = ClientStream::new();
        let t = Instant::now();
        for _ in 0..20 {
            assert_eq!(s.plan(&c, &all, &no_analytics, &set).len(), 2500);
        }
        let per_plan = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        assert!(per_plan < 25.0, "{per_plan:.2} ms per plan");
    }
}
