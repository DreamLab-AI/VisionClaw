//! The physics edge-weight stage (ADR-2204 D2/D3, slice S3).
//!
//! [`physics_pairs`] is the one function that turns a graph into the physics edges the layout
//! attracts along, behind a stored [`WeightingProfile`]:
//!
//! * [`WeightingMode::Legacy`] (the default) keeps every stored edge and its stored weight, plus
//!   the optional hierarchy boost below, bit for bit as before.
//! * [`WeightingMode::Evidence`] reduces instances to evidence items (an inverse predicate is the
//!   same item as its forward form; an inferred copy of an asserted fact adds nothing; hierarchy
//!   is the asserted direct parents only; co-citation is one item per pair and shared target;
//!   self-loops are dropped), combines each pair's items as `q = 1 − Π(1 − aᵢ)` with the
//!   affinities of `relations.yaml`, optionally damps hubs by `(deg_u·deg_v)^(−α/2)`, and scales
//!   each connected component by the `c` that makes `Σ clamp(c·q, wMin, wMax) = |E|`, so every
//!   component's mean weight is exactly 1. One physics edge per pair; the store and wire are
//!   untouched.
//!
//! Affinities are policy values (the pre-S3 registry strengths), not tuned: see the header of
//! `relations.yaml`. Evidence mode ignores `hierarchy_affinity` and the layout experiment; both
//! are Legacy-mode controls.
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
use crate::relations::{Producer, RelationKind, RelationSchema};
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

/// Which weighting the layout uses (ADR-2204 D3, stored `physics.weightingMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WeightingMode {
    /// Today's behaviour: each stored edge keeps its stored weight (plus the optional
    /// `hierarchyAffinity` boost); one physics edge per stored edge.
    #[default]
    Legacy,
    /// Evidence identity, saturating combination and per-component normalisation (D2 + D3):
    /// one physics edge per connected pair. Ignores stored weights, `hierarchy_affinity`
    /// and the dev experiment.
    Evidence,
}

/// Default `wMin` of the evidence normalisation.
pub const WEIGHT_MIN_DEFAULT: f32 = 0.25;
/// Default `wMax` of the evidence normalisation.
pub const WEIGHT_MAX_DEFAULT: f32 = 4.0;

/// What shapes the physics weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightingProfile {
    /// Strength `a` of the hierarchy boost (Legacy mode only); `0` (or non-finite) is off.
    pub hierarchy_affinity: f32,
    /// Optional development experiment (Legacy mode only).
    pub experiment: Option<LayoutExperiment>,
    /// Legacy (default) or Evidence.
    pub mode: WeightingMode,
    /// Hub damping exponent `α` (Evidence mode): weights scale by `(deg_u·deg_v)^(−α/2)`.
    /// `0` is off.
    pub hub_damping: f32,
    /// Lower weight bound (Evidence mode), at most 1.
    pub weight_min: f32,
    /// Upper weight bound (Evidence mode), at least 1.
    pub weight_max: f32,
}

impl Default for WeightingProfile {
    fn default() -> Self {
        Self {
            hierarchy_affinity: 0.0,
            experiment: None,
            mode: WeightingMode::Legacy,
            hub_damping: 0.0,
            weight_min: WEIGHT_MIN_DEFAULT,
            weight_max: WEIGHT_MAX_DEFAULT,
        }
    }
}

/// One undirected physics edge as the CSR builder consumes it.
///
/// Legacy mode yields one per stored edge, in edge order, with the stored direction. Evidence
/// mode yields one per connected pair, `source < target`, sorted by `(source, target)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsPair {
    /// First endpoint (node id).
    pub source: u32,
    /// Second endpoint (node id).
    pub target: u32,
    /// Attraction weight.
    pub weight: f32,
}

/// Edge metadata key under which a co-citation edge lists its shared targets as
/// `targetId:referrerCount` joined by `,` (Evidence mode reads it; the sync writes it).
pub const COCITE_TARGETS_KEY: &str = "cocite_targets";

/// Metadata value for [`COCITE_TARGETS_KEY`]: `targetId:referrerCount` pairs joined by `,`.
pub fn format_cocite_targets(targets: &[(u32, usize)]) -> String {
    targets
        .iter()
        .map(|(t, d)| format!("{t}:{d}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The physics edges of a graph under `profile` (ADR-2204 D3): the one stage the CSR builder
/// reads.
///
/// [`WeightingMode::Legacy`] returns one pair per stored edge, in edge order, weighted by
/// [`physics_edge_weights`]. [`WeightingMode::Evidence`] returns one pair per connected pair of
/// nodes, `source < target`, sorted by `(source, target)`: see the module documentation.
pub fn physics_pairs(
    nodes: &[Node],
    edges: &[Edge],
    profile: &WeightingProfile,
) -> Vec<PhysicsPair> {
    match profile.mode {
        WeightingMode::Legacy => {
            let weights = physics_edge_weights(nodes, edges, profile);
            edges
                .iter()
                .zip(weights)
                .map(|(e, weight)| PhysicsPair {
                    source: e.source,
                    target: e.target,
                    weight,
                })
                .collect()
        }
        WeightingMode::Evidence => evidence_pairs(nodes, edges, profile),
    }
}

/// Key of one evidence item. Ordered, so a pair's items are always combined in the same order
/// and the result does not depend on the order edges were stored in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ItemKey {
    lo: u32,
    hi: u32,
    relation: usize,
    /// Canonical predicate IRI (the smaller of an inverse pair), a co-citation target, or empty.
    predicate: String,
    from: u32,
    to: u32,
}

fn evidence_pairs(nodes: &[Node], edges: &[Edge], profile: &WeightingProfile) -> Vec<PhysicsPair> {
    let schema = RelationSchema::builtin();
    let known: HashSet<u32> = nodes.iter().map(|n| n.id).collect();
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for (i, r) in schema.relations().iter().enumerate() {
        by_label.insert(r.label.as_str(), i);
        for a in &r.aliases {
            by_label.insert(a.as_str(), i);
        }
    }
    let threshold = schema.fanout_threshold();

    // Items of asserted / derived provenance, then inferred ones that do not repeat them.
    let mut firm: BTreeMap<ItemKey, f64> = BTreeMap::new();
    let mut inferred: BTreeMap<ItemKey, f64> = BTreeMap::new();
    for e in edges {
        if e.source == e.target || !known.contains(&e.source) || !known.contains(&e.target) {
            continue;
        }
        let Some(&ri) = e.edge_type.as_deref().and_then(|t| by_label.get(t)) else {
            continue;
        };
        let rel = &schema.relations()[ri];
        let Some(affinity) = rel.affinity.filter(|_| rel.physics) else {
            continue;
        };
        let affinity = f64::from(affinity);
        let (lo, hi) = (e.source.min(e.target), e.source.max(e.target));
        let push =
            |map: &mut BTreeMap<ItemKey, f64>, predicate: String, from: u32, to: u32, a: f64| {
                map.insert(
                    ItemKey {
                        lo,
                        hi,
                        relation: ri,
                        predicate,
                        from,
                        to,
                    },
                    a,
                );
            };

        if rel.label == "co_citation" {
            // One item per (pair, shared target); a popular target gives little, a hub none.
            match e.metadata.as_ref().and_then(|m| m.get(COCITE_TARGETS_KEY)) {
                None => push(&mut firm, "cocite:".into(), lo, hi, affinity),
                Some(list) => {
                    let mut targets: BTreeMap<&str, usize> = BTreeMap::new();
                    for part in list.split(',') {
                        if let Some((t, d)) = part.split_once(':') {
                            if let Ok(d) = d.trim().parse::<usize>() {
                                targets.insert(t.trim(), d);
                            }
                        }
                    }
                    for (t, deg) in targets {
                        if schema.admits_cocitation_at(deg, threshold) {
                            let a = affinity / (std::f64::consts::E + deg as f64).ln();
                            push(&mut firm, format!("cocite:{t}"), lo, hi, a);
                        }
                    }
                }
            }
            continue;
        }

        // The materialiser's own label (once it stops stamping `hierarchical`) is the reasoner's
        // closure: never evidence.
        if rel.label == Producer::Inferred.label() {
            continue;
        }
        let inferred_edge = e.provenance == Some(EdgeProvenance::Inferred);
        // Hierarchy is the asserted direct parents only: an inferred or unprovenanced hierarchy
        // edge may be the reasoner's closure, which adds nothing.
        if rel.kind == RelationKind::Hierarchy && e.provenance != Some(EdgeProvenance::Asserted) {
            continue;
        }
        // An inverse-predicate assertion is the same item as its forward form.
        let (mut predicate, mut from, mut to) = (
            e.owl_property_iri.clone().unwrap_or_default(),
            e.source,
            e.target,
        );
        if let Some(inv) = rel.inverse_predicates.get(&predicate) {
            if *inv < predicate {
                predicate = inv.clone();
                std::mem::swap(&mut from, &mut to);
            }
        }
        push(
            if inferred_edge {
                &mut inferred
            } else {
                &mut firm
            },
            predicate,
            from,
            to,
            affinity,
        );
    }
    for (k, a) in inferred {
        firm.entry(k).or_insert(a);
    }

    // q = 1 − Π(1 − a_i) per pair.
    let mut pairs: Vec<(u32, u32, f64)> = Vec::new();
    let mut survive: Option<((u32, u32), f64)> = None;
    for (k, a) in &firm {
        match survive {
            Some((pair, ref mut keep)) if pair == (k.lo, k.hi) => *keep *= 1.0 - a,
            _ => {
                if let Some(((lo, hi), keep)) = survive {
                    pairs.push((lo, hi, 1.0 - keep));
                }
                survive = Some(((k.lo, k.hi), 1.0 - a));
            }
        }
    }
    if let Some(((lo, hi), keep)) = survive {
        pairs.push((lo, hi, 1.0 - keep));
    }
    pairs.retain(|p| p.2 > 0.0 && p.2.is_finite());

    // Optional hub damping on the reduced graph's distinct-neighbour degrees.
    let alpha = if profile.hub_damping.is_finite() && profile.hub_damping > 0.0 {
        f64::from(profile.hub_damping)
    } else {
        0.0
    };
    if alpha > 0.0 {
        let mut degree: HashMap<u32, usize> = HashMap::new();
        for &(a, b, _) in &pairs {
            *degree.entry(a).or_insert(0) += 1;
            *degree.entry(b).or_insert(0) += 1;
        }
        for p in &mut pairs {
            let product = (degree[&p.0] * degree[&p.1]) as f64;
            p.2 *= product.powf(-alpha / 2.0);
        }
    }

    // Connected components over the reduced pairs (union-find), then per-component scale.
    let mut slot: HashMap<u32, usize> = HashMap::new();
    for &(a, b, _) in &pairs {
        let next = slot.len();
        slot.entry(a).or_insert(next);
        let next = slot.len();
        slot.entry(b).or_insert(next);
    }
    let mut parent: Vec<usize> = (0..slot.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for &(a, b, _) in &pairs {
        let (ra, rb) = (find(&mut parent, slot[&a]), find(&mut parent, slot[&b]));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &(a, _, _)) in pairs.iter().enumerate() {
        let root = find(&mut parent, slot[&a]);
        members.entry(root).or_default().push(i);
    }
    let (lo, hi) = sanitised_bounds(profile.weight_min, profile.weight_max);
    let mut out: Vec<PhysicsPair> = pairs
        .iter()
        .map(|&(source, target, _)| PhysicsPair {
            source,
            target,
            weight: 1.0,
        })
        .collect();
    for idx in members.values() {
        let q: Vec<f64> = idx.iter().map(|&i| pairs[i].2).collect();
        for (&i, w) in idx.iter().zip(normalise(&q, lo, hi)) {
            out[i].weight = w as f32;
        }
    }
    out
}

/// `wMin ≤ 1 ≤ wMax`, both finite and positive; anything else falls back to the defaults
/// before the clamp to that interval.
fn sanitised_bounds(min: f32, max: f32) -> (f64, f64) {
    let lo = if min.is_finite() && min > 0.0 {
        min
    } else {
        WEIGHT_MIN_DEFAULT
    };
    let hi = if max.is_finite() && max > 0.0 {
        max
    } else {
        WEIGHT_MAX_DEFAULT
    };
    (f64::from(lo.min(1.0)), f64::from(hi.max(1.0)))
}

/// Solve `Σ clamp(c·qᵢ, lo, hi) = n` for `c` and return the clamped weights. `Σ` is monotone
/// and continuous in `c` and runs from `n·lo ≤ n` to `n·hi ≥ n`, so a solution exists. When no
/// weight is clamped the closed form `c = n / Σq` is used, which is exact.
fn normalise(q: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let n = q.len() as f64;
    let sum: f64 = q.iter().sum();
    let c = n / sum;
    if q.iter().all(|&x| (lo..=hi).contains(&(c * x))) {
        return q.iter().map(|&x| c * x).collect();
    }
    let total = |c: f64| -> f64 { q.iter().map(|&x| (c * x).clamp(lo, hi)).sum() };
    let qmin = q.iter().copied().fold(f64::INFINITY, f64::min);
    let (mut a, mut b) = (0.0f64, hi / qmin);
    for _ in 0..200 {
        let mid = 0.5 * (a + b);
        if total(mid) < n {
            a = mid;
        } else {
            b = mid;
        }
    }
    let c = 0.5 * (a + b);
    q.iter().map(|&x| (c * x).clamp(lo, hi)).collect()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

#[cfg(test)]
mod evidence_tests {
    //! ADR-2204 E2: the evidence weighting stage against hand-computed oracles, edge-order
    //! invariance, three encodings of one body of knowledge, bounds and per-component mean.

    use super::*;
    use std::collections::BTreeMap;

    const NS: &str = "https://narrativegoldmine.com/ns/v1#";

    fn nodes(n: u32) -> Vec<Node> {
        (1..=n)
            .map(|i| Node::new_with_id(format!("page-{i}"), Some(i)))
            .collect()
    }

    fn ev() -> WeightingProfile {
        WeightingProfile {
            mode: WeightingMode::Evidence,
            ..Default::default()
        }
    }

    fn typed(a: u32, b: u32, label: &str, pred: Option<&str>, prov: EdgeProvenance) -> Edge {
        let mut e = Edge::new(a, b, 9.99) // the stored weight must never matter
            .with_edge_type(label.to_string())
            .with_provenance(prov);
        e.owl_property_iri = pred.map(|p| format!("{NS}{p}"));
        e
    }

    fn link(a: u32, b: u32) -> Edge {
        typed(a, b, "explicit_link", None, EdgeProvenance::Asserted)
    }

    fn sub(child: u32, parent: u32, prov: EdgeProvenance) -> Edge {
        let mut e = typed(child, parent, "hierarchical", None, prov);
        e.owl_property_iri = Some(RDFS_SUBCLASS_OF_IRI.to_string());
        e
    }

    fn as_map(p: &[PhysicsPair]) -> BTreeMap<(u32, u32), u32> {
        p.iter()
            .map(|x| ((x.source, x.target), x.weight.to_bits()))
            .collect()
    }

    fn weights(n: &[Node], e: &[Edge], prof: &WeightingProfile) -> Vec<PhysicsPair> {
        physics_pairs(n, e, prof)
    }

    #[test]
    fn legacy_mode_is_one_pair_per_edge_in_edge_order_with_stored_weights() {
        let n = nodes(4);
        let e = vec![link(2, 1), link(3, 3), sub(4, 1, EdgeProvenance::Inferred)];
        let p = weights(&n, &e, &WeightingProfile::default());
        let got: Vec<(u32, u32, u32)> = p
            .iter()
            .map(|x| (x.source, x.target, x.weight.to_bits()))
            .collect();
        let want: Vec<(u32, u32, u32)> = e
            .iter()
            .map(|x| (x.source, x.target, x.weight.to_bits()))
            .collect();
        assert_eq!(got, want);
    }

    /// Hand oracle. 1–2 carries a wikilink (0.5) and an associative link (0.4):
    /// q = 1 − 0.5·0.6 = 0.7. 2–3 is `requires` (0.7): q = 0.7. 3–4 is a wikilink: 0.5.
    /// One component, three pairs: c = 3 / (0.7 + 0.7 + 0.5) = 3/1.9.
    #[test]
    fn small_graph_matches_the_hand_computed_oracle() {
        let n = nodes(4);
        let e = vec![
            link(1, 2),
            typed(
                1,
                2,
                "associative",
                Some("relatesTo"),
                EdgeProvenance::Asserted,
            ),
            typed(
                2,
                3,
                "dependency",
                Some("requires"),
                EdgeProvenance::Asserted,
            ),
            link(3, 4),
        ];
        let p = weights(&n, &e, &ev());
        let m: BTreeMap<_, _> = p.iter().map(|x| ((x.source, x.target), x.weight)).collect();
        let c = 3.0f32 / 1.9;
        assert_eq!(m.len(), 3);
        assert!((m[&(1, 2)] - 0.7 * c).abs() < 1e-6, "{:?}", m);
        assert!((m[&(2, 3)] - 0.7 * c).abs() < 1e-6);
        assert!((m[&(3, 4)] - 0.5 * c).abs() < 1e-6);
    }

    /// Bounds bind: q = [0.8, 0.8, 0.25] in [0.9, 1.1] solves 2·(0.8c) + 0.9 = 3 → c = 1.3125,
    /// so the weights are exactly 1.05, 1.05, 0.9 (the third is held at the floor).
    #[test]
    fn clamping_is_solved_so_the_mean_is_still_exactly_one() {
        let n = nodes(5);
        let e = vec![
            sub(1, 2, EdgeProvenance::Asserted), // 0.8
            sub(2, 3, EdgeProvenance::Asserted), // 0.8
            typed(
                3,
                4,
                "provenance",
                Some("wasDerivedFrom"),
                EdgeProvenance::Asserted,
            ), // 0.25
        ];
        let prof = WeightingProfile {
            weight_min: 0.9,
            weight_max: 1.1,
            ..ev()
        };
        let m: BTreeMap<_, _> = weights(&n, &e, &prof)
            .iter()
            .map(|x| ((x.source, x.target), x.weight))
            .collect();
        assert!((m[&(1, 2)] - 1.05).abs() < 1e-5, "{m:?}");
        assert!((m[&(2, 3)] - 1.05).abs() < 1e-5);
        assert!((m[&(3, 4)] - 0.9).abs() < 1e-5);
        assert!((m.values().sum::<f32>() - 3.0).abs() < 1e-5);
    }

    #[test]
    fn stored_weights_and_the_legacy_boost_never_enter_evidence_mode() {
        let n = nodes(4);
        let mut e = vec![link(1, 2), link(2, 3), sub(3, 4, EdgeProvenance::Asserted)];
        let base = as_map(&weights(&n, &e, &ev()));
        for x in e.iter_mut() {
            x.weight = 123.0;
        }
        let boosted = WeightingProfile {
            hierarchy_affinity: 3.0,
            experiment: Some(LayoutExperiment {
                holdout_seed: 1,
                holdout_fraction: 0.5,
                null_arm: false,
            }),
            ..ev()
        };
        assert_eq!(as_map(&weights(&n, &e, &boosted)), base);
    }

    fn permute(e: &[Edge], k: usize) -> Vec<Edge> {
        let mut v: Vec<Edge> = e.iter().rev().cloned().collect();
        let len = v.len().max(1);
        v.rotate_left(k % len);
        v
    }

    fn knowledge(prefix_inverse: bool, closure: bool) -> Vec<Edge> {
        let mut e = vec![
            sub(10, 11, EdgeProvenance::Asserted),
            sub(11, 12, EdgeProvenance::Asserted),
            typed(
                20,
                21,
                "structural",
                Some("hasPart"),
                EdgeProvenance::Asserted,
            ),
            link(12, 20),
            link(21, 22),
            typed(
                22,
                23,
                "dependency",
                Some("enables"),
                EdgeProvenance::Asserted,
            ),
        ];
        if prefix_inverse {
            // the same part-of fact, said from the other side
            e.push(typed(
                21,
                20,
                "structural",
                Some("isPartOf"),
                EdgeProvenance::Asserted,
            ));
            // an inferred copy of an asserted structural fact
            e.push(typed(
                20,
                21,
                "structural",
                Some("hasPart"),
                EdgeProvenance::Inferred,
            ));
            // `partOf` is a spelling of the same inverse; so is `enabledBy` of `enables`.
            e.push(typed(
                21,
                20,
                "structural",
                Some("partOf"),
                EdgeProvenance::Asserted,
            ));
            e.push(typed(
                23,
                22,
                "dependency",
                Some("enabledBy"),
                EdgeProvenance::Asserted,
            ));
        }
        if closure {
            e.push(sub(10, 12, EdgeProvenance::Inferred)); // transitive closure
            e.push(sub(10, 11, EdgeProvenance::Inferred)); // inferred copy of a direct parent
                                                           // The same closure once the materialiser labels it `inferred` (D2 mislabel fixed).
            e.push(typed(10, 12, "inferred", None, EdgeProvenance::Inferred));
            e.push(typed(10, 11, "inferred", None, EdgeProvenance::Inferred));
        }
        e
    }

    /// Codex's probe: the same knowledge once, with inverses, and with the transitive closure
    /// gives identical weights, bit for bit.
    #[test]
    fn three_encodings_of_the_same_knowledge_give_identical_weights() {
        let n = nodes(23);
        let once = as_map(&weights(&n, &knowledge(false, false), &ev()));
        let inv = as_map(&weights(&n, &knowledge(true, false), &ev()));
        let clo = as_map(&weights(&n, &knowledge(true, true), &ev()));
        assert_eq!(once.len(), 6);
        assert_eq!(once, inv, "inverse + inferred duplicate must add nothing");
        assert_eq!(once, clo, "the transitive closure must add nothing");
        assert!(
            !clo.contains_key(&(10, 12)),
            "a closure pair is not a physics edge"
        );
    }

    #[test]
    fn the_result_is_invariant_to_edge_order() {
        let n = nodes(23);
        let e = knowledge(true, true);
        let base = weights(&n, &e, &ev());
        for k in 0..e.len() {
            assert_eq!(weights(&n, &permute(&e, k), &ev()), base, "rotation {k}");
        }
    }

    #[test]
    fn inferred_edges_alone_and_unprovenanced_hierarchy_add_no_hierarchy_evidence() {
        let n = nodes(4);
        let mut none = sub(1, 2, EdgeProvenance::Asserted);
        none.provenance = None;
        let e = vec![sub(2, 3, EdgeProvenance::Inferred), none, link(3, 4)];
        let m = as_map(&weights(&n, &e, &ev()));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![(3, 4)]);
    }

    #[test]
    fn unprovenanced_non_hierarchy_edges_count_as_asserted() {
        let n = nodes(3);
        let mut a = link(1, 2);
        a.provenance = None;
        let m = as_map(&weights(&n, &[a, link(2, 3)], &ev()));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn self_loops_unknown_labels_untyped_and_unknown_nodes_are_dropped() {
        let n = nodes(3);
        let e = vec![
            link(1, 1),
            typed(1, 2, "no-such-relation", None, EdgeProvenance::Asserted),
            Edge::new(1, 2, 1.0), // no label at all
            link(2, 99),          // endpoint not in the graph
            link(2, 3),
        ];
        let m = as_map(&weights(&n, &e, &ev()));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![(2, 3)]);
    }

    fn cocite(a: u32, b: u32, targets: &str) -> Edge {
        let mut e = typed(a, b, "co_citation", None, EdgeProvenance::Derived);
        e.metadata = Some(
            [(COCITE_TARGETS_KEY.to_string(), targets.to_string())]
                .into_iter()
                .collect(),
        );
        e
    }

    /// One item per (pair, shared target), each `0.25 / ln(e + deg_t)`: a popular target gives
    /// little. A target over the fan-out threshold (3 referrers by default) gives none.
    #[test]
    fn co_citation_items_follow_the_resource_allocation_form() {
        let n = nodes(3);
        // Two targets with 2 and 3 referrers, listed twice (a duplicate item) and once over
        // the threshold (4 referrers).
        let e = vec![cocite(1, 2, "100:2,101:3,100:2,102:4"), link(2, 3)];
        let m: BTreeMap<_, _> = weights(&n, &e, &ev())
            .iter()
            .map(|x| ((x.source, x.target), x.weight))
            .collect();
        let item = |d: f64| 0.25 / (std::f64::consts::E + d).ln();
        let q12 = 1.0 - (1.0 - item(2.0)) * (1.0 - item(3.0));
        let q23 = 0.5;
        let c = 2.0 / (q12 + q23);
        assert!((m[&(1, 2)] as f64 - c * q12).abs() < 1e-5, "{m:?}");
        assert!((m[&(2, 3)] as f64 - c * q23).abs() < 1e-5);
    }

    #[test]
    fn a_co_citation_edge_with_only_over_threshold_targets_makes_no_pair() {
        let n = nodes(3);
        let e = vec![cocite(1, 2, "100:4,101:9"), link(2, 3)];
        let m = as_map(&weights(&n, &e, &ev()));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec![(2, 3)]);
    }

    /// Without the target list (a reloaded store does not keep it yet) a co-citation edge is one
    /// item at degree 0: `0.25 / ln e = 0.25`.
    #[test]
    fn a_co_citation_edge_without_targets_is_one_item() {
        let n = nodes(3);
        let e = vec![
            typed(1, 2, "co_citation", None, EdgeProvenance::Derived),
            link(2, 3),
        ];
        let m: BTreeMap<_, _> = weights(&n, &e, &ev())
            .iter()
            .map(|x| ((x.source, x.target), x.weight))
            .collect();
        let c = 2.0f32 / (0.25 + 0.5);
        assert!((m[&(1, 2)] - 0.25 * c).abs() < 1e-5);
    }

    /// Star hub 1 with leaves 2, 3 and a spur 1–4–5, all q = 0.5, α = 2:
    /// factors (deg product)^−1 = 1/3, 1/3, 1/6, 1/2 → weights 3·factor = 1, 1, 0.5, 1.5.
    #[test]
    fn hub_damping_scales_by_the_degree_product() {
        let n = nodes(5);
        let e = vec![link(1, 2), link(1, 3), link(1, 4), link(4, 5)];
        let prof = WeightingProfile {
            hub_damping: 2.0,
            ..ev()
        };
        let m: BTreeMap<_, _> = weights(&n, &e, &prof)
            .iter()
            .map(|x| ((x.source, x.target), x.weight))
            .collect();
        for (k, w) in [((1, 2), 1.0), ((1, 3), 1.0), ((1, 4), 0.5), ((4, 5), 1.5)] {
            assert!((m[&k] - w).abs() < 1e-5, "{k:?} {m:?}");
        }
    }

    #[test]
    fn the_target_list_the_sync_writes_is_the_one_the_stage_reads() {
        let n = nodes(3);
        let mut e = typed(1, 2, "co_citation", None, EdgeProvenance::Derived);
        e.metadata = Some(
            [(
                COCITE_TARGETS_KEY.to_string(),
                format_cocite_targets(&[(100, 2), (101, 3)]),
            )]
            .into_iter()
            .collect(),
        );
        let direct = as_map(&weights(&n, &[e, link(2, 3)], &ev()));
        let literal = as_map(&weights(
            &n,
            &[cocite(1, 2, "100:2,101:3"), link(2, 3)],
            &ev(),
        ));
        assert_eq!(direct, literal);
    }

    #[test]
    fn zero_damping_changes_nothing_exactly() {
        let n = nodes(5);
        let e = vec![link(1, 2), link(1, 3), link(1, 4), link(4, 5)];
        let m = weights(&n, &e, &ev());
        assert!(m.iter().all(|x| x.weight == 1.0), "{m:?}");
    }

    #[test]
    fn empty_single_edge_and_edgeless_graphs_are_finite() {
        assert!(weights(&[], &[], &ev()).is_empty());
        let n = nodes(3);
        assert!(weights(&n, &[], &ev()).is_empty());
        let one = weights(&n, &[sub(1, 2, EdgeProvenance::Asserted)], &ev());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].weight, 1.0);
    }

    /// A tiny deterministic generator, so the property test needs no dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, m: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) % m
        }
    }

    fn components(n: u32, pairs: &[PhysicsPair]) -> Vec<Vec<usize>> {
        let mut parent: Vec<u32> = (0..=n).collect();
        fn find(p: &mut Vec<u32>, x: u32) -> u32 {
            let mut r = x;
            while p[r as usize] != r {
                r = p[r as usize];
            }
            r
        }
        for q in pairs {
            let (a, b) = (find(&mut parent, q.source), find(&mut parent, q.target));
            parent[a as usize] = b;
        }
        let mut by: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, q) in pairs.iter().enumerate() {
            let r = find(&mut parent, q.source);
            by.entry(r).or_default().push(i);
        }
        by.into_values().collect()
    }

    #[test]
    fn every_component_has_mean_one_and_every_weight_is_in_bounds() {
        let labels = [
            ("explicit_link", None),
            ("dependency", Some("requires")),
            ("associative", Some("relatesTo")),
            ("provenance", Some("wasDerivedFrom")),
        ];
        for seed in 0..40u64 {
            let mut g = Lcg(seed + 1);
            let nn = 8 + g.next(40) as u32;
            let n = nodes(nn);
            let mut e = Vec::new();
            for _ in 0..(5 + g.next(60)) {
                let (a, b) = (1 + g.next(nn as u64) as u32, 1 + g.next(nn as u64) as u32);
                if g.next(4) == 0 {
                    e.push(sub(a, b, EdgeProvenance::Asserted));
                } else {
                    let (l, p) = labels[g.next(4) as usize];
                    e.push(typed(a, b, l, p, EdgeProvenance::Asserted));
                }
            }
            for prof in [
                ev(),
                WeightingProfile {
                    hub_damping: 1.0,
                    ..ev()
                },
            ] {
                let pairs = weights(&n, &e, &prof);
                for comp in components(nn, &pairs) {
                    let sum: f64 = comp.iter().map(|&i| pairs[i].weight as f64).sum();
                    assert!(
                        (sum / comp.len() as f64 - 1.0).abs() < 1e-5,
                        "seed {seed}: mean {}",
                        sum / comp.len() as f64
                    );
                }
                for q in &pairs {
                    assert!(q.weight.is_finite());
                    assert!(q.weight >= 0.25 - 1e-6 && q.weight <= 4.0 + 1e-6, "{q:?}");
                    assert!(q.source < q.target);
                }
            }
        }
    }

    #[test]
    fn adding_a_component_does_not_reweight_another() {
        let n = nodes(8);
        let a = vec![link(1, 2), sub(2, 3, EdgeProvenance::Asserted), link(3, 4)];
        let mut b = a.clone();
        b.push(typed(
            6,
            7,
            "provenance",
            Some("wasDerivedFrom"),
            EdgeProvenance::Asserted,
        ));
        b.push(link(7, 8));
        let wa = as_map(&weights(&n, &a, &ev()));
        let wb = as_map(&weights(&n, &b, &ev()));
        for (k, v) in &wa {
            assert_eq!(wb[k], *v, "{k:?}");
        }
    }

    #[test]
    fn a_profile_with_inverted_or_non_finite_bounds_is_repaired_not_trusted() {
        let n = nodes(4);
        let e = vec![link(1, 2), sub(2, 3, EdgeProvenance::Asserted), link(3, 4)];
        for (lo, hi) in [(2.0, 0.5), (f32::NAN, f32::INFINITY), (-1.0, 0.0)] {
            let prof = WeightingProfile {
                weight_min: lo,
                weight_max: hi,
                ..ev()
            };
            let p = weights(&n, &e, &prof);
            assert!(p.iter().all(|x| x.weight.is_finite() && x.weight > 0.0));
            assert!((p.iter().map(|x| x.weight).sum::<f32>() - 3.0).abs() < 1e-4);
        }
    }
}
