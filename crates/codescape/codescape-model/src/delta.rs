//! Incremental changes between two revisions of a [`GraphSnapshot`].

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::graph::{CodeEdge, CodeNode, GraphSnapshot};

/// What changed between `from_revision` and `to_revision`.
///
/// A delta is small on purpose: graphicle's live path re-extracts only the
/// files that changed, and the layout engine warm-starts everything else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GraphDelta {
    /// Revision this delta applies to.
    pub from_revision: u64,
    /// Revision after applying it (always `from_revision + 1`).
    pub to_revision: u64,
    /// Unix milliseconds when the delta was produced.
    pub generated_at_ms: u64,
    /// Repository-relative paths whose re-extraction produced this delta.
    pub changed_paths: Vec<String>,
    /// Nodes that did not exist before.
    pub added_nodes: Vec<CodeNode>,
    /// Nodes whose attributes changed (same id).
    pub changed_nodes: Vec<CodeNode>,
    /// Ids of nodes that no longer exist.
    pub removed_nodes: Vec<u32>,
    /// New edges.
    pub added_edges: Vec<CodeEdge>,
    /// Edges that are gone (matched on source, target and relation).
    pub removed_edges: Vec<CodeEdge>,
}

impl GraphDelta {
    /// `true` when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.changed_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
    }

    /// Ids touched by this delta (added, changed, removed), for focus bursts.
    pub fn dirty_ids(&self) -> Vec<u32> {
        let mut set: HashSet<u32> = HashSet::new();
        set.extend(self.added_nodes.iter().map(|n| n.id));
        set.extend(self.changed_nodes.iter().map(|n| n.id));
        set.extend(self.removed_nodes.iter().copied());
        for e in self.added_edges.iter().chain(self.removed_edges.iter()) {
            set.insert(e.source);
            set.insert(e.target);
        }
        let mut v: Vec<u32> = set.into_iter().collect();
        v.sort_unstable();
        v
    }

    /// Compute the delta from `before` to `after`. Both must share an epoch.
    pub fn diff(before: &GraphSnapshot, after: &GraphSnapshot) -> Self {
        let b_idx = before.index();
        let a_idx = after.index();
        let mut d = GraphDelta {
            from_revision: before.revision,
            to_revision: after.revision,
            generated_at_ms: after.generated_at_ms,
            ..Default::default()
        };
        for n in &after.nodes {
            match b_idx.get(&n.id) {
                None => d.added_nodes.push(n.clone()),
                Some(&i) if before.nodes[i] != *n => d.changed_nodes.push(n.clone()),
                Some(_) => {}
            }
        }
        for n in &before.nodes {
            if !a_idx.contains_key(&n.id) {
                d.removed_nodes.push(n.id);
            }
        }
        let key = |e: &CodeEdge| (e.source, e.target, e.relation.clone());
        let b_edges: HashSet<_> = before.edges.iter().map(key).collect();
        let a_edges: HashSet<_> = after.edges.iter().map(key).collect();
        for e in &after.edges {
            if !b_edges.contains(&key(e)) {
                d.added_edges.push(e.clone());
            }
        }
        for e in &before.edges {
            if !a_edges.contains(&key(e)) {
                d.removed_edges.push(e.clone());
            }
        }
        d
    }

    /// Apply to `snapshot` in place. Returns `Err` if revisions do not line up.
    pub fn apply(&self, snapshot: &mut GraphSnapshot) -> Result<(), String> {
        if snapshot.revision != self.from_revision {
            return Err(format!(
                "delta is for revision {}, snapshot is at {}",
                self.from_revision, snapshot.revision
            ));
        }
        let removed: HashSet<u32> = self.removed_nodes.iter().copied().collect();
        snapshot.nodes.retain(|n| !removed.contains(&n.id));
        snapshot
            .edges
            .retain(|e| !removed.contains(&e.source) && !removed.contains(&e.target));
        let removed_edges: HashSet<_> = self
            .removed_edges
            .iter()
            .map(|e| (e.source, e.target, e.relation.clone()))
            .collect();
        snapshot
            .edges
            .retain(|e| !removed_edges.contains(&(e.source, e.target, e.relation.clone())));
        let mut idx = snapshot.index();
        for n in &self.changed_nodes {
            match idx.get(&n.id) {
                Some(&i) => snapshot.nodes[i] = n.clone(),
                None => {
                    idx.insert(n.id, snapshot.nodes.len());
                    snapshot.nodes.push(n.clone());
                }
            }
        }
        for n in &self.added_nodes {
            match idx.get(&n.id) {
                Some(&i) => snapshot.nodes[i] = n.clone(),
                None => {
                    idx.insert(n.id, snapshot.nodes.len());
                    snapshot.nodes.push(n.clone());
                }
            }
        }
        snapshot.edges.extend(self.added_edges.iter().cloned());
        snapshot.revision = self.to_revision;
        snapshot.generated_at_ms = self.generated_at_ms;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::graph::{Confidence, Lang, NodeKind, Relation, Tier};

    fn node(id: u32, label: &str) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("k{id}")),
            kind: NodeKind::Function,
            lang: Lang::Rust,
            label: Arc::from(label),
            path: None,
            span: None,
            parent: 0,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
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
    fn diff_then_apply_reproduces_after() {
        let before = GraphSnapshot {
            revision: 4,
            nodes: vec![node(1, "a"), node(2, "b"), node(3, "c")],
            edges: vec![edge(1, 2), edge(2, 3)],
            ..Default::default()
        };
        let after = GraphSnapshot {
            revision: 5,
            generated_at_ms: 99,
            nodes: vec![node(1, "a"), node(2, "b2"), node(4, "d")],
            edges: vec![edge(1, 2), edge(2, 4)],
            ..Default::default()
        };
        let d = GraphDelta::diff(&before, &after);
        assert_eq!(d.added_nodes.len(), 1);
        assert_eq!(d.changed_nodes.len(), 1);
        assert_eq!(d.removed_nodes, vec![3]);
        assert_eq!(d.added_edges, vec![edge(2, 4)]);
        assert_eq!(d.removed_edges, vec![edge(2, 3)]);
        assert_eq!(d.dirty_ids(), vec![2, 3, 4]);
        let mut rebuilt = before.clone();
        d.apply(&mut rebuilt).unwrap();
        let mut a = after.clone();
        a.nodes.sort_by_key(|n| n.id);
        rebuilt.nodes.sort_by_key(|n| n.id);
        a.edges.sort_by_key(|e| (e.source, e.target));
        rebuilt.edges.sort_by_key(|e| (e.source, e.target));
        assert_eq!(rebuilt, a);
        assert!(
            d.apply(&mut rebuilt).is_err(),
            "revision mismatch must be refused"
        );
    }
}
