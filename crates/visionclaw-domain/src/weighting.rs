//! The physics edge-weight function (ADR-2204 D3, seed of slice S3).
//!
//! [`physics_edge_weights`] is the one place the layout's per-edge attraction
//! weight is derived from a graph. With the default profile it returns each
//! edge's stored `weight` bit for bit. With `hierarchy_affinity > 0` it
//! strengthens *asserted* hierarchy edges by the intrinsic information content
//! of the parent class (Sánchez et al. 2011), so broad classes pull weakly and
//! specific ones strongly (ADR-2204 D5, ontology mode).
//!
//! Hierarchy edges are recognised through the relation schema
//! ([`crate::relations`], kind `hierarchy`); this module holds no relation
//! labels or weights of its own.
//!
//! A [`LayoutExperiment`] is a development-only control: it withholds a
//! deterministic fraction of asserted hierarchy edges (weight 0 in every arm,
//! excluded from the information content) so a layout can be scored on pairs it
//! never saw, and offers a null arm that applies the same multipliers to
//! non-hierarchy edges.
//!
//! # Example
//!
//! ```
//! use visionclaw_domain::weighting::{physics_edge_weights, WeightingProfile};
//! use visionclaw_domain::{Edge, Node};
//!
//! let nodes: Vec<Node> = (1..=3u32)
//!     .map(|i| Node::new_with_id(format!("n{i}"), Some(i)))
//!     .collect();
//! let edges = vec![Edge::new(1, 2, 1.0), Edge::new(1, 3, 1.0)];
//! // Off: weights come back exactly as stored.
//! let w = physics_edge_weights(&nodes, &edges, &WeightingProfile::default());
//! assert_eq!(w, vec![1.0, 1.0]);
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::models::edge::{Edge, EdgeProvenance, RDFS_SUBCLASS_OF_IRI};
use crate::models::node::Node;
use crate::relations::{RelationKind, RelationSchema};
use serde::{Deserialize, Serialize};

/// Development-only layout experiment (ADR-2204 D5 pre-registration).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutExperiment {
    /// Seed of the deterministic holdout and null-arm selections.
    pub holdout_seed: u64,
    /// Fraction (0..=1) of distinct asserted hierarchy pairs withheld.
    pub holdout_fraction: f32,
    /// Apply the hierarchy multipliers to a random sample of non-hierarchy
    /// edges instead of to the hierarchy.
    #[serde(default)]
    pub null_arm: bool,
}

impl LayoutExperiment {
    /// Whether the experiment is well formed: a finite fraction in `0..=1`.
    pub fn is_valid(&self) -> bool {
        self.holdout_fraction.is_finite() && (0.0..=1.0).contains(&self.holdout_fraction)
    }
}

/// What shapes the physics weights.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeightingProfile {
    /// Strength `a` of the hierarchy boost; `0` (or non-finite) is off.
    pub hierarchy_affinity: f32,
    /// Optional development experiment.
    pub experiment: Option<LayoutExperiment>,
}

/// One withheld hierarchy edge, for scoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldPair {
    /// Index of the edge in the slice given to the weighting function.
    pub edge_index: usize,
    /// Child node id (the edge `source`).
    pub source: u32,
    /// Parent node id (the edge `target`).
    pub target: u32,
}

/// Per-edge physics weights, aligned to `edges`.
///
/// With the default profile every weight is the edge's stored `weight`
/// unchanged. See the module documentation for the boost, the holdout and the
/// null arm.
pub fn physics_edge_weights(
    nodes: &[Node],
    edges: &[Edge],
    profile: &WeightingProfile,
) -> Vec<f32> {
    let mut out: Vec<f32> = edges.iter().map(|e| e.weight).collect();
    let affinity = effective_affinity(profile.hierarchy_affinity);
    if affinity == 0.0 && profile.experiment.is_none() {
        return out;
    }
    let ctx = Context::new(nodes, edges);
    let held = profile
        .experiment
        .map(|x| ctx.withheld_indices(&x))
        .unwrap_or_default();

    if affinity > 0.0 {
        let ic = intrinsic_ic(&ctx, &held);
        // Multipliers of the asserted, non-withheld hierarchy edges, in edge order.
        let mut boosts: Vec<(usize, f32)> = Vec::new();
        for &i in &ctx.asserted {
            if held.contains(&i) {
                continue;
            }
            let parent = edges[i].target;
            let norm = ic.get(&parent).copied().unwrap_or(0.0);
            boosts.push((i, 1.0 + affinity * norm));
        }
        match profile.experiment {
            Some(x) if x.null_arm => {
                for (j, m) in null_assignments(&ctx, &x, &boosts) {
                    out[j] *= m;
                }
            }
            _ => {
                for (i, m) in boosts {
                    out[i] *= m;
                }
            }
        }
    }
    for &i in &held {
        out[i] = 0.0;
    }
    out
}

/// The asserted hierarchy edges `experiment` withholds, in edge order.
pub fn withheld_pairs(
    nodes: &[Node],
    edges: &[Edge],
    experiment: &LayoutExperiment,
) -> Vec<WithheldPair> {
    let ctx = Context::new(nodes, edges);
    let mut idx: Vec<usize> = ctx.withheld_indices(experiment).into_iter().collect();
    idx.sort_unstable();
    idx.into_iter()
        .map(|i| WithheldPair {
            edge_index: i,
            source: edges[i].source,
            target: edges[i].target,
        })
        .collect()
}

/// How many asserted parent/child hierarchy edges `edges` holds.
pub fn asserted_hierarchy_edge_count(nodes: &[Node], edges: &[Edge]) -> usize {
    Context::new(nodes, edges).asserted.len()
}

fn effective_affinity(a: f32) -> f32 {
    if a.is_finite() && a > 0.0 {
        a
    } else {
        0.0
    }
}

/// Stable 64-bit hash (FNV-1a folded through splitmix64). `DefaultHasher`
/// is not stable across releases, and a holdout must be reproducible.
fn stable_hash(seed: u64, parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
    for part in parts {
        for b in part.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h = h.wrapping_add(0x9e37_79b9_7f4a_7c15);
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

/// Edge classification shared by every step.
struct Context<'a> {
    nodes: &'a [Node],
    edges: &'a [Edge],
    index: HashMap<u32, usize>,
    /// Indices of asserted parent/child hierarchy edges between known nodes.
    asserted: Vec<usize>,
    /// Indices of edges whose relation is of kind `hierarchy`, whatever the
    /// provenance (the null arm never lands on these).
    hierarchy_kind: HashSet<usize>,
}

impl<'a> Context<'a> {
    fn new(nodes: &'a [Node], edges: &'a [Edge]) -> Self {
        let index: HashMap<u32, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let schema = RelationSchema::builtin();
        let labels: HashSet<&str> = schema
            .relations()
            .iter()
            .filter(|r| r.kind == RelationKind::Hierarchy)
            .flat_map(|r| {
                std::iter::once(r.label.as_str()).chain(r.aliases.iter().map(String::as_str))
            })
            .collect();
        let mut asserted = Vec::new();
        let mut hierarchy_kind = HashSet::new();
        for (i, e) in edges.iter().enumerate() {
            let is_kind = e.edge_type.as_deref().is_some_and(|t| labels.contains(t));
            if !is_kind {
                continue;
            }
            hierarchy_kind.insert(i);
            // The label `hierarchical` also carries `owl:sameAs`, `equivalentClass` and
            // `instanceOf`; only an edge whose own predicate is `rdfs:subClassOf` is
            // parent/child (the rule of `Edge::asserts_subsumption`). An edge with no
            // predicate (a spoke, a wikilink) is never one.
            let parent_child = e.owl_property_iri.as_deref() == Some(RDFS_SUBCLASS_OF_IRI);
            if e.provenance == Some(EdgeProvenance::Asserted)
                && parent_child
                && e.source != e.target
                && index.contains_key(&e.source)
                && index.contains_key(&e.target)
            {
                asserted.push(i);
            }
        }
        Self {
            nodes,
            edges,
            index,
            asserted,
            hierarchy_kind,
        }
    }

    /// Persistent key of a node: its metadata id, never a compact id.
    fn key(&self, id: u32) -> &str {
        &self.nodes[self.index[&id]].metadata_id
    }

    /// Edge indices withheld by `x`: the first `floor(fraction × n)` distinct
    /// asserted pairs when ranked by a seeded stable hash of their keys.
    fn withheld_indices(&self, x: &LayoutExperiment) -> HashSet<usize> {
        let mut pairs: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        for &i in &self.asserted {
            let e = &self.edges[i];
            pairs
                .entry((self.key(e.source), self.key(e.target)))
                .or_default()
                .push(i);
        }
        let mut ranked: Vec<(u64, (&str, &str))> = pairs
            .keys()
            .map(|&(a, b)| (stable_hash(x.holdout_seed, &[a, b]), (a, b)))
            .collect();
        ranked.sort_unstable();
        let fraction = if x.holdout_fraction.is_finite() {
            x.holdout_fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let take = (fraction * ranked.len() as f32).floor() as usize;
        ranked
            .into_iter()
            .take(take)
            .flat_map(|(_, k)| pairs[&k].iter().copied())
            .collect()
    }
}

/// Sánchez et al. (2011) intrinsic information content of every class in the
/// asserted hierarchy (minus `held`), normalised to `[0, 1]` by the maximum.
///
/// `IC(c) = −ln((|leaves(c)| / |subsumers(c)| + 1) / (max_leaves + 1))`, where
/// `leaves(c)` are the leaf descendants of `c` (`c` itself when it is a leaf),
/// `subsumers(c)` is `c` plus its ancestors, and `max_leaves` is the leaf count
/// of the whole hierarchy. The closure is a statistic only; no closure edge is
/// ever produced. Cycles and multiple parents are handled by visited sets; a
/// class on a cycle with no leaf below it counts one leaf (itself).
fn intrinsic_ic(ctx: &Context<'_>, held: &HashSet<usize>) -> HashMap<u32, f32> {
    let mut parents: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    let mut children: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    for &i in &ctx.asserted {
        if held.contains(&i) {
            continue;
        }
        let e = &ctx.edges[i];
        parents.entry(e.source).or_default().insert(e.target);
        children.entry(e.target).or_default().insert(e.source);
        parents.entry(e.target).or_default();
        children.entry(e.source).or_default();
    }
    let leaf = |c: u32| children.get(&c).is_none_or(BTreeSet::is_empty);
    let max_leaves = children.keys().filter(|&&c| leaf(c)).count().max(1);

    let reach = |start: u32, next: &HashMap<u32, BTreeSet<u32>>| -> HashSet<u32> {
        let mut seen = HashSet::from([start]);
        let mut stack = vec![start];
        while let Some(c) = stack.pop() {
            for &n in next.get(&c).into_iter().flatten() {
                if seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        seen
    };

    let mut raw: HashMap<u32, f64> = HashMap::new();
    for &c in children.keys() {
        let leaves = reach(c, &children)
            .into_iter()
            .filter(|&d| leaf(d))
            .count()
            .max(1);
        let subsumers = reach(c, &parents).len();
        let ratio = leaves as f64 / subsumers as f64;
        raw.insert(c, -((ratio + 1.0) / (max_leaves as f64 + 1.0)).ln());
    }
    let max = raw.values().copied().fold(0.0f64, f64::max);
    raw.into_iter()
        .map(|(c, v)| {
            (
                c,
                if max > 0.0 {
                    (v / max).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                },
            )
        })
        .collect()
}

/// Null arm: the same multiplier values as `boosts`, on a deterministic random
/// sample of non-hierarchy edges (fewer if there are fewer to take).
fn null_assignments(
    ctx: &Context<'_>,
    x: &LayoutExperiment,
    boosts: &[(usize, f32)],
) -> Vec<(usize, f32)> {
    let mut eligible: Vec<(u64, usize)> = (0..ctx.edges.len())
        .filter(|i| !ctx.hierarchy_kind.contains(i))
        .filter(|&i| {
            ctx.index.contains_key(&ctx.edges[i].source)
                && ctx.index.contains_key(&ctx.edges[i].target)
        })
        .map(|i| {
            let e = &ctx.edges[i];
            let h = stable_hash(
                x.holdout_seed ^ 0x6e75_6c6c,
                &[
                    ctx.key(e.source),
                    ctx.key(e.target),
                    e.edge_type.as_deref().unwrap_or(""),
                ],
            );
            (h, i)
        })
        .collect();
    eligible.sort_unstable();
    boosts
        .iter()
        .zip(eligible)
        .map(|(&(_, m), (_, j))| (j, m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIER: &str = "hierarchical";

    fn node(id: u32) -> Node {
        Node::new_with_id(format!("page-{id}"), Some(id))
    }

    fn nodes(n: u32) -> Vec<Node> {
        (1..=n).map(node).collect()
    }

    /// child → parent asserted `subClassOf`.
    fn sub(child: u32, parent: u32) -> Edge {
        let mut e = Edge::new(child, parent, 1.6)
            .with_edge_type(HIER.to_string())
            .with_provenance(EdgeProvenance::Asserted);
        e.owl_property_iri = Some(RDFS_SUBCLASS_OF_IRI.to_string());
        e
    }

    fn link(a: u32, b: u32, w: f32) -> Edge {
        Edge::new(a, b, w)
            .with_edge_type("wikilink".to_string())
            .with_provenance(EdgeProvenance::Asserted)
    }

    fn on(a: f32) -> WeightingProfile {
        WeightingProfile {
            hierarchy_affinity: a,
            experiment: None,
        }
    }

    /// root(1) ← mid(2) ← leafA(3), leafB(4); root ← leafC(5).
    fn tree() -> (Vec<Node>, Vec<Edge>) {
        (
            nodes(6),
            vec![sub(2, 1), sub(3, 2), sub(4, 2), sub(5, 1), link(5, 6, 0.7)],
        )
    }

    #[test]
    fn off_returns_stored_weights_bit_for_bit() {
        let (n, mut e) = tree();
        e[4].weight = 0.1 + 0.2; // not exactly representable: bits must survive
        let w = physics_edge_weights(&n, &e, &WeightingProfile::default());
        let want: Vec<u32> = e.iter().map(|x| x.weight.to_bits()).collect();
        let got: Vec<u32> = w.iter().map(|x| x.to_bits()).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn ic_matches_a_hand_computed_oracle() {
        // leaves: 3,4,5 → max_leaves 3.
        // IC(1)=-ln((3/1+1)/4)=0; IC(2)=-ln((2/2+1)/4)=ln2; leaves ln((1/3+1)/4)…
        // Parents are 1 (IC 0) and 2 (IC ln 2). max IC over all nodes:
        // IC(3)=IC(4)=-ln((1/3+1)/4)=ln3 ; IC(5)=-ln((1/2+1)/4)=ln(8/3).
        // max = ln 3, so IC_norm(2) = ln2/ln3.
        let (n, e) = tree();
        let w = physics_edge_weights(&n, &e, &on(1.0));
        let norm2 = (2.0f64.ln() / 3.0f64.ln()) as f32;
        // edges 0 (2→1, parent 1: IC 0) unchanged
        assert_eq!(w[0], 1.6);
        // edges 1,2 (→2) boosted by 1 + ln2/ln3
        for i in [1, 2] {
            assert!((w[i] - 1.6 * (1.0 + norm2)).abs() < 1e-5, "{} {}", i, w[i]);
        }
        assert_eq!(w[3], 1.6); // →1
        assert_eq!(w[4], 0.7); // non-hierarchy untouched
    }

    #[test]
    fn affinity_scales_the_boost_linearly() {
        let (n, e) = tree();
        let a = physics_edge_weights(&n, &e, &on(1.0))[1] / 1.6 - 1.0;
        let b = physics_edge_weights(&n, &e, &on(3.0))[1] / 1.6 - 1.0;
        assert!((b - 3.0 * a).abs() < 1e-5);
    }

    #[test]
    fn inferred_and_unprovenanced_hierarchy_edges_are_not_boosted() {
        let (n, mut e) = tree();
        e[1].provenance = Some(EdgeProvenance::Inferred);
        e[2].provenance = None;
        let w = physics_edge_weights(&n, &e, &on(2.0));
        assert_eq!(w[1], 1.6);
        assert_eq!(w[2], 1.6);
    }

    #[test]
    fn same_as_style_hierarchical_edges_are_not_parent_child() {
        let (n, mut e) = tree();
        e[1].owl_property_iri = Some("http://www.w3.org/2002/07/owl#sameAs".into());
        let w = physics_edge_weights(&n, &e, &on(2.0));
        assert_eq!(w[1], 1.6);
    }

    #[test]
    fn a_hierarchical_edge_with_no_predicate_is_not_parent_child() {
        let (n, mut e) = tree();
        e[1].owl_property_iri = None;
        let w = physics_edge_weights(&n, &e, &on(2.0));
        assert_eq!(w[1], 1.6);
    }

    #[test]
    fn cycles_and_multiple_parents_terminate_with_finite_weights() {
        let n = nodes(5);
        let e = vec![
            sub(1, 2),
            sub(2, 3),
            sub(3, 1), // cycle
            sub(4, 2),
            sub(4, 3), // two parents
            sub(5, 5), // self loop
        ];
        let w = physics_edge_weights(&n, &e, &on(1.0));
        assert_eq!(w.len(), e.len());
        assert!(w.iter().all(|x| x.is_finite() && *x >= 1.6));
    }

    #[test]
    fn unknown_endpoints_and_empty_input_are_safe() {
        assert!(physics_edge_weights(&[], &[], &on(1.0)).is_empty());
        let w = physics_edge_weights(&nodes(1), &[sub(1, 99)], &on(1.0));
        assert_eq!(w, vec![1.6]);
    }

    fn exp(seed: u64, frac: f32, null_arm: bool) -> WeightingProfile {
        WeightingProfile {
            hierarchy_affinity: 2.0,
            experiment: Some(LayoutExperiment {
                holdout_seed: seed,
                holdout_fraction: frac,
                null_arm,
            }),
        }
    }

    /// A wide two-level tree (39 hierarchy edges) followed by plain links.
    fn bigger() -> (Vec<Node>, Vec<Edge>) {
        let n = nodes(60);
        let mut e = Vec::new();
        for c in 2..=10u32 {
            e.push(sub(c, 1));
        }
        for c in 11..=40u32 {
            e.push(sub(c, 2 + (c % 9)));
        }
        for c in 41..=59u32 {
            e.push(link(c, c + 1, 1.0 + (c % 3) as f32 * 0.25));
        }
        // Enough plain links that the null arm is never short of edges.
        for a in 1..=58u32 {
            e.push(link(a, a + 2, 0.5));
        }
        (n, e)
    }

    #[test]
    fn holdout_is_deterministic_and_zeroes_in_every_arm() {
        let (n, e) = bigger();
        let x = LayoutExperiment {
            holdout_seed: 7,
            holdout_fraction: 0.25,
            null_arm: false,
        };
        let held = withheld_pairs(&n, &e, &x);
        assert_eq!(held, withheld_pairs(&n, &e, &x));
        assert_eq!(held.len(), (39.0f32 * 0.25).floor() as usize);
        for null in [false, true] {
            let w = physics_edge_weights(&n, &e, &exp(7, 0.25, null));
            for p in &held {
                assert_eq!(w[p.edge_index], 0.0);
            }
        }
        // Also with the boost off.
        let off = WeightingProfile {
            hierarchy_affinity: 0.0,
            experiment: Some(x),
        };
        let w = physics_edge_weights(&n, &e, &off);
        for p in &held {
            assert_eq!(w[p.edge_index], 0.0);
        }
    }

    #[test]
    fn holdout_is_independent_of_edge_order_and_node_ids() {
        let (n, e) = bigger();
        let x = LayoutExperiment {
            holdout_seed: 11,
            holdout_fraction: 0.3,
            null_arm: false,
        };
        let key = |nodes: &[Node], edges: &[Edge], p: &WithheldPair| {
            let name = |id: u32| {
                nodes
                    .iter()
                    .find(|n| n.id == id)
                    .unwrap()
                    .metadata_id
                    .clone()
            };
            let _ = edges;
            (name(p.source), name(p.target))
        };
        let mut a: Vec<_> = withheld_pairs(&n, &e, &x)
            .iter()
            .map(|p| key(&n, &e, p))
            .collect();
        // Reverse the edges and renumber every node id.
        let mut n2 = n.clone();
        for (i, node) in n2.iter_mut().enumerate() {
            node.id = 1000 + 7 * i as u32;
        }
        let remap = |id: u32| 1000 + 7 * (id - 1);
        let mut e2: Vec<Edge> = e
            .iter()
            .rev()
            .map(|x| {
                let mut y = x.clone();
                y.source = remap(x.source);
                y.target = remap(x.target);
                y
            })
            .collect();
        e2.shrink_to_fit();
        let mut b: Vec<_> = withheld_pairs(&n2, &e2, &x)
            .iter()
            .map(|p| key(&n2, &e2, p))
            .collect();
        a.sort();
        b.sort();
        assert!(!a.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn holdout_pairs_are_excluded_from_ic() {
        // Withholding changes the hierarchy IC is computed on, so some
        // surviving edge's weight differs from the no-holdout case.
        let (n, e) = bigger();
        let plain = physics_edge_weights(&n, &e, &on(2.0));
        let held = physics_edge_weights(&n, &e, &exp(3, 0.4, false));
        let differs = plain
            .iter()
            .zip(&held)
            .enumerate()
            .any(|(i, (a, b))| *b != 0.0 && (a - b).abs() > 1e-6 && i < 39);
        assert!(differs);
    }

    #[test]
    fn null_arm_moves_the_same_multipliers_onto_non_hierarchy_edges() {
        let (n, e) = bigger();
        let real = physics_edge_weights(&n, &e, &exp(5, 0.2, false));
        let null = physics_edge_weights(&n, &e, &exp(5, 0.2, true));
        let ratios = |w: &[f32], range: std::ops::Range<usize>| -> Vec<f32> {
            let mut v: Vec<f32> = range
                .filter(|&i| w[i] != 0.0)
                .map(|i| w[i] / e[i].weight)
                .filter(|r| (r - 1.0).abs() > 1e-6)
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v
        };
        let real_hier = ratios(&real, 0..39);
        let null_links = ratios(&null, 39..e.len());
        assert!(!real_hier.is_empty());
        // Hierarchy edges at base weight in the null arm (or zero if withheld).
        let held: Vec<usize> = withheld_pairs(
            &n,
            &e,
            &LayoutExperiment {
                holdout_seed: 5,
                holdout_fraction: 0.2,
                null_arm: true,
            },
        )
        .iter()
        .map(|p| p.edge_index)
        .collect();
        for i in 0..39 {
            let want = if held.contains(&i) { 0.0 } else { e[i].weight };
            assert_eq!(null[i], want, "edge {i}");
        }
        // Same number and values of multipliers (the link set is large enough).
        assert_eq!(null_links.len(), real_hier.len());
        for (a, b) in null_links.iter().zip(&real_hier) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }
}
