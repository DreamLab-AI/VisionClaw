//! Page kinds, per-type status and current recall (ADR-VG-010 §6, ADR-VG-013).
//!
//! Vocabulary 3 widens the working vault from `Note`, `Episode` and
//! `Draft Concept` to the memory kinds. The kinds and the sets of `status`
//! values each takes are:
//!
//! | type | `status` |
//! |---|---|
//! | `Episode`, `Belief`, `Procedure` (memory kinds, ADR-VG-013 §1) | `draft`, `approved`, `superseded`, `invalidated`, `archived` |
//! | `Brief`, `Proposal`, `Note`, `Transcript`, `Journal`, `Canvas`, and every knowledge type | `draft`, `stable`, `deprecated` |
//!
//! A vocabulary may state a closed set for a type, which replaces the built-in
//! one (ruling B6). A type outside the table is not rejected here: whether a
//! vault admits it is the vocabulary's business.
//!
//! # Current recall
//!
//! [`is_recallable`] is the one place that says whether a page may be returned
//! as current knowledge. It excludes, by construction, briefs and proposals
//! (they are working papers, not memory kinds), and every memory page that is
//! not `approved`, which removes `superseded` and `invalidated` (withdrawn)
//! ones along with `draft` and `archived`. A page whose `valid-from` is still
//! ahead, or whose `valid-to` has passed, is excluded too. An absent
//! `valid-from` reads as already started and is never back-filled (ruling B5).
//! The rule is the same function ontomark calls, and
//! `tests/vocab_v3_fixture.rs` pins the two together.
//!
//! ```
//! use vault_core::lifecycle::is_recallable;
//!
//! let page = |y: &str| serde_yaml::from_str::<serde_yaml::Mapping>(y).unwrap();
//! let now = 1_800_000_000;
//! assert!(is_recallable(&page("type: Belief\nstatus: approved"), now));
//! assert!(!is_recallable(&page("type: Brief\nstatus: stable"), now));
//! assert!(!is_recallable(&page("type: Belief\nstatus: superseded"), now));
//! ```

use serde_yaml::{Mapping, Value};
use time::{format_description::FormatItem, macros::format_description, PrimitiveDateTime};

/// The durable memory kinds of ADR-VG-013 §1.
pub const MEMORY_KINDS: [&str; 3] = ["Episode", "Belief", "Procedure"];

/// The memory lifecycle of ADR-VG-013 §1.
pub const MEMORY_STATUSES: [&str; 5] =
    ["draft", "approved", "superseded", "invalidated", "archived"];

/// The lifecycle every other type uses (vocabulary 2's, kept for `Brief`,
/// `Proposal`, `Note` and the knowledge types).
pub const LEGACY_STATUSES: [&str; 3] = ["draft", "stable", "deprecated"];

/// The working types vocabulary 3 recognises. `Draft Concept` is retiring into
/// `Proposal` (ruling B2) and `Retraction` is not admitted.
pub const WORKING_TYPES: [&str; 9] = [
    "Note",
    "Episode",
    "Brief",
    "Proposal",
    "Belief",
    "Procedure",
    "Transcript",
    "Journal",
    "Canvas",
];

/// `true` for `Episode`, `Belief` and `Procedure`.
#[must_use]
pub fn is_memory_kind(ty: &str) -> bool {
    MEMORY_KINDS.contains(&ty)
}

/// The built-in set of `status` values for `ty`.
#[must_use]
pub fn builtin_statuses(ty: &str) -> &'static [&'static str] {
    if is_memory_kind(ty) {
        &MEMORY_STATUSES
    } else {
        &LEGACY_STATUSES
    }
}

/// Is `status` allowed for `ty`? `declared` is the closed set the vocabulary
/// states for the type, if any; it replaces the built-in one.
#[must_use]
pub fn status_is_valid(ty: &str, status: &str, declared: &[String]) -> bool {
    if declared.is_empty() {
        builtin_statuses(ty).contains(&status)
    } else {
        declared.iter().any(|d| d == status)
    }
}

const SECONDS: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");

/// Parse the contract's datetime, `YYYY-MM-DDTHH:MM:SSZ` (UTC, second
/// precision, nothing else), into Unix seconds.
#[must_use]
pub fn parse_utc_seconds(s: &str) -> Option<i64> {
    let body = s.strip_suffix('Z')?;
    if body.len() != 19 {
        return None;
    }
    PrimitiveDateTime::parse(body, SECONDS)
        .ok()
        .map(|t| t.assume_utc().unix_timestamp())
}

fn text(map: &Mapping, key: &str) -> Option<String> {
    match map.get(Value::String(key.to_owned()))? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `true` when current recall may return the page at `now` (Unix seconds): an
/// `approved` memory kind whose `valid-from` is not after `now` and whose
/// `valid-to` is after `now`. An unreadable time reads as absent; `validate`
/// reports it.
#[must_use]
pub fn is_recallable(map: &Mapping, now: i64) -> bool {
    let Some(ty) = text(map, "type") else {
        return false;
    };
    if !is_memory_kind(&ty) || text(map, "status").as_deref() != Some("approved") {
        return false;
    }
    let at = |key: &str| text(map, key).and_then(|s| parse_utc_seconds(&s));
    at("valid-from").is_none_or(|t| t <= now) && at("valid-to").is_none_or(|t| t > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(y: &str) -> Mapping {
        serde_yaml::from_str(y).unwrap()
    }
    const NOW: i64 = 1_794_961_600; // 2026-11-18T00:26:40Z

    #[test]
    fn the_new_kinds_and_statuses_are_recognised() {
        for t in WORKING_TYPES {
            assert!(!builtin_statuses(t).is_empty(), "{t}");
        }
        for s in MEMORY_STATUSES {
            assert!(status_is_valid("Belief", s, &[]), "{s}");
            assert!(status_is_valid("Episode", s, &[]), "{s}");
        }
        assert!(status_is_valid("Brief", "stable", &[]));
        assert!(status_is_valid("Proposal", "deprecated", &[]));
        assert!(!status_is_valid("Brief", "approved", &[]));
        assert!(!status_is_valid("Belief", "stable", &[]));
    }

    #[test]
    fn a_declared_set_replaces_the_builtin_one() {
        let note = ["draft".to_owned(), "final".to_owned()];
        assert!(status_is_valid("Note", "final", &note));
        assert!(!status_is_valid("Note", "stable", &note));
    }

    #[test]
    fn briefs_and_proposals_are_never_recalled() {
        for y in [
            "type: Brief\nstatus: stable",
            "type: Brief\nstatus: approved",
            "type: Proposal\nstatus: draft",
            "type: Note\nstatus: stable",
            "type: Class\nstatus: stable",
        ] {
            assert!(!is_recallable(&page(y), NOW), "{y}");
        }
    }

    #[test]
    fn only_approved_memory_pages_are_recalled() {
        assert!(is_recallable(&page("type: Episode\nstatus: approved"), NOW));
        assert!(is_recallable(
            &page("type: Procedure\nstatus: approved"),
            NOW
        ));
        for s in ["draft", "superseded", "invalidated", "archived"] {
            assert!(
                !is_recallable(&page(&format!("type: Belief\nstatus: {s}")), NOW),
                "{s}"
            );
        }
        assert!(!is_recallable(&page("type: Belief"), NOW), "no status");
        assert!(!is_recallable(&page("status: approved"), NOW), "no type");
    }

    #[test]
    fn the_valid_time_window_applies_and_an_absent_start_reads_as_started() {
        let at = |valid: &str| page(&format!("type: Belief\nstatus: approved\n{valid}"));
        assert!(
            is_recallable(&at(""), NOW),
            "B5: absent valid-from is not a future start"
        );
        assert!(is_recallable(
            &at("valid-from: '2026-11-01T00:00:00Z'"),
            NOW
        ));
        assert!(!is_recallable(
            &at("valid-from: '2027-01-01T00:00:00Z'"),
            NOW
        ));
        assert!(!is_recallable(&at("valid-to: '2026-11-01T00:00:00Z'"), NOW));
        assert!(is_recallable(&at("valid-to: '2027-01-01T00:00:00Z'"), NOW));
        // `valid-to` is exclusive: at the instant itself the page is closed.
        assert!(!is_recallable(&at("valid-to: '2026-11-18T00:26:40Z'"), NOW));
    }

    #[test]
    fn datetimes_are_utc_seconds_only() {
        assert_eq!(parse_utc_seconds("2026-11-18T00:26:40Z"), Some(NOW));
        for bad in [
            "2026-11-18",
            "2026-11-18T00:26:40",
            "2026-11-18T00:26:40+00:00",
            "2026-11-18T00:26:40.5Z",
            "2026-13-18T00:26:40Z",
        ] {
            assert_eq!(parse_utc_seconds(bad), None, "{bad}");
        }
    }
}
