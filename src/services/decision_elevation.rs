//! Decision elevation — the inverse corpus path (ADR-050).
//!
//! The symmetric image of the class-elevation loop ([`crate::actors::elevation_actor`]):
//! a governed [`DecisionRecord`](crate::services::decision_service::DecisionInput)
//! is born in `urn:ngm:graph:ontology:assert` at runtime through the governed
//! decision write door, but that graph is periodically CLEAR+INSERT-rebuilt from
//! the corpus on a `force_full` sync ([`crate::services::github_sync_service`]).
//! A runtime decision is born-in-the-graph / absent-from-source, so the rebuild
//! would erase it. This module routes a **significant** decision *into the
//! corpus* — drafted as a page, gated through the broker, PR'd to the corpus
//! repository (the visionGraph vault, `GITHUB_OWNER`/`GITHUB_REPO`) on approve — so the same sync→rebuild path that re-derives every class
//! re-derives the decision too.
//!
//! This module is the PURE half (no actor, no I/O): the significance predicate,
//! the page drafter/parser (a byte-faithful inverse pair), the `dl:` quad
//! re-materialiser, and the fire-and-forget [`DecisionElevationSink`] seam the
//! governed write door calls. The actor half — the broker case open, the
//! approve→PR commit and GOV-2 poll — lives in
//! [`crate::actors::decision_elevation_actor`] (which implements the sink),
//! mirroring the class-elevation `ElevationActor` shape (reuse, not reinvent).
//!
//! ## What lands where (ADR-049 boundary, preserved)
//!
//! The corpus page carries the decision SUMMARY plus OKF frontmatter — the
//! `type: Individual` / `resource: <decision URN>` node typing, the direct
//! causal edges as vocabulary-declared relation keys, and a *provenance
//! summary* (the `did:nostr` attribution + `generatedAtTime`). It does NOT carry
//! the signed envelope — the authoritative signed PROV-O attribution stays in the
//! `:provenance` graph. Re-materialisation ([`decision_page_quads`]) therefore
//! emits ONLY the asserted `dl:` quads via [`build_decision_quads`], never
//! attribution — exactly the asserted-graph projection the runtime write door
//! produces.

use log::warn;
use oxigraph::model::Quad;
use visionclaw_domain::vault;

use crate::services::decision_service::{build_decision_quads, DecisionInput};

/// Corpus namespace for elevated decision pages. Mirrors the class-elevation
/// convention (`knowledge/pages/…`) with a dedicated `decisions/`
/// sub-namespace so a `force_full` read-half can list them cheaply.
///
/// Repo-root-relative, matching the prefixes [`CorpusSource::list_pages_under`]
/// takes — `knowledge/pages` is the governed role's page root in the
/// visionGraph `vault.toml`. The old `mainKnowledgeGraph/pages/decisions`
/// named a corpus layout that no longer exists (PRD-sovereign-corpus Q1: the
/// canonical vault is `visionGraph`, `knowledge/` + `working/`); against the
/// real corpus it resolved to nothing, so the ADR-050 read-half silently
/// re-materialised zero decision records on every sync.
pub const DECISIONS_DIR: &str = "knowledge/pages/decisions";

/// The governed vault's page root — `$VAULT_ROOT/knowledge/pages`, the prefix
/// [`DECISIONS_DIR`] and the class-elevation writer both live under.
///
/// The old `mainKnowledgeGraph/pages` named a corpus layout that no longer
/// exists (PRD-sovereign-corpus Q1), so every writer that still used it was
/// writing to a path the corpus source never lists.
pub const KNOWLEDGE_PAGES_DIR: &str = "knowledge/pages";

// ---------------------------------------------------------------------------
// Significance predicate (ADR-050 DECIDED: broker-gated, significant-only)
// ---------------------------------------------------------------------------

/// The elevation-significance predicate. A decision elevates to the public
/// corpus IFF it is *significant*; routine/edgeless decisions stay runtime-only
/// so the public corpus stays high-signal and the broker volume stays bounded.
///
/// Significant ⇔ **any** of the three signals already present at decision time:
///   1. it governed a graph mutation — carries a `proposal_urn`
///      (`urn:agentbox:activity:…`), i.e. the decision was *about* a change;
///   2. it carries any causal edge — a non-empty `dl:caused` / `dl:precedentFor`
///      / `dl:influenced` set (it shaped the decision graph);
///   3. it was ACSP-approved (`acsp_approved`) — a governance verdict was recorded.
///
/// `dl:consideredInput` / `dl:governedBy` are NOT significance signals on their
/// own (a decision can weigh inputs / cite a policy without being consequential);
/// an edgeless, proposal-less, un-approved decision is routine.
pub fn is_significant(input: &DecisionInput, acsp_approved: bool) -> bool {
    input.proposal_urn.is_some()
        || !input.caused.is_empty()
        || !input.precedent_for.is_empty()
        || !input.influenced.is_empty()
        || acsp_approved
}

// ---------------------------------------------------------------------------
// The elevation payload + the fire-and-forget sink
// ---------------------------------------------------------------------------

/// Everything the elevation path needs about a just-recorded decision. Built by
/// the governed write door and handed to the [`DecisionElevationSink`]; enough to
/// draft the corpus page and open the broker case without re-reading the store.
#[derive(Debug, Clone)]
pub struct ElevatedDecision {
    /// The minted `urn:agentbox:decision:<pubkey>:sha256-12-<hex>` — the page `@id`.
    pub decision_urn: String,
    /// The direct decision claims (summary/rationale + the 5 dl: edge sets).
    pub input: DecisionInput,
    /// `did:nostr:<hex>` of the deciding principal — the provenance-summary
    /// attribution on the page (NOT the signed envelope).
    pub agent_did: String,
    /// RFC-3339 activity timestamp for the provenance summary.
    pub generated_at: String,
    /// Whether an ACSP governance verdict approved the act (a significance signal).
    pub acsp_approved: bool,
}

/// The fire-and-forget seam the governed decision write door
/// ([`crate::services::decision_service::DecisionService`]) calls after a
/// SIGNIFICANT decision commits. Implemented by the actor adapter in
/// [`crate::actors::decision_elevation_actor`].
///
/// CONTRACT (ADR-050 fail-open): `elevate` MUST NOT block and MUST NOT be able to
/// fail the governed decision write. It returns `Result` only to report a
/// *synchronous enqueue* failure (e.g. the actor mailbox is gone), which the
/// caller logs and ignores — a broker/PR outage or missing token never blocks or
/// rolls back the decision itself.
pub trait DecisionElevationSink: Send + Sync {
    fn elevate(&self, decision: ElevatedDecision) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// Slug / page drafting (mirror of draft_class_page)
// ---------------------------------------------------------------------------

/// The content-address tail (`sha256-12-<12hex>` → `<12hex>`) of a decision URN,
/// used to make the corpus slug collision-proof and the page path deterministic.
fn urn_hash_tail(decision_urn: &str) -> String {
    decision_urn
        .rsplit("sha256-12-")
        .next()
        .map(|s| s.chars().take(12).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Slugify to the corpus convention (mirrors the elevation slugifier).
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = true;
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Deterministic corpus slug for a decision: `<summary-slug>-<12hex>`. The URN
/// hash tail guarantees uniqueness even when two decisions share a summary.
pub fn decision_slug(decision_urn: &str, summary: &str) -> String {
    let base = slugify(summary);
    let base = if base.is_empty() {
        "decision".to_string()
    } else {
        base
    };
    // Bound the summary component so paths stay sane.
    let base: String = base.chars().take(64).collect();
    let base = base.trim_end_matches('-');
    format!("{base}-{}", urn_hash_tail(decision_urn))
}

/// The vocabulary-governed frontmatter relation key each `dl:` decision edge is
/// authored as, and the order they are emitted in.
///
/// PRD-sovereign-corpus Q4/Q5: the ontology lives in typed Obsidian Properties,
/// and a relation key is valid only if `ontology/vocabulary.yaml` declares it.
/// None of the `dl:` predicate names are declared, so each edge set is authored
/// as the declared relation that carries the same meaning:
///
/// | decision edge | frontmatter key | vocabulary `owl:` |
/// |---|---|---|
/// | `dl:caused`          | `causes`       | `vc:causes` |
/// | `dl:precedentFor`    | `precedes`     | `vc:precedes` |
/// | `dl:influenced`      | `influences`   | `vc:influences` |
/// | `dl:consideredInput` | `informed-by`  | `vc:informedBy` |
/// | `dl:governedBy`      | `regulated-by` | `vc:regulatedBy` |
///
/// The mapping is a *projection for authoring only*. The assert-graph quads are
/// still built by [`build_decision_quads`] from the recovered
/// [`DecisionInput`], so the `dl:` predicates the governed write door emits are
/// unchanged — this table decides how the edge is written down, never what it
/// means in the graph.
pub const DECISION_RELATION_KEYS: [&str; 5] = [
    "causes",
    "precedes",
    "influences",
    "informed-by",
    "regulated-by",
];

/// The OKF actor that stamps a drafted decision page (`okf.actors.process`).
fn drafting_process() -> String {
    format!("process:visionclaw/{}", env!("CARGO_PKG_VERSION"))
}

/// Author a relation target as a wikilink.
///
/// A decision edge points at a URN, not at a page, and contract C1's long-tail
/// rule is to write the reference as a wikilink whose target text is the thing
/// itself — so the URN survives verbatim and the read half recovers it by
/// stripping the brackets. An empty edge set emits no key at all rather than an
/// empty list: absence and "explicitly nothing" are the same fact here, and the
/// shorter page is the one a human reviews.
fn wikilinks(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| format!("[[{t}]]"))
        .collect()
}

/// Recover the targets of a relation key written by [`wikilinks`].
fn unwikilink(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|s| {
            s.trim()
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Draft the canonical corpus page for a significant decision — the inverse of
/// [`crate::actors::elevation_actor::draft_class_page`], for `dl:DecisionRecord`
/// instances. Emits `knowledge/pages/decisions/<slug>.md` as **frontmatter
/// only** (PRD-sovereign-corpus Q4: no ```json-ld fence; `vault validate` lists
/// a fence under `rejected_constructs`):
///
///   * `type: Individual` — a decision record is a `prov:Activity` instance,
///     not a class, and the ontology build keys the bundle on this;
///   * `resource` = the decision URN (the old json-ld `@id`);
///   * `status: draft` — an elevated decision is a proposal until a human 31403
///     promotes it;
///   * `generated: {by: process:visionclaw/<version>, at: <RFC3339>}` — the OKF
///     trust stamp, carrying the *summary* provenance only;
///   * the five causal edge sets as declared relation keys
///     ([`DECISION_RELATION_KEYS`]);
///   * `public: true`, as before — without it the §V4 gate reads every drafted
///     decision as private and the record never reaches the knowledge graph.
///
/// ## What is deliberately NOT here (ADR-049 boundary, preserved)
///
/// The signed PROV-O envelope. The page carries the `did:nostr` attribution as
/// the `generated.by`-adjacent `decided-by` scalar and `prov:generatedAtTime`
/// as `generated.at`; the authoritative *signed* attribution stays in the
/// `:provenance` graph. Re-materialisation emits only asserted `dl:` quads.
///
/// Returns `(file_path, markdown)`.
pub fn draft_decision_page(dec: &ElevatedDecision) -> (String, String) {
    let slug = decision_slug(&dec.decision_urn, &dec.input.summary);
    let file_path = format!("{DECISIONS_DIR}/{slug}.md");

    let heading = if dec.input.summary.trim().is_empty() {
        "Decision".to_string()
    } else {
        dec.input.summary.trim().to_string()
    };

    let mut extra_lists = std::collections::BTreeMap::new();
    let edge_sets: [&[String]; 5] = [
        &dec.input.caused,
        &dec.input.precedent_for,
        &dec.input.influenced,
        &dec.input.considered_inputs,
        &dec.input.governed_by,
    ];
    for (key, targets) in DECISION_RELATION_KEYS.iter().zip(edge_sets) {
        let links = wikilinks(targets);
        if !links.is_empty() {
            extra_lists.insert((*key).to_string(), links);
        }
    }

    let meta = vault::PageMeta {
        public: true,
        page_type: Some(DECISION_PAGE_TYPE.to_string()),
        resource: Some(dec.decision_urn.clone()),
        status: Some("draft".to_string()),
        // The provenance SUMMARY (ADR-049). `by` is the deciding principal as
        // an OKF `did:` actor — the attribution the record carries; `rule` is
        // the process that wrote the page down. Neither is the signed envelope.
        generated: Some(vault::GeneratedStamp {
            by: dec.agent_did.clone(),
            at: dec.generated_at.clone(),
            rule: Some(drafting_process()),
        }),
        title: Some(heading.clone()),
        extra_lists,
        ..vault::PageMeta::default()
    };

    // The rationale has no declared frontmatter home, so it goes where
    // vocabulary.yaml's own migration rule C puts such a value: into the body
    // as `**key:** value`. It is human-readable, corpus-only and never
    // asserted as a quad, so prose loses nothing.
    let rationale = dec.input.rationale.trim();
    let rationale_line = if rationale.is_empty() {
        String::new()
    } else {
        format!("**{RATIONALE_LABEL}:** {rationale}\n\n")
    };

    let body = format!(
        "# {heading}\n\n{rationale_line}\
         > Elevated decision record (ADR-050). Re-derived into \
         `urn:ngm:graph:ontology:assert` on the next corpus sync. Signed \
         attribution lives in the provenance graph; this page carries the summary.\n"
    );

    (file_path, vault::render_page(&meta, &body))
}

/// `type` of a decision-record page: a `prov:Activity` **instance**, so the OKF
/// type is `Individual` — it is never an ontology class.
pub const DECISION_PAGE_TYPE: &str = "Individual";

/// Body label carrying the human-readable rationale (corpus-only; never
/// asserted as a quad). Matches vocabulary.yaml migration rule C's `**key:**`
/// prose convention, so a human editing the page in Obsidian sees the same
/// shape a migrated `rationale::` line produces.
const RATIONALE_LABEL: &str = "Rationale";

// ---------------------------------------------------------------------------
// Page parsing / re-materialisation (the read-half recogniser)
// ---------------------------------------------------------------------------

/// A decision recognised from a corpus page — the parser's node typing of a
/// `dl:DecisionRecord` json-ld block back into the pieces the assert graph needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDecision {
    pub decision_urn: String,
    /// The direct claims recovered from the page (summary/rationale + edge sets).
    /// `proposal_urn` is intentionally `None`: it is a provenance concern and is
    /// not an asserted edge, so it is not carried on the corpus page.
    pub input: DecisionInput,
    /// The provenance-summary attribution, if the page carried one.
    pub agent_did: Option<String>,
}

/// Recognise an elevated decision page and recover its [`ParsedDecision`] —
/// the byte-faithful inverse of [`draft_decision_page`].
///
/// The node typing is now the OKF frontmatter, not a json-ld `@type`: a page is
/// a decision record IFF `type: Individual` and its `resource` is a decision
/// URN (`urn:agentbox:decision:…`). That second half matters — `Individual` is
/// the OKF type of *every* named individual, so without the URN check the read
/// half would claim every individual page in `knowledge/` as a decision.
///
/// Returns `None` for a page with no frontmatter, the wrong `type`, a missing
/// or non-decision `resource` — so class pages and ordinary individuals are
/// left to the class-rebuild path untouched.
pub fn parse_decision_page(markdown: &str) -> Option<ParsedDecision> {
    let meta = vault::parse(markdown);

    if meta.page_type.as_deref() != Some(DECISION_PAGE_TYPE) {
        return None;
    }
    let decision_urn = meta.resource.clone()?;
    if !decision_urn.starts_with(DECISION_URN_PREFIX) {
        return None;
    }

    let edges = |key: &str| {
        meta.extra_lists
            .get(key)
            .map(|items| unwikilink(items))
            .unwrap_or_default()
    };

    let input = DecisionInput {
        summary: meta.title.clone().unwrap_or_default(),
        rationale: parse_rationale(markdown),
        proposal_urn: None,
        caused: edges(DECISION_RELATION_KEYS[0]),
        precedent_for: edges(DECISION_RELATION_KEYS[1]),
        influenced: edges(DECISION_RELATION_KEYS[2]),
        considered_inputs: edges(DECISION_RELATION_KEYS[3]),
        governed_by: edges(DECISION_RELATION_KEYS[4]),
    };

    // ADR-049 provenance summary: the deciding principal, not the process that
    // wrote the page — `generated.rule` holds that.
    let agent_did = meta.generated.as_ref().map(|g| g.by.clone());

    Some(ParsedDecision {
        decision_urn,
        input,
        agent_did,
    })
}

/// Recover the `**Rationale:** …` prose line [`draft_decision_page`] writes.
/// Returns an empty string when the page carries none (a rationale-less
/// decision is legal; inventing one would be worse than recording none).
fn parse_rationale(markdown: &str) -> String {
    let prefix = format!("**{RATIONALE_LABEL}:**");
    markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

/// URN prefix that distinguishes a decision record from any other OKF
/// `Individual`. Minted by the governed decision write door as
/// `urn:agentbox:decision:<pubkey>:sha256-12-<hex>`.
const DECISION_URN_PREFIX: &str = "urn:agentbox:decision:";

/// Re-materialise a decision corpus page into its `urn:ngm:graph:ontology:assert`
/// quads — the type memberships + direct `dl:` edges, and NOTHING ELSE (no
/// attribution: that stays in `:provenance`, ADR-049). Reuses the runtime write
/// door's [`build_decision_quads`] so the rebuilt projection is byte-identical to
/// the one the governed door produces. Returns an empty vec for a non-decision
/// page (so the caller can fold it over every synced page cheaply).
pub fn decision_page_quads(markdown: &str) -> Vec<Quad> {
    match parse_decision_page(markdown) {
        Some(parsed) => build_decision_quads(&parsed.decision_urn, &parsed.input),
        None => {
            // Not a decision page — nothing to re-materialise. (Logged at trace
            // by callers that expect one; silent here so bulk folds stay quiet.)
            Vec::new()
        }
    }
}

/// Best-effort helper for callers that fold over many pages: parse + build,
/// logging (not failing) a page that types as a decision record but yields no
/// quads. Kept separate from [`decision_page_quads`] so the silent bulk path and
/// the diagnostic path are both available.
pub fn decision_page_quads_logged(markdown: &str, source: &str) -> Vec<Quad> {
    let quads = decision_page_quads(markdown);
    if quads.is_empty() && parse_decision_page(markdown).is_some() {
        warn!("[DecisionElevation] decision page '{source}' parsed but produced no assert quads");
    }
    quads
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::decision_service::{
        p_caused, p_governed_by, p_influenced, p_precedent_for, PROV_ACTIVITY, RDF_TYPE,
    };

    const PK: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn sample(input: DecisionInput) -> ElevatedDecision {
        let urn = format!("urn:agentbox:decision:{PK}:sha256-12-9ec3d090ff23");
        ElevatedDecision {
            decision_urn: urn,
            input,
            agent_did: format!("did:nostr:{PK}"),
            generated_at: "2026-08-08T00:00:00Z".to_string(),
            acsp_approved: false,
        }
    }

    // --- (b) significance predicate ---------------------------------------

    #[test]
    fn routine_edgeless_decision_is_not_significant() {
        // No proposal, no causal edges, not ACSP-approved → routine → runtime-only.
        let routine = DecisionInput {
            summary: "note a routine observation".into(),
            rationale: "nothing structural".into(),
            proposal_urn: None,
            considered_inputs: vec!["urn:agentbox:activity:src".into()], // input alone ≠ significant
            governed_by: vec!["urn:agentbox:decision:policy".into()], // cited policy alone ≠ significant
            ..Default::default()
        };
        assert!(!is_significant(&routine, false));
    }

    #[test]
    fn causal_or_mutation_or_approved_decision_is_significant() {
        // 1. carries a causal edge.
        let causal = DecisionInput {
            summary: "merge duplicate concepts".into(),
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            ..Default::default()
        };
        assert!(is_significant(&causal, false));

        // 2. governed a graph mutation (has a proposal_urn).
        let mutation = DecisionInput {
            summary: "adopt the delta-scoped gate".into(),
            proposal_urn: Some("urn:agentbox:activity:abc".into()),
            ..Default::default()
        };
        assert!(is_significant(&mutation, false));

        // 3. ACSP-approved.
        let approved = DecisionInput {
            summary: "a plain decision".into(),
            ..Default::default()
        };
        assert!(!is_significant(&approved, false));
        assert!(is_significant(&approved, true));

        // precedentFor / influenced also qualify.
        let precedent = DecisionInput {
            precedent_for: vec!["urn:agentbox:decision:AA:sha256-12-ghi".into()],
            ..Default::default()
        };
        assert!(is_significant(&precedent, false));
        let influenced = DecisionInput {
            influenced: vec!["urn:agentbox:decision:AA:sha256-12-jkl".into()],
            ..Default::default()
        };
        assert!(is_significant(&influenced, false));
    }

    // --- (a) draft round-trips to a parseable page with dl: @type + edges --

    #[test]
    fn draft_decision_page_path_is_namespaced_and_deterministic() {
        let dec = sample(DecisionInput {
            summary: "Merge Duplicate Concepts".into(),
            rationale: "resolves DUPLICATE_CONCEPT".into(),
            ..Default::default()
        });
        let (path, _) = draft_decision_page(&dec);
        // Repo-root-relative under the governed vault role, so the write-half's
        // path and the read-half's `list_pages_under(DECISIONS_DIR)` prefix are
        // the same string against the real visionGraph corpus.
        assert!(path.starts_with("knowledge/pages/decisions/"));
        assert_eq!(DECISIONS_DIR, "knowledge/pages/decisions");
        assert!(path.ends_with(".md"));
        // Deterministic: same decision → same path.
        let (path2, _) = draft_decision_page(&dec);
        assert_eq!(path, path2);
        // Slug carries the URN hash tail for collision-safety.
        assert!(
            path.contains("9ec3d090ff23"),
            "path lacks URN hash tail: {path}"
        );
    }

    #[test]
    fn draft_round_trips_type_edges_and_urn() {
        let input = DecisionInput {
            summary: "merge duplicate concepts".into(),
            rationale: "resolves DUPLICATE_CONCEPT".into(),
            proposal_urn: Some("urn:agentbox:activity:abc".into()),
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            precedent_for: vec!["urn:agentbox:decision:AA:sha256-12-ghi".into()],
            influenced: vec!["urn:agentbox:decision:AA:sha256-12-jkl".into()],
            considered_inputs: vec!["urn:agentbox:activity:in".into()],
            governed_by: vec!["urn:agentbox:decision:policy".into()],
        };
        let dec = sample(input.clone());
        let (_, markdown) = draft_decision_page(&dec);

        // The block parses and is typed as a dl:DecisionRecord.
        let parsed = parse_decision_page(&markdown).expect("decision page parses");
        assert_eq!(parsed.decision_urn, dec.decision_urn);
        assert_eq!(parsed.agent_did.as_deref(), Some(dec.agent_did.as_str()));

        // Every direct edge set round-trips (proposal_urn is provenance-only).
        assert_eq!(parsed.input.caused, input.caused);
        assert_eq!(parsed.input.precedent_for, input.precedent_for);
        assert_eq!(parsed.input.influenced, input.influenced);
        assert_eq!(parsed.input.considered_inputs, input.considered_inputs);
        assert_eq!(parsed.input.governed_by, input.governed_by);
        assert_eq!(parsed.input.summary, input.summary);
        assert_eq!(parsed.input.rationale, input.rationale);
        assert_eq!(
            parsed.input.proposal_urn, None,
            "proposal_urn is not on the corpus page"
        );

        // Q4: the node typing is frontmatter, and there is no fence to read.
        assert!(!markdown.contains("```"), "no json-ld fence: {markdown}");
        let meta = vault::parse(&markdown);
        assert_eq!(meta.page_type.as_deref(), Some(DECISION_PAGE_TYPE));
        assert_eq!(meta.resource.as_deref(), Some(dec.decision_urn.as_str()));
        assert_eq!(meta.status.as_deref(), Some("draft"));

        // Every edge is a wikilink list under a vocabulary-DECLARED key.
        for key in DECISION_RELATION_KEYS {
            let items = meta.extra_lists.get(key).expect("edge key present");
            assert!(
                items
                    .iter()
                    .all(|i| i.starts_with("[[") && i.ends_with("]]")),
                "{key} targets are wikilinks: {items:?}"
            );
        }
        assert_eq!(
            meta.extra_lists.get("causes"),
            Some(&vec![format!("[[{}]]", input.caused[0])])
        );

        // The OKF trust stamp carries the drafting process and the activity time.
        let stamp = meta.generated.as_ref().expect("generated stamp");
        assert_eq!(stamp.by, dec.agent_did, "attributed to the principal");
        assert_eq!(stamp.at, dec.generated_at);
        assert!(stamp
            .rule
            .as_deref()
            .is_some_and(|r| r.starts_with("process:visionclaw/")));
    }

    #[test]
    fn every_relation_key_the_drafter_emits_is_declared_in_the_vocabulary() {
        // Contract C1 / PRD Q5: an undeclared frontmatter key fails
        // `vault validate` on a `knowledge/` page, so the writer may only reach
        // for keys `ontology/vocabulary.yaml` declares. Pinned here because the
        // vocabulary is read-only to this crate and a drift is silent otherwise.
        const DECLARED_RELATIONS: [&str; 5] = [
            // `vc:causes`, `vc:precedes`, `vc:influences`, `vc:informedBy`,
            // `vc:regulatedBy` — all present in vocabulary.yaml `relations:`.
            "causes",
            "precedes",
            "influences",
            "informed-by",
            "regulated-by",
        ];
        assert_eq!(DECISION_RELATION_KEYS, DECLARED_RELATIONS);
    }

    #[test]
    fn an_edgeless_decision_emits_no_empty_relation_keys() {
        let dec = sample(DecisionInput {
            summary: "quiet decision".into(),
            rationale: "r".into(),
            ..Default::default()
        });
        let (_, markdown) = draft_decision_page(&dec);
        let meta = vault::parse(&markdown);
        for key in DECISION_RELATION_KEYS {
            assert!(
                !meta.extra_lists.contains_key(key),
                "{key} should be absent, not an empty list"
            );
        }
        // …and the page still round-trips to an edgeless decision.
        let parsed = parse_decision_page(&markdown).expect("parses");
        assert!(parsed.input.caused.is_empty());
        assert_eq!(parsed.decision_urn, dec.decision_urn);
    }

    #[test]
    fn decision_page_quads_match_the_runtime_write_door_projection() {
        let input = DecisionInput {
            summary: "s".into(),
            rationale: "r".into(),
            proposal_urn: Some("urn:agentbox:activity:p".into()),
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            precedent_for: vec![],
            influenced: vec!["urn:agentbox:decision:AA:sha256-12-jkl".into()],
            considered_inputs: vec![],
            governed_by: vec!["urn:agentbox:decision:policy".into()],
        };
        let dec = sample(input.clone());
        let (_, markdown) = draft_decision_page(&dec);

        let quads = decision_page_quads(&markdown);
        // Two type quads + caused(1) + influenced(1) + governedBy(1) = 5.
        assert_eq!(quads.len(), 5, "re-materialised quad count");

        let preds: Vec<String> = quads
            .iter()
            .map(|q| q.predicate.as_str().to_string())
            .collect();
        assert_eq!(preds.iter().filter(|p| *p == RDF_TYPE).count(), 2);
        assert_eq!(preds.iter().filter(|p| **p == p_caused()).count(), 1);
        assert_eq!(preds.iter().filter(|p| **p == p_influenced()).count(), 1);
        assert_eq!(preds.iter().filter(|p| **p == p_governed_by()).count(), 1);
        assert_eq!(preds.iter().filter(|p| **p == p_precedent_for()).count(), 0);

        // NO attribution leaks into the asserted projection (ADR-049 boundary).
        for q in &quads {
            let p = q.predicate.as_str();
            assert!(!p.contains("wasAssociatedWith"));
            assert!(!p.contains("generatedAtTime"));
        }
        // Type memberships are exactly prov:Activity + dl:DecisionRecord.
        let types: Vec<String> = quads
            .iter()
            .filter(|q| q.predicate.as_str() == RDF_TYPE)
            .map(|q| q.object.to_string())
            .collect();
        assert!(types.iter().any(|o| o.contains(PROV_ACTIVITY)));
        assert!(types.iter().any(|o| o.contains("DecisionRecord")));
    }

    #[test]
    fn drafted_page_is_vault_frontmatter_and_passes_the_inclusion_gate() {
        // ADR-2040 §V5 + Invariant 2: the drafted page carried no metadata
        // carrier at all before this change, so the §V4 gate would have read
        // every elevated decision as private and dropped it from the graph.
        let dec = sample(DecisionInput {
            summary: "merge duplicate concepts".into(),
            rationale: "resolves DUPLICATE_CONCEPT".into(),
            proposal_urn: None,
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            precedent_for: vec![],
            influenced: vec![],
            considered_inputs: vec![],
            governed_by: vec![],
        });
        let (_, markdown) = draft_decision_page(&dec);

        assert!(markdown.starts_with("---\n"));
        assert!(
            !markdown.contains(":: "),
            "no writer emits `key:: value` lines (Invariant 1)"
        );
        assert!(
            !markdown.contains("```"),
            "a ```json-ld fence is a vocabulary `rejected_construct` (PRD Q4)"
        );

        let meta = vault::parse(&markdown);
        assert!(meta.public);
        assert!(meta.is_kg_included());
        assert_eq!(meta.title.as_deref(), Some("merge duplicate concepts"));

        // The frontmatter alone carries the record, so the read half round-trips.
        let parsed = parse_decision_page(&markdown).expect("decision page parses");
        assert_eq!(parsed.decision_urn, dec.decision_urn);
        assert_eq!(parsed.input.caused, dec.input.caused);
    }

    #[test]
    fn a_summaryless_decision_gets_no_title_key() {
        let dec = sample(DecisionInput {
            summary: "   ".into(),
            rationale: "r".into(),
            proposal_urn: None,
            caused: vec![],
            precedent_for: vec![],
            influenced: vec![],
            considered_inputs: vec![],
            governed_by: vec![],
        });
        let (_, markdown) = draft_decision_page(&dec);
        let meta = vault::parse(&markdown);

        assert!(meta.public, "still ingests");
        // `title` is a required knowledge key, so the placeholder is written —
        // but it is the placeholder, not a summary the decision never had.
        assert_eq!(meta.title.as_deref(), Some("Decision"));
        assert!(markdown.contains("# Decision"));
        // The record is still recoverable: identity lives in `resource`.
        let parsed = parse_decision_page(&markdown).expect("parses");
        assert_eq!(parsed.decision_urn, dec.decision_urn);
        assert_eq!(parsed.input.summary, "Decision");
    }

    #[test]
    fn non_decision_page_is_ignored_by_the_read_half() {
        // A class page (the class-elevation output) must NOT be re-materialised here.
        let class_page =
            "---\ntype: Class\nresource: urn:ngm:class:camera\nstatus: draft\n---\n\n# Camera\n";
        assert!(parse_decision_page(class_page).is_none());
        assert!(decision_page_quads(class_page).is_empty());

        // An ordinary OKF Individual is NOT a decision: the `type` alone is not
        // the node typing — the `resource` must be a decision URN. Without this
        // the read half would claim every named individual in the corpus.
        let individual = "---\ntype: Individual\nresource: urn:ngm:individual:mature\nstatus: stable\n---\n\n# Mature\n";
        assert!(parse_decision_page(individual).is_none());
        assert!(decision_page_quads(individual).is_empty());

        // A page with no metadata carrier at all.
        assert!(parse_decision_page("# Just prose\nno frontmatter here").is_none());
    }

    #[test]
    fn the_drafter_never_writes_a_value_from_the_process_environment() {
        // The elevation path runs in a process holding ACSP_PANEL_NOSTR_PRIVKEY,
        // a GitHub token and relay URLs, and it writes a PUBLIC corpus page. The
        // only environment value it may carry is the COMPILE-time crate version
        // in the OKF `generated.by` actor; nothing read at run time belongs on a
        // page. Pinned as an allowlist so a future writer cannot quietly add an
        // env-sourced key.
        const ALLOWED_KEYS: [&str; 6] =
            ["type", "resource", "status", "public", "title", "generated"];

        let dec = sample(DecisionInput {
            summary: "audit".into(),
            rationale: "r".into(),
            caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
            ..Default::default()
        });
        let (_, markdown) = draft_decision_page(&dec);
        let (meta, _) = vault::split(&markdown);

        // No frontmatter VALUE is an environment value. Exact equality, not a
        // substring scan: the ambient environment of this estate holds short
        // words like `agentbox` that legitimately occur inside a decision URN,
        // and a scan that flags those is a test nobody can keep green. A
        // credential — the thing Invariant 11 is about — is carried whole, so
        // equality is where it would show up.
        let env: std::collections::HashSet<String> = std::env::vars().map(|(_, v)| v).collect();
        let mut emitted: Vec<String> = meta.extra.values().cloned().collect();
        emitted.extend(meta.extra_lists.values().flatten().cloned());
        emitted.extend(
            [&meta.page_type, &meta.resource, &meta.status, &meta.title]
                .into_iter()
                .flatten()
                .cloned(),
        );
        if let Some(ref stamp) = meta.generated {
            emitted.push(stamp.by.clone());
            emitted.push(stamp.at.clone());
        }
        for value in &emitted {
            assert!(
                !env.contains(value),
                "an environment value was written to the page: {value}"
            );
        }

        // Every authored key is either in the allowlist or a declared relation.
        for key in meta
            .extra
            .keys()
            .chain(meta.extra_lists.keys())
            .map(String::as_str)
        {
            assert!(
                ALLOWED_KEYS.contains(&key) || DECISION_RELATION_KEYS.contains(&key),
                "unexpected frontmatter key `{key}` on a public corpus page"
            );
        }
    }

    #[test]
    fn parse_tolerates_a_hand_authored_page() {
        // A human editing the page in Obsidian writes plain wikilinks; the read
        // half must accept them exactly as the drafter's own output.
        let md = format!(
            "---\ntype: Individual\nresource: \"urn:agentbox:decision:{PK}:sha256-12-abc\"\nstatus: draft\npublic: true\ntitle: Hand authored\ncauses:\n  - \"[[urn:agentbox:decision:bare]]\"\nprecedes:\n  - \"[[urn:agentbox:decision:obj]]\"\n---\n\n# Hand authored\n"
        );
        let parsed = parse_decision_page(&md).expect("parses");
        assert_eq!(parsed.input.caused, vec!["urn:agentbox:decision:bare"]);
        assert_eq!(
            parsed.input.precedent_for,
            vec!["urn:agentbox:decision:obj"]
        );
    }
}

#[cfg(test)]
mod vocabulary_conformance {
    use super::*;
    use crate::services::decision_service::DecisionInput;

    /// Where the governed vocabulary lives relative to this repo. Absent on a
    /// machine that has not checked out the corpus, in which case the test
    /// reports that it could not run rather than passing vacuously.
    const VOCABULARY: &str = "../visionGraph/ontology/vocabulary.yaml";

    fn sample_pages() -> Vec<(String, String)> {
        let dec = ElevatedDecision {
            decision_urn: "urn:agentbox:decision:0123456789abcdef:sha256-12-9ec3d090ff23".into(),
            input: DecisionInput {
                summary: "Merge Duplicate Concepts".into(),
                rationale: "resolves DUPLICATE_CONCEPT".into(),
                proposal_urn: None,
                caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
                precedent_for: vec!["urn:agentbox:decision:AA:sha256-12-ghi".into()],
                influenced: vec!["urn:agentbox:decision:AA:sha256-12-jkl".into()],
                considered_inputs: vec!["urn:agentbox:activity:in".into()],
                governed_by: vec!["urn:agentbox:decision:policy".into()],
            },
            agent_did: "did:nostr:0123456789abcdef".into(),
            generated_at: "2026-09-22T00:00:00Z".into(),
            acsp_approved: false,
        };
        let (_, decision) = draft_decision_page(&dec);

        let (_, class) = crate::actors::elevation_actor::draft_class_page(
            &crate::actors::elevation_actor::FrontierCandidate {
                label: "finality mechanism".into(),
                degree: 7,
                domain: "blockchain".into(),
                referenced_by: vec!["Consensus Layer".into()],
            },
        );
        vec![
            ("decision".to_string(), decision),
            ("class".to_string(), class),
        ]
    }

    /// Every frontmatter key either writer emits must be declared in
    /// `ontology/vocabulary.yaml`, because `vault validate` fails a
    /// `knowledge/` page on an unknown key and a failing page never reaches the
    /// corpus at all.
    ///
    /// This is the cheap twin of `vault validate --vault knowledge`: it reads
    /// the SAME file the CLI reads, so a vocabulary change that orphans a
    /// writer's key fails here without needing the CLI built or a vault staged.
    #[test]
    fn every_key_the_corpus_writers_emit_is_declared() {
        let Ok(text) = std::fs::read_to_string(VOCABULARY) else {
            eprintln!("skipping: {VOCABULARY} not checked out on this machine");
            return;
        };
        let vocab: serde_yaml::Value = serde_yaml::from_str(&text).expect("vocabulary.yaml parses");

        let keys_of = |section: &str| -> std::collections::HashSet<String> {
            vocab
                .get(section)
                .and_then(|v| v.as_mapping())
                .map(|m| {
                    m.keys()
                        .filter_map(|k| k.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut declared = keys_of("relations");
        declared.extend(keys_of("scalars"));
        // OKF blocks are declared as nested structures, not as flat scalars.
        declared.extend(["type", "resource", "status", "generated"].map(String::from));

        for (name, markdown) in sample_pages() {
            let meta = vault::parse(&markdown);
            let emitted: Vec<String> = meta
                .extra
                .keys()
                .chain(meta.extra_lists.keys())
                .cloned()
                .chain(meta.source_domain.is_some().then(|| "source-domain".into()))
                .chain(meta.owl_class.is_some().then(|| "owl-class".into()))
                .chain((!meta.aliases.is_empty()).then(|| "aliases".into()))
                .chain((!meta.tags.is_empty()).then(|| "tags".into()))
                .chain(meta.elevated_from.is_some().then(|| "elevatedFrom".into()))
                .collect();
            for key in emitted {
                assert!(
                    declared.contains(&key),
                    "the {name} writer emits `{key}`, which vocabulary.yaml does \
                     not declare — `vault validate` would reject the page"
                );
            }
            // …and no rejected construct sneaks back in.
            assert!(!markdown.contains("```"), "{name}: json-ld fence");
            assert!(!markdown.contains(":: "), "{name}: Logseq property line");
            assert!(!markdown.contains("{{embed"), "{name}: embed");
        }
    }
}

#[cfg(test)]
mod dump_for_vault_cli {
    use super::*;
    use crate::services::decision_service::DecisionInput;

    /// Not an assertion — a fixture emitter, so the real `vault` CLI can be run
    /// against exactly what the drafter writes. `DUMP_DECISION_PAGE=<dir>`.
    #[test]
    fn dump() {
        let Ok(dir) = std::env::var("DUMP_DECISION_PAGE") else {
            return;
        };
        let dec = ElevatedDecision {
            decision_urn: "urn:agentbox:decision:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef:sha256-12-9ec3d090ff23".into(),
            input: DecisionInput {
                summary: "Merge Duplicate Concepts".into(),
                rationale: "resolves DUPLICATE_CONCEPT".into(),
                proposal_urn: None,
                caused: vec!["urn:agentbox:decision:AA:sha256-12-def".into()],
                precedent_for: vec![],
                influenced: vec![],
                considered_inputs: vec![],
                governed_by: vec![],
            },
            agent_did: "did:nostr:0123456789abcdef".into(),
            generated_at: "2026-09-22T00:00:00Z".into(),
            acsp_approved: false,
        };
        let (path, md) = draft_decision_page(&dec);
        let out = std::path::Path::new(&dir).join(path.rsplit('/').next().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&out, md).unwrap();
        eprintln!("wrote {}", out.display());

        // …and the class-elevation sibling, which lands one level up.
        let (cpath, cmd) = crate::actors::elevation_actor::draft_class_page(
            &crate::actors::elevation_actor::FrontierCandidate {
                label: "finality mechanism".into(),
                degree: 7,
                domain: "blockchain".into(),
                referenced_by: vec!["Consensus Layer".into()],
            },
        );
        let parent = std::path::Path::new(&dir).parent().unwrap().to_path_buf();
        let cout = parent.join(cpath.rsplit('/').next().unwrap());
        std::fs::write(&cout, cmd).unwrap();
        eprintln!("wrote {}", cout.display());
    }
}
