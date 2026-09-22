// src/services/ontology_conflict_gate.rs
//! W-A runtime conflict gate (PRD-022 / DDD-020) — the pre-merge integrity check
//! that runs BEFORE Whelk consistency and BEFORE ACSP governance on the governed
//! `/api/ontology-agent/propose` write door.
//!
//! This is a *native* Rust port of the four semantica-style detectors that the
//! batch/corpus path runs in `vault` (`crates/vault/src/conflicts.rs`, itself the
//! port of the retired `logseq/pipeline/conflicts.py`), with **identical semantics**:
//!
//!   * `DUPLICATE_CONCEPT`      — distinct IRIs sharing a normalised label
//!   * `SUBCLASS_CYCLE`         — a cycle in the `subClassOf` graph
//!   * `RELATION_CONTRADICTION` — a class both `subClassOf` and `contrasts_with` a target
//!   * `TYPE_CONFLICT`          — a `subClassOf` parent that is not itself a Class
//!
//! DDD-020 §Ubiquitous Language / I07 ("consistency ≠ integrity"): a proposal must
//! clear THREE distinct gates — this conflict/integrity gate, then Whelk EL
//! consistency, then ACSP governance — none substituting for another. EL-satisfiability
//! alone never authorises a merge (a subclass cycle is often still satisfiable).
//!
//! Gate outcome, per DDD-020 §ConflictReport — DELTA-SCOPED (operator policy,
//! supersedes strict corpus-wide blocking): the gate BLOCKS only on conflicts the
//! candidate proposal INTRODUCES or TOUCHES (a conflict whose IRIs include the
//! candidate identity or an edge the proposal adds). Pre-existing corpus conflicts
//! the candidate does not touch are surfaced as ADVISORY (`pre_existing`) and never
//! block. Per detected KIND:
//!   * `SUBCLASS_CYCLE`         → blocks IFF the candidate touches it
//!   * `TYPE_CONFLICT`          → blocks IFF the candidate touches it
//!   * `RELATION_CONTRADICTION` → blocks IFF the candidate touches it (fail-closed)
//!   * `DUPLICATE_CONCEPT`      → a FRESH pair the candidate creates is NON-blocking
//!                                (routes to the `EntityMerger` via `merge_candidates`);
//!                                a PRE-EXISTING duplicate cluster (≥2 corpus members)
//!                                the candidate JOINS blocks (resolve/merge first)
//!
//! Rationale: a loaded store may already carry dozens of pre-existing
//! `DUPLICATE_CONCEPT` / `SUBCLASS_CYCLE` conflicts; scanning corpus-wide made every
//! proposal 409 on conflicts it never introduced. The gate now keys accept/reject on
//! the DELTA only, while still reporting corpus health as advisory.
//!
//! All detectors are **pure** functions over borrowed slices — no store, no actor,
//! no async — so they unit-test with zero plumbing (mirroring the `conflicts.py`
//! pytest suite). The propose pipeline (transaction spine, T2) calls [`evaluate`]
//! before Whelk and maps a non-empty `blocking` set → HTTP 409 with the serialised
//! report (blocking + pre_existing separated); nothing in this module touches the
//! service, the handler, or the store.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use visionclaw_domain::ports::ontology_repository::OwlClass;

/// The `contrasts_with` relation is carried on [`OwlClass::other_relationships`] under
/// this key (the corpus/ADR-014 relation name); the batch pipeline reads the same
/// relation off `RelationSet.contrasts_with`.
const CONTRASTS_WITH_KEY: &str = "contrasts_with";

/// Entity types that are legitimate `subClassOf` parents. A `subClassOf` edge whose
/// parent's declared type is anything else (e.g. `Individual`) is a `TYPE_CONFLICT`.
/// Mirrors `conflicts.py` `t not in ("Class", "OntologyClass")`.
const CLASS_TYPES: [&str; 2] = ["Class", "OntologyClass"];

/// The four kinds of pre-merge semantic conflict (DDD-020 §ConflictReport).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConflictKind {
    DuplicateConcept,
    SubclassCycle,
    RelationContradiction,
    TypeConflict,
}

impl ConflictKind {
    /// The frozen wire string (matches the `conflicts.py` `--json` CLI contract).
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictKind::DuplicateConcept => "DUPLICATE_CONCEPT",
            ConflictKind::SubclassCycle => "SUBCLASS_CYCLE",
            ConflictKind::RelationContradiction => "RELATION_CONTRADICTION",
            ConflictKind::TypeConflict => "TYPE_CONFLICT",
        }
    }

    /// Whether a conflict of this kind hard-blocks the merge. `DUPLICATE_CONCEPT` is
    /// the sole non-blocking kind — it routes to the `EntityMerger` (fail-into-merge);
    /// every other kind fails closed. This is the DDD-020 I07 gate policy and is
    /// deliberately KIND-based, not severity-ordered.
    pub fn is_blocking(&self) -> bool {
        !matches!(self, ConflictKind::DuplicateConcept)
    }
}

/// Ranked severity, retained for parity with the `conflicts.py` report shape. Note the
/// gate *outcome* is driven by [`ConflictKind::is_blocking`], NOT by severity — a
/// `TYPE_CONFLICT` is `Medium` severity yet still blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictSeverity {
    High,
    Medium,
    Low,
}

/// One detected conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub severity: ConflictSeverity,
    /// The IRIs implicated (the `subjects` field in the batch CLI's `as_dict()`).
    pub iris: Vec<String>,
    pub detail: String,
}

/// A duplicate-label cluster routed to the `EntityMerger` (DDD-020 §EntityMerger).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeCandidate {
    /// The normalised label shared by every IRI in the cluster.
    pub normalised_label: String,
    /// The distinct IRIs that collide on that label (≥2).
    pub iris: Vec<String>,
}

/// Typed, JSON-serialisable result of a DELTA-SCOPED pre-merge conflict scan
/// (PRD-022 W-A). The scan folds the candidate into the corpus, runs the four
/// detectors over the union, then partitions the result by whether each conflict is
/// attributable to THIS proposal:
///
///   * [`blocking`](Self::blocking) — conflicts the candidate introduces or touches
///     that reject the merge; `ok()` / `exit_code()` key on this set ONLY.
///   * [`pre_existing`](Self::pre_existing) — corpus conflicts the candidate does not
///     touch; advisory, never block (surfaced so the client can show corpus health).
///   * [`merge_candidates`](Self::merge_candidates) — normalised-label clusters routed
///     to the `EntityMerger` (a fresh candidate duplicate lands here, non-blocking).
///
/// On a blocking report T2 serialises the whole struct onto the propose HTTP 409
/// response; the handler separates `blocking` (blockingConflicts) from `preExisting`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    /// Conflicts the candidate INTRODUCES or TOUCHES that hard-block the merge.
    pub blocking: Vec<Conflict>,
    /// Pre-existing corpus conflicts the candidate does NOT touch — advisory only.
    pub pre_existing: Vec<Conflict>,
    /// Duplicate-label clusters routed to the `EntityMerger` (fail-into-merge).
    pub merge_candidates: Vec<MergeCandidate>,
}

impl ConflictReport {
    /// True iff nothing the candidate introduces/touches blocks the merge. A report
    /// whose only conflicts are `pre_existing` advisories (or a fresh duplicate routed
    /// to merge) is `ok()`.
    pub fn ok(&self) -> bool {
        self.blocking.is_empty()
    }

    /// Process-style exit code so this composes with `pipeline.gate` the same way the
    /// batch detector does: `1` when any blocking conflict is present, else `0`.
    pub fn exit_code(&self) -> i32 {
        if self.ok() {
            0
        } else {
            1
        }
    }
}

/// A candidate assertion being proposed through the governed write door, folded into
/// the existing corpus for evaluation. This is the ONE entity the gate is guarding on:
/// [`evaluate`] runs the detectors over `corpus ∪ candidate` but BLOCKS only on the
/// conflicts this candidate introduces or touches, demoting every untouched corpus
/// conflict to the `pre_existing` advisory set (delta-scoped operator policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedCandidate {
    pub iri: String,
    pub label: String,
    pub entity_type: String,
    pub subclass_of: Vec<String>,
    pub contrasts_with: Vec<String>,
}

/// Internal uniform view over both an existing [`OwlClass`] and the [`ProposedCandidate`],
/// so the four detectors run over a single homogeneous slice.
struct EntityView {
    iri: String,
    label: String,
    /// `None` when the source declares no type (an unknown type never triggers a
    /// `TYPE_CONFLICT`, matching `conflicts.py` `if t and t not in (...)`).
    entity_type: Option<String>,
    subclass_of: Vec<String>,
    contrasts_with: Vec<String>,
    definition: String,
}

impl EntityView {
    fn from_owl(c: &OwlClass) -> Self {
        EntityView {
            iri: c.iri.clone(),
            label: c.label.clone().unwrap_or_default(),
            entity_type: c.class_type.clone(),
            subclass_of: c.parent_classes.clone(),
            contrasts_with: c
                .other_relationships
                .get(CONTRASTS_WITH_KEY)
                .cloned()
                .unwrap_or_default(),
            definition: c.description.clone().unwrap_or_default(),
        }
    }

    fn from_candidate(c: &ProposedCandidate) -> Self {
        EntityView {
            iri: c.iri.clone(),
            label: c.label.clone(),
            entity_type: Some(c.entity_type.clone()),
            subclass_of: c.subclass_of.clone(),
            contrasts_with: c.contrasts_with.clone(),
            definition: String::new(),
        }
    }
}

/// Normalise a label for duplicate detection: lowercase, collapse every run of
/// non-`[a-z0-9]` characters to a single space, then strip. Byte-for-byte equivalent to
/// `conflicts.py` `_norm_label` (`re.sub(r"[^a-z0-9]+", " ", s.lower()).strip()`).
/// Because only ASCII alphanumerics survive, a purely-punctuation or non-ASCII label
/// normalises to the empty string and is never grouped as a duplicate.
fn norm_label(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_sep = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_sep = false;
        } else if !prev_sep {
            out.push(' ');
            prev_sep = true;
        }
    }
    out.trim().to_string()
}

/// `DUPLICATE_CONCEPT` — distinct IRIs that share a normalised label (the "duplicate
/// merges" failure mode). Escalated in the detail text when the group's definitions
/// also differ (a contradiction, not merely a dupe). Empty-normalised labels are
/// skipped. Ported from `detect_duplicate_concepts` (conflicts.py:103-119).
fn detect_duplicate_concepts(ents: &[EntityView]) -> Vec<Conflict> {
    // Group by normalised label, preserving first-seen order for deterministic output.
    let mut order: Vec<String> = Vec::new();
    let mut by_label: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in ents.iter().enumerate() {
        let key = norm_label(&e.label);
        if key.is_empty() {
            continue;
        }
        by_label
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(i);
    }

    let mut out = Vec::new();
    for label in order {
        let group = &by_label[&label];
        // Distinct IRIs only (a single IRI appearing twice — e.g. an amend of an
        // existing class — is not a duplicate).
        let mut iris: Vec<String> = group
            .iter()
            .map(|&i| ents[i].iri.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        iris.sort();
        if iris.len() < 2 {
            continue;
        }
        let distinct_defs: HashSet<&str> = group
            .iter()
            .map(|&i| ents[i].definition.trim())
            .filter(|d| !d.is_empty())
            .collect();
        let first_label = &ents[group[0]].label;
        let mut detail = format!("{} classes share label \"{}\"", iris.len(), first_label);
        if distinct_defs.len() > 1 {
            detail.push_str(" with DIFFERING definitions (contradiction, not just a duplicate)");
        }
        out.push(Conflict {
            kind: ConflictKind::DuplicateConcept,
            severity: ConflictSeverity::High,
            iris,
            detail,
        });
    }
    out
}

/// `SUBCLASS_CYCLE` — a cycle in the `subClassOf` graph (a logical impossibility).
/// Iterative WHITE/GREY/BLACK DFS with an explicit stack so a deep hierarchy can't blow
/// a recursion limit; a GREY re-entry (back-edge) is a cycle. Ported verbatim from
/// `detect_subclass_cycles` (conflicts.py:122-159).
fn detect_subclass_cycles(ents: &[EntityView]) -> Vec<Conflict> {
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;

    // parents[iri] = its declared subClassOf targets. Every entity is a key (matching
    // the Python dict comprehension); a target that is not itself an entity is a leaf.
    let known: HashSet<&str> = ents.iter().map(|e| e.iri.as_str()).collect();
    let mut parents: HashMap<&str, &[String]> = HashMap::new();
    let mut roots: Vec<&str> = Vec::with_capacity(ents.len());
    for e in ents {
        // Last write wins on a repeated IRI, mirroring the Python dict comprehension.
        if parents
            .insert(e.iri.as_str(), e.subclass_of.as_slice())
            .is_none()
        {
            roots.push(e.iri.as_str());
        }
    }

    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    let mut out = Vec::new();

    for &root in &roots {
        if *color.get(root).unwrap_or(&WHITE) != WHITE {
            continue;
        }
        // Stack of (node, next-child-index); `path` is the current GREY chain.
        let mut stack: Vec<(&str, usize)> = vec![(root, 0)];
        let mut path: Vec<&str> = Vec::new();
        while let Some(&(node, idx)) = stack.last() {
            if idx == 0 {
                color.insert(node, GREY);
                path.push(node);
            }
            let kids: &[String] = parents.get(node).copied().unwrap_or(&[]);
            if idx < kids.len() {
                let last = stack.len() - 1;
                stack[last].1 = idx + 1;
                let child = kids[idx].as_str();
                let child_color = *color.get(child).unwrap_or(&WHITE);
                if child_color == GREY {
                    // Back-edge → cycle. Slice the GREY path from the re-entered node.
                    if let Some(i) = path.iter().position(|&p| p == child) {
                        let cyc: Vec<String> = path[i..].iter().map(|s| s.to_string()).collect();
                        let mut key = cyc.clone();
                        key.sort();
                        if seen.insert(key) {
                            let detail =
                                format!("subClassOf cycle: {} -> {}", cyc.join(" -> "), child);
                            out.push(Conflict {
                                kind: ConflictKind::SubclassCycle,
                                severity: ConflictSeverity::High,
                                iris: cyc,
                                detail,
                            });
                        }
                    }
                } else if child_color == WHITE && known.contains(child) {
                    stack.push((child, 0));
                }
            } else {
                color.insert(node, BLACK);
                path.pop();
                stack.pop();
            }
        }
    }
    out
}

/// `RELATION_CONTRADICTION` — a class that is both `subClassOf` and `contrasts_with` the
/// same target. Ported from `detect_relation_contradictions` (conflicts.py:162-171).
fn detect_relation_contradictions(ents: &[EntityView]) -> Vec<Conflict> {
    let mut out = Vec::new();
    for e in ents {
        let sc: HashSet<&str> = e.subclass_of.iter().map(|s| s.as_str()).collect();
        let cw: HashSet<&str> = e.contrasts_with.iter().map(|s| s.as_str()).collect();
        let mut both: Vec<&str> = sc.intersection(&cw).copied().collect();
        both.sort();
        for t in both {
            out.push(Conflict {
                kind: ConflictKind::RelationContradiction,
                severity: ConflictSeverity::Medium,
                iris: vec![e.iri.clone(), t.to_string()],
                detail: format!("{} is both subClassOf and contrasts_with {}", e.iri, t),
            });
        }
    }
    out
}

/// `TYPE_CONFLICT` — a `subClassOf` parent whose declared type is not a Class. Ported
/// from `detect_type_conflicts` (conflicts.py:174-184). A parent with no known type is
/// never flagged.
fn detect_type_conflicts(ents: &[EntityView]) -> Vec<Conflict> {
    let types: HashMap<&str, &str> = ents
        .iter()
        .filter_map(|e| e.entity_type.as_deref().map(|t| (e.iri.as_str(), t)))
        .collect();
    let mut out = Vec::new();
    for e in ents {
        for parent in &e.subclass_of {
            if let Some(&t) = types.get(parent.as_str()) {
                if !CLASS_TYPES.contains(&t) {
                    out.push(Conflict {
                        kind: ConflictKind::TypeConflict,
                        severity: ConflictSeverity::Medium,
                        iris: vec![e.iri.clone(), parent.clone()],
                        detail: format!(
                            "{} subClassOf {}, but {} is a {}, not a Class",
                            e.iri, parent, parent, t
                        ),
                    });
                }
            }
        }
    }
    out
}

/// Run all four detectors over `entities`, returning the flat conflict list. Order
/// matches the batch pipeline: duplicates, cycles, contradictions, type conflicts.
/// Partitioning into blocking / pre-existing is the caller's ([`evaluate`]) job.
fn detect_all(entities: &[EntityView]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    conflicts.extend(detect_duplicate_concepts(entities));
    conflicts.extend(detect_subclass_cycles(entities));
    conflicts.extend(detect_relation_contradictions(entities));
    conflicts.extend(detect_type_conflicts(entities));
    conflicts
}

/// Evaluate a proposed candidate against the existing corpus, folding the candidate in
/// as an additional entity, and return the typed [`ConflictReport`]. This is the single
/// public entry point the transaction spine (T2) calls BEFORE the Whelk consistency gate.
///
/// `corpus` is the current asserted-graph class set (e.g. `ontology_repo.list_owl_classes()`),
/// `proposed` is the candidate assertion. Detectors run over the whole union, but the
/// report is DELTA-SCOPED (operator policy): a conflict lands in
/// [`blocking`](ConflictReport::blocking) only when the candidate introduces or touches
/// it, otherwise in [`pre_existing`](ConflictReport::pre_existing) as advisory.
/// `ok()` — hence the accept/reject decision — keys on `blocking` alone.
///
/// "Touches" = the conflict's IRIs intersect the candidate's identity + the edges it
/// adds: `{ candidate.iri } ∪ candidate.subclass_of ∪ candidate.contrasts_with`.
/// A `DUPLICATE_CONCEPT` the candidate touches blocks ONLY when the colliding label
/// already had ≥2 corpus members (a pre-existing cluster the candidate is joining); a
/// fresh pair the candidate creates is non-blocking and routes to `merge_candidates`.
pub fn evaluate(corpus: &[OwlClass], proposed: &ProposedCandidate) -> ConflictReport {
    let mut entities: Vec<EntityView> = corpus.iter().map(EntityView::from_owl).collect();
    entities.push(EntityView::from_candidate(proposed));

    let all = detect_all(&entities);
    let merge_candidates = build_merge_candidates(&entities);

    // The delta the gate guards on: the candidate's own identity plus every edge it
    // introduces. A conflict is attributable to this proposal iff its IRIs intersect it.
    let mut touch: HashSet<&str> = HashSet::new();
    touch.insert(proposed.iri.as_str());
    for parent in &proposed.subclass_of {
        touch.insert(parent.as_str());
    }
    for other in &proposed.contrasts_with {
        touch.insert(other.as_str());
    }

    // Corpus IRIs (the store BEFORE this proposal) — lets a touched duplicate tell a
    // fresh candidate-created pair (route to merge, non-blocking) from a pre-existing
    // cluster the candidate is joining (block).
    let corpus_iris: HashSet<&str> = corpus.iter().map(|c| c.iri.as_str()).collect();

    let mut blocking = Vec::new();
    let mut pre_existing = Vec::new();
    for c in all {
        let touches = c.iris.iter().any(|i| touch.contains(i.as_str()));
        if !touches {
            // A corpus conflict the candidate never touches → advisory only.
            pre_existing.push(c);
            continue;
        }
        let is_blocking = match c.kind {
            // A duplicate blocks only when it pre-existed in the corpus (≥2 corpus
            // members share the label independent of the candidate). A pair the
            // candidate itself creates is non-blocking — it routes to the EntityMerger.
            ConflictKind::DuplicateConcept => {
                c.iris
                    .iter()
                    .filter(|i| i.as_str() != proposed.iri && corpus_iris.contains(i.as_str()))
                    .count()
                    >= 2
            }
            // Cycle / type / relation-contradiction the candidate touches always block.
            _ => true,
        };
        if is_blocking {
            blocking.push(c);
        }
        // else: candidate-introduced fresh duplicate — represented by merge_candidates,
        // deliberately absent from both blocking and pre_existing.
    }

    ConflictReport {
        blocking,
        pre_existing,
        merge_candidates,
    }
}

/// Build the [`MergeCandidate`] clusters (normalised label + colliding IRIs) directly
/// from the entity slice — the canonical duplicate grouping the `EntityMerger` consumes.
fn build_merge_candidates(ents: &[EntityView]) -> Vec<MergeCandidate> {
    let mut order: Vec<String> = Vec::new();
    let mut by_label: HashMap<String, Vec<String>> = HashMap::new();
    for e in ents {
        let key = norm_label(&e.label);
        if key.is_empty() {
            continue;
        }
        by_label
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(e.iri.clone());
    }
    let mut out = Vec::new();
    for label in order {
        let mut iris: Vec<String> = by_label[&label]
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        iris.sort();
        if iris.len() >= 2 {
            out.push(MergeCandidate {
                normalised_label: label,
                iris,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fixture builders ──────────────────────────────────────────────────────

    fn owl(
        iri: &str,
        label: &str,
        class_type: &str,
        parents: &[&str],
        contrasts: &[&str],
        definition: &str,
    ) -> OwlClass {
        let mut c = OwlClass::default();
        c.iri = iri.to_string();
        c.label = Some(label.to_string());
        c.class_type = if class_type.is_empty() {
            None
        } else {
            Some(class_type.to_string())
        };
        c.parent_classes = parents.iter().map(|s| s.to_string()).collect();
        if !contrasts.is_empty() {
            c.other_relationships.insert(
                CONTRASTS_WITH_KEY.to_string(),
                contrasts.iter().map(|s| s.to_string()).collect(),
            );
        }
        c.description = if definition.is_empty() {
            None
        } else {
            Some(definition.to_string())
        };
        c
    }

    fn cand(
        iri: &str,
        label: &str,
        entity_type: &str,
        parents: &[&str],
        contrasts: &[&str],
    ) -> ProposedCandidate {
        ProposedCandidate {
            iri: iri.to_string(),
            label: label.to_string(),
            entity_type: entity_type.to_string(),
            subclass_of: parents.iter().map(|s| s.to_string()).collect(),
            contrasts_with: contrasts.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// A harmless candidate that introduces no conflict against a `Class` parent.
    fn benign_candidate() -> ProposedCandidate {
        cand(
            "ex:New",
            "Wholly Unique Fresh Concept",
            "Class",
            &["ex:Root"],
            &[],
        )
    }

    /// Every detected conflict kind across BOTH partitions (blocking + advisory).
    fn kinds(report: &ConflictReport) -> Vec<ConflictKind> {
        report
            .blocking
            .iter()
            .chain(report.pre_existing.iter())
            .map(|c| c.kind)
            .collect()
    }

    // ── norm_label edge cases ─────────────────────────────────────────────────

    #[test]
    fn norm_label_normalises_case_and_punctuation() {
        assert_eq!(norm_label("Graph Node"), "graph node");
        assert_eq!(norm_label("graph  node"), "graph node");
        assert_eq!(norm_label("Graph-Node!"), "graph node");
        assert_eq!(norm_label("  GRAPH___node  "), "graph node");
    }

    #[test]
    fn norm_label_unicode_and_punctuation_only_becomes_empty() {
        // Only ASCII alphanumerics survive, so pure punctuation / dashes / non-ASCII
        // normalise to the empty string (never grouped as a duplicate).
        assert_eq!(norm_label("!!!"), "");
        assert_eq!(norm_label("—— · ——"), "");
        assert_eq!(norm_label("   "), "");
        // Non-ASCII letters are stripped like any other separator (parity with the
        // Python `[^a-z0-9]` regex applied after lowercasing).
        assert_eq!(norm_label("Café"), "caf");
    }

    // ── DUPLICATE_CONCEPT ─────────────────────────────────────────────────────

    #[test]
    fn duplicate_concept_positive_with_differing_definitions() {
        // A PRE-EXISTING duplicate cluster (D1/D2) with DIFFERING definitions that the
        // benign candidate does NOT touch → advisory (pre_existing), never blocking.
        let corpus2 = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl(
                "ex:D1",
                "Graph Node",
                "Class",
                &["ex:Root"],
                &[],
                "a vertex",
            ),
            owl(
                "ex:D2",
                "graph  node",
                "Class",
                &["ex:Root"],
                &[],
                "a different thing",
            ),
        ];
        let report = evaluate(&corpus2, &benign_candidate());
        let dupes: Vec<&Conflict> = report
            .pre_existing
            .iter()
            .filter(|c| c.kind == ConflictKind::DuplicateConcept)
            .collect();
        assert_eq!(dupes.len(), 1, "exactly one pre-existing duplicate cluster");
        assert_eq!(
            dupes[0].iris,
            vec!["ex:D1".to_string(), "ex:D2".to_string()]
        );
        assert!(
            dupes[0].detail.contains("DIFFERING"),
            "differing definitions escalate the detail: {}",
            dupes[0].detail
        );
        assert!(
            report.ok(),
            "a pre-existing duplicate the candidate does not touch does not block"
        );

        // A FRESH pair the candidate creates (corpus held a single 'Graph Node') routes
        // to the EntityMerger — non-blocking, surfaced ONLY as a merge candidate.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl(
                "ex:D1",
                "Graph Node",
                "Class",
                &["ex:Root"],
                &[],
                "a vertex",
            ),
        ];
        let dup_cand = cand("ex:D2", "graph node", "Class", &["ex:Root"], &[]);
        let folded = evaluate(&corpus, &dup_cand);
        assert!(folded.ok(), "a fresh candidate duplicate is non-blocking");
        assert!(folded
            .merge_candidates
            .iter()
            .any(|m| m.normalised_label == "graph node"));
        assert!(
            !kinds(&folded).contains(&ConflictKind::DuplicateConcept),
            "a fresh duplicate is represented by merge_candidates, not blocking/pre_existing"
        );
    }

    #[test]
    fn duplicate_concept_counter_distinct_labels() {
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:A", "Apple", "Class", &["ex:Root"], &[], ""),
            owl("ex:B", "Banana", "Class", &["ex:Root"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        assert!(!kinds(&report).contains(&ConflictKind::DuplicateConcept));
        assert!(report.merge_candidates.is_empty());
    }

    #[test]
    fn duplicate_concept_empty_labels_not_flagged() {
        // Two classes whose labels normalise to empty must NOT collide.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:P1", "!!!", "Class", &["ex:Root"], &[], ""),
            owl("ex:P2", "———", "Class", &["ex:Root"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        assert!(!kinds(&report).contains(&ConflictKind::DuplicateConcept));
        assert!(report.merge_candidates.is_empty());
    }

    #[test]
    fn duplicate_is_non_blocking_and_routes_to_merge() {
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl(
                "ex:D1",
                "Graph Node",
                "Class",
                &["ex:Root"],
                &[],
                "a vertex",
            ),
        ];
        let dup_cand = cand("ex:D2", "graph node", "Class", &["ex:Root"], &[]);
        let report = evaluate(&corpus, &dup_cand);
        // A fresh candidate duplicate does NOT block — it routes to the EntityMerger and
        // is represented by merge_candidates, not by a blocking/advisory conflict.
        assert!(
            !kinds(&report).contains(&ConflictKind::DuplicateConcept),
            "a fresh duplicate is not a blocking/advisory conflict"
        );
        assert!(report.ok(), "a duplicate-only report is ok()");
        assert!(report.blocking.is_empty());
        assert_eq!(report.exit_code(), 0);
        // The merge candidate carries the normalised label + both IRIs.
        assert_eq!(report.merge_candidates.len(), 1);
        let mc = &report.merge_candidates[0];
        assert_eq!(mc.normalised_label, "graph node");
        assert_eq!(mc.iris, vec!["ex:D1".to_string(), "ex:D2".to_string()]);
    }

    // ── SUBCLASS_CYCLE ────────────────────────────────────────────────────────

    #[test]
    fn subclass_cycle_positive() {
        // The cycle lives entirely in the corpus (A<->B); the benign candidate does not
        // touch it, so it is DETECTED but demoted to the advisory pre_existing set — it
        // does not block this proposal (delta-scoped policy). A cycle the candidate
        // introduces blocking is covered by `candidate_folding_introduces_cycle`.
        let corpus = vec![
            owl("ex:A", "A", "Class", &["ex:B"], &[], ""),
            owl("ex:B", "B", "Class", &["ex:A"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        let cyc: Vec<&Conflict> = report
            .pre_existing
            .iter()
            .filter(|c| c.kind == ConflictKind::SubclassCycle)
            .collect();
        assert_eq!(cyc.len(), 1, "one deduplicated cycle, detected as advisory");
        let iris: HashSet<&str> = cyc[0].iris.iter().map(|s| s.as_str()).collect();
        assert_eq!(iris, HashSet::from(["ex:A", "ex:B"]));
        assert_eq!(cyc[0].severity, ConflictSeverity::High);
        assert!(
            report.ok(),
            "a pre-existing cycle the candidate does not touch is advisory, not blocking"
        );
        assert!(report.blocking.is_empty());
    }

    #[test]
    fn subclass_cycle_counter_clean_chain() {
        // Root <- Mid <- Leaf: a well-formed acyclic hierarchy.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:Mid", "Mid", "Class", &["ex:Root"], &[], ""),
            owl("ex:Leaf", "Leaf", "Class", &["ex:Mid"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        assert!(!kinds(&report).contains(&ConflictKind::SubclassCycle));
        assert!(report.ok());
    }

    #[test]
    fn subclass_cycle_real_corpus_fixture() {
        // The real corpus cycle style: time-series-forecasting <-> probabilistic-forecasting.
        let corpus = vec![
            owl(
                "ex:time-series-forecasting",
                "Time Series Forecasting",
                "Class",
                &["ex:probabilistic-forecasting"],
                &[],
                "",
            ),
            owl(
                "ex:probabilistic-forecasting",
                "Probabilistic Forecasting",
                "Class",
                &["ex:time-series-forecasting"],
                &[],
                "",
            ),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        // Pre-existing corpus cycle the candidate does not touch → advisory.
        let cyc: Vec<&Conflict> = report
            .pre_existing
            .iter()
            .filter(|c| c.kind == ConflictKind::SubclassCycle)
            .collect();
        assert_eq!(cyc.len(), 1);
        let iris: HashSet<&str> = cyc[0].iris.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            iris,
            HashSet::from(["ex:time-series-forecasting", "ex:probabilistic-forecasting"])
        );
        assert!(cyc[0].detail.contains("subClassOf cycle:"));
        assert!(
            report.ok(),
            "advisory cycle does not block the clean candidate"
        );
    }

    #[test]
    fn candidate_folding_introduces_cycle() {
        // Corpus B -> NEW is a dangling edge (NEW isn't yet a class), so no cycle
        // exists in the corpus alone. The proposed NEW -> B closes it.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:B", "B", "Class", &["ex:NEW"], &[], ""),
        ];
        // Without the candidate: no cycle.
        let harmless = cand("ex:Other", "Other", "Class", &["ex:Root"], &[]);
        let clean = evaluate(&corpus, &harmless);
        assert!(!kinds(&clean).contains(&ConflictKind::SubclassCycle));

        // With the candidate NEW -> B, the cycle NEW -> B -> NEW appears — and because
        // the candidate introduces it (NEW is its IRI, B an edge it adds), it BLOCKS.
        let closing = cand("ex:NEW", "New Node", "Class", &["ex:B"], &[]);
        let report = evaluate(&corpus, &closing);
        let cyc: Vec<&Conflict> = report
            .blocking
            .iter()
            .filter(|c| c.kind == ConflictKind::SubclassCycle)
            .collect();
        assert_eq!(cyc.len(), 1, "the candidate introduced the cycle");
        let iris: HashSet<&str> = cyc[0].iris.iter().map(|s| s.as_str()).collect();
        assert_eq!(iris, HashSet::from(["ex:NEW", "ex:B"]));
        assert!(!report.ok());
    }

    // ── RELATION_CONTRADICTION ────────────────────────────────────────────────

    #[test]
    fn relation_contradiction_positive() {
        // Candidate C is both subClassOf and contrasts_with T (a real Class → no
        // incidental TYPE_CONFLICT).
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:T", "T", "Class", &["ex:Root"], &[], ""),
        ];
        let contradict = cand("ex:C", "C", "Class", &["ex:T"], &["ex:T"]);
        let report = evaluate(&corpus, &contradict);
        // The candidate introduces the contradiction (its own IRI) → blocking.
        let rc: Vec<&Conflict> = report
            .blocking
            .iter()
            .filter(|c| c.kind == ConflictKind::RelationContradiction)
            .collect();
        assert_eq!(rc.len(), 1);
        assert_eq!(rc[0].iris, vec!["ex:C".to_string(), "ex:T".to_string()]);
        assert_eq!(rc[0].severity, ConflictSeverity::Medium);
        // No incidental TYPE_CONFLICT because ex:T is a real Class.
        assert!(!kinds(&report).contains(&ConflictKind::TypeConflict));
        // Fail-closed: a relation contradiction blocks pending ACSP override.
        assert!(
            !report.ok(),
            "relation contradiction is blocking (fail-closed)"
        );
    }

    #[test]
    fn relation_contradiction_counter() {
        // subClassOf ex:T and contrasts_with a DIFFERENT target → no contradiction.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:T", "T", "Class", &["ex:Root"], &[], ""),
            owl("ex:U", "U", "Class", &["ex:Root"], &[], ""),
        ];
        let ok_cand = cand("ex:C", "C", "Class", &["ex:T"], &["ex:U"]);
        let report = evaluate(&corpus, &ok_cand);
        assert!(!kinds(&report).contains(&ConflictKind::RelationContradiction));
        assert!(report.ok());
    }

    // ── TYPE_CONFLICT ─────────────────────────────────────────────────────────

    #[test]
    fn type_conflict_positive() {
        // Parent P is declared an Individual → a Class subclassing it is a mismatch.
        let corpus = vec![owl("ex:P", "Parent", "Individual", &[], &[], "")];
        let child = cand("ex:Ch", "Child", "Class", &["ex:P"], &[]);
        let report = evaluate(&corpus, &child);
        // The candidate introduces the type conflict (its IRI + the edge to P) → blocking.
        let tc: Vec<&Conflict> = report
            .blocking
            .iter()
            .filter(|c| c.kind == ConflictKind::TypeConflict)
            .collect();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].iris, vec!["ex:Ch".to_string(), "ex:P".to_string()]);
        assert_eq!(tc[0].severity, ConflictSeverity::Medium);
        assert!(!report.ok(), "type conflict hard-blocks");
    }

    #[test]
    fn type_conflict_counter() {
        // Parent is a real Class (and an untyped parent is also fine).
        let corpus = vec![
            owl("ex:P", "Parent", "OntologyClass", &[], &[], ""),
            owl("ex:Q", "Untyped Parent", "", &[], &[], ""),
        ];
        let child = cand("ex:Ch", "Child", "Class", &["ex:P", "ex:Q"], &[]);
        let report = evaluate(&corpus, &child);
        assert!(!kinds(&report).contains(&ConflictKind::TypeConflict));
        assert!(report.ok());
    }

    // ── report semantics ──────────────────────────────────────────────────────

    #[test]
    fn clean_corpus_empty_report() {
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:Leaf", "Leaf", "Class", &["ex:Root"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        assert!(
            report.blocking.is_empty(),
            "no blocking: {:?}",
            report.blocking
        );
        assert!(
            report.pre_existing.is_empty(),
            "no advisory: {:?}",
            report.pre_existing
        );
        assert!(report.merge_candidates.is_empty());
        assert!(report.ok());
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn blocking_semantics_cycle_type_relation() {
        // A report combining every kind: the three hard/fail-closed kinds block, the
        // duplicate does not.
        assert!(ConflictKind::SubclassCycle.is_blocking());
        assert!(ConflictKind::TypeConflict.is_blocking());
        assert!(ConflictKind::RelationContradiction.is_blocking());
        assert!(!ConflictKind::DuplicateConcept.is_blocking());

        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:A", "A", "Class", &["ex:B"], &[], ""), // cycle A<->B
            owl("ex:B", "B", "Class", &["ex:A"], &[], ""),
            owl("ex:P", "Parent", "Individual", &[], &[], ""),
            owl("ex:Ch", "Child", "Class", &["ex:P"], &[], ""), // type conflict
            owl("ex:D1", "Dup Concept", "Class", &["ex:Root"], &[], ""),
            owl("ex:D2", "dup  concept", "Class", &["ex:Root"], &[], ""), // duplicate
        ];
        // contrasts_with + subClassOf on the candidate → relation contradiction.
        let candidate = cand("ex:C", "C", "Class", &["ex:Root"], &["ex:Root"]);
        let report = evaluate(&corpus, &candidate);

        // All four kinds are DETECTED (across blocking + advisory) …
        let got: HashSet<ConflictKind> = kinds(&report).into_iter().collect();
        assert!(got.contains(&ConflictKind::SubclassCycle));
        assert!(got.contains(&ConflictKind::TypeConflict));
        assert!(got.contains(&ConflictKind::RelationContradiction));
        assert!(got.contains(&ConflictKind::DuplicateConcept));

        assert!(!report.ok());
        assert_eq!(report.exit_code(), 1);

        // … but only the contradiction the CANDIDATE introduces blocks; the pre-existing
        // corpus cycle / type conflict / duplicate are advisory (delta-scoped policy).
        let blocking_kinds: HashSet<ConflictKind> =
            report.blocking.iter().map(|c| c.kind).collect();
        assert_eq!(
            blocking_kinds,
            HashSet::from([ConflictKind::RelationContradiction]),
            "only the introduced contradiction blocks: {:?}",
            report.blocking
        );
        let advisory_kinds: HashSet<ConflictKind> =
            report.pre_existing.iter().map(|c| c.kind).collect();
        assert!(advisory_kinds.contains(&ConflictKind::SubclassCycle));
        assert!(advisory_kinds.contains(&ConflictKind::TypeConflict));
        assert!(advisory_kinds.contains(&ConflictKind::DuplicateConcept));
        // No duplicate ever lands in the blocking set here.
        assert!(report
            .blocking
            .iter()
            .all(|c| c.kind != ConflictKind::DuplicateConcept));
        // The duplicate still surfaces as a merge candidate.
        assert_eq!(report.merge_candidates.len(), 1);
    }

    #[test]
    fn report_serialises_with_frozen_kind_strings() {
        let corpus = vec![
            owl("ex:A", "A", "Class", &["ex:B"], &[], ""),
            owl("ex:B", "B", "Class", &["ex:A"], &[], ""),
        ];
        let report = evaluate(&corpus, &benign_candidate());
        let v = serde_json::to_value(&report).expect("serialises");
        // The pre-existing cycle serialises under the advisory partition with the frozen
        // SCREAMING_SNAKE kind string and lowercase severity.
        let first = &v["preExisting"][0];
        assert_eq!(first["kind"], "SUBCLASS_CYCLE");
        assert_eq!(first["severity"], "high");
        assert!(first["iris"].is_array());
        // camelCase wire contract for the report fields — the 409 body keys on these.
        assert!(v.get("blocking").is_some());
        assert!(v.get("preExisting").is_some());
        assert!(v.get("mergeCandidates").is_some());

        // Round-trips back to an equal report.
        let back: ConflictReport = serde_json::from_value(v).expect("deserialises");
        assert_eq!(back, report);
    }

    #[test]
    fn exit_code_and_ok_helpers() {
        let clean = ConflictReport::default();
        assert!(clean.ok());
        assert_eq!(clean.exit_code(), 0);
        assert!(clean.blocking.is_empty());

        // A pre-existing duplicate advisory (empty blocking set) is ok() — exit 0.
        let advisory_only = ConflictReport {
            blocking: vec![],
            pre_existing: vec![Conflict {
                kind: ConflictKind::DuplicateConcept,
                severity: ConflictSeverity::High,
                iris: vec!["ex:D1".into(), "ex:D2".into()],
                detail: "2 classes share label \"Dup\"".into(),
            }],
            merge_candidates: vec![MergeCandidate {
                normalised_label: "dup".into(),
                iris: vec!["ex:D1".into(), "ex:D2".into()],
            }],
        };
        assert!(advisory_only.ok(), "advisory-only report is non-blocking");
        assert_eq!(advisory_only.exit_code(), 0);

        let blocking = ConflictReport {
            blocking: vec![Conflict {
                kind: ConflictKind::SubclassCycle,
                severity: ConflictSeverity::High,
                iris: vec!["ex:A".into(), "ex:B".into()],
                detail: "subClassOf cycle: ex:A -> ex:B -> ex:A".into(),
            }],
            pre_existing: vec![],
            merge_candidates: vec![],
        };
        assert!(!blocking.ok());
        assert_eq!(blocking.exit_code(), 1);
        assert_eq!(blocking.blocking.len(), 1);
    }

    // ── delta-scoping (operator policy) ───────────────────────────────────────

    /// (a) A proposal that INTRODUCES a contradiction is blocked with EXACTLY that
    /// conflict in `blocking`, and nothing spurious in `pre_existing`.
    #[test]
    fn delta_gate_blocks_only_the_introduced_contradiction() {
        // A clean corpus: Root and a real Class T beneath it.
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            owl("ex:T", "T", "Class", &["ex:Root"], &[], ""),
        ];
        // Candidate is both subClassOf and contrasts_with T → a relation contradiction
        // it alone introduces.
        let candidate = cand("ex:C", "C", "Class", &["ex:T"], &["ex:T"]);
        let report = evaluate(&corpus, &candidate);

        assert!(!report.ok(), "an introduced contradiction blocks");
        assert_eq!(
            report.blocking.len(),
            1,
            "exactly the introduced conflict blocks"
        );
        assert_eq!(report.blocking[0].kind, ConflictKind::RelationContradiction);
        assert_eq!(
            report.blocking[0].iris,
            vec!["ex:C".to_string(), "ex:T".to_string()]
        );
        assert!(
            report.pre_existing.is_empty(),
            "a clean corpus yields no advisory conflicts: {:?}",
            report.pre_existing
        );
    }

    /// (b) A CLEAN proposal over a corpus already carrying pre-existing duplicates AND a
    /// cycle is NOT blocked; the corpus conflicts are surfaced as advisory only.
    #[test]
    fn delta_gate_passes_clean_proposal_over_dirty_corpus() {
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            // Pre-existing subclass cycle A<->B.
            owl("ex:A", "A", "Class", &["ex:B"], &[], ""),
            owl("ex:B", "B", "Class", &["ex:A"], &[], ""),
            // Pre-existing duplicate cluster (two corpus members share a label).
            owl("ex:D1", "Duplicate Thing", "Class", &["ex:Root"], &[], ""),
            owl("ex:D2", "duplicate  thing", "Class", &["ex:Root"], &[], ""),
        ];
        // Wholly-unique candidate wired under a clean parent — touches nothing dirty.
        let report = evaluate(&corpus, &benign_candidate());

        assert!(
            report.ok(),
            "a clean proposal is not blocked by pre-existing corpus conflicts"
        );
        assert!(report.blocking.is_empty());
        let advisory: HashSet<ConflictKind> = report.pre_existing.iter().map(|c| c.kind).collect();
        assert!(
            advisory.contains(&ConflictKind::SubclassCycle),
            "pre-existing cycle surfaced as advisory"
        );
        assert!(
            advisory.contains(&ConflictKind::DuplicateConcept),
            "pre-existing duplicate surfaced as advisory"
        );
    }

    /// (c) A proposal whose label JOINS a PRE-EXISTING duplicate cluster is blocked —
    /// the candidate touches an already-duplicated label, so it must resolve/merge first
    /// (distinct from a fresh candidate-created pair, which routes to the EntityMerger).
    #[test]
    fn delta_gate_blocks_proposal_joining_pre_existing_duplicate() {
        let corpus = vec![
            owl("ex:Root", "Root", "Class", &[], &[], ""),
            // Pre-existing duplicate cluster: TWO corpus members share "graph node".
            owl("ex:D1", "Graph Node", "Class", &["ex:Root"], &[], ""),
            owl("ex:D2", "graph  node", "Class", &["ex:Root"], &[], ""),
        ];
        // Candidate carries the SAME normalised label → it joins the pre-existing cluster.
        let candidate = cand("ex:D3", "Graph Node", "Class", &["ex:Root"], &[]);
        let report = evaluate(&corpus, &candidate);

        assert!(
            !report.ok(),
            "joining a pre-existing duplicate label blocks"
        );
        assert_eq!(report.blocking.len(), 1);
        assert_eq!(report.blocking[0].kind, ConflictKind::DuplicateConcept);
        let iris: HashSet<&str> = report.blocking[0].iris.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            iris,
            HashSet::from(["ex:D1", "ex:D2", "ex:D3"]),
            "the blocking duplicate cluster contains all three colliding IRIs"
        );
    }
}
