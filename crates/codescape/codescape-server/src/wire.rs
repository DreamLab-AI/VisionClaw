//! Per-client frame selection and `0x34` ack pacing. Pure: no I/O, no clocks.
//!
//! Each position socket owns one [`FrameSelector`], one [`AckWindow`] and one
//! [`SeqCounter`]. Every tick the server builds the records for the client's
//! projection and asks:
//!
//! 1. [`AckWindow::admit`]: may a frame go out? At most [`MAX_IN_FLIGHT`]
//!    unacknowledged frames are allowed once the client has shown it acks.
//!    A client that never acks (desktop) is never paced, so it cannot stall.
//!    If the oldest frame waits longer than [`ACK_TIMEOUT_MS`], the window is
//!    cleared and a keyframe forced: the XR freshness gate silently drops (and
//!    never acks) a frame it judged stale, so waiting forever would deadlock.
//! 2. [`FrameSelector::select`]: which records? A keyframe (join, every
//!    [`KEYFRAME_INTERVAL_MS`], after recovery) clears the baseline so every
//!    record counts as new. Otherwise only records that moved more than
//!    [`DISPLACEMENT_EPS`] or whose fields changed ([`needs_resend`]). The diff is
//!    against the last record actually *sent*, so a skipped tick folds into the
//!    next frame instead of being lost. At most `budget` records go per frame;
//!    the rest stay unsent and go next tick, which spreads a 200k-node keyframe
//!    over several sequenced frames.
//! 3. [`SeqCounter::advance`]: strictly increasing per connection, never reused,
//!    as the XR watermark requires.

use std::collections::{HashMap, VecDeque};

use codescape_model::NodeRecord;

/// Minimum displacement (world units) that makes a node worth resending.
pub const DISPLACEMENT_EPS: f32 = 0.01;
/// Minimum heat (`anomaly`) change worth resending; heat decays every tick.
pub const HEAT_EPS: f32 = 0.02;
/// Minimum importance change worth resending.
pub const CENTRALITY_EPS: f32 = 1e-3;
/// Full resend interval.
pub const KEYFRAME_INTERVAL_MS: u64 = 10_000;
/// Unacknowledged frames allowed in flight for an acking client.
pub const MAX_IN_FLIGHT: usize = 2;
/// After this long without an ack for the oldest frame, recover with a keyframe.
pub const ACK_TIMEOUT_MS: u64 = 2_000;
/// Records per V5 frame (16384 × 52 B ≈ 852 KB).
pub const MAX_RECORDS_PER_FRAME: usize = 16_384;

/// Whether a record differs enough from what the client last received.
/// Velocity is deliberately ignored: it changes every step while the layout runs.
pub fn needs_resend(prev: &NodeRecord, cur: &NodeRecord) -> bool {
    let d2: f32 = (0..3)
        .map(|k| (cur.position[k] - prev.position[k]).powi(2))
        .sum();
    if d2 > DISPLACEMENT_EPS * DISPLACEMENT_EPS || !d2.is_finite() {
        return true;
    }
    if cur.community_id != prev.community_id
        || cur.cluster_id != prev.cluster_id
        || cur.sssp_parent != prev.sssp_parent
    {
        return true;
    }
    if !same_f32(cur.sssp_distance, prev.sssp_distance) {
        return true;
    }
    if (cur.anomaly - prev.anomaly).abs() > HEAT_EPS
        || (cur.anomaly == 0.0) != (prev.anomaly == 0.0)
    {
        return true;
    }
    (cur.centrality - prev.centrality).abs() > CENTRALITY_EPS
}

fn same_f32(a: f32, b: f32) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// Monotonic V5 sequence per connection, starting at 1.
#[derive(Debug, Clone, Default)]
pub struct SeqCounter {
    last: u64,
}

impl SeqCounter {
    /// The next sequence number.
    pub fn advance(&mut self) -> u64 {
        self.last += 1;
        self.last
    }
    /// The last sequence handed out (0 before the first).
    pub fn last(&self) -> u64 {
        self.last
    }
}

/// Verdict of [`AckWindow::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Send a frame.
    Send,
    /// Window full: send nothing this tick. `behind` = frames awaiting ack, `skipped` = consecutive skipped ticks.
    Skip {
        /// Unacknowledged frames.
        behind: u32,
        /// Consecutive ticks skipped so far.
        skipped: u32,
    },
    /// The oldest frame timed out; the window was cleared. Send, starting a keyframe.
    Recover,
}

/// In-flight frame accounting for one client.
#[derive(Debug, Clone)]
pub struct AckWindow {
    in_flight: VecDeque<(u64, u64)>,
    max: usize,
    timeout_ms: u64,
    pacing: bool,
    skipped: u32,
}

impl Default for AckWindow {
    fn default() -> Self {
        Self::new(MAX_IN_FLIGHT, ACK_TIMEOUT_MS)
    }
}

impl AckWindow {
    /// A window of `max` frames with an ack timeout.
    pub fn new(max: usize, timeout_ms: u64) -> Self {
        Self {
            in_flight: VecDeque::new(),
            max: max.max(1),
            timeout_ms,
            pacing: false,
            skipped: 0,
        }
    }

    /// `true` once the client has acknowledged at least one frame.
    pub fn is_pacing(&self) -> bool {
        self.pacing
    }

    /// Frames sent and not yet acknowledged.
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// May a frame go out at `now_ms`?
    pub fn admit(&mut self, now_ms: u64) -> Admission {
        if !self.pacing || self.in_flight.len() < self.max {
            self.skipped = 0;
            return Admission::Send;
        }
        let oldest = self.in_flight.front().map(|f| f.1).unwrap_or(now_ms);
        if now_ms.saturating_sub(oldest) >= self.timeout_ms {
            self.in_flight.clear();
            self.skipped = 0;
            return Admission::Recover;
        }
        self.skipped += 1;
        Admission::Skip {
            behind: self.in_flight.len() as u32,
            skipped: self.skipped,
        }
    }

    /// Record a frame sent at `now_ms`. Before pacing starts only the latest `max` are kept.
    pub fn on_sent(&mut self, seq: u64, now_ms: u64) {
        self.in_flight.push_back((seq, now_ms));
        if !self.pacing {
            while self.in_flight.len() > self.max {
                self.in_flight.pop_front();
            }
        }
    }

    /// Apply a cumulative ack: every frame up to and including `seq` is consumed.
    /// Returns `false` for an ack naming a sequence never sent (ignored).
    pub fn on_ack(&mut self, seq: u64, last_sent: u64) -> bool {
        if seq == 0 || seq > last_sent {
            return false;
        }
        self.pacing = true;
        self.in_flight.retain(|f| f.0 > seq);
        true
    }
}

/// Outcome of one [`FrameSelector::select`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    /// This tick started a keyframe (the baseline was cleared).
    pub keyframe: bool,
    /// Records to send now (at most the budget).
    pub records: Vec<NodeRecord>,
    /// Changed records left for the next tick because of the budget.
    pub deferred: usize,
}

/// Remembers what one client last received.
#[derive(Debug, Clone)]
pub struct FrameSelector {
    last_sent: HashMap<u32, NodeRecord>,
    last_keyframe_ms: Option<u64>,
    force: bool,
    interval_ms: u64,
}

impl Default for FrameSelector {
    fn default() -> Self {
        Self::new(KEYFRAME_INTERVAL_MS)
    }
}

impl FrameSelector {
    /// A selector with the given keyframe interval. The first selection is a keyframe.
    pub fn new(interval_ms: u64) -> Self {
        Self {
            last_sent: HashMap::new(),
            last_keyframe_ms: None,
            force: true,
            interval_ms,
        }
    }

    /// Make the next selection a keyframe (projection changed, recovery, new epoch).
    pub fn force_keyframe(&mut self) {
        self.force = true;
    }

    /// Records the client is believed to hold.
    pub fn baseline_len(&self) -> usize {
        self.last_sent.len()
    }

    /// Choose the records to send from `current` (the client's full projection, built this tick).
    pub fn select(&mut self, current: &[NodeRecord], now_ms: u64, budget: usize) -> Selection {
        let due = self
            .last_keyframe_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= self.interval_ms);
        let keyframe = self.force || due;
        if keyframe {
            self.force = false;
            self.last_keyframe_ms = Some(now_ms);
            self.last_sent.clear();
        }
        let mut records = Vec::new();
        let mut deferred = 0usize;
        for r in current {
            let send = match self.last_sent.get(&r.wire_id) {
                None => true,
                Some(prev) => needs_resend(prev, r),
            };
            if !send {
                continue;
            }
            if records.len() < budget {
                records.push(*r);
                self.last_sent.insert(r.wire_id, *r);
            } else {
                deferred += 1;
            }
        }
        // Forget nodes that left the projection, so they are sent again on re-entry.
        let sent_or_held = current
            .iter()
            .filter(|r| self.last_sent.contains_key(&r.wire_id))
            .count();
        if self.last_sent.len() > sent_or_held {
            let keep: std::collections::HashSet<u32> = current.iter().map(|r| r.wire_id).collect();
            self.last_sent.retain(|id, _| keep.contains(id));
        }
        Selection {
            keyframe,
            records,
            deferred,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: u32, x: f32) -> NodeRecord {
        let mut r = NodeRecord::plain(id, [x, 0.0, 0.0]);
        r.sssp_distance = f32::INFINITY;
        r.sssp_parent = -1;
        r
    }

    #[test]
    fn first_select_is_a_keyframe_then_only_movers() {
        let mut s = FrameSelector::new(10_000);
        let cur: Vec<_> = (1..=5).map(|i| rec(i, i as f32)).collect();
        let a = s.select(&cur, 0, usize::MAX);
        assert!(a.keyframe);
        assert_eq!(a.records.len(), 5);

        let b = s.select(&cur, 100, usize::MAX);
        assert!(!b.keyframe);
        assert!(b.records.is_empty(), "nothing moved");

        let mut moved = cur.clone();
        moved[2].position[0] += 0.005; // below threshold
        moved[3].position[0] += 0.5;
        moved[3].velocity = [9.0; 3];
        moved[4].velocity = [9.0; 3]; // velocity alone never resends
        let c = s.select(&moved, 200, usize::MAX);
        assert_eq!(
            c.records.iter().map(|r| r.wire_id).collect::<Vec<_>>(),
            vec![4]
        );
    }

    #[test]
    fn small_moves_accumulate_against_last_sent() {
        let mut s = FrameSelector::new(10_000);
        let mut cur = vec![rec(1, 0.0)];
        s.select(&cur, 0, usize::MAX);
        for t in 1..=3 {
            cur[0].position[0] += 0.004;
            let sel = s.select(&cur, t * 100, usize::MAX);
            assert_eq!(sel.records.len(), usize::from(t == 3), "tick {t}");
        }
    }

    #[test]
    fn keyframe_interval_and_forced_keyframes() {
        let mut s = FrameSelector::new(10_000);
        let cur = vec![rec(1, 0.0), rec(2, 1.0)];
        s.select(&cur, 0, usize::MAX);
        assert!(!s.select(&cur, 9_999, usize::MAX).keyframe);
        let k = s.select(&cur, 10_000, usize::MAX);
        assert!(k.keyframe && k.records.len() == 2);
        s.force_keyframe();
        assert_eq!(s.select(&cur, 10_100, usize::MAX).records.len(), 2);
    }

    #[test]
    fn budget_spreads_a_keyframe_over_ticks() {
        let mut s = FrameSelector::new(10_000);
        let cur: Vec<_> = (1..=10).map(|i| rec(i, i as f32)).collect();
        let a = s.select(&cur, 0, 4);
        assert_eq!((a.records.len(), a.deferred), (4, 6));
        let b = s.select(&cur, 100, 4);
        assert!(!b.keyframe);
        assert_eq!((b.records.len(), b.deferred), (4, 2));
        let c = s.select(&cur, 200, 4);
        assert_eq!((c.records.len(), c.deferred), (2, 0));
        assert_eq!(s.select(&cur, 300, 4).records.len(), 0);
    }

    #[test]
    fn field_changes_resend_and_heat_is_quantised() {
        let base = rec(1, 0.0);
        let mut c = base;
        c.community_id = 3;
        assert!(needs_resend(&base, &c));
        let mut c = base;
        c.sssp_distance = 2.0;
        c.sssp_parent = 7;
        assert!(needs_resend(&base, &c));
        let mut hot = base;
        hot.anomaly = 0.50;
        let mut cooler = hot;
        cooler.anomaly = 0.49;
        assert!(
            !needs_resend(&hot, &cooler),
            "decay below HEAT_EPS does not resend"
        );
        cooler.anomaly = 0.45;
        assert!(needs_resend(&hot, &cooler));
        let mut tiny = base;
        tiny.anomaly = 0.01;
        let mut zero = tiny;
        zero.anomaly = 0.0;
        assert!(
            needs_resend(&tiny, &zero),
            "decay to zero is always delivered"
        );
        assert!(!needs_resend(&base, &base));
    }

    #[test]
    fn leaving_the_projection_forgets_the_node() {
        let mut s = FrameSelector::new(10_000);
        s.select(&[rec(1, 0.0), rec(2, 0.0)], 0, usize::MAX);
        s.select(&[rec(1, 0.0)], 100, usize::MAX);
        assert_eq!(s.baseline_len(), 1);
        let back = s.select(&[rec(1, 0.0), rec(2, 0.0)], 200, usize::MAX);
        assert_eq!(
            back.records.iter().map(|r| r.wire_id).collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn non_acking_clients_are_never_paced() {
        let mut w = AckWindow::new(2, 2_000);
        let mut seq = SeqCounter::default();
        for t in 0..100 {
            assert_eq!(w.admit(t * 100), Admission::Send);
            w.on_sent(seq.advance(), t * 100);
        }
        assert!(!w.is_pacing());
        assert_eq!(w.in_flight(), 2);
    }

    #[test]
    fn acking_clients_get_at_most_two_in_flight() {
        let mut w = AckWindow::new(2, 2_000);
        let mut seq = SeqCounter::default();
        let s1 = seq.advance();
        w.on_sent(s1, 0);
        assert!(w.on_ack(s1, seq.last()));
        assert!(w.is_pacing());
        assert_eq!(w.admit(100), Admission::Send);
        w.on_sent(seq.advance(), 100);
        assert_eq!(w.admit(200), Admission::Send);
        w.on_sent(seq.advance(), 200);
        assert_eq!(
            w.admit(300),
            Admission::Skip {
                behind: 2,
                skipped: 1
            }
        );
        assert_eq!(
            w.admit(400),
            Admission::Skip {
                behind: 2,
                skipped: 2
            }
        );
        // Cumulative ack of the newer frame frees both slots.
        assert!(w.on_ack(3, seq.last()));
        assert_eq!(w.in_flight(), 0);
        assert_eq!(w.admit(500), Admission::Send);
        assert!(
            !w.on_ack(99, seq.last()),
            "ack for a never-sent sequence is ignored"
        );
        assert!(!w.on_ack(0, seq.last()));
    }

    #[test]
    fn timeout_recovers_instead_of_deadlocking() {
        let mut w = AckWindow::new(2, 2_000);
        let mut seq = SeqCounter::default();
        let s = seq.advance();
        w.on_sent(s, 0);
        w.on_ack(s, seq.last());
        w.on_sent(seq.advance(), 100);
        w.on_sent(seq.advance(), 200);
        assert!(matches!(w.admit(1_000), Admission::Skip { .. }));
        assert_eq!(w.admit(2_100), Admission::Recover);
        assert_eq!(w.in_flight(), 0);
        assert_eq!(seq.advance(), 4, "sequence never rewinds after recovery");
    }
}
