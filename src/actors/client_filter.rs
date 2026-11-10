//! Server-side node filter for each `/wss` client.
//!
//! Two kinds of gate (ADR-2183):
//! - population scope (D4), always applied: `linked_page` stubs while
//!   `include_linked_pages` is off;
//! - the quality / authority predicates (D2), which normally only GREY nodes on
//!   the client. The server removes nodes that fail them only when the client
//!   opted in with `server_remove`, so fixing the predicate never thins every
//!   viewer's graph as a side effect.
//!
//! Predicate semantics match the client (`client/src/features/graph/explore/
//! nodeFilter.ts`) and XR (`xr-client/rust/src/settings_sync.rs`), pinned by the
//! shared fixture `tests/node-filter-parity.json`: only enabled predicates
//! combine (AND: all must pass; OR: one must pass; none enabled: pass), and a
//! node without an authored score is neutral to that predicate.

use crate::actors::client_coordinator_actor::{ClientFilter, FilterMode};
use log::debug;
use visionclaw_domain::models::graph::GraphData;

/// Recomputes which node IDs this client receives.
///
/// Called when the client authenticates, updates its filter, or the graph
/// changes. Scope removal always applies; predicate removal, and the
/// `max_nodes` cap (ADR-2192), only when `filter.enabled && filter.server_remove`.
pub fn recompute_filtered_nodes(filter: &mut ClientFilter, graph_data: &GraphData) {
    recompute_filtered_nodes_admitting(filter, graph_data, &|_| true);
}

/// [`recompute_filtered_nodes`] with an extra population predicate, `admit`, applied with the
/// scope gate and before predicate removal and the `max_nodes` cap. The `/wss` stream admits
/// every node; a REST load admits its `graph_type` population, so the cap picks the best N of
/// what the caller asked for rather than of the whole graph (ADR-2192). One function, one
/// order: visibility (by the caller), scope, admit, predicates, cap.
pub fn recompute_filtered_nodes_admitting(
    filter: &mut ClientFilter,
    graph_data: &GraphData,
    admit: &dyn Fn(&visionclaw_domain::models::node::Node) -> bool,
) {
    filter.filtered_node_ids.clear();
    filter.considered_nodes = 0;
    let remove_by_predicate = filter.enabled && filter.server_remove;

    for node in &graph_data.nodes {
        // SINGLE SOURCE OF TRUTH: gate linked_page stubs on the authoritative
        // origin via Node::population_type() (metadata["type"] first, node_type
        // only as legacy fallback), matching the client filter and the GPU.
        if !filter.include_linked_pages && node.population_type() == Some("linked_page") {
            continue;
        }
        if !admit(node) {
            continue;
        }
        // In scope: what this client would receive with no removal (the "of M").
        filter.considered_nodes += 1;
        if remove_by_predicate {
            let quality = node_quality(node, graph_data);
            let meta = graph_data.metadata.get(&node.metadata_id);
            let authority = node
                .metadata
                .get("authority_score")
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| meta.and_then(|m| m.authority_score));
            if !passes_predicates(filter, quality, authority) {
                continue;
            }
        }
        filter.filtered_node_ids.insert(node.id);
    }

    if remove_by_predicate {
        if let Some(cap) = filter.max_nodes.filter(|&c| c > 0) {
            if filter.filtered_node_ids.len() > cap {
                let keep = top_nodes(&filter.filtered_node_ids, graph_data, cap);
                filter.filtered_node_ids = keep;
            }
        }
    }

    debug!(
        "Recomputed filtered nodes: {} of {} visible (enabled={}, server_remove={}, quality_threshold={}, mode={:?}, max_nodes={:?})",
        filter.filtered_node_ids.len(),
        graph_data.nodes.len(),
        filter.enabled,
        filter.server_remove,
        filter.quality_threshold,
        filter.filter_mode,
        filter.max_nodes
    );
}

/// Apply a `filter_update` to a client's filter: the one mapping from the wire message,
/// used by the `/wss` coordinator and by the REST load's `node_filter` snapshot (ADR-2192).
pub fn apply_filter_update(
    filter: &mut ClientFilter,
    msg: &crate::actors::messages::UpdateClientFilter,
) -> Result<(), String> {
    let filter_mode = msg
        .filter_mode
        .parse::<FilterMode>()
        .map_err(|e| format!("Invalid filter mode: {}", e))?;
    filter.enabled = msg.enabled;
    filter.quality_threshold = msg.quality_threshold;
    filter.authority_threshold = msg.authority_threshold;
    filter.filter_by_quality = msg.filter_by_quality;
    filter.filter_by_authority = msg.filter_by_authority;
    filter.filter_mode = filter_mode;
    filter.include_linked_pages = msg.include_linked_pages;
    filter.server_remove = msg.server_remove;
    filter.max_nodes = msg
        .max_nodes
        .and_then(|n| usize::try_from(n).ok())
        .filter(|&n| n > 0);
    Ok(())
}

/// A node's authored quality score, as the quality predicate reads it: node
/// metadata `quality_score`, or `qualityScore` (the ontology-entity sync path
/// stores the same signal camelCase, from vc:qualityScore), then the page
/// metadata row.
pub fn node_quality(
    node: &visionclaw_domain::models::node::Node,
    graph: &GraphData,
) -> Option<f64> {
    node.metadata
        .get("quality_score")
        .or_else(|| node.metadata.get("qualityScore"))
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            graph
                .metadata
                .get(&node.metadata_id)
                .and_then(|m| m.quality_score)
        })
}

/// The `cap` best of `candidates` for the `max_nodes` cap (ADR-2192): higher
/// authored quality first (unscored nodes after every scored one), then higher
/// degree in `graph`, then lower id, so the result is deterministic.
pub fn top_nodes(
    candidates: &std::collections::HashSet<u32>,
    graph: &GraphData,
    cap: usize,
) -> std::collections::HashSet<u32> {
    use std::cmp::Ordering;
    let mut degree: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for e in &graph.edges {
        *degree.entry(e.source).or_default() += 1;
        *degree.entry(e.target).or_default() += 1;
    }
    let mut ranked: Vec<(Option<f64>, usize, u32)> = graph
        .nodes
        .iter()
        .filter(|n| candidates.contains(&n.id))
        .map(|n| {
            let q = node_quality(n, graph).filter(|q| q.is_finite());
            (q, degree.get(&n.id).copied().unwrap_or(0), n.id)
        })
        .collect();
    ranked.sort_by(|a, b| {
        let by_quality = match (a.0, b.0) {
            (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        by_quality.then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2))
    });
    ranked.into_iter().take(cap).map(|(_, _, id)| id).collect()
}

/// Whether a node passes the enabled quality / authority predicates.
///
/// Ignores `enabled` and `server_remove`; the caller decides whether the
/// predicates apply. A `None` score is neutral to its predicate.
pub fn passes_predicates(
    filter: &ClientFilter,
    quality: Option<f64>,
    authority: Option<f64>,
) -> bool {
    let mut verdicts = Vec::with_capacity(2);
    if filter.filter_by_quality {
        verdicts.push(quality.is_none_or(|q| q >= filter.quality_threshold));
    }
    if filter.filter_by_authority {
        verdicts.push(authority.is_none_or(|a| a >= filter.authority_threshold));
    }
    if verdicts.is_empty() {
        return true;
    }
    match filter.filter_mode {
        FilterMode::And => verdicts.iter().all(|&p| p),
        FilterMode::Or => verdicts.iter().any(|&p| p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use visionclaw_domain::models::metadata::Metadata;
    use visionclaw_domain::models::node::Node;

    fn create_test_graph() -> GraphData {
        let mut graph = GraphData::new();

        // Node 1: High quality, high authority
        let node1 = Node::new_with_id("node1.md".to_string(), Some(1));
        let meta1 = Metadata {
            quality_score: Some(0.9),
            authority_score: Some(0.85),
            ..Default::default()
        };

        // Node 2: Low quality, high authority
        let node2 = Node::new_with_id("node2.md".to_string(), Some(2));
        let meta2 = Metadata {
            quality_score: Some(0.4),
            authority_score: Some(0.9),
            ..Default::default()
        };

        // Node 3: High quality, low authority
        let node3 = Node::new_with_id("node3.md".to_string(), Some(3));
        let meta3 = Metadata {
            quality_score: Some(0.85),
            authority_score: Some(0.3),
            ..Default::default()
        };

        // Node 4: No metadata (should use defaults)
        let node4 = Node::new_with_id("node4.md".to_string(), Some(4));

        graph.nodes.push(node1);
        graph.nodes.push(node2);
        graph.nodes.push(node3);
        graph.nodes.push(node4);

        graph.metadata.insert("node1.md".to_string(), meta1);
        graph.metadata.insert("node2.md".to_string(), meta2);
        graph.metadata.insert("node3.md".to_string(), meta3);

        graph
    }

    /// ADR-2192: `max_nodes` keeps the best `cap` of the nodes that pass, ranked by quality
    /// (unscored last), then degree; it applies only with `enabled && server_remove`, and the
    /// recompute records the size of the view it was given for "showing N of M".
    #[test]
    fn max_nodes_caps_only_an_opted_in_removal_and_keeps_the_best_ranked() {
        // Nodes 1..4: qualities 0.9, 0.4, 0.85, unscored (create_test_graph).
        let mut graph = create_test_graph();
        // Node 4 (unscored) has the most edges; degree must not lift it above scored nodes.
        for (a, b) in [(4, 1), (4, 2), (4, 3), (2, 3)] {
            graph
                .edges
                .push(visionclaw_domain::models::edge::Edge::new(a, b, 1.0));
        }
        let mut filter = ClientFilter {
            enabled: true,
            server_remove: true,
            filter_by_quality: false,
            max_nodes: Some(2),
            ..Default::default()
        };
        recompute_filtered_nodes(&mut filter, &graph);
        assert_eq!(filter.considered_nodes, 4);
        let mut kept: Vec<u32> = filter.filtered_node_ids.iter().copied().collect();
        kept.sort_unstable();
        assert_eq!(kept, vec![1, 3], "the two highest quality scores");

        filter.max_nodes = Some(3);
        recompute_filtered_nodes(&mut filter, &graph);
        let mut kept: Vec<u32> = filter.filtered_node_ids.iter().copied().collect();
        kept.sort_unstable();
        assert_eq!(
            kept,
            vec![1, 2, 3],
            "every scored node before the unscored one"
        );

        // Not opted in to removal: the cap is ignored, nothing is removed.
        filter.server_remove = false;
        filter.max_nodes = Some(1);
        recompute_filtered_nodes(&mut filter, &graph);
        assert_eq!(filter.filtered_node_ids.len(), 4);
        filter.server_remove = true;
        filter.enabled = false;
        recompute_filtered_nodes(&mut filter, &graph);
        assert_eq!(filter.filtered_node_ids.len(), 4);
    }

    #[test]
    fn top_nodes_breaks_quality_ties_by_degree_then_id() {
        let mut graph = GraphData::new();
        for id in 1..=4u32 {
            let mut n = Node::new_with_id(format!("n{id}"), Some(id));
            n.metadata.insert("quality_score".into(), "0.5".into());
            graph.nodes.push(n);
        }
        graph
            .edges
            .push(visionclaw_domain::models::edge::Edge::new(3, 4, 1.0));
        let all: std::collections::HashSet<u32> = [1, 2, 3, 4].into_iter().collect();
        let mut top: Vec<u32> = top_nodes(&all, &graph, 3).into_iter().collect();
        top.sort_unstable();
        assert_eq!(top, vec![1, 3, 4], "degree first, then the lower id");
    }

    #[test]
    fn test_filter_disabled_shows_all() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: false,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        assert_eq!(filter.filtered_node_ids.len(), 4);
    }

    #[test]
    fn test_filter_by_quality_only() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            server_remove: true,
            filter_by_quality: true,
            filter_by_authority: false,
            quality_threshold: 0.7,
            ..Default::default()
        };
        filter.filter_mode = FilterMode::And;

        recompute_filtered_nodes(&mut filter, &graph);

        // Should include nodes 1 and 3 (high quality >= 0.7)
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(!filter.filtered_node_ids.contains(&2));
        assert!(filter.filtered_node_ids.contains(&3));
    }

    #[test]
    fn test_filter_by_authority_only() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            server_remove: true,
            filter_by_quality: false,
            filter_by_authority: true,
            authority_threshold: 0.7,
            ..Default::default()
        };
        filter.filter_mode = FilterMode::And;

        recompute_filtered_nodes(&mut filter, &graph);

        // Should include nodes 1 and 2 (high authority >= 0.7)
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(filter.filtered_node_ids.contains(&2));
        assert!(!filter.filtered_node_ids.contains(&3));
    }

    #[test]
    fn test_filter_and_mode() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            filter_by_quality: true,
            filter_by_authority: true,
            quality_threshold: 0.7,
            authority_threshold: 0.7,
            filter_mode: FilterMode::And,
            server_remove: true,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        // Only node 1 passes both thresholds
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(!filter.filtered_node_ids.contains(&2));
        assert!(!filter.filtered_node_ids.contains(&3));
    }

    #[test]
    fn test_filter_or_mode() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            filter_by_quality: true,
            filter_by_authority: true,
            quality_threshold: 0.7,
            authority_threshold: 0.7,
            filter_mode: FilterMode::Or,
            server_remove: true,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        // Nodes 1, 2, and 3 pass at least one threshold
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(filter.filtered_node_ids.contains(&2));
        assert!(filter.filtered_node_ids.contains(&3));
    }

    #[test]
    fn test_unscored_node_is_neutral_to_quality() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            server_remove: true,
            filter_by_quality: true,
            filter_by_authority: false,
            quality_threshold: 0.6,
            filter_mode: FilterMode::And,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        // Node 4 has no quality score: neutral, so it is kept (was 0.5 → removed).
        assert!(filter.filtered_node_ids.contains(&4));
        assert!(!filter.filtered_node_ids.contains(&2));
    }

    fn create_test_graph_with_linked_pages() -> GraphData {
        let mut graph = create_test_graph();
        // Add a linked_page stub node
        let mut stub = Node::new_with_id("stub.md".to_string(), Some(10));
        stub.node_type = Some("linked_page".to_string());
        graph.nodes.push(stub);
        graph
    }

    #[test]
    fn test_include_linked_pages_true_passes_stubs() {
        let graph = create_test_graph_with_linked_pages();
        let mut filter = ClientFilter {
            enabled: false, // disabled = all pass
            include_linked_pages: true,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        assert!(
            filter.filtered_node_ids.contains(&10),
            "stub should be included when include_linked_pages=true"
        );
    }

    #[test]
    fn test_include_linked_pages_false_excludes_stubs() {
        let graph = create_test_graph_with_linked_pages();
        let mut filter = ClientFilter {
            enabled: false, // disabled = all pass (except linked_page gate)
            include_linked_pages: false,
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        assert!(
            !filter.filtered_node_ids.contains(&10),
            "stub should be excluded when include_linked_pages=false"
        );
        // Regular page nodes still pass
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(filter.filtered_node_ids.contains(&2));
    }

    // ---- Shared parity fixture (tests/node-filter-parity.json) ----

    const PARITY: &str = include_str!("../../tests/node-filter-parity.json");

    fn parity() -> serde_json::Value {
        serde_json::from_str(PARITY).expect("parity fixture parses")
    }

    fn parity_graph(v: &serde_json::Value) -> GraphData {
        let mut graph = GraphData::new();
        for n in v["nodes"].as_array().unwrap() {
            let id = n["id"].as_u64().unwrap() as u32;
            let mut node = Node::new_with_id(format!("n{id}.md"), Some(id));
            for (k, val) in n["metadata"].as_object().unwrap() {
                node.metadata
                    .insert(k.clone(), val.as_str().unwrap().to_string());
            }
            graph.nodes.push(node);
        }
        graph
    }

    fn filter_from(f: &serde_json::Value, server_remove: bool) -> ClientFilter {
        ClientFilter {
            enabled: f["enabled"].as_bool().unwrap(),
            quality_threshold: f["qualityThreshold"].as_f64().unwrap(),
            authority_threshold: f["authorityThreshold"].as_f64().unwrap(),
            filter_by_quality: f["filterByQuality"].as_bool().unwrap(),
            filter_by_authority: f["filterByAuthority"].as_bool().unwrap(),
            filter_mode: f["filterMode"].as_str().unwrap().parse().unwrap(),
            server_remove,
            include_linked_pages: true,
            ..Default::default()
        }
    }

    fn removed(filter: &ClientFilter, graph: &GraphData) -> Vec<u32> {
        let mut out: Vec<u32> = graph
            .nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| !filter.filtered_node_ids.contains(id))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn parity_fixture_with_server_remove_matches_client_grey_set() {
        let v = parity();
        let graph = parity_graph(&v);
        for c in v["cases"].as_array().unwrap() {
            let mut filter = filter_from(&c["filter"], true);
            recompute_filtered_nodes(&mut filter, &graph);
            let want: Vec<u32> = c["greyed"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect();
            assert_eq!(removed(&filter, &graph), want, "case: {}", c["name"]);
        }
    }

    #[test]
    fn server_removal_is_off_unless_opted_in() {
        let v = parity();
        let graph = parity_graph(&v);
        for c in v["cases"].as_array().unwrap() {
            let mut filter = filter_from(&c["filter"], false);
            recompute_filtered_nodes(&mut filter, &graph);
            assert!(
                removed(&filter, &graph).is_empty(),
                "case {} removed nodes without server_remove",
                c["name"]
            );
        }
    }

    #[test]
    fn fresh_client_filter_removes_nothing_but_scope() {
        let filter0 = ClientFilter::default();
        assert!(!filter0.server_remove, "server removal must be opt-in");
        let mut filter = filter0;
        // Even switched on with a strict quality test, the default removes nothing.
        filter.enabled = true;
        filter.quality_threshold = 0.99;
        let graph = create_test_graph_with_linked_pages();
        recompute_filtered_nodes(&mut filter, &graph);
        for id in [1, 2, 3, 4] {
            assert!(filter.filtered_node_ids.contains(&id));
        }
        // The linked_page population scope (ADR-2183 D4) still removes.
        assert!(!filter.filtered_node_ids.contains(&10));
        // ADR-2192: "of M" counts what is in scope, so with nothing removed N == M ("all").
        assert_eq!(filter.considered_nodes, filter.filtered_node_ids.len());
    }

    #[test]
    fn shipped_defaults_match_the_fixture_default_case() {
        let v = parity();
        let d = v["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["default"].as_bool() == Some(true))
            .unwrap();
        let s = crate::settings::models::NodeFilterSettings::default();
        let json = serde_json::to_value(&s).unwrap();
        for (k, want) in d["filter"].as_object().unwrap() {
            assert_eq!(&json[k], want, "NodeFilterSettings::default().{k}");
        }
        assert_eq!(json["serverRemove"], serde_json::json!(false));
        // Every field of the coordinator's per-client default, against the same case.
        let c = ClientFilter::default();
        let f = &d["filter"];
        assert_eq!(serde_json::json!(c.enabled), f["enabled"]);
        assert_eq!(
            serde_json::json!(c.quality_threshold),
            f["qualityThreshold"]
        );
        assert_eq!(
            serde_json::json!(c.authority_threshold),
            f["authorityThreshold"]
        );
        assert_eq!(serde_json::json!(c.filter_by_quality), f["filterByQuality"]);
        assert_eq!(
            serde_json::json!(c.filter_by_authority),
            f["filterByAuthority"]
        );
        assert_eq!(
            serde_json::json!(if matches!(c.filter_mode, FilterMode::And) {
                "and"
            } else {
                "or"
            }),
            f["filterMode"]
        );
        assert_eq!(
            serde_json::json!(c.include_linked_pages),
            f["includeLinkedPages"]
        );
        assert_eq!(serde_json::json!(c.server_remove), f["serverRemove"]);
        assert_eq!(c.max_nodes, None);
        assert_eq!(s.max_nodes, 0, "no cap by default (ADR-2192)");
        assert!(c.filtered_node_ids.is_empty());
    }

    #[test]
    fn persisted_document_without_server_remove_deserialises_to_off() {
        let s: crate::settings::models::NodeFilterSettings = serde_json::from_str(
            r#"{"enabled":false,"qualityThreshold":0.74,"authorityThreshold":0.5,"filterByQuality":false,"filterByAuthority":false,"filterMode":"and","includeLinkedPages":true}"#,
        )
        .unwrap();
        assert!(!s.server_remove);
        assert_eq!(s.quality_threshold, 0.74);
    }
}
