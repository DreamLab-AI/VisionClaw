//! Declared-intent ↔ recorded-act matching (PRD-augmentation-conditions FR5.2,
//! EXP-AC-005).
//!
//! D7 gives an agent a place to say what it is **about to** do
//! (`AgentActionEnvelope::intent`). This module answers the follow-on question
//! the augmentation-conditions lens asks: *did it then do that?* The answer
//! rides `/api/trace` as `intent_match` and feeds HITL Precision — an escalation
//! is warranted when the agent's act diverged from its declaration.
//!
//! ## The rule (deliberately narrow)
//!
//! Matching is **exact, case-insensitive and structural**. Semantic similarity
//! is explicitly out of scope (EXP-AC-005 "Out of scope"): a fuzzy matcher would
//! manufacture agreement, which is precisely the failure mode this context
//! exists to remove. An intent declares up to two components, each compared by
//! the rule its own shape warrants:
//!
//!   * an **operation** — token containment against the recorded
//!     `action_type_name`, so a declared `update` matches `graph_update`;
//!   * a **target** — WHOLE `:`/`/`-delimited segment containment against the
//!     recorded `target_urn`, so a declared `urn:kg:node-7` does NOT match a
//!     recorded `urn:kg:node-70` ([`urn_names_segment`]).
//!
//! Only components the agent ACTUALLY DECLARED are required to match, and at
//! least one must have been declared. So:
//!
//! | declared | recorded | verdict |
//! |---|---|---|
//! | nothing (no intent, blank, or unparseable) | anything | `None` — no claim to check |
//! | operation only, found | anything | `Some(true)` |
//! | operation + target, both found | matching act | `Some(true)` |
//! | any declared component absent or different | — | `Some(false)` |
//!
//! `None` is the honest answer for "the agent made no claim", and is never
//! conflated with `Some(false)` ("the agent's claim did not hold"). An intent
//! naming a target the act did not record is `Some(false)`, not `None`: the
//! claim was made and the evidence does not support it.
//!
//! ## Intent syntax accepted
//!
//! Both a structured form and prose:
//!
//! ```text
//! op=update target=urn:visionclaw:kg:aaaa:node-7     // structured
//! update urn:visionclaw:kg:aaaa:node-7               // positional
//! about to: elevate urn:ngm:class:Foo                // prose with a lead-in
//! ```
//!
//! Nothing here rewrites or normalises the stored intent — the envelope's text
//! is persisted verbatim (FR5.1) and only *read* here.

/// The components an agent declared in its `intent`, as parsed. Either may be
/// absent: an agent that named only an operation made no claim about a target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredIntent {
    /// The declared operation (a verb), lowercased and stripped of punctuation.
    pub operation: Option<String>,
    /// The declared target (a URN/path), as written.
    pub target: Option<String>,
}

impl DeclaredIntent {
    /// True when the agent declared nothing verifiable.
    pub fn is_empty(&self) -> bool {
        self.operation.is_none() && self.target.is_none()
    }
}

/// Keys accepted in the structured `key=value` intent form.
const OPERATION_KEYS: [&str; 3] = ["op", "operation", "action"];
const TARGET_KEYS: [&str; 3] = ["target", "on", "subject"];

/// Prose lead-ins an agent may write before the operation ("about to: update …").
/// Stripped so the operation is not mistaken for the lead-in's first word.
const LEAD_INS: [&str; 6] = ["about", "to", "going", "will", "intend", "intends"];

/// Trim the punctuation an agent's prose wraps tokens in, without touching the
/// characters URNs are built from (`:`, `/`, `-`, `_`, `.`, `#`).
fn trim_token(t: &str) -> &str {
    t.trim_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, ':' | '/' | '-' | '_' | '.' | '#')
    })
}

/// Does a token look like a target reference rather than a word? A URN, an IRI
/// or a path — a structural separator with content on BOTH sides.
///
/// The both-sides rule matters: prose writes lead-ins like `about to:` and
/// `intends to:`, whose trailing colon would otherwise make `to:` look like a
/// declared target and turn every prose intent into a mismatch.
fn looks_like_target(t: &str) -> bool {
    t.char_indices()
        .any(|(i, c)| (c == ':' || c == '/') && i > 0 && i + c.len_utf8() < t.len())
}

/// Parse an agent's declared intent into its verifiable components.
///
/// Recognises the structured `op=… target=…` form first (any order, any subset);
/// otherwise reads positionally: the first word-like token that is not a prose
/// lead-in is the operation, and the first target-shaped token is the target.
pub fn parse_intent(intent: &str) -> DeclaredIntent {
    let mut declared = DeclaredIntent::default();
    let tokens: Vec<&str> = intent.split_whitespace().collect();

    // --- structured form ---------------------------------------------------
    for raw in &tokens {
        let Some((key, value)) = raw.split_once('=') else {
            continue;
        };
        let key = trim_token(key).to_ascii_lowercase();
        let value = trim_token(value);
        if value.is_empty() {
            continue;
        }
        if OPERATION_KEYS.contains(&key.as_str()) {
            declared.operation = Some(value.to_ascii_lowercase());
        } else if TARGET_KEYS.contains(&key.as_str()) {
            declared.target = Some(value.to_string());
        }
    }
    if !declared.is_empty() {
        return declared;
    }

    // --- positional form ---------------------------------------------------
    for raw in &tokens {
        let t = trim_token(raw);
        if t.is_empty() {
            continue;
        }
        if declared.target.is_none() && looks_like_target(t) {
            declared.target = Some(t.to_string());
            continue;
        }
        if declared.operation.is_none()
            && t.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            && t.chars().any(|c| c.is_alphabetic())
            && !LEAD_INS.contains(&t.to_ascii_lowercase().as_str())
        {
            declared.operation = Some(t.to_ascii_lowercase());
        }
    }
    declared
}

/// Does `haystack` name `needle` as one or more WHOLE `:`/`/`-delimited
/// segments?
///
/// URNs, IRIs and paths are delimited identifiers, so a contained name must
/// start and end on a delimiter (or on the ends of the string) to have actually
/// been named. A plain `contains` would let `urn:kg:node-7` match
/// `urn:kg:node-70` — a DIFFERENT node — and both of those strings arrive on
/// the agent's own envelope, so the loose rule is an integrity hole and not
/// merely an imprecision: an agent could declare an intent against one id,
/// act on another whose id merely extends it, and have the divergence scored
/// as compliant.
///
/// Comparison is byte-exact on what it is given; callers that need
/// case-insensitivity lowercase both sides first (see [`target_matches`]).
/// This is the single implementation of the rule — `kpi_compute`'s
/// case-id correlation shares it.
pub fn urn_names_segment(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let is_delim = |c: char| c == ':' || c == '/';
    haystack.match_indices(needle).any(|(i, _)| {
        let before_ok = i == 0 || haystack[..i].chars().next_back().is_some_and(is_delim);
        let end = i + needle.len();
        let after_ok =
            end == haystack.len() || haystack[end..].chars().next().is_some_and(is_delim);
        before_ok && after_ok
    })
}

/// Operation comparison: case-insensitive containment in either direction, so a
/// declared `update` matches a recorded `graph_update` and vice versa. Both
/// sides are trimmed of the punctuation prose adds; neither is stemmed or
/// fuzzily compared.
///
/// Substring containment is right HERE and wrong for targets: action-type names
/// are `_`-joined words in which the declared verb is a genuine part
/// (`graph_update` really is an update), whereas a target id that merely
/// extends another names something else entirely.
fn operation_matches(declared: &str, recorded: &str) -> bool {
    let d = trim_token(declared).to_ascii_lowercase();
    let r = trim_token(recorded).to_ascii_lowercase();
    if d.is_empty() || r.is_empty() {
        return false;
    }
    d.contains(&r) || r.contains(&d)
}

/// Target comparison: case-insensitive, but on WHOLE delimited segments in
/// either direction (see [`urn_names_segment`]).
///
/// Either direction, because an agent may legitimately declare a coarser target
/// than the act records (`urn:kg` for an act on `urn:kg:node-7`) or a bare
/// local id where the act recorded the full URN — both are claims the record
/// bears out. What neither direction permits is a partial segment: a declared
/// `urn:kg:node` names no part of `urn:kg:node-7`.
fn target_matches(declared: &str, recorded: &str) -> bool {
    let d = trim_token(declared).to_ascii_lowercase();
    let r = trim_token(recorded).to_ascii_lowercase();
    if d.is_empty() || r.is_empty() {
        return false;
    }
    urn_names_segment(&r, &d) || urn_names_segment(&d, &r)
}

/// Compare an agent's declared intent with the act actually recorded for it.
///
/// * `None` — the agent declared nothing verifiable (no intent, blank, or an
///   intent from which no operation or target could be read). Never a verdict.
/// * `Some(true)` — every component the agent declared appears in the record.
/// * `Some(false)` — a declared component is absent from, or differs from, the
///   record.
///
/// See the module docs for the full rule table.
pub fn intent_match(
    intent: Option<&str>,
    action_type_name: Option<&str>,
    target_urn: Option<&str>,
) -> Option<bool> {
    let intent = intent?;
    if intent.trim().is_empty() {
        return None;
    }
    let declared = parse_intent(intent);
    if declared.is_empty() {
        return None;
    }

    if let Some(op) = declared.operation.as_deref() {
        match action_type_name {
            Some(recorded) if operation_matches(op, recorded) => {}
            // Declared but absent from — or contradicted by — the record.
            _ => return Some(false),
        }
    }
    if let Some(target) = declared.target.as_deref() {
        match target_urn {
            Some(recorded) if target_matches(target, recorded) => {}
            _ => return Some(false),
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_intent_is_unknown_never_a_verdict() {
        assert_eq!(
            intent_match(None, Some("graph_update"), Some("urn:a")),
            None
        );
        assert_eq!(intent_match(Some("   "), Some("graph_update"), None), None);
    }

    #[test]
    fn declared_operation_and_target_both_found_is_a_match() {
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            (
                "update urn:visionclaw:kg:aaaa:node-7",
                Some("graph_update"),
                Some("urn:visionclaw:kg:aaaa:node-7"),
            ),
            (
                "op=update target=urn:kg:node-7",
                Some("update"),
                Some("urn:kg:node-7"),
            ),
            (
                "operation=Create target=URN:KG:Node-7",
                Some("create_class"),
                Some("urn:kg:node-7"),
            ),
            (
                "about to: elevate urn:ngm:class:Foo",
                Some("elevate"),
                Some("urn:ngm:class:Foo"),
            ),
        ];
        for (intent, atn, target) in cases {
            assert_eq!(
                intent_match(Some(intent), *atn, *target),
                Some(true),
                "intent {intent:?} should match {atn:?}/{target:?}"
            );
        }
    }

    #[test]
    fn a_declared_target_that_differs_is_a_mismatch() {
        assert_eq!(
            intent_match(
                Some("update urn:kg:node-7"),
                Some("graph_update"),
                Some("urn:kg:node-99")
            ),
            Some(false)
        );
    }

    #[test]
    fn a_declared_operation_that_differs_is_a_mismatch() {
        assert_eq!(
            intent_match(
                Some("delete urn:kg:node-7"),
                Some("graph_update"),
                Some("urn:kg:node-7")
            ),
            Some(false)
        );
    }

    #[test]
    fn a_declared_component_with_nothing_recorded_cannot_match() {
        // The agent said what it was about to do; the act recorded no such
        // field. That is a mismatch, not an unknown — the intent WAS declared.
        assert_eq!(
            intent_match(Some("update urn:kg:node-7"), None, Some("urn:kg:node-7")),
            Some(false)
        );
        assert_eq!(
            intent_match(Some("update urn:kg:node-7"), Some("graph_update"), None),
            Some(false)
        );
    }

    #[test]
    fn an_operation_only_intent_matches_on_the_operation_alone() {
        // Only DECLARED components are required to match: an intent naming no
        // target makes no claim about the target.
        assert_eq!(
            intent_match(
                Some("update"),
                Some("graph_update"),
                Some("urn:kg:anything")
            ),
            Some(true)
        );
        assert_eq!(
            intent_match(
                Some("update"),
                Some("graph_delete"),
                Some("urn:kg:anything")
            ),
            Some(false)
        );
    }

    #[test]
    fn an_unparseable_intent_declares_nothing_and_cannot_be_verified() {
        // Punctuation only: nothing was actually declared, so there is no claim
        // to verify. Honest answer is "unknown", never a fabricated verdict.
        assert_eq!(
            intent_match(Some("!!! ???"), Some("graph_update"), Some("urn:a")),
            None
        );
    }

    #[test]
    fn parse_splits_operation_from_target() {
        let d = parse_intent("update urn:kg:node-7");
        assert_eq!(d.operation.as_deref(), Some("update"));
        assert_eq!(d.target.as_deref(), Some("urn:kg:node-7"));

        let d = parse_intent("op=elevate target=urn:ngm:class:Foo");
        assert_eq!(d.operation.as_deref(), Some("elevate"));
        assert_eq!(d.target.as_deref(), Some("urn:ngm:class:Foo"));

        let d = parse_intent("rebalance");
        assert_eq!(d.operation.as_deref(), Some("rebalance"));
        assert_eq!(d.target, None);
    }

    #[test]
    fn a_declared_target_matches_on_whole_segments_not_substrings() {
        // urn:kg:node-7 is a DIFFERENT node from urn:kg:node-70. A plain
        // substring check calls this a match and so reports the agent did what
        // it declared when it demonstrably did not — and, because both strings
        // come off the agent's own envelope, lets an agent launder a divergent
        // act past HITL Precision by declaring a target whose id is a prefix of
        // the one it actually touched.
        assert_eq!(
            intent_match(
                Some("update urn:kg:node-7"),
                Some("graph_update"),
                Some("urn:kg:node-70")
            ),
            Some(false)
        );
        // The same declaration against the node actually named still matches.
        assert_eq!(
            intent_match(
                Some("update urn:kg:node-7"),
                Some("graph_update"),
                Some("urn:kg:node-7")
            ),
            Some(true)
        );
        // A declared target naming only a LEADING RUN OF WHOLE SEGMENTS is a
        // genuine (if coarse) claim about the act, and still matches...
        assert_eq!(
            intent_match(
                Some("update urn:kg"),
                Some("graph_update"),
                Some("urn:kg:node-7")
            ),
            Some(true)
        );
        // ...but a prefix that stops part-way THROUGH a segment names nothing.
        assert_eq!(
            intent_match(
                Some("update urn:kg:node"),
                Some("graph_update"),
                Some("urn:kg:node-7")
            ),
            Some(false)
        );
    }

    #[test]
    fn matching_is_case_insensitive_and_tolerates_affixes() {
        assert_eq!(
            intent_match(
                Some("UPDATE URN:KG:NODE-7"),
                Some("graph_update"),
                Some("urn:kg:node-7")
            ),
            Some(true)
        );
    }
}
