//! Level-of-detail tiers.
//!
//! The server computes the same three tiers for every client: L0 directory
//! super-nodes, L1 files, L2 symbols. Aggregates get their own interned ids
//! (never a member's), and every symbol points at its file through `parent`,
//! every file at its top-level directory. Positions for aggregates are the
//! centroid of their members, so a client can cross-fade between tiers.

use std::collections::HashMap;
use std::sync::Arc;

use crate::graph::{CodeEdge, CodeNode, Confidence, GraphSnapshot, Lang, NodeKind, Relation, Tier};
use crate::ids::{IdInterner, InternError};

/// Key prefix for directory aggregates in the interner (never collides with a
/// graphicle id, which starts with a relative path or a symbol name).
pub const DIR_KEY_PREFIX: &str = "\u{1}dir:";

/// The top-level directory of a repository-relative path (`crates/x/src/a.rs` → `crates`),
/// or `.` for a root file.
pub fn top_dir(path: &str) -> &str {
    match path.find('/') {
        Some(i) if i > 0 => &path[..i],
        _ => ".",
    }
}

/// Ensure every L1 file node has an L0 directory parent, minting directory
/// aggregates as needed, and link symbols that lack a parent to their file.
///
/// Returns the number of directory nodes created. `contains` edges are added
/// for each new parent link so the layout's structural springs see them.
pub fn build_tiers(
    snapshot: &mut GraphSnapshot,
    interner: &mut IdInterner,
) -> Result<usize, InternError> {
    let mut file_by_path: HashMap<Arc<str>, u32> = HashMap::new();
    for n in &snapshot.nodes {
        if n.kind == NodeKind::File {
            if let Some(p) = &n.path {
                file_by_path.insert(p.clone(), n.id);
            }
        }
    }
    // Directories minted by an earlier pass are reused, so the pass is idempotent.
    let mut dir_ids: HashMap<String, u32> = snapshot
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Directory)
        .filter_map(|n| {
            n.key
                .strip_prefix(DIR_KEY_PREFIX)
                .map(|d| (d.to_string(), n.id))
        })
        .collect();
    let mut created = 0usize;
    let mut new_edges: Vec<CodeEdge> = Vec::new();
    let mut new_dirs: Vec<CodeNode> = Vec::new();

    for n in snapshot.nodes.iter_mut() {
        match n.kind {
            NodeKind::Directory => {
                n.tier = Tier::L0;
            }
            NodeKind::File => {
                n.tier = Tier::L1;
                let Some(path) = n.path.clone() else { continue };
                let dir = top_dir(&path).to_string();
                let id = match dir_ids.get(&dir) {
                    Some(&id) => id,
                    None => {
                        let id = interner.intern(&format!("{DIR_KEY_PREFIX}{dir}"))?;
                        dir_ids.insert(dir.clone(), id);
                        new_dirs.push(CodeNode {
                            id,
                            key: Arc::from(format!("{DIR_KEY_PREFIX}{dir}")),
                            kind: NodeKind::Directory,
                            lang: Lang::Other,
                            label: Arc::from(dir.as_str()),
                            path: Some(Arc::from(dir.as_str())),
                            span: None,
                            parent: 0,
                            community: 0,
                            cluster: 0,
                            importance: 0.0,
                            tier: Tier::L0,
                            size: 0,
                        });
                        created += 1;
                        id
                    }
                };
                if n.parent == 0 {
                    n.parent = id;
                    new_edges.push(CodeEdge {
                        source: id,
                        target: n.id,
                        relation: Relation::Contains,
                        weight: 1.0,
                        confidence: Confidence::Extracted,
                    });
                }
            }
            _ => {
                n.tier = Tier::L2;
                if n.parent == 0 {
                    if let Some(p) = &n.path {
                        if let Some(&fid) = file_by_path.get(p) {
                            n.parent = fid;
                            new_edges.push(CodeEdge {
                                source: fid,
                                target: n.id,
                                relation: Relation::Contains,
                                weight: 1.0,
                                confidence: Confidence::Extracted,
                            });
                        }
                    }
                }
            }
        }
    }

    // Cluster index = directory ordinal, assigned in sorted order for stability.
    let mut dirs: Vec<&String> = dir_ids.keys().collect();
    dirs.sort();
    let cluster_of: HashMap<u32, u32> = dirs
        .iter()
        .enumerate()
        .map(|(i, d)| (dir_ids[*d], i as u32 + 1))
        .collect();
    for d in new_dirs.iter_mut() {
        d.cluster = cluster_of[&d.id];
        d.size = 0;
    }
    snapshot.nodes.extend(new_dirs);
    snapshot.edges.extend(new_edges);

    // Propagate cluster down: file ← dir, symbol ← file. Two passes suffice.
    let mut cluster_by_id: HashMap<u32, u32> = snapshot
        .nodes
        .iter()
        .filter(|n| n.cluster != 0)
        .map(|n| (n.id, n.cluster))
        .collect();
    for _ in 0..2 {
        let mut updates = Vec::new();
        for n in &snapshot.nodes {
            if n.cluster == 0 && n.parent != 0 {
                if let Some(&c) = cluster_by_id.get(&n.parent) {
                    updates.push((n.id, c));
                }
            }
        }
        if updates.is_empty() {
            break;
        }
        let idx = snapshot.index();
        for (id, c) in updates {
            snapshot.nodes[idx[&id]].cluster = c;
            cluster_by_id.insert(id, c);
        }
    }

    // Directory size = member file count, for label weighting.
    let mut dir_size: HashMap<u32, u32> = HashMap::new();
    for n in &snapshot.nodes {
        if n.tier == Tier::L1 && n.parent != 0 {
            *dir_size.entry(n.parent).or_default() += 1;
        }
    }
    for n in snapshot.nodes.iter_mut() {
        if n.tier == Tier::L0 {
            n.size = dir_size.get(&n.id).copied().unwrap_or(0);
        }
    }
    Ok(created)
}

/// Centroid positions for every non-L2 node from its members' positions.
///
/// `positions` is indexed like `snapshot.nodes`. Members are L2 → L1 → L0;
/// an aggregate with no positioned members keeps its own position.
pub fn aggregate_positions(snapshot: &GraphSnapshot, positions: &mut [[f32; 3]]) {
    let idx = snapshot.index();
    let mut sum: HashMap<u32, ([f64; 3], usize)> = HashMap::new();
    for tier in [Tier::L2, Tier::L1] {
        sum.clear();
        for (i, n) in snapshot.nodes.iter().enumerate() {
            if n.tier == tier && n.parent != 0 {
                let e = sum.entry(n.parent).or_insert(([0.0; 3], 0));
                for (acc, p) in e.0.iter_mut().zip(positions[i].iter()) {
                    *acc += *p as f64;
                }
                e.1 += 1;
            }
        }
        for (parent, (s, c)) in &sum {
            if let Some(&pi) = idx.get(parent) {
                for k in 0..3 {
                    positions[pi][k] = (s[k] / *c as f64) as f32;
                }
            }
        }
    }
}

/// Select the ids a client at a given tier should receive: every node at or
/// above `tier`, plus L2 symbols whose file is in `focus_files`.
pub fn project(snapshot: &GraphSnapshot, tier: Tier, focus_files: &[u32]) -> Vec<u32> {
    let focus: std::collections::HashSet<u32> = focus_files.iter().copied().collect();
    snapshot
        .nodes
        .iter()
        .filter(|n| n.tier <= tier || (n.tier == Tier::L2 && focus.contains(&n.parent)))
        .map(|n| n.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(interner: &mut IdInterner, key: &str, kind: NodeKind, path: Option<&str>) -> CodeNode {
        let id = interner.intern(key).unwrap();
        CodeNode {
            id,
            key: Arc::from(key),
            kind,
            lang: Lang::Rust,
            label: Arc::from(key),
            path: path.map(Arc::from),
            span: None,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: 0,
        }
    }

    #[test]
    fn tiers_are_built_and_clusters_propagate() {
        let mut it = IdInterner::new(1);
        let mut g = GraphSnapshot::default();
        g.nodes
            .push(node(&mut it, "src/a.rs", NodeKind::File, Some("src/a.rs")));
        g.nodes.push(node(
            &mut it,
            "src/a.rs::foo",
            NodeKind::Function,
            Some("src/a.rs"),
        ));
        g.nodes.push(node(
            &mut it,
            "crates/x/lib.rs",
            NodeKind::File,
            Some("crates/x/lib.rs"),
        ));
        g.nodes.push(node(
            &mut it,
            "README.md",
            NodeKind::File,
            Some("README.md"),
        ));
        let created = build_tiers(&mut g, &mut it).unwrap();
        assert_eq!(created, 3, "src, crates and . directories");
        assert!(g.validate().is_ok());
        assert_eq!(g.tier_counts(), [3, 3, 1]);
        let foo = g.nodes.iter().find(|n| &*n.key == "src/a.rs::foo").unwrap();
        let a = g.nodes.iter().find(|n| &*n.key == "src/a.rs").unwrap();
        assert_eq!(foo.parent, a.id);
        assert_ne!(a.parent, 0);
        assert_eq!(foo.cluster, a.cluster);
        assert_ne!(foo.cluster, 0);
        let dir = g.node(a.parent).unwrap();
        assert_eq!(dir.kind, NodeKind::Directory);
        assert_eq!(dir.size, 1);
        assert_eq!(
            g.edges
                .iter()
                .filter(|e| e.relation == Relation::Contains)
                .count(),
            4
        );
        // Idempotent: a second pass mints nothing new.
        assert_eq!(build_tiers(&mut g, &mut it).unwrap(), 0);
    }

    #[test]
    fn aggregate_positions_take_centroids() {
        let mut it = IdInterner::new(1);
        let mut g = GraphSnapshot::default();
        g.nodes
            .push(node(&mut it, "f.rs", NodeKind::File, Some("f.rs")));
        g.nodes
            .push(node(&mut it, "f.rs::a", NodeKind::Function, Some("f.rs")));
        g.nodes
            .push(node(&mut it, "f.rs::b", NodeKind::Function, Some("f.rs")));
        build_tiers(&mut g, &mut it).unwrap();
        let idx = g.index();
        let mut pos = vec![[0.0f32; 3]; g.nodes.len()];
        pos[idx[&2]] = [2.0, 0.0, 0.0];
        pos[idx[&3]] = [4.0, 2.0, 0.0];
        aggregate_positions(&g, &mut pos);
        assert_eq!(pos[idx[&1]], [3.0, 1.0, 0.0]);
        let dir = g.nodes.iter().find(|n| n.tier == Tier::L0).unwrap();
        assert_eq!(pos[idx[&dir.id]], [3.0, 1.0, 0.0]);
        assert_eq!(project(&g, Tier::L1, &[]).len(), 2);
        assert_eq!(project(&g, Tier::L1, &[1]).len(), 4);
    }

    #[test]
    fn top_dir_handles_edges() {
        assert_eq!(top_dir("a/b/c"), "a");
        assert_eq!(top_dir("file"), ".");
        assert_eq!(top_dir("/abs"), ".");
    }
}
