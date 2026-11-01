//! Who is holding which node, and which nodes are pinned on purpose.
//!
//! The physics actor used to keep one global map of "pinned" nodes. A drag end left the node in
//! it, so a released node stayed anchored for ever, and two connections touching the same node
//! unpinned each other. This registry keeps the two reasons for a pin apart:
//!
//! * a **hold** is a live grab. It belongs to one holder (a connection) and ends when that holder
//!   lets go, drops off, or times out. The node then rejoins the simulation.
//! * an **explicit pin** is an operator assertion (`nodePin`). It survives any number of holds
//!   and is cleared only by an explicit unpin.
//!
//! A node is pinned on the GPU while it has either. Positions are display-space, exactly as
//! clients sent them. The type is pure bookkeeping, with no actor or GPU, so every rule here is
//! unit-tested without a device.
//!
//! Two clients grabbing the same node: the last write wins. Every hold update takes the node
//! over, and a release only counts from the holder that currently owns the node, so one client
//! letting go cannot free a node another client still has in its hand.

use glam::Vec3;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// What a release did to the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// The node was not held, or another holder owns it now. Nothing changed.
    Ignored,
    /// The hold ended and nothing else pins the node: it rejoins the simulation.
    Rejoined,
    /// The hold ended but the node carries an explicit pin: it stays anchored where it was dropped.
    KeptPinned,
}

/// Holds and explicit pins, with the effective pinned positions derived from both.
#[derive(Debug, Default, Clone)]
pub struct HoldRegistry {
    /// node → holder id, for live grabs.
    held: HashMap<u32, u64>,
    /// Nodes pinned by an explicit `nodePin`.
    explicit: HashSet<u32>,
    /// Effective pinned position of every node that is held or explicitly pinned.
    positions: HashMap<u32, Vec3>,
    /// When each held node was last written. A hold nobody refreshes is expired by [`expire`],
    /// so a hold whose release was lost or overtaken cannot stay for ever.
    ///
    /// [`expire`]: HoldRegistry::expire
    touched: HashMap<u32, Instant>,
}

impl HoldRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Hold `node` at `pos` for `holder`. The last write wins: a different holder takes the node
    /// over. Returns true when the pinned set or the pinned position changed.
    pub fn hold(&mut self, holder: u64, node: u32, pos: [f32; 3], now: Instant) -> bool {
        let v = Vec3::from(pos);
        self.touched.insert(node, now);
        let owner_changed = self.held.insert(node, holder) != Some(holder);
        let moved = self.positions.insert(node, v) != Some(v);
        owner_changed || moved
    }

    /// End `holder`'s grab on `node`.
    pub fn release(&mut self, holder: u64, node: u32) -> Release {
        if self.held.get(&node) != Some(&holder) {
            return Release::Ignored;
        }
        self.held.remove(&node);
        self.touched.remove(&node);
        if self.explicit.contains(&node) {
            Release::KeptPinned
        } else {
            self.positions.remove(&node);
            Release::Rejoined
        }
    }

    /// End every grab `holder` owns (disconnect or timeout). Returns each node with its outcome.
    pub fn release_holder(&mut self, holder: u64) -> Vec<(u32, Release)> {
        let owned: Vec<u32> = self
            .held
            .iter()
            .filter(|(_, h)| **h == holder)
            .map(|(n, _)| *n)
            .collect();
        owned
            .into_iter()
            .map(|n| (n, self.release(holder, n)))
            .collect()
    }

    /// Release every hold not written for longer than `max_age`. This is the backstop for a
    /// release that never arrives or arrives before the hold it ends (the two travel as separate
    /// futures); the connection's own timeout normally gets there first. Returns each node freed.
    pub fn expire(&mut self, now: Instant, max_age: Duration) -> Vec<(u32, Release)> {
        let stale: Vec<(u32, u64)> = self
            .held
            .iter()
            .filter(|(n, _)| {
                self.touched
                    .get(n)
                    .is_none_or(|t| now.saturating_duration_since(*t) > max_age)
            })
            .map(|(n, h)| (*n, *h))
            .collect();
        stale
            .into_iter()
            .map(|(n, h)| (n, self.release(h, n)))
            .collect()
    }

    /// Pin `node` explicitly at `pos`. Returns true when the pinned set or position changed.
    pub fn pin(&mut self, node: u32, pos: [f32; 3]) -> bool {
        let v = Vec3::from(pos);
        let newly = self.explicit.insert(node);
        let moved = self.positions.insert(node, v) != Some(v);
        newly || moved
    }

    /// Clear the explicit pin on `node`. A node that is still held stays pinned for its holder.
    /// Returns true when the node left the pinned set.
    pub fn unpin(&mut self, node: u32) -> bool {
        let had = self.explicit.remove(&node);
        if had && !self.held.contains_key(&node) {
            self.positions.remove(&node);
            return true;
        }
        false
    }

    /// True while any connection holds a node.
    pub fn any_held(&self) -> bool {
        !self.held.is_empty()
    }

    /// Number of nodes currently held.
    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    /// The holder that owns `node`, if it is held.
    pub fn holder_of(&self, node: u32) -> Option<u64> {
        self.held.get(&node).copied()
    }

    /// True when `node` carries an explicit pin.
    pub fn is_explicit(&self, node: u32) -> bool {
        self.explicit.contains(&node)
    }

    /// The effective pinned positions (held and explicit).
    pub fn positions(&self) -> &HashMap<u32, Vec3> {
        &self.positions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u64 = 1;
    const B: u64 = 2;
    fn t() -> Instant {
        Instant::now()
    }

    #[test]
    fn a_released_hold_rejoins_the_simulation() {
        let mut r = HoldRegistry::new();
        assert!(r.hold(A, 7, [1.0, 2.0, 3.0], t()));
        assert!(r.positions().contains_key(&7));
        assert_eq!(r.release(A, 7), Release::Rejoined);
        assert!(
            r.positions().is_empty(),
            "a released node must not stay pinned"
        );
        assert!(!r.any_held());
    }

    #[test]
    fn an_explicit_pin_survives_a_drag_that_started_after_it() {
        let mut r = HoldRegistry::new();
        r.pin(7, [0.0, 0.0, 0.0]);
        r.hold(A, 7, [5.0, 5.0, 5.0], t());
        assert_eq!(r.release(A, 7), Release::KeptPinned);
        assert_eq!(
            r.positions().get(&7),
            Some(&Vec3::new(5.0, 5.0, 5.0)),
            "anchored where it was dropped"
        );
        assert!(r.is_explicit(7));
    }

    #[test]
    fn an_explicit_pin_made_during_a_drag_survives_its_release() {
        let mut r = HoldRegistry::new();
        r.hold(A, 7, [1.0, 1.0, 1.0], t());
        r.pin(7, [2.0, 2.0, 2.0]);
        assert_eq!(r.release(A, 7), Release::KeptPinned);
        assert!(r.positions().contains_key(&7));
    }

    #[test]
    fn an_explicit_unpin_during_a_hold_keeps_the_hold_and_frees_the_node_on_release() {
        let mut r = HoldRegistry::new();
        r.pin(7, [0.0; 3]);
        r.hold(A, 7, [1.0; 3], t());
        assert!(!r.unpin(7), "still held, so it stays in the pinned set");
        assert_eq!(r.release(A, 7), Release::Rejoined);
        assert!(r.positions().is_empty());
    }

    #[test]
    fn unpin_frees_a_node_that_is_not_held() {
        let mut r = HoldRegistry::new();
        r.pin(7, [0.0; 3]);
        assert!(r.unpin(7));
        assert!(r.positions().is_empty());
        assert!(!r.unpin(7), "a second unpin changes nothing");
    }

    #[test]
    fn one_connection_can_hold_two_nodes_at_once() {
        let mut r = HoldRegistry::new();
        r.hold(A, 1, [1.0; 3], t());
        r.hold(A, 2, [2.0; 3], t());
        assert_eq!(r.held_count(), 2);
        assert_eq!(r.release(A, 1), Release::Rejoined);
        assert!(r.any_held(), "the other hand still holds");
        assert!(r.positions().contains_key(&2));
    }

    #[test]
    fn two_connections_hold_two_nodes_independently() {
        let mut r = HoldRegistry::new();
        r.hold(A, 1, [1.0; 3], t());
        r.hold(B, 2, [2.0; 3], t());
        assert_eq!(r.holder_of(1), Some(A));
        assert_eq!(r.holder_of(2), Some(B));
        r.release_holder(A);
        assert!(!r.positions().contains_key(&1));
        assert!(r.positions().contains_key(&2), "B's hold is untouched");
    }

    #[test]
    fn the_last_write_wins_when_two_connections_grab_one_node() {
        let mut r = HoldRegistry::new();
        r.hold(A, 9, [1.0; 3], t());
        r.hold(B, 9, [8.0; 3], t());
        assert_eq!(r.holder_of(9), Some(B));
        assert_eq!(r.positions().get(&9), Some(&Vec3::splat(8.0)));
        assert_eq!(r.release(A, 9), Release::Ignored, "A no longer owns it");
        assert!(
            r.any_held() && r.positions().contains_key(&9),
            "B still has it in hand"
        );
        assert_eq!(r.release(B, 9), Release::Rejoined);
    }

    #[test]
    fn disconnect_releases_only_what_the_connection_owned() {
        let mut r = HoldRegistry::new();
        r.pin(3, [0.0; 3]);
        r.hold(A, 3, [1.0; 3], t());
        r.hold(A, 4, [1.0; 3], t());
        r.hold(B, 5, [1.0; 3], t());
        let mut out = r.release_holder(A);
        out.sort_by_key(|(n, _)| *n);
        assert_eq!(out, vec![(3, Release::KeptPinned), (4, Release::Rejoined)]);
        assert!(r.positions().contains_key(&3) && r.positions().contains_key(&5));
        assert!(!r.positions().contains_key(&4));
    }

    #[test]
    fn re_sending_the_same_hold_reports_no_change() {
        let mut r = HoldRegistry::new();
        assert!(r.hold(A, 1, [1.0; 3], t()));
        assert!(!r.hold(A, 1, [1.0; 3], t()));
        assert!(r.hold(A, 1, [1.5; 3], t()), "a moved hold is a change");
    }

    #[test]
    fn releasing_a_node_nobody_holds_is_ignored() {
        let mut r = HoldRegistry::new();
        assert_eq!(r.release(A, 1), Release::Ignored);
    }

    #[test]
    fn a_hold_nobody_refreshes_expires_and_the_node_rejoins() {
        let mut r = HoldRegistry::new();
        let t0 = Instant::now();
        r.hold(A, 1, [1.0; 3], t0);
        r.hold(B, 2, [2.0; 3], t0 + Duration::from_secs(4));
        let out = r.expire(t0 + Duration::from_secs(6), Duration::from_secs(5));
        assert_eq!(
            out,
            vec![(1, Release::Rejoined)],
            "only the stale hold goes"
        );
        assert!(!r.positions().contains_key(&1));
        assert!(r.positions().contains_key(&2), "the fresh hold stays");
    }

    #[test]
    fn an_expired_hold_leaves_an_explicit_pin_in_place() {
        let mut r = HoldRegistry::new();
        let t0 = Instant::now();
        r.pin(3, [0.0; 3]);
        r.hold(A, 3, [1.0; 3], t0);
        let out = r.expire(t0 + Duration::from_secs(10), Duration::from_secs(5));
        assert_eq!(out, vec![(3, Release::KeptPinned)]);
        assert!(r.positions().contains_key(&3));
    }

    #[test]
    fn a_refreshed_hold_does_not_expire() {
        let mut r = HoldRegistry::new();
        let t0 = Instant::now();
        r.hold(A, 1, [1.0; 3], t0);
        r.hold(A, 1, [1.5; 3], t0 + Duration::from_secs(4));
        assert!(r
            .expire(t0 + Duration::from_secs(8), Duration::from_secs(5))
            .is_empty());
    }
}
