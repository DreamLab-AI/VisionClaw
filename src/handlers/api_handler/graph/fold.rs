//! Fold-level ladder — server-side fold-plan computation (Wave 3, Phase 1).
//!
//! A discrete density ladder for the immersive graph. The server computes a
//! *fold plan* — which nodes to hide and which groups collapse into a single
//! representative — and returns it over `GET /api/graph/fold`. The client
//! applies the plan as an id→representative remap in its render store; the
//! server never mutates the graph.
//!
//! Levels:
//! * **L0 (∅)** — everything visible, no groups.
//! * **L1** — hide low-signal nodes (bottom-quartile PageRank centrality).
//! * **L2** — L1 + fold each `rdfs:subClassOf` chain into its chain root.
//! * **L3** — L2 + fold each Louvain community (that isn't already inside a
//!   subclass group) into its highest-centrality medoid.
//!
//! `community_id` + `centrality` come from the shared per-node analytics map
//! (`AppState::node_analytics`, populated by the GPU PageRank/Louvain actors —
//! the same source the V3 wire encoder reads). Hierarchy edges are identified by
//! the label accept set in [`is_subclass_relation`], which is deliberately wider
//! than the DAG ranker's provenance rule (`Edge::asserts_subsumption`).
//!
//! The pin-agnostic base plan is memoised by `(level, graph_type, generation)`;
//! per-view pinned-node promotion is a cheap post-step applied outside the memo
//! so it never pollutes the cache key.

use super::{fetch_graph_snapshot, PopulationFilter};
use crate::AppState;
use actix_web::{web, HttpResponse, Responder};
use log::error;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::node::Node;

/// Per-node analytics value object shared with the wire encoder
/// (`community_id`, `centrality`, …).
type Analytics = crate::utils::binary_protocol::NodeAnalytics;

/// Highest ladder step. `?level=` is clamped to `[0, MAX_FOLD_LEVEL]`.
const MAX_FOLD_LEVEL: u8 = 3;

/// Quantile below which a node counts as "low-signal" for L1 hiding.
const LOW_SIGNAL_QUANTILE: f32 = 0.25;

/// Accepted relation strings for a hierarchy-fold edge (child = source, parent =
/// target). Includes the explicit subclass provenance strings AND the collapsed
/// `"hierarchical"` semantic label.
///
/// The live graph stores the COLLAPSED `SemanticEdgeType` label on the wire, not
/// the raw relation: on the running deployment there are 7548 `hierarchical`
/// edges and ZERO `subclass_of`/`SUBCLASS_OF` strings, so a subclass-only accept
/// set folds nothing. The DAG ranker stays narrow (`Edge::asserts_subsumption`,
/// subClassOf provenance only) because it fabricates radial *ranks*, where a
/// symmetric or membership edge would corrupt the layout; folding only *groups*
/// connected hierarchy nodes into a representative, so treating the whole
/// hierarchy class as foldable is both correct and what actually reduces density
/// here. The explicit strings are kept so deployments that DO carry raw
/// provenance still fold.
fn is_subclass_relation(rel: &str) -> bool {
    matches!(
        rel,
        "is_subclass_of" | "subclass_of" | "SUBCLASS_OF" | "hierarchical"
    )
}

// ---------------------------------------------------------------------------
// Query + response wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldQuery {
    /// Ladder step 0..=3. Absent ⇒ 0 (∅, everything visible).
    pub level: Option<u8>,
    /// Optional population filter (`knowledge|ontology|agent`), same semantics
    /// as `GET /graph/data?graph_type=`.
    pub graph_type: Option<String>,
    /// Comma-separated node ids the caller has pinned in this view; each is
    /// promoted to its group's representative and never folded away.
    pub pinned: Option<String>,
}

/// One folded group: `member_ids` collapse into `representative_id`, which shows
/// a "+badge" count. `kind` is `"subclass"` or `"community"`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoldGroup {
    pub representative_id: u32,
    pub member_ids: Vec<u32>,
    pub badge: u32,
    pub kind: String,
}

/// The full fold plan returned to a client.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoldPlan {
    pub level: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_type: Option<String>,
    pub generation: u64,
    pub hidden: Vec<u32>,
    pub groups: Vec<FoldGroup>,
    /// Diagnostics — make an empty plan explainable from a single curl.
    /// `analytics_nodes`: how many nodes have populated analytics (0 ⇒ L1/L3 run
    /// on the degree fallback / produce no communities). `hierarchy_edges`: how
    /// many in-scope edges the L2 fold accepts as hierarchy edges.
    pub analytics_nodes: usize,
    pub hierarchy_edges: usize,
}

/// Pin-agnostic core of a plan — the memoised unit. Pinned promotion is applied
/// on top per request.
#[derive(Debug, Clone, PartialEq)]
struct FoldBase {
    hidden: Vec<u32>,
    groups: Vec<FoldGroup>,
}

// ---------------------------------------------------------------------------
// Topology generation — a cheap content hash used as the cache/staleness key
// ---------------------------------------------------------------------------

#[inline]
fn fnv_step(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x0000_0100_0000_01b3)
}

/// FNV-1a over node ids and edge (source, target, type). Changes whenever the
/// topology the fold plan depends on changes, so a stale plan minted before a
/// graph rebuild is detectable client-side and the memo self-invalidates. O(n+e),
/// sub-millisecond at 13k/145k.
fn topology_generation(nodes: &[Node], edges: &[Edge]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv_step(h, nodes.len() as u64);
    for n in nodes {
        h = fnv_step(h, n.id as u64);
    }
    h = fnv_step(h, edges.len() as u64);
    for e in edges {
        h = fnv_step(h, e.source as u64);
        h = fnv_step(h, e.target as u64);
        if let Some(t) = e.edge_type.as_deref() {
            for b in t.bytes() {
                h = fnv_step(h, b as u64);
            }
        }
        h = fnv_step(h, 0xff); // field separator
    }
    h
}

// ---------------------------------------------------------------------------
// Union-find (subclass component detection)
// ---------------------------------------------------------------------------

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        // Path compression.
        let mut c = x;
        while self.parent[c] != r {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Level computations
// ---------------------------------------------------------------------------

/// Ids of nodes matching the optional population filter, in input order. `None`
/// filter ⇒ every node.
fn visible_node_ids(nodes: &[Node], graph_type: Option<&str>) -> Vec<u32> {
    match PopulationFilter::parse(graph_type) {
        None => nodes.iter().map(|n| n.id).collect(),
        Some(f) => nodes
            .iter()
            .filter(|n| f.matches(n.node_type.as_deref(), &n.metadata))
            .map(|n| n.id)
            .collect(),
    }
}

/// L1 low-signal set: ids whose "signal" score is strictly below the
/// `LOW_SIGNAL_QUANTILE` of the score distribution over the candidates.
///
/// Prefers PageRank **centrality** (from analytics). When analytics haven't
/// populated yet — GPU analytics warm-up, or a deployment where they never run —
/// it DEGRADES to incident-edge **degree** so L1 folds immediately instead of
/// returning an empty plan. Nodes with no score under the chosen metric are never
/// hidden. Returns a sorted vec; empty only when there are too few candidates to
/// define a quartile.
fn low_signal_ids(
    candidates: &[u32],
    analytics: &HashMap<u32, Analytics>,
    degrees: &HashMap<u32, u32>,
) -> Vec<u32> {
    // Use analytics only when a meaningful fraction of candidates are scored with
    // a non-zero centrality; otherwise fall back to degree.
    let scored = candidates
        .iter()
        .filter(|id| analytics.get(id).is_some_and(|a| a.centrality > 0.0))
        .count();
    let use_analytics = scored >= 4;

    let score = |id: u32| -> Option<f32> {
        if use_analytics {
            analytics.get(&id).map(|a| a.centrality)
        } else {
            degrees.get(&id).map(|&d| d as f32)
        }
    };

    let mut vals: Vec<f32> = candidates.iter().filter_map(|&id| score(id)).collect();
    if vals.len() < 4 {
        return Vec::new(); // too few scored nodes to define a meaningful quartile
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (LOW_SIGNAL_QUANTILE * (vals.len() as f32 - 1.0)).floor() as usize;
    let threshold = vals[rank];
    let mut hidden: Vec<u32> = candidates
        .iter()
        .copied()
        .filter(|&id| score(id).is_some_and(|v| v < threshold))
        .collect();
    hidden.sort_unstable();
    hidden
}

/// Incident-edge degree per node over the visible set (edges counted only when
/// both endpoints are visible). Used as the L1 fallback signal when analytics are
/// absent.
fn degree_map(visible: &HashSet<u32>, edges: &[Edge]) -> HashMap<u32, u32> {
    let mut deg: HashMap<u32, u32> = visible.iter().map(|&id| (id, 0)).collect();
    for e in edges {
        if visible.contains(&e.source) && visible.contains(&e.target) {
            *deg.entry(e.source).or_insert(0) += 1;
            *deg.entry(e.target).or_insert(0) += 1;
        }
    }
    deg
}

/// L2 subclass groups over the visible set. Each weakly-connected component of
/// the directed subclass edges with ≥2 members folds into its chain root (the
/// node that is never a child; min id when several, min id of the component for
/// a pure cycle). Groups are sorted by `representative_id`; members sorted.
fn subclass_groups(visible: &HashSet<u32>, edges: &[Edge]) -> Vec<FoldGroup> {
    // Compact the visible ids that actually participate in a subclass edge.
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut sub_edges: Vec<(u32, u32)> = Vec::new(); // (child=source, parent=target)
    for e in edges {
        let is_sub = e.edge_type.as_deref().is_some_and(is_subclass_relation);
        if !is_sub || !visible.contains(&e.source) || !visible.contains(&e.target) {
            continue;
        }
        for id in [e.source, e.target] {
            if !index.contains_key(&id) {
                index.insert(id, ids.len());
                ids.push(id);
            }
        }
        sub_edges.push((e.source, e.target));
    }
    if ids.is_empty() {
        return Vec::new();
    }

    let mut uf = UnionFind::new(ids.len());
    let mut is_child: HashSet<u32> = HashSet::new();
    for &(child, parent) in &sub_edges {
        uf.union(index[&child], index[&parent]);
        is_child.insert(child);
    }

    // Bucket members by component root.
    let mut comps: HashMap<usize, Vec<u32>> = HashMap::new();
    for (i, &id) in ids.iter().enumerate() {
        comps.entry(uf.find(i)).or_default().push(id);
    }

    let mut groups: Vec<FoldGroup> = Vec::new();
    for (_root, mut members) in comps {
        if members.len() < 2 {
            continue;
        }
        members.sort_unstable();
        // Representative = chain root: never a child. Fall back to min id.
        let rep = members
            .iter()
            .copied()
            .find(|id| !is_child.contains(id))
            .unwrap_or(members[0]);
        let member_ids: Vec<u32> = members.into_iter().filter(|&id| id != rep).collect();
        let badge = member_ids.len() as u32;
        groups.push(FoldGroup {
            representative_id: rep,
            member_ids,
            badge,
            kind: "subclass".to_string(),
        });
    }
    groups.sort_by_key(|g| g.representative_id);
    groups
}

/// L3 community groups over the eligible set (visible, not hidden, not already in
/// a subclass group). Each `community_id != 0` with ≥2 members folds into its
/// medoid — the member with highest centrality (min id breaks ties). Groups
/// sorted by `representative_id`; members sorted.
fn community_groups(eligible: &[u32], analytics: &HashMap<u32, Analytics>) -> Vec<FoldGroup> {
    let mut by_comm: HashMap<u32, Vec<u32>> = HashMap::new();
    for &id in eligible {
        if let Some(a) = analytics.get(&id) {
            if a.community_id != 0 {
                by_comm.entry(a.community_id).or_default().push(id);
            }
        }
    }
    let mut groups: Vec<FoldGroup> = Vec::new();
    for (_comm, mut members) in by_comm {
        if members.len() < 2 {
            continue;
        }
        members.sort_unstable();
        // Medoid = highest-centrality member; min id breaks ties (members sorted).
        let rep = pick_medoid(&members, analytics).unwrap_or(members[0]);
        let member_ids: Vec<u32> = members.into_iter().filter(|&id| id != rep).collect();
        let badge = member_ids.len() as u32;
        groups.push(FoldGroup {
            representative_id: rep,
            member_ids,
            badge,
            kind: "community".to_string(),
        });
    }
    groups.sort_by_key(|g| g.representative_id);
    groups
}

/// Highest-centrality member, min id on a tie. `members` assumed sorted ascending.
fn pick_medoid(members: &[u32], analytics: &HashMap<u32, Analytics>) -> Option<u32> {
    members
        .iter()
        .copied()
        .map(|id| (id, analytics.get(&id).map(|a| a.centrality).unwrap_or(0.0)))
        .reduce(|best, cur| {
            if cur.1 > best.1 {
                cur // strictly higher centrality wins
            } else {
                best // equal or lower keeps the earlier (smaller id, since sorted)
            }
        })
        .map(|(id, _)| id)
}

/// Compute the pin-agnostic base plan for a level. Pure — no actix, no locks.
fn compute_base_plan(
    level: u8,
    nodes: &[Node],
    edges: &[Edge],
    analytics: &HashMap<u32, Analytics>,
    graph_type: Option<&str>,
) -> FoldBase {
    if level == 0 {
        return FoldBase {
            hidden: Vec::new(),
            groups: Vec::new(),
        };
    }

    let visible_ids = visible_node_ids(nodes, graph_type);
    let visible_set: HashSet<u32> = visible_ids.iter().copied().collect();
    // Degree over the visible set — the L1 fallback signal when analytics are cold.
    let degrees = degree_map(&visible_set, edges);
    let hidden = low_signal_ids(&visible_ids, analytics, &degrees);
    if level == 1 {
        return FoldBase {
            hidden,
            groups: Vec::new(),
        };
    }

    // Visible-and-not-hidden set is the grouping domain.
    let hidden_set: HashSet<u32> = hidden.iter().copied().collect();
    let domain: HashSet<u32> = visible_ids
        .iter()
        .copied()
        .filter(|id| !hidden_set.contains(id))
        .collect();

    let sub_groups = subclass_groups(&domain, edges);

    if level == 2 {
        return FoldBase {
            hidden,
            groups: sub_groups,
        };
    }

    // L3: community-fold everything not already inside a subclass group.
    let mut in_subclass: HashSet<u32> = HashSet::new();
    for g in &sub_groups {
        in_subclass.insert(g.representative_id);
        in_subclass.extend(g.member_ids.iter().copied());
    }
    let mut eligible: Vec<u32> = domain
        .iter()
        .copied()
        .filter(|id| !in_subclass.contains(id))
        .collect();
    eligible.sort_unstable();
    let comm_groups = community_groups(&eligible, analytics);

    let mut groups = sub_groups;
    groups.extend(comm_groups);
    groups.sort_by_key(|g| g.representative_id);
    FoldBase { hidden, groups }
}

// ---------------------------------------------------------------------------
// Pinned-node promotion (per-request, outside the memo)
// ---------------------------------------------------------------------------

/// Apply pinned-node promotion to a base plan. A pinned node is never hidden and
/// never folded away: within any group containing a pin, the smallest pinned id
/// becomes the representative, all other pins are lifted out (rendered as
/// themselves), and the remaining non-pinned nodes fold into the pinned rep. A
/// group left with no foldable members is dropped.
fn apply_pins(base: &FoldBase, pinned: &HashSet<u32>) -> (Vec<u32>, Vec<FoldGroup>) {
    let hidden: Vec<u32> = base
        .hidden
        .iter()
        .copied()
        .filter(|id| !pinned.contains(id))
        .collect();

    let mut groups: Vec<FoldGroup> = Vec::with_capacity(base.groups.len());
    for g in &base.groups {
        // All nodes in the group (rep + members).
        let mut all: Vec<u32> = Vec::with_capacity(g.member_ids.len() + 1);
        all.push(g.representative_id);
        all.extend(g.member_ids.iter().copied());

        let pins_here: Vec<u32> = all
            .iter()
            .copied()
            .filter(|id| pinned.contains(id))
            .collect();
        if pins_here.is_empty() {
            groups.push(g.clone());
            continue;
        }
        // Smallest pinned id leads; all other pins are excluded from folding.
        let rep = *pins_here.iter().min().unwrap();
        let mut member_ids: Vec<u32> = all
            .into_iter()
            .filter(|id| *id != rep && !pinned.contains(id))
            .collect();
        member_ids.sort_unstable();
        if member_ids.is_empty() {
            continue; // nothing left to fold — the group dissolves
        }
        groups.push(FoldGroup {
            representative_id: rep,
            member_ids: member_ids.clone(),
            badge: member_ids.len() as u32,
            kind: g.kind.clone(),
        });
    }
    groups.sort_by_key(|g| g.representative_id);
    (hidden, groups)
}

/// Parse the `?pinned=1,2,3` list into a masked id set. Non-numeric tokens are
/// skipped.
fn parse_pinned(raw: Option<&str>) -> HashSet<u32> {
    let mask = crate::utils::binary_protocol::NODE_ID_MASK;
    raw.map(|s| {
        s.split(',')
            .filter_map(|t| t.trim().parse::<u32>().ok())
            .map(|id| id & mask)
            .collect()
    })
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Memo — pin-agnostic base plans by (level, graph_type, generation)
// ---------------------------------------------------------------------------

type FoldCacheKey = (u8, String, u64);
static FOLD_CACHE: Lazy<Mutex<HashMap<FoldCacheKey, Arc<FoldBase>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Memoised base-plan lookup. On a generation change the whole cache is dropped
/// (topology rebuilt ⇒ every prior plan is stale), so it stays bounded to the
/// handful of live `(level, graph_type)` combinations for the current topology.
fn base_plan_memoised(
    level: u8,
    graph_type: Option<&str>,
    generation: u64,
    nodes: &[Node],
    edges: &[Edge],
    analytics: &HashMap<u32, Analytics>,
) -> Arc<FoldBase> {
    let key: FoldCacheKey = (level, graph_type.unwrap_or("").to_string(), generation);
    if let Ok(mut cache) = FOLD_CACHE.lock() {
        // Drop stale-generation entries.
        if cache.keys().any(|(_, _, g)| *g != generation) {
            cache.retain(|(_, _, g), _| *g == generation);
        }
        if let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
        let plan = Arc::new(compute_base_plan(
            level, nodes, edges, analytics, graph_type,
        ));
        cache.insert(key, plan.clone());
        return plan;
    }
    // Poisoned lock: compute without caching rather than fail the request.
    Arc::new(compute_base_plan(
        level, nodes, edges, analytics, graph_type,
    ))
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/graph/fold?level=<0..3>&graph_type=<..>&pinned=<csv>`
///
/// Returns the fold plan for the requested ladder level. Read-only, public (same
/// posture as the other `/graph` reads); mutates nothing.
pub async fn get_fold_plan(
    state: web::Data<AppState>,
    query: web::Query<FoldQuery>,
) -> impl Responder {
    let level = query.level.unwrap_or(0).min(MAX_FOLD_LEVEL);
    let graph_type = query.graph_type.clone();
    let pinned = parse_pinned(query.pinned.as_deref());

    let graph = match fetch_graph_snapshot(&state).await {
        Ok(g) => g,
        Err(e) => {
            error!("fold: failed to get graph snapshot: {}", e);
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to retrieve graph data"}));
        }
    };

    // Snapshot the shared analytics map (Copy values → a cheap clone; decoupled
    // from the live writer for the rest of the request).
    let analytics: HashMap<u32, Analytics> = state
        .node_analytics
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    let generation = topology_generation(&graph.nodes, &graph.edges);
    let base = base_plan_memoised(
        level,
        graph_type.as_deref(),
        generation,
        &graph.nodes,
        &graph.edges,
        &analytics,
    );
    let (hidden, groups) = apply_pins(&base, &pinned);

    // Diagnostics: count in-scope hierarchy edges (both endpoints visible) so an
    // empty L2 plan is explainable straight from curl; analytics_nodes exposes
    // whether L1/L3 had live analytics or ran on the degree fallback.
    let scope_ids: HashSet<u32> = visible_node_ids(&graph.nodes, graph_type.as_deref())
        .into_iter()
        .collect();
    let hierarchy_edges = graph
        .edges
        .iter()
        .filter(|e| {
            e.edge_type.as_deref().is_some_and(is_subclass_relation)
                && scope_ids.contains(&e.source)
                && scope_ids.contains(&e.target)
        })
        .count();

    HttpResponse::Ok().json(FoldPlan {
        level,
        graph_type,
        generation,
        hidden,
        groups,
        analytics_nodes: analytics.len(),
        hierarchy_edges,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, node_type: &str) -> Node {
        let mut n = Node::new_with_id(format!("meta-{id}"), Some(id));
        n.node_type = if node_type.is_empty() {
            None
        } else {
            Some(node_type.to_string())
        };
        n
    }

    fn edge(source: u32, target: u32, rel: &str) -> Edge {
        Edge::new(source, target, 1.0).with_edge_type(rel.to_string())
    }

    fn analytics(entries: &[(u32, u32, f32)]) -> HashMap<u32, Analytics> {
        entries
            .iter()
            .map(|&(id, community_id, centrality)| {
                (
                    id,
                    Analytics {
                        cluster_id: 0,
                        community_id,
                        anomaly: 0.0,
                        centrality,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn level_zero_is_empty() {
        let nodes = vec![node(1, "page"), node(2, "page")];
        let base = compute_base_plan(0, &nodes, &[], &HashMap::new(), None);
        assert!(base.hidden.is_empty());
        assert!(base.groups.is_empty());
    }

    #[test]
    fn subclass_chain_folds_into_root() {
        // Chain: 4→3→2→1 as child→parent (source=child, target=parent).
        // Node 1 is the top superclass (never a child) = chain root = rep.
        let nodes = vec![
            node(1, "owl_class"),
            node(2, "owl_class"),
            node(3, "owl_class"),
            node(4, "owl_class"),
            node(5, "owl_class"), // unconnected — must not form a group
        ];
        let edges = vec![
            edge(2, 1, "subclass_of"),
            edge(3, 2, "SUBCLASS_OF"),
            edge(4, 3, "is_subclass_of"),
        ];
        let base = compute_base_plan(2, &nodes, &edges, &HashMap::new(), None);
        assert_eq!(base.groups.len(), 1, "one subclass component");
        let g = &base.groups[0];
        assert_eq!(g.representative_id, 1, "chain root is representative");
        assert_eq!(g.member_ids, vec![2, 3, 4]);
        assert_eq!(g.badge, 3);
        assert_eq!(g.kind, "subclass");
    }

    #[test]
    fn non_hierarchy_edges_never_fold() {
        // Non-hierarchy relations (symmetric equivalence, plain links, structural
        // part-of) must NOT fold — only the hierarchy accept set does.
        let nodes = vec![node(1, "owl_class"), node(2, "owl_class")];
        let edges = vec![
            edge(1, 2, "equivalent_class"),
            edge(2, 1, "explicit_link"),
            edge(1, 2, "structural"),
        ];
        let base = compute_base_plan(2, &nodes, &edges, &HashMap::new(), None);
        assert!(
            base.groups.is_empty(),
            "non-hierarchy relations do not fold"
        );
    }

    #[test]
    fn collapsed_hierarchical_label_folds() {
        // Live deployments carry the COLLAPSED `hierarchical` semantic label (not
        // the raw `subclass_of`); it must fold, else L2 does nothing in production.
        let nodes = vec![
            node(1, "owl_class"),
            node(2, "owl_class"),
            node(3, "owl_class"),
        ];
        let edges = vec![edge(2, 1, "hierarchical"), edge(3, 2, "hierarchical")];
        let base = compute_base_plan(2, &nodes, &edges, &HashMap::new(), None);
        assert_eq!(base.groups.len(), 1, "hierarchical chain folds");
        assert_eq!(base.groups[0].representative_id, 1, "chain root is rep");
        assert_eq!(base.groups[0].member_ids, vec![2, 3]);
    }

    #[test]
    fn l1_degrades_to_degree_when_analytics_are_cold() {
        // No analytics at all: L1 must still hide the bottom-quartile-by-degree
        // nodes rather than returning an empty plan. Node 5 is isolated (degree 0),
        // the others form a connected core.
        let nodes: Vec<Node> = (1..=8).map(|i| node(i, "page")).collect();
        let edges = vec![
            edge(1, 2, "explicit_link"),
            edge(2, 3, "explicit_link"),
            edge(3, 4, "explicit_link"),
            edge(4, 6, "explicit_link"),
            edge(6, 7, "explicit_link"),
            edge(7, 8, "explicit_link"),
            edge(1, 4, "explicit_link"),
        ];
        // Empty analytics → degree fallback. Node 5 (degree 0) is bottom-quartile.
        let base = compute_base_plan(1, &nodes, &edges, &HashMap::new(), None);
        assert!(
            base.hidden.contains(&5),
            "isolated node hidden via degree fallback"
        );
        assert!(
            !base.hidden.is_empty(),
            "degree fallback produces a non-empty L1"
        );
    }

    #[test]
    fn community_folds_into_highest_centrality_medoid() {
        // Community 7 = {1,2,3}; node 2 has the top centrality → medoid.
        // Community 9 = {4} alone → no group. Node 5 community 0 → ignored.
        // Centralities keep community members above the L1 low-signal quartile
        // (only node 5, centrality 0.02, is bottom-quartile → hidden) so this
        // test isolates medoid selection from L1 hiding.
        let nodes = vec![
            node(1, "page"),
            node(2, "page"),
            node(3, "page"),
            node(4, "page"),
            node(5, "page"),
        ];
        let a = analytics(&[
            (1, 7, 0.50),
            (2, 7, 0.90),
            (3, 7, 0.60),
            (4, 9, 0.40),
            (5, 0, 0.02),
        ]);
        let base = compute_base_plan(3, &nodes, &[], &a, None);
        // No subclass edges, so all groups are community.
        assert_eq!(base.groups.len(), 1);
        let g = &base.groups[0];
        assert_eq!(g.kind, "community");
        assert_eq!(g.representative_id, 2, "highest-centrality node is medoid");
        assert_eq!(g.member_ids, vec![1, 3]);
        assert_eq!(g.badge, 2);
    }

    #[test]
    fn l3_excludes_subclass_members_from_community_fold() {
        // Nodes 1,2 are a subclass chain AND share community 7 with node 3.
        // At L3 the subclass group owns 1,2; community fold sees only leftovers,
        // so node 3 alone can't form a community group.
        let nodes = vec![node(1, "owl_class"), node(2, "owl_class"), node(3, "page")];
        let edges = vec![edge(2, 1, "subclass_of")];
        let a = analytics(&[(1, 7, 0.5), (2, 7, 0.4), (3, 7, 0.9)]);
        let base = compute_base_plan(3, &nodes, &edges, &a, None);
        assert_eq!(base.groups.len(), 1);
        assert_eq!(base.groups[0].kind, "subclass");
        assert_eq!(base.groups[0].representative_id, 1);
    }

    #[test]
    fn low_signal_hides_bottom_quartile() {
        // Eight scored nodes; bottom quartile (below 25th pct) hidden.
        let nodes: Vec<Node> = (1..=8).map(|i| node(i, "page")).collect();
        let a = analytics(&[
            (1, 0, 0.05),
            (2, 0, 0.10),
            (3, 0, 0.20),
            (4, 0, 0.30),
            (5, 0, 0.40),
            (6, 0, 0.50),
            (7, 0, 0.60),
            (8, 0, 0.90),
        ]);
        let base = compute_base_plan(1, &nodes, &[], &a, None);
        // p25 of the 8 sorted centralities = index floor(0.25*7)=1 → 0.10.
        // Strictly below 0.10 → only node 1 (0.05).
        assert_eq!(base.hidden, vec![1]);
    }

    #[test]
    fn low_signal_empty_without_analytics() {
        let nodes: Vec<Node> = (1..=8).map(|i| node(i, "page")).collect();
        let base = compute_base_plan(1, &nodes, &[], &HashMap::new(), None);
        assert!(base.hidden.is_empty(), "no analytics ⇒ nothing hidden");
    }

    #[test]
    fn pinned_member_is_promoted_to_representative() {
        let base = FoldBase {
            hidden: vec![],
            groups: vec![FoldGroup {
                representative_id: 1,
                member_ids: vec![2, 3, 4],
                badge: 3,
                kind: "subclass".to_string(),
            }],
        };
        // Pin node 3 (a member): it becomes the rep; old rep 1 folds in.
        let pinned: HashSet<u32> = [3].into_iter().collect();
        let (_hidden, groups) = apply_pins(&base, &pinned);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].representative_id, 3);
        assert_eq!(groups[0].member_ids, vec![1, 2, 4]);
        assert_eq!(groups[0].badge, 3);
    }

    #[test]
    fn multiple_pins_lift_extras_out_of_the_fold() {
        let base = FoldBase {
            hidden: vec![],
            groups: vec![FoldGroup {
                representative_id: 1,
                member_ids: vec![2, 3],
                badge: 2,
                kind: "community".to_string(),
            }],
        };
        // Pin 2 and 3: smallest (2) becomes rep; 3 is lifted out (not folded);
        // only original rep 1 remains a member.
        let pinned: HashSet<u32> = [2, 3].into_iter().collect();
        let (_hidden, groups) = apply_pins(&base, &pinned);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].representative_id, 2);
        assert_eq!(groups[0].member_ids, vec![1], "pin 3 is not folded away");
    }

    #[test]
    fn pinned_node_is_never_hidden() {
        let base = FoldBase {
            hidden: vec![1, 2, 3],
            groups: vec![],
        };
        let pinned: HashSet<u32> = [2].into_iter().collect();
        let (hidden, _groups) = apply_pins(&base, &pinned);
        assert_eq!(hidden, vec![1, 3], "pinned id 2 lifted out of hidden");
    }

    #[test]
    fn group_dissolves_when_all_members_pinned() {
        let base = FoldBase {
            hidden: vec![],
            groups: vec![FoldGroup {
                representative_id: 1,
                member_ids: vec![2],
                badge: 1,
                kind: "subclass".to_string(),
            }],
        };
        // Pin both nodes → nothing left to fold → group dropped.
        let pinned: HashSet<u32> = [1, 2].into_iter().collect();
        let (_hidden, groups) = apply_pins(&base, &pinned);
        assert!(groups.is_empty());
    }

    #[test]
    fn generation_changes_with_topology() {
        let n1 = vec![node(1, "page"), node(2, "page")];
        let e1 = vec![edge(1, 2, "subclass_of")];
        let g_base = topology_generation(&n1, &e1);
        // Adding a node changes it.
        let n2 = vec![node(1, "page"), node(2, "page"), node(3, "page")];
        assert_ne!(g_base, topology_generation(&n2, &e1));
        // Adding an edge changes it.
        let e2 = vec![edge(1, 2, "subclass_of"), edge(2, 3, "subclass_of")];
        assert_ne!(g_base, topology_generation(&n1, &e2));
        // Same topology → identical generation (stable, deterministic).
        assert_eq!(g_base, topology_generation(&n1, &e1));
    }

    #[test]
    fn population_filter_scopes_the_fold() {
        // Two ontology classes forming a chain, plus a page node. graph_type=page
        // (knowledge) must exclude the ontology nodes → no subclass group.
        let nodes = vec![node(1, "owl_class"), node(2, "owl_class"), node(3, "page")];
        let edges = vec![edge(2, 1, "subclass_of")];
        let base = compute_base_plan(2, &nodes, &edges, &HashMap::new(), Some("knowledge"));
        assert!(
            base.groups.is_empty(),
            "ontology subclass edge filtered out under knowledge scope"
        );
        // Under ontology scope the chain folds.
        let base_o = compute_base_plan(2, &nodes, &edges, &HashMap::new(), Some("ontology"));
        assert_eq!(base_o.groups.len(), 1);
        assert_eq!(base_o.groups[0].representative_id, 1);
    }
}
