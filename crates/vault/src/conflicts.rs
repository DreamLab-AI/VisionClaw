//! `vault conflicts` — semantica-style pre-merge conflict detection, ported
//! from `pipeline/conflicts.py`.
//!
//! Where [`crate::validate`] enforces structural well-formedness, this finds
//! the **semantic** conflicts a multi-agent swarm creates and that must be
//! resolved before a merge:
//!
//! | kind | severity | meaning |
//! |---|---|---|
//! | `DUPLICATE_CONCEPT` | high | distinct IRIs share a normalised label |
//! | `SUBCLASS_CYCLE` | high | a cycle in `is-a` — logically impossible |
//! | `RELATION_CONTRADICTION` | medium | a class is both `is-a` and `contrasts-with` the same target |
//! | `TYPE_CONFLICT` | medium | a class's parent is declared an Individual |
//!
//! Cycles and contradictions are **blockers** for `vault propose`, never
//! approvals a human can wave through (decision Q6).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use vault_core::promotion::Blocker;

use crate::model::{Corpus, EntityType};

/// Conflict severity, ordered most-severe-first so a gate can compare ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Must be resolved before a merge.
    High,
    /// Should be resolved.
    Medium,
    /// Worth knowing.
    Low,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    /// Parse a `--severity` value, defaulting to `high`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "medium" => Self::Medium,
            "low" | "all" => Self::Low,
            _ => Self::High,
        }
    }
}

/// One detected conflict.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    /// The detector that found it.
    pub kind: String,
    /// How serious it is.
    pub severity: Severity,
    /// The IRIs involved.
    pub subjects: Vec<String>,
    /// One line a human can act on.
    pub detail: String,
}

impl Conflict {
    /// Render as a promotion [`Blocker`].
    #[must_use]
    pub fn to_blocker(&self) -> Blocker {
        Blocker::new(self.kind.clone(), self.detail.clone())
    }
}

/// A scan's result.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Counts by kind and severity.
    pub summary: Summary,
    /// The severity at or above which a conflict blocks.
    pub severity_gate: String,
    /// Every conflict, most severe first.
    pub conflicts: Vec<Conflict>,
}

/// Aggregate counts.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Total conflicts.
    pub total: usize,
    /// Count per kind.
    pub by_kind: BTreeMap<String, usize>,
    /// Count per severity.
    pub by_severity: BTreeMap<String, usize>,
}

impl Report {
    /// Conflicts at or above the gate.
    #[must_use]
    pub fn blocking(&self) -> Vec<&Conflict> {
        let gate = Severity::parse(&self.severity_gate);
        self.conflicts
            .iter()
            .filter(|c| c.severity <= gate)
            .collect()
    }

    /// `true` when nothing blocks.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.blocking().is_empty()
    }

    /// `0` when nothing blocks, `1` otherwise.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.ok())
    }
}

fn normalise_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_owned()
}

/// Distinct IRIs sharing a normalised label — likely un-merged duplicates.
fn duplicate_concepts(corpus: &Corpus) -> Vec<Conflict> {
    let mut by_label: BTreeMap<String, Vec<&crate::model::ClassRecord>> = BTreeMap::new();
    for r in entities(corpus) {
        by_label
            .entry(normalise_label(&r.label))
            .or_default()
            .push(r);
    }
    by_label
        .into_iter()
        .filter(|(label, _)| !label.is_empty())
        .filter_map(|(_, group)| {
            let mut iris: Vec<String> = group.iter().map(|r| r.iri.clone()).collect();
            iris.sort();
            iris.dedup();
            if iris.len() < 2 {
                return None;
            }
            let mut definitions: Vec<&str> = group
                .iter()
                .map(|r| r.definition.trim())
                .filter(|d| !d.is_empty())
                .collect();
            definitions.sort_unstable();
            definitions.dedup();
            let mut detail = format!("{} classes share label \"{}\"", iris.len(), group[0].label);
            if definitions.len() > 1 {
                detail
                    .push_str(" with DIFFERING definitions (contradiction, not just a duplicate)");
            }
            Some(Conflict {
                kind: "DUPLICATE_CONCEPT".into(),
                severity: Severity::High,
                subjects: iris,
                detail,
            })
        })
        .collect()
}

/// Cycles in the `is-a` graph, found with an iterative DFS so a deep hierarchy
/// cannot blow the stack.
fn subclass_cycles(corpus: &Corpus) -> Vec<Conflict> {
    #[derive(Clone, Copy, PartialEq)]
    enum Colour {
        White,
        Grey,
        Black,
    }

    let parents: BTreeMap<&str, Vec<&str>> = entities(corpus)
        .map(|r| {
            (
                r.iri.as_str(),
                r.sub_class_of
                    .iter()
                    .filter(|p| !p.iri.is_empty())
                    .map(|p| p.iri.as_str())
                    .collect(),
            )
        })
        .collect();

    let mut colour: HashMap<&str, Colour> = HashMap::new();
    let mut seen: std::collections::HashSet<Vec<&str>> = std::collections::HashSet::new();
    let mut out = Vec::new();

    for root in parents.keys() {
        if *colour.get(root).unwrap_or(&Colour::White) != Colour::White {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(root, 0)];
        let mut path: Vec<&str> = Vec::new();
        while let Some((node, idx)) = stack.pop() {
            if idx == 0 {
                colour.insert(node, Colour::Grey);
                path.push(node);
            }
            let kids = parents.get(node).map_or(&[][..], Vec::as_slice);
            if idx < kids.len() {
                stack.push((node, idx + 1));
                let child = kids[idx];
                match colour.get(child).copied().unwrap_or(Colour::White) {
                    Colour::Grey => {
                        let start = path.iter().position(|n| *n == child).unwrap_or(0);
                        let cycle: Vec<&str> = path[start..].to_vec();
                        let mut key = cycle.clone();
                        key.sort_unstable();
                        if seen.insert(key) {
                            out.push(Conflict {
                                kind: "SUBCLASS_CYCLE".into(),
                                severity: Severity::High,
                                subjects: cycle.iter().map(|s| (*s).to_owned()).collect(),
                                detail: format!("is-a cycle: {} -> {child}", cycle.join(" -> ")),
                            });
                        }
                    }
                    Colour::White if parents.contains_key(child) => {
                        stack.push((child, 0));
                    }
                    _ => {}
                }
            } else {
                colour.insert(node, Colour::Black);
                path.pop();
            }
        }
    }
    out
}

/// A class that both `is-a` and `contrasts-with` the same target.
fn relation_contradictions(corpus: &Corpus) -> Vec<Conflict> {
    let mut out = Vec::new();
    for r in entities(corpus) {
        let parents: std::collections::BTreeSet<&str> = r
            .sub_class_of
            .iter()
            .map(|p| p.iri.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        let contrasts: std::collections::BTreeSet<&str> = r
            .relation("contrastsWith")
            .iter()
            .map(|p| p.iri.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        for target in parents.intersection(&contrasts) {
            out.push(Conflict {
                kind: "RELATION_CONTRADICTION".into(),
                severity: Severity::Medium,
                subjects: vec![r.iri.clone(), (*target).to_owned()],
                detail: format!("{} is both is-a and contrasts-with {target}", r.iri),
            });
        }
    }
    out
}

/// A class whose `is-a` parent is declared an Individual.
fn type_conflicts(corpus: &Corpus) -> Vec<Conflict> {
    let types: HashMap<&str, EntityType> = entities(corpus)
        .map(|r| (r.iri.as_str(), r.entity_type))
        .collect();
    let mut out = Vec::new();
    for r in entities(corpus) {
        for parent in &r.sub_class_of {
            if types.get(parent.iri.as_str()) == Some(&EntityType::Individual) {
                out.push(Conflict {
                    kind: "TYPE_CONFLICT".into(),
                    severity: Severity::Medium,
                    subjects: vec![r.iri.clone(), parent.iri.clone()],
                    detail: format!(
                        "{} is-a {}, but {} is an Individual, not a Class",
                        r.iri, parent.iri, parent.iri
                    ),
                });
            }
        }
    }
    out
}

fn entities(corpus: &Corpus) -> impl Iterator<Item = &crate::model::ClassRecord> {
    corpus
        .records
        .iter()
        .filter(|r| r.has_ontology && !r.iri.is_empty())
}

/// Run every detector.
#[must_use]
pub fn analyse(corpus: &Corpus, severity_gate: &str) -> Report {
    let mut conflicts: Vec<Conflict> = duplicate_concepts(corpus)
        .into_iter()
        .chain(subclass_cycles(corpus))
        .chain(relation_contradictions(corpus))
        .chain(type_conflicts(corpus))
        .collect();
    conflicts.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| a.kind.cmp(&b.kind))
    });

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_severity: BTreeMap<String, usize> = BTreeMap::new();
    for c in &conflicts {
        *by_kind.entry(c.kind.clone()).or_insert(0) += 1;
        *by_severity
            .entry(c.severity.as_str().to_owned())
            .or_insert(0) += 1;
    }

    Report {
        summary: Summary {
            total: conflicts.len(),
            by_kind,
            by_severity,
        },
        severity_gate: severity_gate.to_owned(),
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use vault_core::page::{Page, Vault, VaultKind};
    use vault_core::vocabulary::Vocabulary;

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:            { owl: "rdfs:subClassOf" }
  contrasts-with:  { owl: "vc:contrastsWith" }
"#,
        )
        .unwrap()
    }

    fn corpus(pages: Vec<(&str, &str)>) -> Corpus {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: pages
                .into_iter()
                .map(|(id, fm)| {
                    Page::parse(
                        format!("/v/pages/{id}.md"),
                        format!("pages/{id}.md"),
                        id,
                        &format!("---\ntype: Class\npublic: true\n{fm}---\n"),
                    )
                    .unwrap()
                })
                .collect(),
        };
        Corpus::build(&vault, &vocab())
    }

    fn kinds(r: &Report) -> Vec<&str> {
        r.conflicts.iter().map(|c| c.kind.as_str()).collect()
    }

    #[test]
    fn a_clean_corpus_has_no_conflicts() {
        let r = analyse(&corpus(vec![("A", ""), ("B", "")]), "high");
        assert!(r.ok());
        assert_eq!(r.exit_code(), 0);
        assert_eq!(r.summary.total, 0);
    }

    #[test]
    fn two_iris_with_the_same_label_are_a_high_duplicate() {
        let r = analyse(
            &corpus(vec![
                ("A", "resource: urn:ngm:class:one\nlabel: Knowledge Graph\n"),
                ("B", "resource: urn:ngm:class:two\nlabel: knowledge-graph\n"),
            ]),
            "high",
        );
        assert_eq!(kinds(&r), vec!["DUPLICATE_CONCEPT"]);
        assert!(!r.ok());
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn differing_definitions_are_called_out_as_a_contradiction() {
        let r = analyse(
            &corpus(vec![
                (
                    "A",
                    "resource: urn:ngm:class:one\nlabel: Graph\ndefinition: One thing.\n",
                ),
                (
                    "B",
                    "resource: urn:ngm:class:two\nlabel: Graph\ndefinition: Another thing.\n",
                ),
            ]),
            "high",
        );
        assert!(r.conflicts[0].detail.contains("DIFFERING definitions"));
    }

    #[test]
    fn a_subclass_cycle_is_detected_once() {
        let r = analyse(
            &corpus(vec![
                ("A", "is-a: [\"[[B]]\"]\n"),
                ("B", "is-a: [\"[[C]]\"]\n"),
                ("C", "is-a: [\"[[A]]\"]\n"),
            ]),
            "high",
        );
        assert_eq!(
            r.conflicts
                .iter()
                .filter(|c| c.kind == "SUBCLASS_CYCLE")
                .count(),
            1
        );
    }

    #[test]
    fn a_relation_contradiction_is_medium() {
        let r = analyse(
            &corpus(vec![
                ("A", "is-a: [\"[[B]]\"]\ncontrasts-with: [\"[[B]]\"]\n"),
                ("B", ""),
            ]),
            "high",
        );
        let c = r
            .conflicts
            .iter()
            .find(|c| c.kind == "RELATION_CONTRADICTION")
            .unwrap();
        assert_eq!(c.severity, Severity::Medium);
        // The default gate is `high`, so a medium conflict does not block.
        assert!(r.ok());
    }

    #[test]
    fn the_gate_can_be_lowered_to_catch_medium_conflicts() {
        let r = analyse(
            &corpus(vec![
                ("A", "is-a: [\"[[B]]\"]\ncontrasts-with: [\"[[B]]\"]\n"),
                ("B", ""),
            ]),
            "medium",
        );
        assert!(!r.ok());
    }

    #[test]
    fn a_class_under_an_individual_is_a_type_conflict() {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: vec![
                Page::parse(
                    "/v/pages/A.md",
                    "pages/A.md",
                    "A",
                    "---\ntype: Class\npublic: true\nis-a: [\"[[B]]\"]\n---\n",
                )
                .unwrap(),
                Page::parse(
                    "/v/pages/B.md",
                    "pages/B.md",
                    "B",
                    "---\ntype: Individual\npublic: true\n---\n",
                )
                .unwrap(),
            ],
        };
        let r = analyse(&Corpus::build(&vault, &vocab()), "high");
        assert!(kinds(&r).contains(&"TYPE_CONFLICT"));
    }

    #[test]
    fn conflicts_become_blockers() {
        let r = analyse(
            &corpus(vec![
                ("A", "is-a: [\"[[B]]\"]\n"),
                ("B", "is-a: [\"[[A]]\"]\n"),
            ]),
            "high",
        );
        let blockers: Vec<_> = r.blocking().iter().map(|c| c.to_blocker()).collect();
        assert_eq!(blockers[0].code, "SUBCLASS_CYCLE");
    }

    #[test]
    fn label_normalisation_ignores_punctuation_and_case() {
        assert_eq!(normalise_label("Knowledge-Graph!"), "knowledge graph");
        assert_eq!(normalise_label("  A/B  Testing "), "a b testing");
        assert_eq!(normalise_label("---"), "");
    }

    #[test]
    fn non_ontology_records_are_ignored() {
        let vault = Vault {
            root: std::path::PathBuf::new(),
            kind: VaultKind::Knowledge,
            journals: Vec::new(),
            skipped: Vec::new(),
            pages: vec![
                Page::parse(
                    "/v/pages/A.md",
                    "pages/A.md",
                    "A",
                    "---\npublic: true\n---\n",
                )
                .unwrap(),
                Page::parse(
                    "/v/pages/B.md",
                    "pages/B.md",
                    "B",
                    "---\npublic: true\n---\n",
                )
                .unwrap(),
            ],
        };
        let _ = IndexMap::<String, usize>::new();
        assert!(analyse(&Corpus::build(&vault, &vocab()), "high").ok());
    }
}
