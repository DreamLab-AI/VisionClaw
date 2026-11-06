//! Owner-pubkey visibility drop-set (ADR-060, ADR-059 §Phase-4).
//!
//! Pure, framework-free implementation of the binary-position-encoder
//! visibility filter. The handler layer (`socket_flow_handler::position_updates`)
//! owns the env-flag gate (`PUBKEY_VISIBILITY_FILTER`, default ON — secure by
//! default; opt out with 0/false/off/no) and the wire
//! encoding; this module owns the load-bearing set logic so it is unit-testable
//! without linking the CUDA-bearing monolith.
//!
//! ADR-059 §6 filter clause, evaluated per caller:
//! ```text
//! KEEP  WHERE visibility = 'public' OR owner_pubkey = $session_pubkey
//! DROP  everything else  (private-of-others, or private-when-anonymous)
//! ```
//! Filtering is **fail-closed**: a missing session pubkey drops every private
//! node (public-only graph), matching ADR-050 §Visibility transitions.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use crate::models::graph::GraphData;
use crate::models::node::Node;

/// Node metadata key carrying the ADR-050 public flag (`"true"`, case-insensitive, means public).
pub const PUBLIC_METADATA_KEY: &str = "public";
/// Node metadata key carrying the ADR-050 owner pubkey of a private node.
pub const OWNER_PUBKEY_METADATA_KEY: &str = "owner_pubkey";

/// Env flag gating the drop-set filter. Default ON (secure by default); opt out with
/// `0` / `false` / `off` / `no`.
pub const PUBKEY_VISIBILITY_FILTER_ENV: &str = "PUBKEY_VISIBILITY_FILTER";

/// Parse the flag. Absent or unrecognised means ON: the filter fails closed.
pub fn parse_visibility_flag(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        None => true,
    }
}

/// Whether the filter is enabled, read once from the environment. Every position path (connect
/// sync, snapshot request AND the live broadcast) must consult this one function.
pub fn filter_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        parse_visibility_flag(std::env::var(PUBKEY_VISIBILITY_FILTER_ENV).ok().as_deref())
    })
}

/// Per-node visibility metadata (ADR-050 primitives) projected onto the wire id.
///
/// `wire_id` is the flagged/opaque id exactly as it appears in the `(id, data)`
/// pairs the encoder consumes, so a computed drop set can be matched directly
/// against those pairs with no further translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVisibility {
    /// Flagged/opaque wire id as it appears in the `(id, data)` pair.
    pub wire_id: u32,
    /// `true` when the node is public (ADR-050 `visibility = 'public'`).
    pub is_public: bool,
    /// ADR-050 `owner_pubkey`, when known. `None` ⇒ owner unknown ⇒ treated as
    /// not-owned-by-caller (fail-closed).
    pub owner_pubkey: Option<String>,
}

impl NodeVisibility {
    /// Convenience constructor for a public node (no owner needed).
    pub fn public(wire_id: u32) -> Self {
        Self {
            wire_id,
            is_public: true,
            owner_pubkey: None,
        }
    }

    /// Convenience constructor for a private node with a known owner.
    pub fn private_owned_by(wire_id: u32, owner_pubkey: impl Into<String>) -> Self {
        Self {
            wire_id,
            is_public: false,
            owner_pubkey: Some(owner_pubkey.into()),
        }
    }

    /// Convenience constructor for a private node whose owner is unknown.
    pub fn private_unowned(wire_id: u32) -> Self {
        Self {
            wire_id,
            is_public: false,
            owner_pubkey: None,
        }
    }

    /// Project a graph node's visibility from its metadata map: public iff `public` is `"true"`
    /// (case-insensitive), owner from `owner_pubkey`. This is the only reading of those keys;
    /// every wire path (REST graph reads, `/wss` `initialGraphLoad`, the position drop set) goes
    /// through it.
    pub fn of_node(node: &Node) -> Self {
        let is_public = node
            .metadata
            .get(PUBLIC_METADATA_KEY)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        Self {
            wire_id: node.id,
            is_public,
            owner_pubkey: if is_public {
                None
            } else {
                node.metadata.get(OWNER_PUBKEY_METADATA_KEY).cloned()
            },
        }
    }

    /// Whether this node may reach a caller holding `session_pubkey` (`None` means anonymous).
    #[inline]
    pub fn is_visible_to(&self, session_pubkey: Option<&str>) -> bool {
        !self.is_dropped_for(session_pubkey)
    }

    /// Whether this node must be dropped for the given caller.
    ///
    /// Keep iff public, or owned by the caller. Everything else drops.
    #[inline]
    fn is_dropped_for(&self, session_pubkey: Option<&str>) -> bool {
        if self.is_public {
            return false;
        }
        match (session_pubkey, self.owner_pubkey.as_deref()) {
            // Caller is authenticated and owns this private node ⇒ keep.
            (Some(caller), Some(owner)) => caller != owner,
            // Anonymous caller, or unknown owner ⇒ drop (fail-closed).
            _ => true,
        }
    }
}

/// Compute the caller's private-opaque-id drop set per ADR-059 §6.
///
/// Returns the set of `wire_id`s that must **not** reach the wire for a caller
/// holding `session_pubkey` (`None` ⇒ anonymous ⇒ every private node drops).
pub fn compute_private_opaque_ids(
    nodes: &[NodeVisibility],
    session_pubkey: Option<&str>,
) -> HashSet<u32> {
    nodes
        .iter()
        .filter(|n| n.is_dropped_for(session_pubkey))
        .map(|n| n.wire_id)
        .collect()
}

/// Whether `node` may reach a caller holding `session_pubkey` when the filter is on.
///
/// ADR-059 §6: public, or private and owned by the caller. Fails closed for an anonymous caller
/// and for a private node with no known owner.
#[inline]
pub fn node_is_visible(node: &Node, session_pubkey: Option<&str>) -> bool {
    NodeVisibility::of_node(node).is_visible_to(session_pubkey)
}

/// The part of `graph` a caller may see: the visible nodes, the edges with **both** endpoints
/// visible, and the metadata entries of the visible nodes only.
///
/// `filter_on == false` returns `graph` unchanged (the explicit `PUBKEY_VISIBILITY_FILTER=0`
/// opt-out). With the filter on, a metadata entry is kept only when a visible node names it, so a
/// metadata row with no node fails closed. When nothing would be removed the input `Arc` is
/// returned without copying.
pub fn visible_subgraph(
    graph: &Arc<GraphData>,
    session_pubkey: Option<&str>,
    filter_on: bool,
) -> Arc<GraphData> {
    if !filter_on {
        return Arc::clone(graph);
    }
    let kept: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| node_is_visible(n, session_pubkey))
        .collect();
    let kept_metadata_ids: HashSet<&str> = kept.iter().map(|n| n.metadata_id.as_str()).collect();
    let nothing_removed = kept.len() == graph.nodes.len()
        && graph
            .metadata
            .keys()
            .all(|k| kept_metadata_ids.contains(k.as_str()))
        && graph
            .id_to_metadata
            .values()
            .all(|m| kept_metadata_ids.contains(m.as_str()));
    if nothing_removed {
        return Arc::clone(graph);
    }

    let kept_ids: HashSet<u32> = kept.iter().map(|n| n.id).collect();
    let names = PageNames::new(graph, &kept, &kept_metadata_ids);
    Arc::new(GraphData {
        nodes: kept
            .into_iter()
            .map(|n| {
                let mut n = n.clone();
                names.scrub_node(&mut n);
                n
            })
            .collect(),
        edges: graph
            .edges
            .iter()
            .filter(|e| kept_ids.contains(&e.source) && kept_ids.contains(&e.target))
            .cloned()
            .collect(),
        metadata: graph
            .metadata
            .iter()
            .filter(|(k, _)| kept_metadata_ids.contains(k.as_str()))
            .map(|(k, v)| {
                let mut v = v.clone();
                names.scrub_row(&mut v);
                (k.clone(), v)
            })
            .collect(),
        id_to_metadata: graph
            .id_to_metadata
            .iter()
            .filter(|(_, m)| kept_metadata_ids.contains(m.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}

/// The page names a caller may and may not see, for scrubbing references inside visible
/// rows and nodes (ADR-060 M1). Compared case-insensitively, `.md` trimmed.
struct PageNames {
    visible: HashSet<String>,
    hidden: HashSet<String>,
}

impl PageNames {
    fn norm(s: &str) -> String {
        s.trim().trim_end_matches(".md").to_lowercase()
    }

    fn new(graph: &GraphData, kept: &[&Node], kept_metadata_ids: &HashSet<&str>) -> Self {
        let mut visible = HashSet::new();
        for n in kept {
            visible.insert(Self::norm(&n.metadata_id));
            visible.insert(Self::norm(&n.label));
        }
        for k in graph.metadata.keys() {
            if kept_metadata_ids.contains(k.as_str()) {
                visible.insert(Self::norm(k));
            }
        }
        let kept_ids: HashSet<u32> = kept.iter().map(|n| n.id).collect();
        let mut hidden = HashSet::new();
        for n in graph.nodes.iter().filter(|n| !kept_ids.contains(&n.id)) {
            hidden.insert(Self::norm(&n.metadata_id));
            hidden.insert(Self::norm(&n.label));
            if let Some(iri) = &n.owl_class_iri {
                hidden.insert(Self::norm(iri));
            }
            for key in ["file_name", "page_iri", "class_iri"] {
                if let Some(v) = n.metadata.get(key) {
                    hidden.insert(Self::norm(v));
                }
            }
        }
        for (k, row) in &graph.metadata {
            if !kept_metadata_ids.contains(k.as_str()) {
                hidden.insert(Self::norm(k));
                hidden.insert(Self::norm(&row.file_name));
            }
        }
        hidden.remove("");
        Self { visible, hidden }
    }

    /// A removed page's name, unless a visible page shares it.
    fn is_hidden(&self, name: &str) -> bool {
        let n = Self::norm(name);
        self.hidden.contains(&n) && !self.visible.contains(&n)
    }

    fn scrub_row(&self, row: &mut crate::models::metadata::Metadata) {
        // Topic keys are page names from the metadata store: keep only visible pages.
        row.topic_counts
            .retain(|k, _| self.visible.contains(&Self::norm(k)));
        row.is_subclass_of.retain(|v| !self.is_hidden(v));
        row.belongs_to_domain.retain(|v| !self.is_hidden(v));
        if let Some(def) = row.definition.as_mut() {
            *def = self.scrub_links(def);
        }
    }

    /// Free text with every `[[Page]]` / `[[Page|alias]]` wikilink to a removed page cut out,
    /// alias included (the alias is authored text about the hidden page). Plain words are left
    /// alone, for the reason `domain` is (N3). A link to a page that does not exist stays, as
    /// written, exactly as a link to a page that is hidden would if it did not exist.
    fn scrub_links(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("[[") {
            let Some(len) = rest[start + 2..].find("]]") else {
                break;
            };
            let inner = &rest[start + 2..start + 2 + len];
            // `[[Page#Heading|alias]]` / `[[Page^block]]` name the page before `#`, `^` or `|`.
            let target = inner.split(['|', '#', '^']).next().unwrap_or(inner);
            let hidden = self.is_hidden(target);
            // An embed `![[Page]]` is cut with its `!`.
            let lead = if hidden && rest[..start].ends_with('!') {
                start - 1
            } else {
                start
            };
            out.push_str(&rest[..lead]);
            if !hidden {
                out.push_str(&rest[start..start + 2 + len + 2]);
            }
            rest = &rest[start + 2 + len + 2..];
        }
        out.push_str(rest);
        out
    }

    /// Scrub only node keys whose values are page references (N3): `is_subclass_of`
    /// (`[[Parent]]` links, joined ","), `tags` (`#tag` / `tags::`, joined ", ") and the
    /// wikilinks inside the authored `definition` (the leading paragraph). Plain
    /// frontmatter text such as `domain` / `source_domain` is left alone, because removing a
    /// value that merely shares a hidden page's name would itself reveal that page.
    fn scrub_node(&self, node: &mut Node) {
        if let Some(def) = node.metadata.get("definition") {
            let scrubbed = self.scrub_links(def);
            node.metadata.insert("definition".to_string(), scrubbed);
        }
        for (key, sep) in [("is_subclass_of", ","), ("tags", ", ")] {
            let Some(list) = node.metadata.get(key) else {
                continue;
            };
            let kept: Vec<&str> = list
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty() && !self.is_hidden(v))
                .collect();
            if kept.is_empty() {
                node.metadata.remove(key);
            } else {
                let joined = kept.join(sep);
                node.metadata.insert(key.to_string(), joined);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drop set applied to `(id, payload)` pairs, as every wire path applies it (the
    /// production paths filter inline; this helper only lets the tests state the result).
    fn apply_drop_set<T>(nodes: &mut Vec<(u32, T)>, drop_set: &HashSet<u32>) -> usize {
        let before = nodes.len();
        nodes.retain(|(id, _)| !drop_set.contains(id));
        before - nodes.len()
    }

    // A stand-in for the encoder payload; the filter is generic over the data half.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Payload(u32);

    fn pairs() -> Vec<(u32, Payload)> {
        vec![
            (10, Payload(10)), // public
            (11, Payload(11)), // private, owned by "alice"
            (12, Payload(12)), // private, owned by "bob"
            (13, Payload(13)), // private, unknown owner
        ]
    }

    fn meta() -> Vec<NodeVisibility> {
        vec![
            NodeVisibility::public(10),
            NodeVisibility::private_owned_by(11, "alice"),
            NodeVisibility::private_owned_by(12, "bob"),
            NodeVisibility::private_unowned(13),
        ]
    }

    #[test]
    fn empty_drop_set_is_passthrough_untouched() {
        // Simulates the flag-OFF path: no drop set is ever computed, so the
        // encoder input must be byte-for-byte identical.
        let mut nodes = pairs();
        let original = nodes.clone();
        let empty: HashSet<u32> = HashSet::new();
        let dropped = apply_drop_set(&mut nodes, &empty);
        assert_eq!(dropped, 0);
        assert_eq!(nodes, original, "empty drop set must not mutate the vec");
    }

    #[test]
    fn flag_on_drops_private_ids_for_non_owner() {
        // Caller "alice" keeps public (10) + her own private (11); bob's (12)
        // and the unknown-owner (13) private nodes drop.
        let drop = compute_private_opaque_ids(&meta(), Some("alice"));
        assert_eq!(drop, HashSet::from([12, 13]));

        let mut nodes = pairs();
        let dropped = apply_drop_set(&mut nodes, &drop);
        assert_eq!(dropped, 2);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            kept,
            vec![10, 11],
            "non-owner private nodes must be dropped"
        );
    }

    #[test]
    fn owner_sees_own_private_nodes() {
        // "bob" keeps public (10) + his own private (12); alice's (11) and the
        // unknown-owner (13) drop.
        let drop = compute_private_opaque_ids(&meta(), Some("bob"));
        assert_eq!(drop, HashSet::from([11, 13]));

        let mut nodes = pairs();
        apply_drop_set(&mut nodes, &drop);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(kept, vec![10, 12], "owner must still see their own nodes");
    }

    #[test]
    fn anonymous_caller_is_fail_closed_public_only() {
        // No session pubkey ⇒ every private node drops regardless of owner.
        let drop = compute_private_opaque_ids(&meta(), None);
        assert_eq!(drop, HashSet::from([11, 12, 13]));

        let mut nodes = pairs();
        apply_drop_set(&mut nodes, &drop);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(kept, vec![10], "anonymous callers get a public-only graph");
    }

    #[test]
    fn all_public_yields_empty_drop_set() {
        let all_public = vec![
            NodeVisibility::public(1),
            NodeVisibility::public(2),
            NodeVisibility::public(3),
        ];
        assert!(compute_private_opaque_ids(&all_public, None).is_empty());
        assert!(compute_private_opaque_ids(&all_public, Some("anyone")).is_empty());
    }

    #[test]
    fn drop_set_ids_absent_from_pairs_are_ignored() {
        // A drop set carrying ids not present in the frame removes nothing extra.
        let mut nodes = pairs();
        let drop = HashSet::from([99, 100]);
        let dropped = apply_drop_set(&mut nodes, &drop);
        assert_eq!(dropped, 0);
        assert_eq!(nodes.len(), 4);
    }

    // ---- ADR-2003: the three caller classes, over one corpus ---------------

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// One corpus exercising every visibility shape at once:
    /// 1 public, 2 private-owned-by-OWNER, 3 private-owned-by-OTHER,
    /// 4 private with an unknown owner.
    fn corpus() -> Vec<NodeVisibility> {
        vec![
            NodeVisibility::public(1),
            NodeVisibility::private_owned_by(2, OWNER),
            NodeVisibility::private_owned_by(3, OTHER),
            NodeVisibility::private_unowned(4),
        ]
    }

    fn kept_for(session: Option<&str>) -> Vec<u32> {
        let drop = compute_private_opaque_ids(&corpus(), session);
        let mut kept: Vec<u32> = corpus()
            .iter()
            .map(|n| n.wire_id)
            .filter(|id| !drop.contains(id))
            .collect();
        kept.sort_unstable();
        kept
    }

    /// An anonymous caller sees the public-only graph. Every private node
    /// drops, whether or not its owner is known — the fail-closed rule.
    #[test]
    fn anonymous_caller_sees_only_public_nodes() {
        assert_eq!(kept_for(None), vec![1]);
    }

    /// An owner sees the public graph plus their own private nodes, and nobody
    /// else's.
    #[test]
    fn owner_sees_public_plus_their_own_private_nodes() {
        assert_eq!(kept_for(Some(OWNER)), vec![1, 2]);
    }

    /// A different authenticated caller sees the public graph plus *their* own
    /// private nodes — here, none.
    #[test]
    fn non_owner_sees_public_plus_only_their_own() {
        assert_eq!(kept_for(Some(OTHER)), vec![1, 3]);
    }

    /// A private node whose owner is unknown is never visible to anyone. It is
    /// not "owned by the caller" for any caller, so it fails closed.
    #[test]
    fn an_unowned_private_node_is_invisible_to_every_caller() {
        for session in [None, Some(OWNER), Some(OTHER)] {
            assert!(
                !kept_for(session).contains(&4),
                "an unowned private node must never reach the wire"
            );
        }
    }

    /// The union of what every caller can see never exceeds the corpus, and no
    /// caller sees another's private node. Stated as an invariant over the
    /// three caller classes rather than three separate assertions.
    #[test]
    fn no_caller_ever_sees_another_callers_private_node() {
        let owner_view = kept_for(Some(OWNER));
        let other_view = kept_for(Some(OTHER));
        assert!(!owner_view.contains(&3), "OWNER must not see OTHER's node");
        assert!(!other_view.contains(&2), "OTHER must not see OWNER's node");
    }

    /// A public-to-private transition: the same node id, re-published as
    /// private, disappears for every caller but its owner. This is the
    /// visibility-change case the ADR names — the filter is evaluated per
    /// snapshot, so a re-published corpus takes effect immediately.
    #[test]
    fn a_public_to_private_transition_takes_effect_on_the_next_snapshot() {
        let before = vec![NodeVisibility::public(7)];
        for session in [None, Some(OWNER), Some(OTHER)] {
            assert!(compute_private_opaque_ids(&before, session).is_empty());
        }

        let after = vec![NodeVisibility::private_owned_by(7, OWNER)];
        assert!(compute_private_opaque_ids(&after, None).contains(&7));
        assert!(compute_private_opaque_ids(&after, Some(OTHER)).contains(&7));
        assert!(
            !compute_private_opaque_ids(&after, Some(OWNER)).contains(&7),
            "the owner keeps seeing their own node"
        );
    }

    /// An owner change hands visibility to the new owner and takes it from the
    /// old one, with no stale retention in the filter itself.
    #[test]
    fn an_owner_change_moves_visibility() {
        let before = vec![NodeVisibility::private_owned_by(8, OWNER)];
        assert!(!compute_private_opaque_ids(&before, Some(OWNER)).contains(&8));
        assert!(compute_private_opaque_ids(&before, Some(OTHER)).contains(&8));

        let after = vec![NodeVisibility::private_owned_by(8, OTHER)];
        assert!(
            compute_private_opaque_ids(&after, Some(OWNER)).contains(&8),
            "the previous owner loses visibility"
        );
        assert!(!compute_private_opaque_ids(&after, Some(OTHER)).contains(&8));
    }

    /// Re-authenticating as a different pubkey changes what the same connection
    /// may see: the filter holds no per-connection state, so the drop set is a
    /// pure function of (corpus, session pubkey).
    #[test]
    fn reauthentication_changes_the_drop_set_with_no_retained_state() {
        let nodes = corpus();
        let anon = compute_private_opaque_ids(&nodes, None);
        let as_owner = compute_private_opaque_ids(&nodes, Some(OWNER));
        let back_to_anon = compute_private_opaque_ids(&nodes, None);

        assert_ne!(anon, as_owner);
        assert_eq!(
            anon, back_to_anon,
            "dropping the session must restore the anonymous view exactly"
        );
    }

    /// Edge visibility: an edge survives only when BOTH endpoints survive. This
    /// is the rule the socket path applies to initial edges, asserted here over
    /// the same drop set so the two cannot diverge.
    #[test]
    fn an_edge_survives_only_when_both_endpoints_do() {
        let drop = compute_private_opaque_ids(&corpus(), Some(OWNER));
        let edges = [(1u32, 2u32), (1, 3), (2, 3), (1, 4), (2, 2)];
        let kept: Vec<(u32, u32)> = edges
            .into_iter()
            .filter(|(s, t)| !drop.contains(s) && !drop.contains(t))
            .collect();
        assert_eq!(
            kept,
            vec![(1, 2), (2, 2)],
            "only edges between nodes the owner can see survive"
        );
    }

    /// An empty drop set leaves the payload untouched and allocates nothing —
    /// the flag-off hot path.
    #[test]
    fn an_empty_drop_set_is_a_no_op() {
        let mut pairs = vec![(1u32, Payload(10)), (2, Payload(20))];
        let before = pairs.clone();
        assert_eq!(apply_drop_set(&mut pairs, &HashSet::new()), 0);
        assert_eq!(pairs, before);
    }

    /// The drop set applies identically to any payload type, which is what lets
    /// the same set filter node positions, agent nodes and labels.
    #[test]
    fn the_drop_set_applies_to_any_payload_type() {
        let drop = compute_private_opaque_ids(&corpus(), None);

        let mut positions = vec![(1u32, [0.0f32; 3]), (2, [1.0; 3]), (3, [2.0; 3])];
        assert_eq!(apply_drop_set(&mut positions, &drop), 2);
        assert_eq!(positions.len(), 1);

        let mut labels = vec![
            (1u32, "public".to_string()),
            (2, "owner private".to_string()),
            (4, "unowned private".to_string()),
        ];
        assert_eq!(apply_drop_set(&mut labels, &drop), 2);
        assert_eq!(labels, vec![(1, "public".to_string())]);
    }
}

#[cfg(test)]
mod subgraph_tests {
    use super::*;
    use crate::models::edge::Edge;
    use crate::models::metadata::Metadata;

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn node(id: u32, metadata_id: &str, meta: &[(&str, &str)]) -> Node {
        let mut n = Node::new_with_id(metadata_id.to_string(), Some(id));
        for (k, v) in meta {
            n.metadata.insert((*k).to_string(), (*v).to_string());
        }
        n
    }

    /// 1 public, 2 private owned by OWNER, 3 private with an unknown owner, plus a metadata row
    /// (`orphan.md`) that no node names.
    fn graph() -> Arc<GraphData> {
        let mut g = GraphData::new();
        g.nodes = vec![
            node(1, "public.md", &[("public", "TRUE")]),
            node(2, "owned.md", &[("owner_pubkey", OWNER)]),
            node(3, "unowned.md", &[]),
        ];
        g.edges = vec![
            Edge::new(1, 2, 1.0),
            Edge::new(1, 3, 1.0),
            Edge::new(2, 2, 1.0),
        ];
        for id in ["public.md", "owned.md", "unowned.md", "orphan.md"] {
            g.metadata.insert(id.to_string(), Metadata::default());
            g.id_to_metadata.insert(format!("k-{id}"), id.to_string());
        }
        Arc::new(g)
    }

    fn ids(g: &GraphData) -> Vec<u32> {
        g.nodes.iter().map(|n| n.id).collect()
    }

    fn edges(g: &GraphData) -> Vec<(u32, u32)> {
        g.edges.iter().map(|e| (e.source, e.target)).collect()
    }

    fn metadata_keys(g: &GraphData) -> Vec<String> {
        let mut k: Vec<String> = g.metadata.keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn anonymous_caller_gets_the_public_subgraph_and_its_metadata_only() {
        let v = visible_subgraph(&graph(), None, true);
        assert_eq!(ids(&v), vec![1]);
        assert!(edges(&v).is_empty(), "edges touching dropped nodes go");
        assert_eq!(metadata_keys(&v), vec!["public.md".to_string()]);
        assert_eq!(v.id_to_metadata.len(), 1);
    }

    #[test]
    fn a_non_owner_sees_what_an_anonymous_caller_sees() {
        let v = visible_subgraph(&graph(), Some(OTHER), true);
        assert_eq!(ids(&v), vec![1]);
        assert_eq!(metadata_keys(&v), vec!["public.md".to_string()]);
    }

    #[test]
    fn the_owner_also_sees_their_private_node_and_its_edges() {
        let v = visible_subgraph(&graph(), Some(OWNER), true);
        assert_eq!(ids(&v), vec![1, 2]);
        assert_eq!(edges(&v), vec![(1, 2), (2, 2)]);
        assert_eq!(
            metadata_keys(&v),
            vec!["owned.md".to_string(), "public.md".to_string()]
        );
    }

    #[test]
    fn an_orphan_metadata_row_fails_closed_even_when_no_node_drops() {
        let mut g = GraphData::new();
        g.nodes = vec![node(1, "public.md", &[("public", "true")])];
        g.metadata
            .insert("public.md".to_string(), Metadata::default());
        g.metadata
            .insert("orphan.md".to_string(), Metadata::default());
        let v = visible_subgraph(&Arc::new(g), None, true);
        assert_eq!(metadata_keys(&v), vec!["public.md".to_string()]);
    }

    #[test]
    fn nothing_to_remove_returns_the_same_allocation() {
        let mut g = GraphData::new();
        g.nodes = vec![node(1, "public.md", &[("public", "true")])];
        g.metadata
            .insert("public.md".to_string(), Metadata::default());
        let g = Arc::new(g);
        assert!(Arc::ptr_eq(&visible_subgraph(&g, None, true), &g));
    }

    #[test]
    fn filter_off_is_the_explicit_opt_out_and_passes_everything() {
        let g = graph();
        assert!(Arc::ptr_eq(&visible_subgraph(&g, None, false), &g));
    }

    #[test]
    fn a_public_node_ignores_its_owner_key() {
        let n = node(9, "p.md", &[("public", "true"), ("owner_pubkey", OWNER)]);
        assert!(node_is_visible(&n, None));
        assert_eq!(NodeVisibility::of_node(&n), NodeVisibility::public(9));
    }

    #[test]
    fn of_node_agrees_with_the_drop_set_rule() {
        let g = graph();
        let projected: Vec<NodeVisibility> = g.nodes.iter().map(NodeVisibility::of_node).collect();
        for caller in [None, Some(OWNER), Some(OTHER)] {
            let drop = compute_private_opaque_ids(&projected, caller);
            for n in &g.nodes {
                assert_eq!(node_is_visible(n, caller), !drop.contains(&n.id));
            }
        }
    }

    /// ADR-060 M1: a visible page that links to, subclasses or sits in a hidden page must not
    /// carry the hidden page's name. `topic_counts` keys are page names, so they are kept only
    /// when they name a visible page; the page-reference fields (`is_subclass_of`,
    /// `belongs_to_domain`, `tags`) drop only names of removed pages; plain text such as `domain`
    /// is never touched (N2, N3).
    #[test]
    fn hidden_page_names_are_scrubbed_from_visible_rows_and_nodes() {
        let mut g = (*graph()).clone();
        g.nodes[0].label = "Public".into();
        g.nodes[1].label = "Owned".into();
        g.nodes[2].label = "Unowned Secret".into();
        g.nodes[0]
            .metadata
            .insert("is_subclass_of".into(), "Thing,Unowned Secret".into());
        g.nodes[0]
            .metadata
            .insert("domain".into(), "unowned".into());
        g.nodes[0]
            .metadata
            .insert("source_domain".into(), "ai".into());
        // `#tag` / `tags::` values are page references (N2), joined ", " by the parser.
        g.nodes[0]
            .metadata
            .insert("tags".into(), "unowned, Unowned Secret, public".into());
        let row = g.metadata.get_mut("public.md").unwrap();
        row.topic_counts = [("unowned".to_string(), 2), ("public".to_string(), 1)]
            .into_iter()
            .collect();
        row.is_subclass_of = vec!["Thing".into(), "unowned".into()];
        row.belongs_to_domain = vec!["Unowned Secret".into(), "ai".into()];
        let g = Arc::new(g);

        let out = visible_subgraph(&g, None, true);
        let row = &out.metadata["public.md"];
        let topics: Vec<&String> = row.topic_counts.keys().collect();
        assert_eq!(
            topics,
            vec!["public"],
            "a hidden page name must not survive as a topic"
        );
        assert_eq!(row.is_subclass_of, vec!["Thing".to_string()]);
        assert_eq!(row.belongs_to_domain, vec!["ai".to_string()]);
        let n = &out.nodes[0].metadata;
        assert_eq!(n.get("is_subclass_of").map(String::as_str), Some("Thing"));
        // N3: `domain` is plain frontmatter text, not a page reference. Dropping it because a
        // hidden page shares the name would itself reveal that page.
        assert_eq!(
            n.get("domain").map(String::as_str),
            Some("unowned"),
            "a plain domain value must not vanish because a hidden page shares its name"
        );
        assert_eq!(
            n.get("tags").map(String::as_str),
            Some("public"),
            "a tag naming a hidden page is a page reference and is scrubbed"
        );
        assert_eq!(n.get("source_domain").map(String::as_str), Some("ai"));

        // The owner sees their own page named; the unowned one is still scrubbed.
        let owner = visible_subgraph(&g, Some(OWNER), true);
        assert!(!owner.metadata["public.md"]
            .topic_counts
            .contains_key("unowned"));
    }
}

#[cfg(test)]
mod definition_scrub_tests {
    use super::*;
    use crate::models::metadata::Metadata;
    use crate::models::node::Node;

    /// A visible page whose authored definition links a hidden page, and (when `with_hidden`)
    /// the hidden page itself. Without it, the same definition links a page that does not
    /// exist, which is left alone.
    fn graph(with_hidden: bool) -> Arc<GraphData> {
        let mut g = GraphData::default();
        let mut public = Node::new_with_id("Public.md".to_string(), None);
        public.id = 1;
        public.label = "Public".into();
        public.metadata.insert("public".into(), "true".into());
        public.metadata.insert(
            "definition".into(),
            "Built on [[Secret Plan]], [[Secret Plan|the plan]], [[Secret Plan#Goals]] and \
             ![[Secret Plan]], see [[Public]]."
                .into(),
        );
        g.nodes.push(public);
        g.metadata.insert(
            "Public.md".into(),
            Metadata {
                file_name: "Public.md".into(),
                definition: Some("Built on [[Secret Plan]], see [[Public]].".into()),
                ..Metadata::default()
            },
        );
        if with_hidden {
            let mut hidden = Node::new_with_id("Secret Plan.md".to_string(), None);
            hidden.id = 2;
            hidden.label = "Secret Plan".into();
            g.nodes.push(hidden);
            g.metadata.insert(
                "Secret Plan.md".into(),
                Metadata {
                    file_name: "Secret Plan.md".into(),
                    ..Metadata::default()
                },
            );
        }
        Arc::new(g)
    }

    /// Review VC-19 Open: `scrub_row` left free-text `definition` alone, so an authored
    /// definition that links a hidden page named it to every caller.
    #[test]
    fn a_definition_never_names_a_hidden_page() {
        let out = visible_subgraph(&graph(true), None, true);
        let row = out.metadata["Public.md"].definition.clone().unwrap();
        let node = out.nodes[0].metadata["definition"].clone();
        assert!(!row.contains("Secret"), "row definition: {row}");
        assert!(!node.contains("Secret"), "node definition: {node}");
        assert!(
            !node.contains("Goals") && !node.contains('!'),
            "node definition: {node}"
        );
        assert!(row.contains("[[Public]]"), "a visible link survives: {row}");
    }

    #[test]
    fn a_link_to_a_page_that_does_not_exist_is_left_as_written() {
        // Nothing hidden, so the input is returned unchanged.
        let g = graph(false);
        let out = visible_subgraph(&g, None, true);
        assert!(out.metadata["Public.md"]
            .definition
            .as_deref()
            .unwrap()
            .contains("[[Secret Plan]]"));
    }
}

#[cfg(test)]
mod flag_tests {
    use super::parse_visibility_flag;

    /// The secure-by-default truth table. Pure: no process env is touched.
    #[test]
    fn parse_visibility_flag_defaults_on_and_honours_opt_out() {
        assert!(
            parse_visibility_flag(None),
            "absent value must default the filter ON"
        );
        for on in ["1", "true", "on", "yes", "TRUE", "  1  ", "banana", ""] {
            assert!(
                parse_visibility_flag(Some(on)),
                "{on:?} must leave the filter ON"
            );
        }
        for off in ["0", "false", "off", "no", "FALSE", "  off  ", "No"] {
            assert!(
                !parse_visibility_flag(Some(off)),
                "{off:?} must turn the filter OFF"
            );
        }
    }
}
