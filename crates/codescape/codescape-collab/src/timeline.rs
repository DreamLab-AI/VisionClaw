//! A bounded, time-ordered log of presence events and revision markers.
//!
//! The timeline scrubber ("rewind 5 minutes", "replay") reads this log. It
//! asks which revision was current at time `t`, and how hot each node was at
//! that moment.

use std::collections::{HashMap, VecDeque};

use codescape_model::PresenceEvent;
use serde::{Deserialize, Serialize};

use crate::heat::{self, HeatField};

/// One logged item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// Someone touched the graph.
    Presence(PresenceEvent),
    /// The graph moved to this revision.
    Revision(u64),
}

/// An [`Entry`] with the time it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Unix milliseconds.
    pub ts_ms: u64,
    /// What happened.
    pub entry: Entry,
}

/// A bounded log of entries, kept sorted by timestamp. When the log is full,
/// the oldest entry is evicted.
///
/// ```
/// use codescape_collab::{TimelineLog, Entry};
/// let mut log = TimelineLog::new(100);
/// log.push(1_000, Entry::Revision(1));
/// log.push(5_000, Entry::Revision(2));
/// assert_eq!(log.revision_at(4_999), Some(1));
/// assert_eq!(log.revision_at(5_000), Some(2));
/// assert_eq!(log.revision_at(999), None);
/// ```
#[derive(Debug, Clone)]
pub struct TimelineLog {
    cap: usize,
    entries: VecDeque<TimelineEntry>,
}

impl TimelineLog {
    /// An empty log holding at most `cap` entries (at least one).
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            entries: VecDeque::with_capacity(cap.min(4096)),
        }
    }

    /// Append an entry. An out-of-order timestamp is inserted at its sorted
    /// position, after any entries with the same timestamp. If that pushes
    /// the log over capacity, the oldest entry is evicted, which can be the
    /// one just inserted.
    pub fn push(&mut self, ts_ms: u64, entry: Entry) {
        let at = self.entries.partition_point(|e| e.ts_ms <= ts_ms);
        self.entries.insert(at, TimelineEntry { ts_ms, entry });
        while self.entries.len() > self.cap {
            self.entries.pop_front();
        }
    }

    /// Append a presence event at its own `ts_ms`.
    pub fn push_presence(&mut self, ev: PresenceEvent) {
        self.push(ev.ts_ms, Entry::Presence(ev));
    }

    /// Append a revision marker.
    pub fn push_revision(&mut self, ts_ms: u64, revision: u64) {
        self.push(ts_ms, Entry::Revision(revision));
    }

    /// Entries with `from <= ts_ms <= to`, oldest first. Empty when `from > to`.
    pub fn window(&self, from: u64, to: u64) -> Vec<&TimelineEntry> {
        if from > to {
            return Vec::new();
        }
        let start = self.entries.partition_point(|e| e.ts_ms < from);
        let end = self.entries.partition_point(|e| e.ts_ms <= to);
        self.entries.range(start..end).collect()
    }

    /// Per-node heat as it was at `t`. This replays every logged presence
    /// event at or before `t` with the same intensities, decay and clamp as
    /// [`ActorRegistry::heat`](crate::ActorRegistry::heat), and returns only
    /// nodes above [`heat::FLOOR`]. Node `0` is skipped.
    pub fn heat_at(&self, t: u64, half_life_ms: u64) -> HashMap<u32, f32> {
        let mut field = HeatField::new(half_life_ms);
        let end = self.entries.partition_point(|e| e.ts_ms <= t);
        for e in self.entries.range(..end) {
            if let Entry::Presence(ev) = &e.entry {
                let amount = heat::intensity(ev.kind);
                for &n in ev.node_ids.iter().filter(|&&n| n != 0) {
                    field.add(n, amount, e.ts_ms);
                }
            }
        }
        field.snapshot(t)
    }

    /// The latest revision marker at or before `t`.
    pub fn revision_at(&self, t: u64) -> Option<u64> {
        let end = self.entries.partition_point(|e| e.ts_ms <= t);
        self.entries.range(..end).rev().find_map(|e| match e.entry {
            Entry::Revision(r) => Some(r),
            Entry::Presence(_) => None,
        })
    }

    /// The first and last timestamps, if the log is not empty.
    pub fn span(&self) -> Option<(u64, u64)> {
        Some((self.entries.front()?.ts_ms, self.entries.back()?.ts_ms))
    }

    /// Every entry, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &TimelineEntry> {
        self.entries.iter()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when the log holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum number of entries.
    pub fn capacity(&self) -> usize {
        self.cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codescape_model::PresenceKind;

    fn ev(kind: PresenceKind, nodes: &[u32], ts: u64) -> PresenceEvent {
        PresenceEvent {
            actor: 1,
            actor_label: "a".into(),
            actor_kind: "agent".into(),
            kind,
            node_ids: nodes.to_vec(),
            paths: vec![],
            text: None,
            ts_ms: ts,
        }
    }

    #[test]
    fn ring_evicts_oldest_and_sorts_late_arrivals() {
        let mut log = TimelineLog::new(3);
        log.push_revision(10, 1);
        log.push_revision(30, 3);
        log.push_revision(20, 2);
        assert_eq!(
            log.iter().map(|e| e.ts_ms).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        log.push_revision(40, 4);
        assert_eq!(log.span(), Some((20, 40)));
        log.push_revision(5, 0);
        assert_eq!(log.span(), Some((20, 40)), "too old for a full ring");
        assert_eq!(log.len(), 3);
        assert_eq!(log.capacity(), 3);
        assert_eq!(TimelineLog::new(0).capacity(), 1);
    }

    #[test]
    fn window_is_inclusive() {
        let mut log = TimelineLog::new(10);
        for t in [10, 20, 20, 30, 40] {
            log.push_revision(t, t);
        }
        let ts = |w: Vec<&TimelineEntry>| w.iter().map(|e| e.ts_ms).collect::<Vec<_>>();
        assert_eq!(ts(log.window(20, 30)), vec![20, 20, 30]);
        assert_eq!(ts(log.window(0, 9)), Vec::<u64>::new());
        assert_eq!(ts(log.window(30, 20)), Vec::<u64>::new());
        assert_eq!(ts(log.window(0, u64::MAX)).len(), 5);
        assert!(TimelineLog::new(1).span().is_none());
        assert!(TimelineLog::new(1).is_empty());
    }

    #[test]
    fn heat_at_replays_decay() {
        let mut log = TimelineLog::new(100);
        log.push_presence(ev(PresenceKind::Update, &[1], 0));
        log.push_presence(ev(PresenceKind::Query, &[2, 0], 60_000));
        log.push_presence(ev(PresenceKind::Update, &[3], 120_000));
        log.push_revision(60_000, 9);
        let h = log.heat_at(60_000, heat::HALF_LIFE_MS);
        assert_eq!(h[&1], 0.5);
        assert_eq!(h[&2], 0.4);
        assert!(!h.contains_key(&3), "future events are ignored");
        assert!(!h.contains_key(&0));
        let h = log.heat_at(120_000, heat::HALF_LIFE_MS);
        assert_eq!(h[&1], 0.25);
        assert_eq!(h[&2], 0.2);
        assert_eq!(h[&3], 1.0);
        assert_eq!(log.revision_at(120_000), Some(9));
        assert_eq!(log.revision_at(59_999), None);
    }
}
