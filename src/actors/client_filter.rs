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
//! node without an authored quality score is neutral to the quality predicate.

use crate::actors::client_coordinator_actor::{ClientFilter, FilterMode};
use log::debug;
use visionclaw_domain::models::graph::GraphData;

/// Authority for a node with no authority score (no copy has a better signal
/// on the server; the client uses hierarchy depth, XR 1.0).
const UNSCORED_AUTHORITY: f64 = 0.5;

/// Recomputes which node IDs this client receives.
///
/// Called when the client authenticates, updates its filter, or the graph
/// changes. Scope removal always applies; predicate removal only when
/// `filter.enabled && filter.server_remove`.
pub fn recompute_filtered_nodes(filter: &mut ClientFilter, graph_data: &GraphData) {
    filter.filtered_node_ids.clear();
    let remove_by_predicate = filter.enabled && filter.server_remove;

    for node in &graph_data.nodes {
        // SINGLE SOURCE OF TRUTH: gate linked_page stubs on the authoritative
        // origin via Node::population_type() (metadata["type"] first, node_type
        // only as legacy fallback), matching the client filter and the GPU.
        if !filter.include_linked_pages && node.population_type() == Some("linked_page") {
            continue;
        }
        if remove_by_predicate {
            let meta = graph_data.metadata.get(&node.metadata_id);
            let quality = node
                .metadata
                .get("quality_score")
                // The ontology-entity sync path stores the same signal camelCase
                // (`qualityScore`, from vc:qualityScore): accept both keys.
                .or_else(|| node.metadata.get("qualityScore"))
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| meta.and_then(|m| m.quality_score));
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

    debug!(
        "Recomputed filtered nodes: {} of {} visible (enabled={}, server_remove={}, quality_threshold={}, mode={:?})",
        filter.filtered_node_ids.len(),
        graph_data.nodes.len(),
        filter.enabled,
        filter.server_remove,
        filter.quality_threshold,
        filter.filter_mode
    );
}

/// Whether a node passes the enabled quality / authority predicates.
///
/// Ignores `enabled` and `server_remove`; the caller decides whether the
/// predicates apply. `None` quality is neutral; `None` authority scores
/// [`UNSCORED_AUTHORITY`].
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
        verdicts.push(authority.unwrap_or(UNSCORED_AUTHORITY) >= filter.authority_threshold);
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
        let c = ClientFilter::default();
        assert_eq!(c.enabled, s.enabled);
        assert_eq!(c.quality_threshold, s.quality_threshold);
        assert_eq!(c.server_remove, s.server_remove);
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
