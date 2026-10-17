//! The single push-to-talk floor.
//!
//! The speech recogniser is single-stream and may be shared with another
//! consumer, so at most one person speaks at a time. Requests queue first in,
//! first out. A holder is cut off after the hold cap. While the ASR reports
//! itself busy, nobody holds the floor.

use std::collections::VecDeque;

use codescape_model::FloorState;

/// Default hold cap: 30 s.
pub const DEFAULT_HOLD_CAP_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Holder {
    actor: u32,
    since_ms: u64,
}

/// Arbiter of the single floor. All times are Unix milliseconds supplied by
/// the caller.
///
/// Semantics:
/// * a holder that requests again keeps the floor without its hold clock
///   being reset;
/// * a promoted holder's clock starts at the `now` of the call that promoted
///   them;
/// * a holder is expired once `now - since >= hold_cap`;
/// * [`mark_busy(true)`](Self::mark_busy) puts the current holder back at
///   the front of the queue, and [`mark_busy(false)`](Self::mark_busy)
///   promotes the head of the queue.
///
/// ```
/// use codescape_collab::FloorArbiter;
/// use codescape_model::FloorState;
/// let mut f = FloorArbiter::new(30_000);
/// f.request(1, 0);
/// f.request(2, 5);
/// assert_eq!(f.tick(30_000), Some(FloorState::Held { holder: 2, queue: vec![] }));
/// assert_eq!(f.tick(30_001), None);
/// ```
#[derive(Debug, Clone)]
pub struct FloorArbiter {
    hold_cap_ms: u64,
    holder: Option<Holder>,
    queue: VecDeque<u32>,
    busy: bool,
    clock_ms: u64,
}

impl Default for FloorArbiter {
    fn default() -> Self {
        Self::new(DEFAULT_HOLD_CAP_MS)
    }
}

impl FloorArbiter {
    /// A free floor with the given hold cap in milliseconds.
    pub fn new(hold_cap_ms: u64) -> Self {
        Self {
            hold_cap_ms,
            holder: None,
            queue: VecDeque::new(),
            busy: false,
            clock_ms: 0,
        }
    }

    /// The state to broadcast.
    pub fn state(&self) -> FloorState {
        if self.busy {
            FloorState::Busy
        } else if let Some(h) = self.holder {
            FloorState::Held {
                holder: h.actor,
                queue: self.queue.iter().copied().collect(),
            }
        } else {
            FloorState::Free
        }
    }

    /// The current holder, if any.
    pub fn holder(&self) -> Option<u32> {
        self.holder.map(|h| h.actor)
    }

    /// `actor` pressed push-to-talk. They take the floor if it is free.
    /// Otherwise they join the queue, at most once.
    pub fn request(&mut self, actor: u32, now_ms: u64) -> FloorState {
        self.advance(now_ms);
        if self.holder() == Some(actor) {
            return self.state();
        }
        if !self.busy && self.holder.is_none() {
            self.queue.retain(|&a| a != actor);
            self.holder = Some(Holder {
                actor,
                since_ms: now_ms,
            });
        } else if !self.queue.contains(&actor) {
            self.queue.push_back(actor);
        }
        self.state()
    }

    /// `actor` let go. A holder hands the floor to the next in the queue; a
    /// waiting actor just leaves the queue.
    pub fn release(&mut self, actor: u32, now_ms: u64) -> FloorState {
        self.advance(now_ms);
        if self.holder() == Some(actor) {
            self.holder = None;
            self.promote(now_ms);
        } else {
            self.queue.retain(|&a| a != actor);
        }
        self.state()
    }

    /// Expire a holder who has passed the hold cap and promote the next one.
    /// Returns the new state only if it changed.
    pub fn tick(&mut self, now_ms: u64) -> Option<FloorState> {
        let before = self.state();
        self.advance(now_ms);
        let after = self.state();
        (after != before).then_some(after)
    }

    /// Report whether the shared ASR is serving someone else.
    pub fn mark_busy(&mut self, busy: bool) -> FloorState {
        if busy && !self.busy {
            if let Some(h) = self.holder.take() {
                self.queue.push_front(h.actor);
            }
        }
        let was_busy = self.busy;
        self.busy = busy;
        if was_busy && !busy {
            self.promote(self.clock_ms);
        }
        self.state()
    }

    fn advance(&mut self, now_ms: u64) {
        self.clock_ms = self.clock_ms.max(now_ms);
        if let Some(h) = self.holder {
            if now_ms.saturating_sub(h.since_ms) >= self.hold_cap_ms {
                self.holder = None;
                self.promote(now_ms);
            }
        }
    }

    fn promote(&mut self, now_ms: u64) {
        if self.busy || self.holder.is_some() {
            return;
        }
        if let Some(actor) = self.queue.pop_front() {
            self.holder = Some(Holder {
                actor,
                since_ms: now_ms,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FloorState::*;

    fn held(holder: u32, queue: &[u32]) -> FloorState {
        Held {
            holder,
            queue: queue.to_vec(),
        }
    }

    #[test]
    fn fifo_sequence() {
        let mut f = FloorArbiter::default();
        assert_eq!(f.state(), Free);
        assert_eq!(f.request(1, 0), held(1, &[]));
        assert_eq!(f.request(2, 1), held(1, &[2]));
        assert_eq!(f.request(3, 2), held(1, &[2, 3]));
        assert_eq!(f.request(2, 3), held(1, &[2, 3]), "no double queueing");
        assert_eq!(
            f.request(1, 4),
            held(1, &[2, 3]),
            "holder re-request is idempotent"
        );
        assert_eq!(f.release(1, 5), held(2, &[3]));
        assert_eq!(f.release(2, 6), held(3, &[]));
        assert_eq!(f.release(3, 7), Free);
        assert_eq!(f.holder(), None);
    }

    #[test]
    fn non_holder_release_dequeues() {
        let mut f = FloorArbiter::default();
        f.request(1, 0);
        f.request(2, 0);
        f.request(3, 0);
        assert_eq!(f.release(2, 1), held(1, &[3]));
        assert_eq!(f.release(9, 1), held(1, &[3]), "unknown actor is a no-op");
    }

    #[test]
    fn hold_cap_expiry_promotes_and_restarts_clock() {
        let mut f = FloorArbiter::new(30_000);
        f.request(1, 0);
        f.request(2, 10_000);
        assert_eq!(f.tick(29_999), None);
        assert_eq!(f.tick(30_000), Some(held(2, &[])));
        assert_eq!(f.tick(59_999), None, "2's clock started at 30 s");
        assert_eq!(f.tick(60_000), Some(Free));
        assert_eq!(f.tick(90_000), None);
        f.request(1, 100_000);
        f.request(1, 120_000);
        assert_eq!(
            f.tick(130_000),
            Some(Free),
            "re-request did not reset the clock"
        );
    }

    #[test]
    fn expiry_is_applied_lazily_by_request() {
        let mut f = FloorArbiter::new(1_000);
        f.request(1, 0);
        assert_eq!(f.request(2, 5_000), held(2, &[]));
    }

    #[test]
    fn busy_demotes_holder_and_blocks_promotion() {
        let mut f = FloorArbiter::new(30_000);
        f.request(1, 0);
        f.request(2, 0);
        assert_eq!(f.mark_busy(true), Busy);
        assert_eq!(f.holder(), None);
        assert_eq!(f.request(3, 100), Busy);
        assert_eq!(f.release(2, 200), Busy);
        assert_eq!(f.tick(100_000), None, "nothing to expire while busy");
        assert_eq!(f.mark_busy(true), Busy, "idempotent");
        assert_eq!(f.mark_busy(false), held(1, &[3]));
        assert_eq!(
            f.tick(100_000 + 29_999),
            None,
            "clock starts at last seen time"
        );
        assert_eq!(f.tick(130_000), Some(held(3, &[])));
        assert_eq!(
            f.mark_busy(false),
            held(3, &[]),
            "unbusy when not busy is a no-op"
        );
    }

    #[test]
    fn busy_with_empty_queue_then_free() {
        let mut f = FloorArbiter::default();
        assert_eq!(f.mark_busy(true), Busy);
        assert_eq!(f.mark_busy(false), Free);
    }
}
