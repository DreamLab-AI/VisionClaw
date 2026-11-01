//! Elastic-overlay state for "spring mode while held" (ADR-2169 amendment).
//!
//! While any node is held, and through the settle window that follows, the force pass adds a
//! linear spring `spring_k * (dist - rest_snapshot)` per edge on top of the stored LinLog
//! attraction. The rest lengths are captured once, when the overlay switches on, so the term is
//! exactly zero at the moment of grab. This type owns only the "when to snapshot" decision, so it
//! can be tested without a GPU.

/// Snapshot-once state machine for the elastic overlay.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HoldOverlay {
    active: bool,
    valid: bool,
    snapshots: u64,
}

impl HoldOverlay {
    /// Switch the overlay on or off for the coming step.
    ///
    /// Turning it on from off marks the snapshot stale (one is due). Staying on, as in a
    /// continuous hold, a second hold, or a re-grab inside the settle window, keeps the snapshot.
    pub(crate) fn set_active(&mut self, on: bool) {
        if on && !self.active {
            self.valid = false;
        }
        self.active = on;
    }

    /// Whether the overlay is on.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// The CSR changed (upload or resize): captured lengths no longer match its slots, so the
    /// next overlay step re-snapshots.
    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
    }

    /// True exactly once per overlay activation: the caller must run the snapshot kernel now.
    pub(crate) fn snapshot_due(&mut self) -> bool {
        if self.active && !self.valid {
            self.valid = true;
            self.snapshots += 1;
            true
        } else {
            false
        }
    }

    /// Snapshots taken so far (diagnostics and tests).
    pub(crate) fn snapshot_count(&self) -> u64 {
        self.snapshots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_hold_snapshots_once() {
        let mut o = HoldOverlay::default();
        assert!(!o.snapshot_due(), "nothing held: no snapshot");
        o.set_active(true);
        assert!(o.snapshot_due());
        for _ in 0..100 {
            o.set_active(true);
            assert!(!o.snapshot_due(), "a continuous hold never re-snapshots");
        }
        assert_eq!(o.snapshot_count(), 1);
    }

    #[test]
    fn hold_to_settle_to_regrab_keeps_the_snapshot() {
        let mut o = HoldOverlay::default();
        o.set_active(true);
        assert!(o.snapshot_due());
        // Release: overlay stays on through the settle window, then a second hold begins.
        o.set_active(true);
        o.set_active(true);
        assert!(!o.snapshot_due());
        assert_eq!(o.snapshot_count(), 1);
    }

    #[test]
    fn a_hold_after_the_overlay_ended_snapshots_again() {
        let mut o = HoldOverlay::default();
        o.set_active(true);
        assert!(o.snapshot_due());
        o.set_active(false);
        assert!(!o.snapshot_due());
        o.set_active(true);
        assert!(o.snapshot_due());
        assert_eq!(o.snapshot_count(), 2);
    }

    #[test]
    fn a_csr_change_during_the_overlay_resnapshots() {
        let mut o = HoldOverlay::default();
        o.set_active(true);
        assert!(o.snapshot_due());
        o.invalidate();
        assert!(o.snapshot_due());
        assert!(!o.snapshot_due());
    }
}
