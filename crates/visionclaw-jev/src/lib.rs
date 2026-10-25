//! # visionclaw-jev — the Jev option
//!
//! An opt-in typed-decision step (TypeSafe System One: **Choice**, **Score**,
//! **Noul** over `POST /v1/systemone`) that sits *beside* existing VisionClaw
//! behaviour. Every consumer in this crate follows one rule: with no Jev
//! availability the caller's code path, and its output, are the pre-change
//! ones.
//!
//! * Off by default: each item has its own `VISIONCLAW_JEV_<ITEM>=1` flag
//!   ([`config::Item`]). With the flag off no request is built and nothing
//!   leaves the process.
//! * Endpoint resolution follows the estate contract ([`config::resolve`]).
//! * Every failure maps to a [`Fallback`] reason; callers receive `Err` and
//!   keep their existing result. Nothing here panics, throws or blocks past
//!   a hard deadline. Reasons are logged as one line with no request content.
//! * Authority stays in code: Jev ranks, scores and annotates. It never
//!   writes, approves, signs or marks anything successful.
//!
//! The transport is a trait ([`client::Transport`]) so tests mock it; the real
//! one is [`client::HttpTransport`].
//!
//! Item modules, one per opportunity:
//!
//! | Item | Module |
//! |---|---|
//! | VC-38-S1-01 rerank memory hits | [`rerank`] |
//! | VC-20-S1-01 rank ontology patch proposals | [`patch_rank`] |
//! | VC-21-S1-01 ingest metadata suggestions | [`ingest`] |
//! | VC-24-S1-01 proposal queue triage | [`proposal_triage`] |
//! | VC-08-S1-01 degraded-state triage | [`health_triage`] |
//!
//! These are proposed research candidates: wiring and fallback tests show
//! correctness of the seam, not measured uplift.

pub mod client;
pub mod config;
pub mod health_triage;
pub mod ingest;
pub mod patch_rank;
pub mod proposal_triage;
pub mod rerank;
pub mod wire;

pub use client::{HttpTransport, Jev, Transport, TransportError};
pub use config::{Item, Resolved};

/// Why a Jev call fell back to the pre-change behaviour.
///
/// [`Fallback::tag`] is the exact token written to the log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// The item's flag is off.
    FlagOff,
    /// No endpoint resolved (or the consumer is local-only and none is local).
    NoEndpoint,
    /// The hard deadline elapsed.
    Timeout,
    /// A non-2xx status (including 429/529 with zero retries).
    Http(u16),
    /// The request could not be sent for another transport reason.
    Transport,
    /// The body did not parse, an expected answer was missing or mistyped, or
    /// a Choice key was outside the offered set.
    Malformed,
    /// Confidence or probability was below the acceptance threshold.
    LowConfidence,
}

impl Fallback {
    /// The log token: `flag-off`, `no-endpoint`, `timeout`, `http-<code>`,
    /// `transport`, `malformed` or `low-confidence`.
    pub fn tag(&self) -> String {
        match self {
            Fallback::FlagOff => "flag-off".into(),
            Fallback::NoEndpoint => "no-endpoint".into(),
            Fallback::Timeout => "timeout".into(),
            Fallback::Http(c) => format!("http-{c}"),
            Fallback::Transport => "transport".into(),
            Fallback::Malformed => "malformed".into(),
            Fallback::LowConfidence => "low-confidence".into(),
        }
    }
}

/// Log the fallback once: item, question-set version and reason, never request
/// content. `flag-off` is silent (it is the normal state).
pub fn log_fallback(item: Item, version: &str, reason: &Fallback) {
    if *reason != Fallback::FlagOff {
        log::info!(
            "jev fallback item={} qset={} reason={}",
            item.name(),
            version,
            reason.tag()
        );
    }
}

/// Reorder only the slots that carry a key.
///
/// `keys[i]` is `Some(priority)` for an item that was judged and `None` for one
/// that was not. Returns `order` where `order[pos]` is the original index now at
/// `pos`: judged items are placed, highest key first (ties keep original
/// order), into the slots judged items already held; unjudged items never
/// move. All `None` (or no items) yields the identity permutation.
pub fn slot_order(keys: &[Option<f64>]) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..keys.len()).filter(|&i| keys[i].is_some()).collect();
    let mut ranked = slots.clone();
    ranked.sort_by(|&a, &b| {
        keys[b]
            .partial_cmp(&keys[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut order: Vec<usize> = (0..keys.len()).collect();
    slots.sort_unstable();
    for (slot, item) in slots.into_iter().zip(ranked) {
        order[slot] = item;
    }
    order
}

/// Apply a permutation from [`slot_order`] to `items`.
///
/// # Panics
///
/// If `order` is not a permutation of `0..items.len()`.
pub fn apply_order<T>(items: Vec<T>, order: &[usize]) -> Vec<T> {
    assert_eq!(items.len(), order.len(), "order must cover every item");
    let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
    order.iter().map(|&i| slots[i].take().expect("order is a permutation")).collect()
}

/// Truncate to at most `max` characters (not bytes), appending `…` if cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn slot_order_moves_only_judged_items() {
        assert_eq!(slot_order(&[]), Vec::<usize>::new());
        assert_eq!(slot_order(&[None, None]), vec![0, 1]);
        assert_eq!(slot_order(&[Some(0.1), None, Some(0.9)]), vec![2, 1, 0]);
        // ties keep original order
        assert_eq!(slot_order(&[Some(0.5), Some(0.5)]), vec![0, 1]);
    }

    #[test]
    fn apply_order_permutes() {
        assert_eq!(apply_order(vec!["a", "b", "c"], &[2, 1, 0]), vec!["c", "b", "a"]);
    }

    #[test]
    fn clip_counts_characters() {
        assert_eq!(clip("héllo", 10), "héllo");
        assert_eq!(clip("héllo", 2), "hé…");
    }

    #[test]
    fn fallback_tags_match_the_contract() {
        assert_eq!(Fallback::FlagOff.tag(), "flag-off");
        assert_eq!(Fallback::NoEndpoint.tag(), "no-endpoint");
        assert_eq!(Fallback::Timeout.tag(), "timeout");
        assert_eq!(Fallback::Http(429).tag(), "http-429");
        assert_eq!(Fallback::Malformed.tag(), "malformed");
        assert_eq!(Fallback::LowConfidence.tag(), "low-confidence");
    }
}
