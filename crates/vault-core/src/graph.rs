//! The graph over frontmatter links, with per-edge-type expansion.
//!
//! This is what `vault find`, `vault retrieve` and `vault tree` read. Edges are
//! typed by the frontmatter key that produced them, so a caller can ask for
//! "two hops of `is-a`, one hop of `requires`, and nothing else" —
//! `--expand is-a=2,requires=1` — instead of an undifferentiated N-hop
//! neighbourhood that drags in the whole corpus.
//!
//! Untyped body wikilinks are carried under the predicate [`LINK_PREDICATE`]
//! so backlinks and "related" expansion stay available without pretending the
//! link means something ontological.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::page::{Page, Vault};
use crate::vocabulary::Vocabulary;

/// The predicate used for untyped outbound wikilinks.
pub const LINK_PREDICATE: &str = "links";

/// One typed edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// The frontmatter key that declared it, or [`LINK_PREDICATE`].
    pub predicate: String,
    /// The target page id.
    pub target: String,
}

/// The per-node facts `find` scores and `retrieve` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Page id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// The OKF `type`, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    /// Alternative titles from `aliases`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `public: true`?
    pub public: bool,
}

/// A hit from [`VaultGraph::find`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    /// Page id.
    pub id: String,
    /// Display title.
    pub title: String,
    /// The OKF `type`, when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub node_type: Option<String>,
    /// Match score in `(0, 1]`; exact title match is `1.0`.
    pub score: f64,
}

/// One expanded node and how it was reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expansion {
    /// The reached page id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Hop distance from the nearest seed.
    pub depth: usize,
    /// The predicate traversed on the final hop.
    pub via: String,
    /// The node the final hop came from.
    pub from: String,
}

/// The result of [`VaultGraph::retrieve`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retrieval {
    /// The requested ids that exist, in request order.
    pub seeds: Vec<Node>,
    /// Everything reached by expansion, nearest first.
    pub expanded: Vec<Expansion>,
    /// Requested ids that are not pages.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// `true` when `max_documents` cut the expansion short.
    pub truncated: bool,
}

/// A node of [`VaultGraph::tree`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    /// Page id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Children, in edge order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

/// An immutable index over a vault's link structure.
#[derive(Debug, Clone)]
pub struct VaultGraph {
    nodes: IndexMap<String, Node>,
    out: HashMap<String, Vec<Edge>>,
    incoming: HashMap<String, Vec<Edge>>,
    /// Lowercased title / alias to id, for `find`.
    by_name: HashMap<String, String>,
}

impl VaultGraph {
    /// Build the graph from a loaded vault and its vocabulary.
    ///
    /// Relation keys come from `vocabulary.relations`; everything else in the
    /// frontmatter is ignored here. A link to a page that does not exist is
    /// kept as an edge (so `vault validate` can report it) but does not create
    /// a node.
    #[must_use]
    pub fn build(vault: &Vault, vocab: &Vocabulary) -> Self {
        let relation_keys: Vec<&str> = vocab.relations.keys().map(String::as_str).collect();
        let mut nodes = IndexMap::new();
        let mut by_name = HashMap::new();
        for page in &vault.pages {
            let title = page.title();
            let aliases = page.frontmatter.strings("aliases");
            by_name
                .entry(title.to_lowercase())
                .or_insert_with(|| page.id.clone());
            by_name
                .entry(page.id.to_lowercase())
                .or_insert_with(|| page.id.clone());
            for a in &aliases {
                by_name
                    .entry(a.to_lowercase())
                    .or_insert_with(|| page.id.clone());
            }
            nodes.insert(
                page.id.clone(),
                Node {
                    id: page.id.clone(),
                    title,
                    node_type: page.frontmatter.text("type"),
                    aliases,
                    public: page.is_public(),
                },
            );
        }

        let mut out: HashMap<String, Vec<Edge>> = HashMap::new();
        let mut incoming: HashMap<String, Vec<Edge>> = HashMap::new();
        for page in &vault.pages {
            let mut edges = Vec::new();
            for (predicate, links) in page.relations(relation_keys.iter().copied()) {
                for link in links {
                    edges.push(Edge {
                        predicate: predicate.clone(),
                        target: resolve(&by_name, &link.target),
                    });
                }
            }
            for link in page.outbound_links() {
                let target = resolve(&by_name, &link.target);
                if !edges.iter().any(|e| e.target == target) {
                    edges.push(Edge {
                        predicate: LINK_PREDICATE.to_owned(),
                        target,
                    });
                }
            }
            for e in &edges {
                incoming.entry(e.target.clone()).or_default().push(Edge {
                    predicate: e.predicate.clone(),
                    target: page.id.clone(),
                });
            }
            out.insert(page.id.clone(), edges);
        }

        Self {
            nodes,
            out,
            incoming,
            by_name,
        }
    }

    /// Number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` when the vault has no pages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Look a node up by id.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Resolve a title, alias or id to a page id.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.by_name.get(&name.to_lowercase()).map(String::as_str)
    }

    /// Outgoing edges from `id`.
    #[must_use]
    pub fn edges(&self, id: &str) -> &[Edge] {
        self.out.get(id).map_or(&[], Vec::as_slice)
    }

    /// Incoming edges to `id` (the backlink set).
    #[must_use]
    pub fn backlinks(&self, id: &str) -> &[Edge] {
        self.incoming.get(id).map_or(&[], Vec::as_slice)
    }

    /// Search titles and aliases.
    ///
    /// Scoring is deterministic and explainable, not a ranking model: exact
    /// title match `1.0`, exact alias `0.95`, prefix `0.8`, substring `0.6`,
    /// and — only with `fuzzy` — token overlap scaled into `(0, 0.5]`.
    #[must_use]
    pub fn find(
        &self,
        query: &str,
        node_type: Option<&str>,
        limit: usize,
        fuzzy: bool,
    ) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let q_tokens: HashSet<&str> = q.split_whitespace().collect();
        let mut hits: Vec<Hit> = self
            .nodes
            .values()
            .filter(|n| node_type.is_none_or(|t| n.node_type.as_deref() == Some(t)))
            .filter_map(|n| {
                let title = n.title.to_lowercase();
                let score = if title == q {
                    1.0
                } else if n.aliases.iter().any(|a| a.to_lowercase() == q) {
                    0.95
                } else if title.starts_with(&q) {
                    0.8
                } else if title.contains(&q) {
                    0.6
                } else if fuzzy {
                    let tokens: HashSet<&str> = title.split_whitespace().collect();
                    let shared = q_tokens.intersection(&tokens).count();
                    if shared == 0 {
                        return None;
                    }
                    // Token counts are tiny; the usize -> f64 conversion is exact.
                    #[allow(clippy::cast_precision_loss)]
                    let overlap = shared as f64 / q_tokens.len().max(1) as f64;
                    0.5 * overlap
                } else {
                    return None;
                };
                Some(Hit {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    node_type: n.node_type.clone(),
                    score,
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
    }

    /// Expand from `seeds`, following each predicate only as far as its
    /// declared depth.
    ///
    /// `expand` maps predicate to maximum hop count. A predicate absent from
    /// the map is never traversed, so the blast radius is always declared.
    /// `max_documents` caps the expansion (not the seeds).
    #[must_use]
    pub fn retrieve(
        &self,
        seeds: &[String],
        expand: &BTreeMap<String, usize>,
        max_documents: usize,
    ) -> Retrieval {
        let mut resolved = Vec::new();
        let mut missing = Vec::new();
        for s in seeds {
            match self.resolve(s) {
                Some(id) => resolved.push(id.to_owned()),
                None => missing.push(s.clone()),
            }
        }

        let mut visited: HashSet<String> = resolved.iter().cloned().collect();
        let mut expanded = Vec::new();
        let mut truncated = false;
        let mut queue: VecDeque<(String, usize)> =
            resolved.iter().map(|id| (id.clone(), 0usize)).collect();

        while let Some((id, depth)) = queue.pop_front() {
            for edge in self.edges(&id) {
                let Some(&max_depth) = expand.get(&edge.predicate) else {
                    continue;
                };
                if depth >= max_depth {
                    continue;
                }
                if !self.nodes.contains_key(&edge.target) || !visited.insert(edge.target.clone()) {
                    continue;
                }
                if expanded.len() >= max_documents {
                    truncated = true;
                    break;
                }
                expanded.push(Expansion {
                    id: edge.target.clone(),
                    title: self.nodes[&edge.target].title.clone(),
                    depth: depth + 1,
                    via: edge.predicate.clone(),
                    from: id.clone(),
                });
                queue.push_back((edge.target.clone(), depth + 1));
            }
            if truncated {
                break;
            }
        }

        Retrieval {
            seeds: resolved
                .iter()
                .filter_map(|id| self.nodes.get(id).cloned())
                .collect(),
            expanded,
            missing,
            truncated,
        }
    }

    /// The inverse-taxonomy tree rooted at `id`: the pages that declare `id` as
    /// a parent under `predicate`, recursively.
    ///
    /// Cycle-safe: a node already on the current path is not re-entered.
    #[must_use]
    pub fn tree(&self, id: &str, predicate: &str, depth: usize) -> Option<TreeNode> {
        let root = self.resolve(id)?.to_owned();
        let mut path = HashSet::new();
        Some(self.tree_inner(&root, predicate, depth, &mut path))
    }

    fn tree_inner(
        &self,
        id: &str,
        predicate: &str,
        depth: usize,
        path: &mut HashSet<String>,
    ) -> TreeNode {
        let title = self
            .nodes
            .get(id)
            .map_or_else(|| id.to_owned(), |n| n.title.clone());
        let mut children = Vec::new();
        if depth > 0 && path.insert(id.to_owned()) {
            let mut seen = HashSet::new();
            for edge in self.backlinks(id) {
                if edge.predicate == predicate
                    && !path.contains(&edge.target)
                    && seen.insert(edge.target.clone())
                {
                    children.push(self.tree_inner(&edge.target, predicate, depth - 1, path));
                }
            }
            children.sort_by(|a, b| a.title.cmp(&b.title));
            path.remove(id);
        }
        TreeNode {
            id: id.to_owned(),
            title,
            children,
        }
    }
}

/// Map a wikilink target onto a page id, falling back to the raw target when
/// nothing matches (dangling links stay visible to validation).
fn resolve(by_name: &HashMap<String, String>, target: &str) -> String {
    by_name
        .get(&target.to_lowercase())
        .cloned()
        .unwrap_or_else(|| target.to_owned())
}

/// Convenience: build a graph straight from a page slice, for callers that
/// already have pages in hand and do not want a [`Vault`].
#[must_use]
pub fn graph_of(pages: &[Page], vocab: &Vocabulary, kind: crate::page::VaultKind) -> VaultGraph {
    let vault = Vault {
        root: std::path::PathBuf::new(),
        kind,
        pages: pages.to_vec(),
        journals: Vec::new(),
        skipped: Vec::new(),
    };
    VaultGraph::build(&vault, vocab)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{Page, VaultKind};

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf", characteristics: [transitive] }
  requires: { owl: "vc:requires" }
"#,
        )
        .unwrap()
    }

    fn page(id: &str, fm: &str, body: &str) -> Page {
        Page::parse(
            format!("/v/pages/{id}.md"),
            format!("pages/{id}.md"),
            id,
            &format!("---\n{fm}---\n{body}"),
        )
        .unwrap()
    }

    fn fixture() -> VaultGraph {
        let pages = vec![
            page("Thing", "type: Class\npublic: true\n", ""),
            page(
                "Graph",
                "type: Class\npublic: true\naliases: [KG]\nis-a: [\"[[Thing]]\"]\nrequires: [\"[[Store]]\"]\n",
                "see [[Thing]]\n",
            ),
            page("Store", "type: Class\npublic: true\n", ""),
            page("Leaf", "type: Class\npublic: false\nis-a: [\"[[Graph]]\"]\n", ""),
        ];
        graph_of(&pages, &vocab(), VaultKind::Knowledge)
    }

    #[test]
    fn builds_typed_edges_and_backlinks() {
        let g = fixture();
        assert_eq!(g.len(), 4);
        let edges = g.edges("Graph");
        assert!(edges
            .iter()
            .any(|e| e.predicate == "is-a" && e.target == "Thing"));
        assert!(edges
            .iter()
            .any(|e| e.predicate == "requires" && e.target == "Store"));
        // The body link to Thing is already covered by the is-a edge.
        assert_eq!(edges.len(), 2);
        assert!(g.backlinks("Thing").iter().any(|e| e.target == "Graph"));
    }

    #[test]
    fn find_ranks_exact_then_alias_then_prefix() {
        let g = fixture();
        assert_eq!(g.find("graph", None, 10, false)[0].id, "Graph");
        assert!((g.find("KG", None, 10, false)[0].score - 0.95).abs() < 1e-9);
        assert!(g.find("Gra", None, 10, false)[0].score > 0.7);
        assert!(g.find("zzz", None, 10, false).is_empty());
    }

    #[test]
    fn find_filters_by_type() {
        let g = fixture();
        assert!(!g.find("Graph", Some("Class"), 5, false).is_empty());
        assert!(g.find("Graph", Some("Note"), 5, false).is_empty());
    }

    #[test]
    fn retrieve_only_follows_declared_predicates() {
        let g = fixture();
        let mut expand = BTreeMap::new();
        expand.insert("is-a".to_owned(), 1);
        let r = g.retrieve(&["Graph".into()], &expand, 100);
        assert_eq!(r.seeds.len(), 1);
        assert_eq!(r.expanded.len(), 1);
        assert_eq!(r.expanded[0].id, "Thing");
        assert_eq!(r.expanded[0].via, "is-a");
        assert!(!r.truncated);
    }

    #[test]
    fn retrieve_respects_max_documents() {
        let g = fixture();
        let mut expand = BTreeMap::new();
        expand.insert("is-a".to_owned(), 2);
        expand.insert("requires".to_owned(), 2);
        let r = g.retrieve(&["Graph".into()], &expand, 1);
        assert_eq!(r.expanded.len(), 1);
        assert!(r.truncated);
    }

    #[test]
    fn retrieve_reports_missing_seeds() {
        let g = fixture();
        let r = g.retrieve(&["Nope".into()], &BTreeMap::new(), 10);
        assert_eq!(r.missing, vec!["Nope".to_owned()]);
        assert!(r.seeds.is_empty());
    }

    #[test]
    fn tree_walks_children_not_parents() {
        let g = fixture();
        let t = g.tree("Thing", "is-a", 3).unwrap();
        assert_eq!(t.id, "Thing");
        assert_eq!(t.children.len(), 1);
        assert_eq!(t.children[0].id, "Graph");
        assert_eq!(t.children[0].children[0].id, "Leaf");
    }

    #[test]
    fn tree_survives_a_cycle() {
        let pages = vec![
            page("A", "is-a: [\"[[B]]\"]\n", ""),
            page("B", "is-a: [\"[[A]]\"]\n", ""),
        ];
        let g = graph_of(&pages, &vocab(), VaultKind::Knowledge);
        let t = g.tree("A", "is-a", 10).unwrap();
        assert_eq!(t.children.len(), 1);
        assert!(t.children[0].children.is_empty());
    }

    #[test]
    fn aliases_resolve_to_ids() {
        assert_eq!(fixture().resolve("kg"), Some("Graph"));
    }
}
