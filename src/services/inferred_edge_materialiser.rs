//! Materialise reasoner-entailed (inferred) subclass relations as GraphData edges
//! for the Wave 3 asserted/inferred visual channel.
//!
//! The whelk-rs reasoner produces inferred subclass axioms (class-level). This
//! module projects the DIRECT inferred parents of each class onto graph edges,
//! tagged so the topology wire can carry `inferred:true` and the XR edge shader
//! renders them amber-dashed (see `render_store::edge_style_code_prov`).
//!
//! ## Design choices (bounded on purpose)
//! * **Direct parents only, not the full closure.** The subclass transitive
//!   closure is up to quadratic; materialising every entailed ancestor link would
//!   swamp the 7548 asserted hierarchical edges. We feed only the reasoner's
//!   DIRECT inferred `SubClassOf` axioms (one hop), and additionally cap the
//!   number of inferred parents per child ([`DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD`]).
//! * **Minus asserted.** An inferred pair is dropped when the SAME node pair
//!   already exists as an asserted edge (either direction) — inference must never
//!   duplicate or regress an asserted edge.
//! * **Additive + tagged.** Materialised edges reuse the `"hierarchical"`
//!   edge-type (so they also fold under the L2 ladder) and carry
//!   `metadata["inferred"] = "true"`; the initial-graph-load builders read that
//!   flag onto `InitialEdgeData.inferred`.
//! * **Gated.** Because it grows the edge set and feeds physics, materialisation
//!   is opt-in via [`InferredMaterialisationConfig::enabled`] (default OFF).

use std::collections::{HashMap, HashSet};
use visionclaw_domain::models::edge::{Edge, RDFS_SUBCLASS_OF_IRI};

/// Edge-type of a materialised inferred subclass edge — the same class label the
/// asserted hierarchy edges use, so inferred edges also fold under the L2 ladder.
pub const INFERRED_EDGE_TYPE: &str = "hierarchical";

/// Metadata key marking an edge as a reasoner entailment.
pub const INFERRED_META_KEY: &str = "inferred";

/// Default per-child cap on materialised inferred parents (safety valve against a
/// pathological closure). Overridable via [`InferredMaterialisationConfig`].
pub const DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD: usize = 8;

/// Opt-in configuration for inferred-edge materialisation.
#[derive(Debug, Clone, Copy)]
pub struct InferredMaterialisationConfig {
    /// Master switch. OFF by default — materialisation grows the edge set and
    /// feeds physics, so it is enabled deliberately (settings / feature flag).
    pub enabled: bool,
    /// Per-child cap on inferred parents.
    pub max_parents_per_child: usize,
}

impl Default for InferredMaterialisationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_parents_per_child: DEFAULT_MAX_INFERRED_PARENTS_PER_CHILD,
        }
    }
}

/// Whether an edge is a materialised inferred edge (reads the provenance tag).
pub fn edge_is_inferred(edge: &Edge) -> bool {
    edge.metadata
        .as_ref()
        .and_then(|m| m.get(INFERRED_META_KEY))
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Build the tagged GraphData edge for a materialised inferred child→parent pair.
pub fn build_inferred_edge(child: u32, parent: u32) -> Edge {
    Edge::new(child, parent, 1.0)
        .with_edge_type(INFERRED_EDGE_TYPE.to_string())
        .with_owl_property_iri(RDFS_SUBCLASS_OF_IRI.to_string())
        .add_metadata(INFERRED_META_KEY.to_string(), "true".to_string())
}

/// Asserted node-pair membership set (BOTH directions) over the current graph
/// edges — the "minus asserted" guard. Built from ALL edges (not just hierarchy)
/// so a materialised inferred edge never duplicates the geometry of ANY existing
/// edge between the same nodes.
pub fn asserted_pairs(edges: &[Edge]) -> HashSet<(u32, u32)> {
    let mut set = HashSet::with_capacity(edges.len() * 2);
    for e in edges {
        set.insert((e.source, e.target));
        set.insert((e.target, e.source));
    }
    set
}

/// Transitive reduction of an inferred subclass ancestor map to IMMEDIATE parents.
///
/// The reasoner's `infer_transitive_subclass` yields TRANSITIVE ancestors (child →
/// every ancestor), so materialising them directly would draw long-range edges
/// from a class to distant ancestors. This keeps, for each child, only the
/// ancestors that are not themselves an ancestor of another kept ancestor — i.e.
/// the nearest inferred parents. `child_to_ancestors` maps a class IRI to its
/// inferred ancestor IRIs (excluding itself); an ancestor whose own ancestor set
/// is unknown is treated as having none (safe under-reduction — never over-prunes).
/// Output is `(child, immediate_parent)` IRI pairs, deterministic (sorted).
pub fn immediate_inferred_parents(
    child_to_ancestors: &HashMap<String, HashSet<String>>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (child, ancestors) in child_to_ancestors {
        for p in ancestors {
            if p == child {
                continue;
            }
            // Drop P if some OTHER ancestor Q of `child` has P as its own ancestor
            // (then P is a grandparent-or-higher via Q, not immediate).
            let redundant = ancestors.iter().any(|q| {
                q != p && q != child && child_to_ancestors.get(q).is_some_and(|qa| qa.contains(p))
            });
            if !redundant {
                out.push((child.clone(), p.clone()));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Pure materialisation set-logic. From candidate inferred `(child, parent)`
/// node-id pairs, keep those that are (a) not self-loops, (b) not already asserted
/// in either direction, (c) unique, and (d) within the per-child cap. Output is
/// deterministic (sorted by child then parent) so the same reasoner state always
/// yields the same edge set.
pub fn select_inferred_edges(
    candidates: &[(u32, u32)],
    asserted: &HashSet<(u32, u32)>,
    cap_per_child: usize,
) -> Vec<(u32, u32)> {
    let mut sorted = candidates.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut per_child: HashMap<u32, usize> = HashMap::new();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (c, p) in sorted {
        if c == p {
            continue; // no self-loops
        }
        if asserted.contains(&(c, p)) || asserted.contains(&(p, c)) {
            continue; // already asserted — inference must not duplicate/regress
        }
        let n = per_child.entry(c).or_insert(0);
        if *n >= cap_per_child {
            continue; // per-child volume cap
        }
        *n += 1;
        out.push((c, p));
    }
    out
}

/// IRI substring marking the OWL bottom class (`owl:Nothing`). A subclass axiom
/// whose CHILD is bottom is a vacuous entailment (bottom is a subclass of
/// everything) and is never materialised.
pub const OWL_BOTTOM_MARKER: &str = "owl#Nothing";

/// IRI substring marking the OWL top class (`owl:Thing`). A subclass axiom whose
/// PARENT is top is a vacuous entailment (everything is a subclass of top) and is
/// never materialised.
pub const OWL_TOP_MARKER: &str = "owl#Thing";

/// Whether a reasoner-entailed `SubClassOf` IRI pair is a materialisation
/// candidate at all: not a self-subsumption, not rooted at `owl:Nothing`, not
/// terminating at `owl:Thing`. This is the single definition of the vacuous-axiom
/// filter — callers must not re-implement it (ADR-2071).
pub fn is_materialisable_subclass_pair(child: &str, parent: &str) -> bool {
    child != parent && !child.contains(OWL_BOTTOM_MARKER) && !parent.contains(OWL_TOP_MARKER)
}

/// Reduce raw reasoner `SubClassOf` IRI pairs to IMMEDIATE inferred parents.
///
/// Applies [`is_materialisable_subclass_pair`] to drop vacuous entailments, groups
/// the survivors into the child → ancestor-IRI map that
/// [`immediate_inferred_parents`] consumes, and returns its transitive reduction.
/// This is the front door for any caller holding reasoner axioms: it keeps the
/// vacuous-axiom filter and the transitive reduction in one place so the sync path
/// and the pipeline path cannot drift (ADR-2071). Output is deterministic (sorted).
pub fn immediate_parents_from_subclass_pairs<'a, I>(pairs: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut child_to_ancestors: HashMap<String, HashSet<String>> = HashMap::new();
    for (child, parent) in pairs {
        if !is_materialisable_subclass_pair(child, parent) {
            continue;
        }
        child_to_ancestors
            .entry(child.to_string())
            .or_default()
            .insert(parent.to_string());
    }
    immediate_inferred_parents(&child_to_ancestors)
}

/// End-to-end helper: given resolved candidate inferred pairs and the current
/// graph edges, return the tagged [`Edge`]s to add (empty when disabled). Keeps
/// the gate + set-logic + edge-construction in one testable place; the caller
/// only resolves IRIs→node ids and calls `graph_repo.add_edges`.
pub fn materialise(
    candidates: &[(u32, u32)],
    current_edges: &[Edge],
    config: &InferredMaterialisationConfig,
) -> Vec<Edge> {
    if !config.enabled {
        return Vec::new();
    }
    let asserted = asserted_pairs(current_edges);
    select_inferred_edges(candidates, &asserted, config.max_parents_per_child)
        .into_iter()
        .map(|(c, p)| build_inferred_edge(c, p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(s: u32, t: u32, ty: &str) -> Edge {
        Edge::new(s, t, 1.0).with_edge_type(ty.to_string())
    }

    #[test]
    fn drops_pairs_already_asserted_either_direction() {
        let current = vec![edge(1, 2, "hierarchical"), edge(3, 4, "explicit_link")];
        let asserted = asserted_pairs(&current);
        // (1,2) asserted same dir; (4,3) asserted reverse dir; (5,6) genuinely new.
        let got = select_inferred_edges(&[(1, 2), (4, 3), (5, 6)], &asserted, 8);
        assert_eq!(got, vec![(5, 6)], "only the non-asserted pair survives");
    }

    #[test]
    fn skips_self_loops_and_dedups() {
        let asserted = HashSet::new();
        let got = select_inferred_edges(&[(7, 7), (5, 6), (5, 6)], &asserted, 8);
        assert_eq!(got, vec![(5, 6)], "self-loop dropped, duplicate collapsed");
    }

    #[test]
    fn caps_inferred_parents_per_child() {
        let asserted = HashSet::new();
        // Child 1 has five candidate parents; cap 2 keeps the two smallest
        // (deterministic sorted order), other children unaffected.
        let cands = [(1, 5), (1, 4), (1, 3), (1, 2), (1, 6), (9, 8)];
        let got = select_inferred_edges(&cands, &asserted, 2);
        assert_eq!(
            got,
            vec![(1, 2), (1, 3), (9, 8)],
            "cap=2 per child, deterministic"
        );
    }

    #[test]
    fn deterministic_order_regardless_of_input_order() {
        let asserted = HashSet::new();
        let a = select_inferred_edges(&[(3, 1), (1, 2), (2, 5)], &asserted, 8);
        let b = select_inferred_edges(&[(2, 5), (3, 1), (1, 2)], &asserted, 8);
        assert_eq!(a, b, "output independent of candidate order");
        assert_eq!(a, vec![(1, 2), (2, 5), (3, 1)]);
    }

    #[test]
    fn build_inferred_edge_is_tagged_and_hierarchical() {
        let e = build_inferred_edge(10, 20);
        assert_eq!(e.source, 10);
        assert_eq!(e.target, 20);
        assert_eq!(e.edge_type.as_deref(), Some("hierarchical"));
        assert!(edge_is_inferred(&e), "carries the inferred provenance tag");
        // An untagged asserted edge is not inferred.
        assert!(!edge_is_inferred(&edge(1, 2, "hierarchical")));
    }

    #[test]
    fn materialise_is_gated_off_by_default() {
        let cfg = InferredMaterialisationConfig::default();
        assert!(!cfg.enabled);
        let out = materialise(&[(5, 6)], &[], &cfg);
        assert!(out.is_empty(), "disabled ⇒ no materialised edges");
    }

    #[test]
    fn materialise_end_to_end_when_enabled() {
        let current = vec![edge(1, 2, "hierarchical")]; // asserted 1—2
        let cfg = InferredMaterialisationConfig {
            enabled: true,
            max_parents_per_child: 8,
        };
        // (2,1) is asserted (reverse), (3,4) is new → one tagged edge.
        let out = materialise(&[(2, 1), (3, 4)], &current, &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].source, out[0].target), (3, 4));
        assert!(edge_is_inferred(&out[0]));
        assert_eq!(out[0].edge_type.as_deref(), Some("hierarchical"));
    }

    #[test]
    fn transitive_reduction_keeps_only_immediate_parents() {
        // 3-level chain A ⊑ B ⊑ C. The reasoner's TRANSITIVE closure gives A its
        // ancestors {B, C} and B its ancestor {C}. Reduction must keep A→B (drop
        // A→C, since C is an ancestor of B) and B→C.
        let mut m: HashMap<String, HashSet<String>> = HashMap::new();
        m.insert(
            "A".into(),
            ["B", "C"].iter().map(|s| s.to_string()).collect(),
        );
        m.insert("B".into(), ["C"].iter().map(|s| s.to_string()).collect());
        let got = immediate_inferred_parents(&m);
        assert_eq!(
            got,
            vec![
                ("A".to_string(), "B".to_string()),
                ("B".to_string(), "C".to_string())
            ],
            "A→C (grandparent) is reduced out; immediate parents only"
        );
    }

    #[test]
    fn transitive_reduction_handles_diamond() {
        // Diamond: D ⊑ B, D ⊑ C, B ⊑ A, C ⊑ A. D's transitive ancestors {A,B,C}.
        // Immediate parents of D are B and C (A is reachable via both → dropped).
        let mut m: HashMap<String, HashSet<String>> = HashMap::new();
        m.insert(
            "D".into(),
            ["A", "B", "C"].iter().map(|s| s.to_string()).collect(),
        );
        m.insert("B".into(), ["A"].iter().map(|s| s.to_string()).collect());
        m.insert("C".into(), ["A"].iter().map(|s| s.to_string()).collect());
        let got = immediate_inferred_parents(&m);
        assert_eq!(
            got,
            vec![
                ("B".to_string(), "A".to_string()),
                ("C".to_string(), "A".to_string()),
                ("D".to_string(), "B".to_string()),
                ("D".to_string(), "C".to_string()),
            ],
            "D→A dropped (via B and C); B→A, C→A kept"
        );
    }

    #[test]
    fn mirror_of_asserted_pair_is_never_materialised() {
        // Regression for the injection HIGH: an inferred pair that mirrors an
        // asserted edge must be dropped, so the materialised Edge (whose id is
        // `source-target`) can never collide with and overwrite the asserted edge.
        let asserted_graph = vec![edge(10, 20, "hierarchical")]; // asserted 10⊑20
        let cfg = InferredMaterialisationConfig {
            enabled: true,
            max_parents_per_child: 8,
        };
        // Candidate (10,20) mirrors the asserted edge exactly; (20,10) is the
        // reverse; only (10,30) is genuinely new.
        let out = materialise(&[(10, 20), (20, 10), (10, 30)], &asserted_graph, &cfg);
        let pairs: Vec<(u32, u32)> = out.iter().map(|e| (e.source, e.target)).collect();
        assert_eq!(
            pairs,
            vec![(10, 30)],
            "asserted pair (both dirs) never re-emitted"
        );
        // And the surviving edge's id does not collide with the asserted edge's id.
        assert_ne!(out[0].id, edge(10, 20, "hierarchical").id);
    }

    #[test]
    fn does_not_regress_asserted_edges() {
        // A graph of only asserted hierarchical edges; every candidate mirrors an
        // asserted pair → nothing materialised, asserted set untouched.
        let current: Vec<Edge> = (0..7548u32)
            .map(|i| edge(i, i + 1, "hierarchical"))
            .collect();
        let cfg = InferredMaterialisationConfig {
            enabled: true,
            max_parents_per_child: 8,
        };
        let cands: Vec<(u32, u32)> = (0..7548u32).map(|i| (i, i + 1)).collect();
        let out = materialise(&cands, &current, &cfg);
        assert!(
            out.is_empty(),
            "all candidates already asserted → no new edges"
        );
    }

    #[test]
    fn vacuous_subclass_pairs_are_rejected() {
        // ADR-2071: the single definition of the vacuous-axiom filter.
        assert!(is_materialisable_subclass_pair("urn:c:A", "urn:c:B"));
        assert!(
            !is_materialisable_subclass_pair("urn:c:A", "urn:c:A"),
            "self"
        );
        assert!(
            !is_materialisable_subclass_pair("http://www.w3.org/2002/07/owl#Nothing", "urn:c:A"),
            "bottom child"
        );
        assert!(
            !is_materialisable_subclass_pair("urn:c:A", "http://www.w3.org/2002/07/owl#Thing"),
            "top parent"
        );
        // Bottom as PARENT and top as CHILD are genuine (non-vacuous) entailments
        // and are not filtered here.
        assert!(is_materialisable_subclass_pair(
            "urn:c:A",
            "http://www.w3.org/2002/07/owl#Nothing"
        ));
    }

    #[test]
    fn pairs_front_door_filters_then_reduces() {
        // Raw reasoner output for A ⊑ B ⊑ C, transitively closed, plus vacuous
        // noise. One call must drop the noise AND the long-range A→C pair.
        let got = immediate_parents_from_subclass_pairs([
            ("A", "B"),
            ("A", "C"),
            ("B", "C"),
            ("A", "A"),
            ("owl#Nothing", "B"),
            ("B", "owl#Thing"),
        ]);
        assert_eq!(
            got,
            vec![
                ("A".to_string(), "B".to_string()),
                ("B".to_string(), "C".to_string())
            ],
            "vacuous pairs dropped, A→C reduced out"
        );
    }

    #[test]
    fn pairs_front_door_matches_manual_map_construction() {
        // Equivalence with the map-building the pipeline path does inline, so the
        // two callers provably share one reduction.
        let raw = [("D", "A"), ("D", "B"), ("D", "C"), ("B", "A"), ("C", "A")];
        let mut m: HashMap<String, HashSet<String>> = HashMap::new();
        for (c, p) in raw {
            m.entry(c.to_string()).or_default().insert(p.to_string());
        }
        assert_eq!(
            immediate_parents_from_subclass_pairs(raw),
            immediate_inferred_parents(&m)
        );
    }
}
