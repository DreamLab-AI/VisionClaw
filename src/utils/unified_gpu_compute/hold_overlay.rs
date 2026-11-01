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
    /// The Verlet span: a hold and its settle window.
    active: bool,
    /// The spring span: a hold only. A release ends it, so a released node is placed, not pulled
    /// back towards the shape it had when it was grabbed.
    springs: bool,
    valid: bool,
    snapshots: u64,
}

impl HoldOverlay {
    /// Switch the overlay on or off for the coming step.
    ///
    /// Turning it on from off marks the snapshot stale (one is due). Staying on, as in a
    /// continuous hold, a second hold, or a re-grab inside the settle window, keeps the snapshot.
    pub(crate) fn set_active(&mut self, on: bool) {
        self.active = on;
        if !on {
            self.springs = false;
        }
    }

    /// Switch the spring term on or off for the coming step (a hold only; the settle window runs
    /// the Verlet integrator without springs). Every off to on transition marks the snapshot
    /// stale, so a re-grab inside the settle window measures the shape the node was dropped into.
    pub(crate) fn set_springs(&mut self, on: bool) {
        let on = on && self.active;
        if on && !self.springs {
            self.valid = false;
        }
        self.springs = on;
    }

    /// Whether the spring term is on.
    pub(crate) fn springs_active(&self) -> bool {
        self.springs
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
        if self.springs && !self.valid {
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

    fn hold(o: &mut HoldOverlay) {
        o.set_active(true);
        o.set_springs(true);
    }

    #[test]
    fn first_hold_snapshots_once() {
        let mut o = HoldOverlay::default();
        assert!(!o.snapshot_due(), "nothing held: no snapshot");
        hold(&mut o);
        assert!(o.snapshot_due());
        for _ in 0..100 {
            hold(&mut o);
            assert!(!o.snapshot_due(), "a continuous hold never re-snapshots");
        }
        assert_eq!(o.snapshot_count(), 1);
    }

    #[test]
    fn a_release_ends_the_springs_but_not_the_verlet_span() {
        let mut o = HoldOverlay::default();
        hold(&mut o);
        assert!(o.snapshot_due());
        o.set_active(true);
        o.set_springs(false);
        assert!(o.is_active() && !o.springs_active());
        assert!(!o.snapshot_due(), "no springs, nothing to snapshot");
    }

    #[test]
    fn a_regrab_inside_the_settle_window_snapshots_the_dropped_shape() {
        let mut o = HoldOverlay::default();
        hold(&mut o);
        assert!(o.snapshot_due());
        o.set_active(true);
        o.set_springs(false);
        hold(&mut o);
        assert!(
            o.snapshot_due(),
            "the second hold measures the shape after the first"
        );
        assert_eq!(o.snapshot_count(), 2);
    }

    #[test]
    fn springs_never_run_outside_the_verlet_span() {
        let mut o = HoldOverlay::default();
        o.set_springs(true);
        assert!(!o.springs_active());
        hold(&mut o);
        o.set_active(false);
        assert!(!o.springs_active());
    }

    #[test]
    fn a_csr_change_during_a_hold_resnapshots() {
        let mut o = HoldOverlay::default();
        hold(&mut o);
        assert!(o.snapshot_due());
        o.invalidate();
        assert!(o.snapshot_due());
        assert!(!o.snapshot_due());
    }
}
