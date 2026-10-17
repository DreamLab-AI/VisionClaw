//! Per-node activity heat that decays exponentially over time.
//!
//! Each [`PresenceEvent`](codescape_model::PresenceEvent) adds an
//! [`intensity`] to every node it touched. A node's heat halves every
//! half-life and is clamped to `1.0` each time something is added, so a burst
//! of edits saturates instead of piling up. Readers only see nodes above
//! [`FLOOR`].

use std::collections::HashMap;

use codescape_model::PresenceKind;

/// Default heat half-life: 60 s.
pub const HALF_LIFE_MS: u64 = 60_000;

/// Heat at or below this value is treated as cold and left out of snapshots.
pub const FLOOR: f32 = 0.01;

/// How much heat one event of `kind` adds to each node it touched.
///
/// ```
/// use codescape_collab::heat::intensity;
/// use codescape_model::PresenceKind;
/// assert_eq!(intensity(PresenceKind::Update), 1.0);
/// assert_eq!(intensity(PresenceKind::Query), 0.4);
/// ```
pub fn intensity(kind: PresenceKind) -> f32 {
    match kind {
        PresenceKind::Update | PresenceKind::Create | PresenceKind::Delete => 1.0,
        PresenceKind::Query => 0.4,
        PresenceKind::Say => 0.3,
        PresenceKind::Focus => 0.2,
    }
}

/// Multiplier for `dt_ms` of decay at the given half-life: `0.5^(dt/half_life)`.
///
/// A zero half-life means instant decay: the multiplier is `1.0` at `dt = 0`
/// and `0.0` after that.
///
/// ```
/// use codescape_collab::heat::decay_factor;
/// assert_eq!(decay_factor(60_000, 60_000), 0.5);
/// assert_eq!(decay_factor(0, 60_000), 1.0);
/// ```
pub fn decay_factor(dt_ms: u64, half_life_ms: u64) -> f64 {
    if half_life_ms == 0 {
        return if dt_ms == 0 { 1.0 } else { 0.0 };
    }
    0.5f64.powf(dt_ms as f64 / half_life_ms as f64)
}

/// The decaying heat of a set of nodes.
///
/// Each node stores its value together with the time of its last change. A
/// read applies the decay lazily, so updates and reads both cost O(1) per node.
#[derive(Debug, Clone)]
pub struct HeatField {
    half_life_ms: u64,
    nodes: HashMap<u32, (f64, u64)>,
}

impl Default for HeatField {
    fn default() -> Self {
        Self::new(HALF_LIFE_MS)
    }
}

impl HeatField {
    /// An empty field with the given half-life in milliseconds.
    pub fn new(half_life_ms: u64) -> Self {
        Self {
            half_life_ms,
            nodes: HashMap::new(),
        }
    }

    /// Add `amount` to `node` at time `ts_ms`, then clamp to `1.0`.
    ///
    /// An event older than the node's last change is decayed forward to that
    /// time, so events arriving out of order still sum correctly.
    pub fn add(&mut self, node: u32, amount: f32, ts_ms: u64) {
        let amount = f64::from(amount.max(0.0));
        let hl = self.half_life_ms;
        let slot = self.nodes.entry(node).or_insert((0.0, ts_ms));
        if ts_ms >= slot.1 {
            slot.0 = slot.0 * decay_factor(ts_ms - slot.1, hl) + amount;
            slot.1 = ts_ms;
        } else {
            slot.0 += amount * decay_factor(slot.1 - ts_ms, hl);
        }
        slot.0 = slot.0.min(1.0);
    }

    /// The heat of `node` at `now_ms`, or `0.0` if it has never been touched.
    /// A time before the node's last change reads as that change.
    pub fn value_at(&self, node: u32, now_ms: u64) -> f32 {
        self.nodes.get(&node).map_or(0.0, |&(v, at)| {
            (v * decay_factor(now_ms.saturating_sub(at), self.half_life_ms)) as f32
        })
    }

    /// Every node hotter than [`FLOOR`] at `now_ms`.
    pub fn snapshot(&self, now_ms: u64) -> HashMap<u32, f32> {
        self.nodes
            .keys()
            .filter_map(|&n| {
                let v = self.value_at(n, now_ms);
                (v > FLOOR).then_some((n, v))
            })
            .collect()
    }

    /// Drop nodes that are cold at `now_ms`. Heat only decays, so they stay
    /// cold at every later time.
    pub fn prune(&mut self, now_ms: u64) {
        let hl = self.half_life_ms;
        self.nodes.retain(|_, &mut (v, at)| {
            (v * decay_factor(now_ms.saturating_sub(at), hl)) as f32 > FLOOR
        });
    }

    /// Number of nodes still tracked, cold ones included until the next
    /// [`prune`](Self::prune).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` when no node is tracked.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_life_is_exact() {
        let mut h = HeatField::default();
        h.add(1, 1.0, 1_000);
        assert_eq!(h.value_at(1, 1_000), 1.0);
        assert_eq!(h.value_at(1, 61_000), 0.5);
        assert_eq!(h.value_at(1, 121_000), 0.25);
        assert_eq!(h.value_at(1, 500), 1.0, "past reads do not amplify");
    }

    #[test]
    fn accumulation_clamps_and_out_of_order_decays_forward() {
        let mut h = HeatField::default();
        h.add(1, 0.4, 60_000);
        h.add(1, 0.4, 0); // 0.4 * 0.5 = 0.2 at t=60 s
        assert!((h.value_at(1, 60_000) - 0.6).abs() < 1e-6);
        for _ in 0..5 {
            h.add(1, 1.0, 60_000);
        }
        assert_eq!(h.value_at(1, 60_000), 1.0);
    }

    #[test]
    fn snapshot_and_prune_drop_cold_nodes() {
        let mut h = HeatField::default();
        h.add(1, 1.0, 0);
        h.add(2, 0.2, 0);
        // 0.2 * 0.5^(6) = 0.003 < 0.01; 1.0 * 0.5^6 = 0.0156 > 0.01
        let s = h.snapshot(360_000);
        assert_eq!(s.len(), 1);
        assert!(s.contains_key(&1));
        h.prune(360_000);
        assert_eq!(h.len(), 1);
        assert_eq!(decay_factor(5, 0), 0.0);
    }
}
