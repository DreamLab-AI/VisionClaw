//! Read-side indexes over a snapshot: adjacency, file lookup, trigram search
//! and the breadth-first queries (shortest path, SSSP fields).
//!
//! graphicle's `serve_find::shortest_path_with` sits in a private module and
//! its `serve_search::SearchIndex` is `#[doc(hidden)]` and keyed by graphicle
//! node positions, so both are implemented here directly on interned ids.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use codescape_model::{GraphSnapshot, NodeKind, Relation};

/// Derived, rebuildable indexes over one snapshot.
#[derive(Debug, Default)]
pub(crate) struct View {
    /// Node id → position in `snapshot.nodes`.
    pub(crate) pos: HashMap<u32, usize>,
    /// Outgoing edge positions per node position.
    pub(crate) out: Vec<Vec<u32>>,
    /// Incoming edge positions per node position.
    pub(crate) inc: Vec<Vec<u32>>,
    /// Repository-relative file path → file node id.
    pub(crate) files: HashMap<Arc<str>, u32>,
    search: Trigrams,
}

/// Lower-cased ASCII-folded text a node is found by.
fn search_text(kind: &NodeKind, label: &str, path: Option<&str>) -> String {
    let mut s = label.to_lowercase();
    if matches!(kind, NodeKind::File | NodeKind::Directory) {
        if let Some(p) = path {
            s.push(' ');
            s.push_str(&p.to_lowercase());
        }
    }
    s
}

fn trigrams(s: &str) -> HashSet<[char; 3]> {
    let chars: Vec<char> = s.chars().collect();
    chars.windows(3).map(|w| [w[0], w[1], w[2]]).collect()
}

/// Inverted trigram index over node labels (and file/directory paths).
#[derive(Debug, Default)]
struct Trigrams {
    postings: HashMap<[char; 3], Vec<u32>>,
    /// Per node position: distinct trigram count and folded label.
    counts: Vec<u32>,
    labels: Vec<String>,
}

impl Trigrams {
    fn build(snapshot: &GraphSnapshot) -> Self {
        let mut me = Self {
            counts: Vec::with_capacity(snapshot.nodes.len()),
            labels: Vec::with_capacity(snapshot.nodes.len()),
            ..Default::default()
        };
        for (k, n) in snapshot.nodes.iter().enumerate() {
            let text = search_text(&n.kind, &n.label, n.path.as_deref());
            let grams = trigrams(&text);
            me.counts.push(grams.len() as u32);
            for g in grams {
                me.postings.entry(g).or_default().push(k as u32);
            }
            me.labels.push(n.label.to_lowercase());
        }
        me
    }

    /// `(position, score)` best first: trigram Jaccard plus bonuses for an
    /// exact (1.0), prefix (0.5) or substring (0.25) label match.
    fn query(&self, snapshot: &GraphSnapshot, q: &str, limit: usize) -> Vec<(usize, f32)> {
        let q = q.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let qgrams = trigrams(&q);
        let mut shared: HashMap<u32, u32> = HashMap::new();
        for g in &qgrams {
            if let Some(list) = self.postings.get(g) {
                for &k in list {
                    *shared.entry(k).or_default() += 1;
                }
            }
        }
        let bonus = |k: usize| {
            let l = &self.labels[k];
            if *l == q {
                1.0
            } else if l.starts_with(&q) {
                0.5
            } else if l.contains(&q) {
                0.25
            } else {
                0.0
            }
        };
        let mut scored: Vec<(usize, f32)> = if qgrams.is_empty() {
            // One- and two-character queries: label prefix/substring scan.
            (0..self.labels.len())
                .filter_map(|k| Some((k, bonus(k))).filter(|&(_, s)| s > 0.0))
                .collect()
        } else {
            shared
                .into_iter()
                .map(|(k, s)| {
                    let k = k as usize;
                    let union = qgrams.len() as f32 + self.counts[k] as f32 - s as f32;
                    (k, s as f32 / union.max(1.0) + bonus(k))
                })
                .collect()
        };
        scored.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(
                    snapshot.nodes[b.0]
                        .importance
                        .total_cmp(&snapshot.nodes[a.0].importance),
                )
                .then(snapshot.nodes[a.0].id.cmp(&snapshot.nodes[b.0].id))
        });
        scored.truncate(limit);
        scored
    }
}

impl View {
    /// Build every index for `snapshot`.
    pub(crate) fn build(snapshot: &GraphSnapshot) -> Self {
        let pos = snapshot.index();
        let mut out = vec![Vec::new(); snapshot.nodes.len()];
        let mut inc = vec![Vec::new(); snapshot.nodes.len()];
        for (k, e) in snapshot.edges.iter().enumerate() {
            if let (Some(&s), Some(&t)) = (pos.get(&e.source), pos.get(&e.target)) {
                out[s].push(k as u32);
                inc[t].push(k as u32);
            }
        }
        let files = snapshot
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .filter_map(|n| n.path.clone().map(|p| (p, n.id)))
            .collect();
        Self {
            pos,
            out,
            inc,
            files,
            search: Trigrams::build(snapshot),
        }
    }

    /// Ranked `(id, score)` matches for `q`.
    pub(crate) fn search(
        &self,
        snapshot: &GraphSnapshot,
        q: &str,
        limit: usize,
    ) -> Vec<(u32, f32)> {
        self.search
            .query(snapshot, q, limit)
            .into_iter()
            .map(|(k, s)| (snapshot.nodes[k].id, s))
            .collect()
    }

    /// Neighbours of `id` with the relation of the connecting edge.
    pub(crate) fn neighbors(
        &self,
        snapshot: &GraphSnapshot,
        id: u32,
        relations: Option<&[Relation]>,
        incoming: bool,
        outgoing: bool,
    ) -> Vec<(u32, Relation)> {
        let Some(&k) = self.pos.get(&id) else {
            return Vec::new();
        };
        let keep = |r: &Relation| relations.is_none_or(|rs| rs.contains(r));
        let mut res = Vec::new();
        if outgoing {
            for &e in &self.out[k] {
                let e = &snapshot.edges[e as usize];
                if keep(&e.relation) {
                    res.push((e.target, e.relation.clone()));
                }
            }
        }
        if incoming {
            for &e in &self.inc[k] {
                let e = &snapshot.edges[e as usize];
                if keep(&e.relation) {
                    res.push((e.source, e.relation.clone()));
                }
            }
        }
        res
    }

    /// Undirected neighbour positions of position `k`, skipping directory
    /// aggregates (they would short-circuit every path through `dir → file`).
    fn undirected<'a>(
        &'a self,
        snapshot: &'a GraphSnapshot,
        k: usize,
    ) -> impl Iterator<Item = usize> + 'a {
        let out = self.out[k]
            .iter()
            .map(move |&e| snapshot.edges[e as usize].target);
        let inc = self.inc[k]
            .iter()
            .map(move |&e| snapshot.edges[e as usize].source);
        out.chain(inc)
            .filter_map(move |id| self.pos.get(&id).copied())
            .filter(move |&p| snapshot.nodes[p].kind != NodeKind::Directory)
    }

    /// Shortest undirected path by bidirectional BFS, endpoints included.
    pub(crate) fn shortest_path(
        &self,
        snapshot: &GraphSnapshot,
        from: u32,
        to: u32,
    ) -> Option<Vec<u32>> {
        let (&a, &b) = (self.pos.get(&from)?, self.pos.get(&to)?);
        if a == b {
            return Some(vec![from]);
        }
        let mut parent_a: HashMap<usize, usize> = HashMap::from([(a, a)]);
        let mut parent_b: HashMap<usize, usize> = HashMap::from([(b, b)]);
        let mut frontier_a = vec![a];
        let mut frontier_b = vec![b];
        let meet = 'search: loop {
            if frontier_a.is_empty() || frontier_b.is_empty() {
                return None;
            }
            // Expand the smaller side one full level.
            let forward = frontier_a.len() <= frontier_b.len();
            let (frontier, parents, others) = if forward {
                (&mut frontier_a, &mut parent_a, &parent_b)
            } else {
                (&mut frontier_b, &mut parent_b, &parent_a)
            };
            let mut next = Vec::new();
            let mut found = None;
            for &v in frontier.iter() {
                for w in self.undirected(snapshot, v) {
                    if parents.contains_key(&w) {
                        continue;
                    }
                    parents.insert(w, v);
                    if others.contains_key(&w) && found.is_none() {
                        found = Some(w);
                    }
                    next.push(w);
                }
            }
            if let Some(m) = found {
                break 'search m;
            }
            *frontier = next;
        };
        let mut left = vec![meet];
        let mut cur = meet;
        while parent_a[&cur] != cur {
            cur = parent_a[&cur];
            left.push(cur);
        }
        left.reverse();
        let mut cur = meet;
        while parent_b[&cur] != cur {
            cur = parent_b[&cur];
            left.push(cur);
        }
        Some(left.into_iter().map(|p| snapshot.nodes[p].id).collect())
    }

    /// Undirected BFS from `from` up to `max_depth` hops: `(id, hops, parent id)`,
    /// the source first with parent 0 (the neutral SSSP parent of `ids.rs`).
    pub(crate) fn bfs_from(
        &self,
        snapshot: &GraphSnapshot,
        from: u32,
        max_depth: u32,
    ) -> Vec<(u32, f32, i32)> {
        let Some(&s) = self.pos.get(&from) else {
            return Vec::new();
        };
        let mut depth: HashMap<usize, u32> = HashMap::from([(s, 0)]);
        let mut res = vec![(from, 0.0, 0)];
        let mut queue = VecDeque::from([s]);
        while let Some(v) = queue.pop_front() {
            let d = depth[&v];
            if d >= max_depth {
                continue;
            }
            for w in self.undirected(snapshot, v) {
                if depth.contains_key(&w) {
                    continue;
                }
                depth.insert(w, d + 1);
                res.push((
                    snapshot.nodes[w].id,
                    (d + 1) as f32,
                    snapshot.nodes[v].id as i32,
                ));
                queue.push_back(w);
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use codescape_model::{CodeEdge, CodeNode, Confidence, Lang, Tier};

    use super::*;

    fn node(id: u32, label: &str, kind: NodeKind) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("k{id}")),
            kind,
            lang: Lang::Rust,
            label: Arc::from(label),
            path: Some(Arc::from(format!("src/{label}.rs"))),
            span: None,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: 0,
        }
    }
    fn edge(s: u32, t: u32, relation: Relation) -> CodeEdge {
        CodeEdge {
            source: s,
            target: t,
            relation,
            weight: 1.0,
            confidence: Confidence::Extracted,
        }
    }

    fn sample() -> GraphSnapshot {
        GraphSnapshot {
            nodes: vec![
                node(1, "parse_config", NodeKind::Function),
                node(2, "load", NodeKind::Function),
                node(3, "config", NodeKind::Struct),
                node(4, "render", NodeKind::Function),
                node(5, "src", NodeKind::Directory),
                node(6, "island", NodeKind::Function),
            ],
            edges: vec![
                edge(1, 2, Relation::Calls),
                edge(2, 3, Relation::References),
                edge(4, 3, Relation::Calls),
                edge(5, 1, Relation::Contains),
                edge(5, 4, Relation::Contains),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn search_ranks_exact_then_partial() {
        let s = sample();
        let v = View::build(&s);
        let hits = v.search(&s, "config", 5);
        assert_eq!(hits[0].0, 3, "exact label first: {hits:?}");
        assert_eq!(hits[1].0, 1);
        assert!(
            v.search(&s, "lo", 5).iter().any(|h| h.0 == 2),
            "short queries scan prefixes"
        );
        assert!(v.search(&s, "", 5).is_empty());
    }

    #[test]
    fn paths_avoid_directories_and_bfs_reports_parents() {
        let s = sample();
        let v = View::build(&s);
        assert_eq!(
            v.shortest_path(&s, 1, 4),
            Some(vec![1, 2, 3, 4]),
            "not the 1-dir-4 shortcut"
        );
        assert_eq!(v.shortest_path(&s, 1, 1), Some(vec![1]));
        assert_eq!(v.shortest_path(&s, 1, 6), None);
        assert_eq!(v.shortest_path(&s, 1, 99), None);
        let bfs = v.bfs_from(&s, 1, 2);
        assert_eq!(bfs, vec![(1, 0.0, 0), (2, 1.0, 1), (3, 2.0, 2)]);
        let n = v.neighbors(&s, 3, None, true, false);
        assert_eq!(n.len(), 2);
        let calls = v.neighbors(&s, 3, Some(&[Relation::Calls]), true, true);
        assert_eq!(calls, vec![(4, Relation::Calls)]);
    }
}
