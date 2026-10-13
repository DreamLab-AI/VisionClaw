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
//! * The next chunk is built when the client acknowledges the previous one, or when `ack_timeout`
//!   passes without an acknowledgement (the client, or the frame, was lost). The delivery rate is
//!   therefore set by each client's own round trip, and when every client has the latest
//!   positions there are no candidates and nothing is sent.
//! * A chunk that timed out is NOT forgotten: the last few stay on a small list, so a healthy but
//!   slow client whose acknowledgement arrives after its chunk was replaced still makes progress
//!   (without this, a client slower than the timeout would never advance). Each consecutive
//!   timeout doubles the wait, up to 8x, so a client that never acknowledges costs a bounded
//!   trickle, and any acknowledgement resets it.
//! * Part of each chunk is reserved for the longest-waiting candidates, so nodes that move a
//!   little cannot be starved forever by nodes that move a lot.
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
    /// The chunk this was acknowledged in: an older acknowledgement never overwrites a newer one,
    /// and a smaller value means a longer wait since the client last heard about the node.
    seq: u64,
}

/// Chunks kept after a timeout so a late acknowledgement can still be honoured.
const IN_FLIGHT_HISTORY: usize = 8;
/// A chunk's capacity reserved for the longest-waiting candidates: `1 / FAIR_SHARE_DIVISOR`.
const FAIR_SHARE_DIVISOR: usize = 4;
/// Most consecutive timeouts that still lengthen the wait (2^3 = 8x).
const MAX_BACKOFF_DOUBLINGS: u32 = 3;

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
    /// Oldest first; the last entry is the chunk most recently sent.
    in_flight: std::collections::VecDeque<InFlight>,
    /// Chunks that timed out one after another since the last acknowledgement.
    consecutive_timeouts: u32,
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

impl ClientStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// The wait before an unacknowledged chunk is replaced: the timeout, doubled for each
    /// consecutive timeout up to 8x.
    pub fn effective_timeout(&self, ack_timeout: Duration) -> Duration {
        ack_timeout * (1u32 << self.consecutive_timeouts.min(MAX_BACKOFF_DOUBLINGS))
    }

    /// True while the latest chunk is on its way and has not yet timed out: nothing new should be sent.
    pub fn is_waiting(&self, now: Instant, ack_timeout: Duration) -> bool {
        let wait = self.effective_timeout(ack_timeout);
        self.in_flight
            .back()
            .is_some_and(|f| now.saturating_duration_since(f.sent_at) < wait)
    }

    /// True when a chunk has been sent and not yet acknowledged.
    pub fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
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
        // (displacement², age key, id). Never-acknowledged nodes are infinitely displaced and the
        // oldest possible (age key 0); an acknowledged node's age key is the chunk it was last
        // acknowledged in, so a smaller key has waited longer.
        let mut candidates: Vec<(f32, u64, u32)> = Vec::new();
        for (&id, p) in cache {
            if !visible(id) {
                continue;
            }
            let pos = [p.x, p.y, p.z];
            match self.acked.get(&id) {
                None => candidates.push((f32::INFINITY, 0, id)),
                Some(held) => {
                    let d2 = dist2(pos, held.pos);
                    // A zero threshold means "any change"; an identical position is never a change.
                    if d2 > thr2 && d2 > 0.0 {
                        candidates.push((d2, held.seq, id));
                    } else if analytics(id) != held.key {
                        // Same place, different cluster / anomaly / centrality: still news, but
                        // behind every node that actually moved.
                        candidates.push((0.0, held.seq, id));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }
        // Largest displacement first; ties (all the never-sent nodes) by id for determinism.
        let by_move = |a: &(f32, u64, u32), b: &(f32, u64, u32)| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2));
        // Longest-waiting first.
        let by_age = |a: &(f32, u64, u32), b: &(f32, u64, u32)| a.1.cmp(&b.1).then(a.2.cmp(&b.2));
        let k = settings.chunk_nodes.max(1);
        if candidates.len() > k {
            // Most of the chunk goes to the biggest movers; a reserved share goes to whatever has
            // waited longest, so small movers are never starved behind a constantly moving set.
            let fair = (k / FAIR_SHARE_DIVISOR).min(k.saturating_sub(1));
            let movers = k - fair;
            candidates.select_nth_unstable_by(movers - 1, by_move);
            let rest = &mut candidates[movers..];
            let take = fair.min(rest.len());
            if take > 0 {
                rest.select_nth_unstable_by(take - 1, by_age);
            }
            candidates.truncate(movers + take);
        }
        candidates.sort_unstable_by(by_move);
        candidates.into_iter().map(|(_, _, id)| id).collect()
    }

    /// Record the chunk just sent. If an earlier chunk is still unacknowledged it has timed out, so
    /// the backoff lengthens; it stays on the list so a late acknowledgement can still be honoured.
    pub fn record_sent(
        &mut self,
        seq: u64,
        now: Instant,
        positions: &[BinaryNodeDataClient],
        analytics: &dyn Fn(u32) -> AnalyticsKey,
    ) {
        if !self.in_flight.is_empty() {
            self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
        }
        if self.in_flight.len() >= IN_FLIGHT_HISTORY {
            self.in_flight.pop_front();
        }
        self.in_flight.push_back(InFlight {
            seq,
            sent_at: now,
            nodes: positions
                .iter()
                .map(|p| (p.node_id, Held { pos: [p.x, p.y, p.z], key: analytics(p.node_id), seq }))
                .collect(),
        });
    }

    /// The client acknowledged chunk `seq`: what that chunk carried is now what it holds. Returns
    /// the number of nodes in it, or `None` for a sequence not on the in-flight list (unknown, or
    /// aged out), which changes nothing.
    ///
    /// A late acknowledgement for a chunk that was already replaced is honoured. It never makes the
    /// server believe the client holds OLDER state than a later acknowledgement established, and
    /// chunks sent before it are dropped from the list (the connection delivers in order, so a
    /// still-unacknowledged older chunk is lost or was rejected; its nodes simply stay candidates).
    pub fn acknowledge(&mut self, seq: u64) -> Option<usize> {
        let at = self.in_flight.iter().position(|f| f.seq == seq)?;
        let f = self.in_flight.remove(at)?;
        self.in_flight.retain(|o| o.seq > seq);
        self.consecutive_timeouts = 0;
        let n = f.nodes.len();
        for (id, held) in f.nodes {
            match self.acked.get(&id) {
                Some(cur) if cur.seq > held.seq => {}
                _ => {
                    self.acked.insert(id, held);
                }
            }
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
    fn a_late_ack_for_a_replaced_chunk_still_commits_it() {
        // A healthy client slower than the timeout: its ack for chunk 1 arrives after chunk 2 was
        // already built. Without honouring it the client would never advance.
        let c = cache(4);
        let set = settings(10, 0.25);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        let chunk: Vec<_> = s.plan(&c, &all, &no_analytics, &set).iter().map(|i| c[i]).collect();
        s.record_sent(1, t0, &chunk, &no_analytics);
        s.record_sent(2, t0 + Duration::from_secs(1), &chunk, &no_analytics);
        assert_eq!(s.acknowledge(1), Some(4), "the late ack for chunk 1 is honoured");
        assert_eq!(s.acknowledged_nodes(), 4);
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "the client is current");
        assert_eq!(s.acknowledge(1), None, "an ack is consumed once");
        assert_eq!(s.acknowledge(2), Some(4), "chunk 2 is still on the list");
        assert_eq!(s.acknowledge(99), None, "an unknown sequence changes nothing");
    }

    #[test]
    fn an_older_ack_never_makes_the_server_believe_the_client_holds_older_state() {
        let mut c = cache(1);
        let set = settings(10, 0.25);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        // chunk 1 carries x=1; the node then moves to x=50 and chunk 2 carries that
        s.record_sent(1, t0, &[c[&1]], &no_analytics);
        c.insert(1, node(1, 50.0));
        s.record_sent(2, t0 + Duration::from_secs(1), &[c[&1]], &no_analytics);
        assert_eq!(s.acknowledge(2), Some(1), "the newer chunk is acknowledged first");
        assert_eq!(s.acknowledge(1), None, "chunks sent before an acknowledged one are dropped from the list");
        assert!(s.plan(&c, &all, &no_analytics, &set).is_empty(), "still believed to hold x=50, not x=1");
    }

    #[test]
    fn each_consecutive_timeout_doubles_the_wait_up_to_eight_times_and_an_ack_resets_it() {
        let c = cache(2);
        let set = settings(10, 0.25);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        let ms = |n: u64| Duration::from_millis(n);
        s.record_sent(1, t0, &[c[&1]], &no_analytics);
        assert_eq!(s.effective_timeout(set.ack_timeout), ms(500));
        s.record_sent(2, t0, &[c[&1]], &no_analytics);
        assert_eq!(s.effective_timeout(set.ack_timeout), ms(1000));
        s.record_sent(3, t0, &[c[&1]], &no_analytics);
        s.record_sent(4, t0, &[c[&1]], &no_analytics);
        s.record_sent(5, t0, &[c[&1]], &no_analytics);
        assert_eq!(s.effective_timeout(set.ack_timeout), ms(4000), "capped at 8x");
        assert!(s.is_waiting(t0 + ms(3999), set.ack_timeout) && !s.is_waiting(t0 + ms(4001), set.ack_timeout));
        s.acknowledge(5);
        assert_eq!(s.effective_timeout(set.ack_timeout), ms(500), "an acknowledgement resets the backoff");
    }

    #[test]
    fn the_late_ack_list_is_bounded() {
        let c = cache(1);
        let mut s = ClientStream::new();
        let t0 = Instant::now();
        for seq in 1..=20u64 {
            s.record_sent(seq, t0, &[c[&1]], &no_analytics);
        }
        assert_eq!(s.acknowledge(1), None, "aged out of the list");
        assert!(s.acknowledge(20).is_some());
    }

    #[test]
    fn small_movers_are_not_starved_behind_a_constantly_moving_set() {
        // 40 nodes keep moving a lot; 10 nodes moved a little once. With a chunk of 8 and no
        // fairness the small movers would never be sent. A quarter of each chunk goes to the
        // longest-waiting, so every one of them is delivered.
        let mut c = cache(50);
        let set = settings(8, 0.1);
        let (mut s, mut seq) = (ClientStream::new(), 0u64);
        while !cycle(&mut s, &c, &set, &mut seq).is_empty() {}
        for id in 41..=50u32 {
            c.insert(id, node(id, id as f32 + 1.0)); // small, one-off
        }
        let mut delivered = std::collections::HashSet::new();
        for round in 0..40u32 {
            for id in 1..=40u32 {
                c.insert(id, node(id, id as f32 + 100.0 * (round as f32 + 1.0))); // large, forever
            }
            for id in cycle(&mut s, &c, &set, &mut seq) {
                if id > 40 {
                    delivered.insert(id);
                }
            }
        }
        assert_eq!(delivered.len(), 10, "every small mover was delivered, got {delivered:?}");
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
