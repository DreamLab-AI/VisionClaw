//! Communities and importance over a snapshot.
//!
//! Leiden comes from graphicle ([`crate::engine::communities`]); its labels are
//! stable-matched against the previous assignment here. Betweenness is
//! computed here because graphicle's Brandes implementation lives in a private
//! module (`graph_algorithms`) with no public wrapper. It is unweighted Brandes
//! over the undirected symbol/file graph (directory aggregates excluded, so the
//! synthetic `dir → file` edges cannot turn every directory into a hub),
//! sampled above [`SAMPLE_THRESHOLD`] nodes, normalised to 0..1.

use std::collections::HashMap;

use codescape_model::{stable_match, GraphSnapshot, NodeKind, Tier};
use rayon::prelude::*;

/// Above this many nodes betweenness is estimated from a sample of sources.
pub(crate) const SAMPLE_THRESHOLD: usize = 20_000;
/// Sample size when the configuration does not set one.
pub(crate) const DEFAULT_SAMPLE: usize = 512;

/// Assign stable-matched community labels and return the retained fraction.
///
/// `labels` are `(node id, raw label ≥ 1)`; ids absent from it get 0.
/// Directories take the most common community of their files.
pub(crate) fn apply_communities(snapshot: &mut GraphSnapshot, labels: &[(u32, u32)]) -> f32 {
    let previous: Vec<(u32, u32)> = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Directory)
        .map(|n| (n.id, n.community))
        .collect();
    let m = stable_match(&previous, labels);
    let by_id: HashMap<u32, u32> = labels.iter().map(|&(id, l)| (id, m.map(l))).collect();
    for n in snapshot.nodes.iter_mut() {
        if n.kind != NodeKind::Directory {
            n.community = by_id.get(&n.id).copied().unwrap_or(0);
        }
    }
    derive_aggregates(snapshot);
    m.retained
}

/// Directory community = most common community of its files (lowest label on
/// ties); directory importance = the highest importance among its files.
pub(crate) fn derive_aggregates(snapshot: &mut GraphSnapshot) {
    let mut votes: HashMap<u32, HashMap<u32, usize>> = HashMap::new();
    let mut best: HashMap<u32, f32> = HashMap::new();
    for n in &snapshot.nodes {
        if n.tier == Tier::L1 && n.parent != 0 {
            if n.community != 0 {
                *votes
                    .entry(n.parent)
                    .or_default()
                    .entry(n.community)
                    .or_default() += 1;
            }
            let b = best.entry(n.parent).or_insert(0.0);
            *b = b.max(n.importance);
        }
    }
    for n in snapshot.nodes.iter_mut() {
        if n.kind == NodeKind::Directory {
            n.community = votes
                .get(&n.id)
                .and_then(|v| {
                    v.iter()
                        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                        .map(|(&c, _)| c)
                })
                .unwrap_or(0);
            n.importance = best.get(&n.id).copied().unwrap_or(0.0);
        }
    }
}

/// Give nodes in `new_ids` that have no community yet the most common
/// community among their neighbours, else their parent's.
pub(crate) fn fill_new(snapshot: &mut GraphSnapshot, new_ids: &[u32]) {
    if new_ids.is_empty() {
        return;
    }
    let idx = snapshot.index();
    let wanted: HashMap<u32, ()> = new_ids
        .iter()
        .filter(|id| {
            idx.get(id)
                .is_some_and(|&k| snapshot.nodes[k].community == 0)
        })
        .map(|&id| (id, ()))
        .collect();
    let mut votes: HashMap<u32, HashMap<u32, usize>> = HashMap::new();
    for e in &snapshot.edges {
        for (me, other) in [(e.source, e.target), (e.target, e.source)] {
            if wanted.contains_key(&me) {
                if let Some(&k) = idx.get(&other) {
                    let c = snapshot.nodes[k].community;
                    if c != 0 && snapshot.nodes[k].kind != NodeKind::Directory {
                        *votes.entry(me).or_default().entry(c).or_default() += 1;
                    }
                }
            }
        }
    }
    let parent_comm: HashMap<u32, u32> =
        snapshot.nodes.iter().map(|n| (n.id, n.community)).collect();
    for n in snapshot.nodes.iter_mut() {
        if wanted.contains_key(&n.id) {
            n.community = votes
                .get(&n.id)
                .and_then(|v| {
                    v.iter()
                        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                        .map(|(&c, _)| c)
                })
                .or_else(|| parent_comm.get(&n.parent).copied())
                .unwrap_or(0);
        }
    }
}

/// SplitMix64: a fixed, seedless mixing of ids so the sample of sources is
/// deterministic and independent of node order.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Brandes betweenness of every non-directory node, normalised so the
/// maximum is 1. Returns one value per entry of `snapshot.nodes`
/// (directories get 0). `sample` is the number of sources used when the
/// graph has more than [`SAMPLE_THRESHOLD`] nodes.
pub(crate) fn betweenness(snapshot: &GraphSnapshot, sample: Option<usize>) -> Vec<f32> {
    let total = snapshot.nodes.len();
    // Dense ordinals over the non-directory nodes.
    let mut ord = vec![u32::MAX; total];
    let mut back = Vec::new();
    for (k, n) in snapshot.nodes.iter().enumerate() {
        if n.kind != NodeKind::Directory {
            ord[k] = back.len() as u32;
            back.push(k);
        }
    }
    let n = back.len();
    if n < 3 {
        return vec![0.0; total];
    }
    let idx = snapshot.index();
    let mut pairs: Vec<(u32, u32)> = Vec::with_capacity(snapshot.edges.len() * 2);
    for e in &snapshot.edges {
        let (Some(&a), Some(&b)) = (idx.get(&e.source), idx.get(&e.target)) else {
            continue;
        };
        let (a, b) = (ord[a], ord[b]);
        if a != u32::MAX && b != u32::MAX && a != b {
            pairs.push((a, b));
            pairs.push((b, a));
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    let mut offsets = vec![0u32; n + 1];
    for &(a, _) in &pairs {
        offsets[a as usize + 1] += 1;
    }
    for i in 0..n {
        offsets[i + 1] += offsets[i];
    }
    let targets: Vec<u32> = pairs.iter().map(|&(_, b)| b).collect();

    let sources: Vec<u32> = if n > SAMPLE_THRESHOLD {
        let k = sample.unwrap_or(DEFAULT_SAMPLE).clamp(1, n);
        let mut ranked: Vec<(u64, u32)> = (0..n as u32)
            .map(|o| (mix(u64::from(snapshot.nodes[back[o as usize]].id)), o))
            .collect();
        ranked.select_nth_unstable(k - 1);
        let mut chosen: Vec<u32> = ranked[..k].iter().map(|&(_, o)| o).collect();
        chosen.sort_unstable();
        chosen
    } else {
        (0..n as u32).collect()
    };

    // Chunks run in parallel; their partial sums are added in chunk order, so
    // the floating-point result does not depend on scheduling.
    let partials: Vec<Vec<f64>> = sources
        .par_chunks(64)
        .map(|chunk| {
            let mut acc = vec![0.0f64; n];
            let mut sigma = vec![0.0f64; n];
            let mut dist = vec![-1i32; n];
            let mut delta = vec![0.0f64; n];
            let mut preds: Vec<Vec<u32>> = vec![Vec::new(); n];
            let mut stack: Vec<u32> = Vec::with_capacity(n);
            let mut queue = std::collections::VecDeque::with_capacity(n);
            for &s in chunk {
                for &v in &stack {
                    let v = v as usize;
                    sigma[v] = 0.0;
                    dist[v] = -1;
                    delta[v] = 0.0;
                    preds[v].clear();
                }
                stack.clear();
                sigma[s as usize] = 1.0;
                dist[s as usize] = 0;
                queue.push_back(s);
                while let Some(v) = queue.pop_front() {
                    stack.push(v);
                    let v = v as usize;
                    for &w in &targets[offsets[v] as usize..offsets[v + 1] as usize] {
                        let wu = w as usize;
                        if dist[wu] < 0 {
                            dist[wu] = dist[v] + 1;
                            queue.push_back(w);
                        }
                        if dist[wu] == dist[v] + 1 {
                            sigma[wu] += sigma[v];
                            preds[wu].push(v as u32);
                        }
                    }
                }
                for &w in stack.iter().rev() {
                    let w = w as usize;
                    for &v in &preds[w] {
                        let v = v as usize;
                        delta[v] += sigma[v] / sigma[w] * (1.0 + delta[w]);
                    }
                    if w != s as usize {
                        acc[w] += delta[w];
                    }
                }
            }
            acc
        })
        .collect();
    let mut sum = vec![0.0f64; n];
    for p in partials {
        for (s, v) in sum.iter_mut().zip(p) {
            *s += v;
        }
    }
    let max = sum.iter().copied().fold(0.0f64, f64::max);
    let mut out = vec![0.0f32; total];
    if max > 0.0 {
        for (o, &k) in back.iter().enumerate() {
            out[k] = (sum[o] / max).clamp(0.0, 1.0) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use codescape_model::{CodeEdge, CodeNode, Confidence, Lang, Relation};

    use super::*;

    fn node(id: u32, kind: NodeKind, tier: Tier, parent: u32) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("k{id}")),
            kind,
            lang: Lang::Rust,
            label: Arc::from(format!("n{id}")),
            path: None,
            span: None,
            parent,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier,
            size: 0,
        }
    }
    fn edge(s: u32, t: u32) -> CodeEdge {
        CodeEdge {
            source: s,
            target: t,
            relation: Relation::Calls,
            weight: 1.0,
            confidence: Confidence::Extracted,
        }
    }

    #[test]
    fn path_centre_has_the_highest_betweenness() {
        // 1 - 2 - 3 - 4 - 5: node 3 lies on the most shortest paths.
        let snap = GraphSnapshot {
            nodes: (1..=5)
                .map(|i| node(i, NodeKind::Function, Tier::L2, 0))
                .collect(),
            edges: vec![edge(1, 2), edge(2, 3), edge(3, 4), edge(4, 5)],
            ..Default::default()
        };
        let b = betweenness(&snap, None);
        assert_eq!(b[2], 1.0);
        assert_eq!(b[0], 0.0);
        assert!(
            (b[1] - 0.75).abs() < 1e-6,
            "3 pairs through 2 vs 4 through 3: {b:?}"
        );
        assert!(b.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn directories_are_excluded_and_aggregate_their_files() {
        let mut snap = GraphSnapshot {
            nodes: vec![
                node(1, NodeKind::Directory, Tier::L0, 0),
                node(2, NodeKind::File, Tier::L1, 1),
                node(3, NodeKind::File, Tier::L1, 1),
                node(4, NodeKind::File, Tier::L1, 1),
            ],
            edges: vec![edge(1, 2), edge(1, 3), edge(1, 4), edge(2, 3), edge(3, 4)],
            ..Default::default()
        };
        let b = betweenness(&snap, None);
        assert_eq!(b[0], 0.0, "directory gets no betweenness");
        assert_eq!(b[2], 1.0);
        for (n, v) in snap.nodes.iter_mut().zip(&b) {
            n.importance = *v;
        }
        let retained = apply_communities(&mut snap, &[(2, 5), (3, 5), (4, 6)]);
        assert_eq!(retained, 1.0, "nothing to retain from: vacuously stable");
        assert_eq!(snap.nodes[0].community, snap.nodes[1].community);
        assert_eq!(snap.nodes[0].importance, 1.0);
    }

    #[test]
    fn new_nodes_join_their_neighbours() {
        let mut snap = GraphSnapshot {
            nodes: vec![
                node(1, NodeKind::Function, Tier::L2, 0),
                node(2, NodeKind::Function, Tier::L2, 0),
                node(3, NodeKind::Function, Tier::L2, 0),
            ],
            edges: vec![edge(3, 1), edge(3, 2), edge(2, 1)],
            ..Default::default()
        };
        snap.nodes[0].community = 4;
        snap.nodes[1].community = 4;
        fill_new(&mut snap, &[3]);
        assert_eq!(snap.nodes[2].community, 4);
    }

    #[test]
    fn sampling_is_deterministic() {
        let n = SAMPLE_THRESHOLD as u32 + 50;
        let snap = GraphSnapshot {
            nodes: (1..=n)
                .map(|i| node(i, NodeKind::Function, Tier::L2, 0))
                .collect(),
            edges: (1..n).map(|i| edge(i, i + 1)).collect(),
            ..Default::default()
        };
        let a = betweenness(&snap, Some(8));
        let b = betweenness(&snap, Some(8));
        assert_eq!(a, b);
        assert!(a.contains(&1.0));
    }
}
