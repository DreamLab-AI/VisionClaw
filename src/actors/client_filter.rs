//! Client-side filtering logic for graph nodes
//!
//! This module implements the filtering logic that determines which nodes
//! are visible to each client based on their filter criteria.

use crate::actors::client_coordinator_actor::{ClientFilter, FilterMode};
use log::{debug, trace};
use visionclaw_domain::models::graph::GraphData;

/// Recomputes which node IDs pass the client's filter criteria
/// Called when:
/// 1. Client authenticates and loads their saved filter
/// 2. Client updates their filter settings
/// 3. Graph data changes (new nodes added)
/// # Arguments
/// * `filter` - The client's filter settings to update
/// * `graph_data` - The complete graph data with node metadata
pub fn recompute_filtered_nodes(filter: &mut ClientFilter, graph_data: &GraphData) {
    filter.filtered_node_ids.clear();

    if !filter.enabled {
        // Filter disabled = all nodes visible (still respect include_linked_pages)
        for node in &graph_data.nodes {
            // SINGLE SOURCE OF TRUTH: gate linked_page stubs on the authoritative
            // origin via Node::population_type() (metadata["type"] first, node_type
            // only as legacy fallback), matching the client filter and the GPU.
            if !filter.include_linked_pages && node.population_type() == Some("linked_page") {
                continue;
            }
            filter.filtered_node_ids.insert(node.id);
        }
        debug!(
            "Filter disabled, {} nodes visible (include_linked_pages={})",
            filter.filtered_node_ids.len(),
            filter.include_linked_pages
        );
        return;
    }

    // Apply filtering logic
    let mut candidates = Vec::new();

    for node in &graph_data.nodes {
        // Gate linked_page stub nodes (wikilink targets with no authored content).
        // SINGLE SOURCE OF TRUTH: read the authoritative origin via
        // Node::population_type() (metadata["type"] first, node_type only as legacy
        // fallback) so the server filter agrees with the GPU and the client.
        if !filter.include_linked_pages {
            let node_type = node.population_type().unwrap_or("");
            if node_type == "linked_page" {
                continue;
            }
        }

        // Extract quality and authority scores from node.metadata HashMap (loaded from Oxigraph store)
        // Falls back to graph_data.metadata if not found in node
        let quality = node
            .metadata
            .get("quality_score")
            // Ontology-entity sync path stores the same signal camelCase
            // (`qualityScore`, from vc:qualityScore) — accept both keys.
            .or_else(|| node.metadata.get("qualityScore"))
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                graph_data
                    .metadata
                    .get(&node.metadata_id)
                    .and_then(|m| m.quality_score)
            })
            .unwrap_or(0.5); // Default to middle value

        let authority = node
            .metadata
            .get("authority_score")
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                graph_data
                    .metadata
                    .get(&node.metadata_id)
                    .and_then(|m| m.authority_score)
            })
            .unwrap_or(0.5);

        // Check individual thresholds
        let passes_quality = !filter.filter_by_quality || quality >= filter.quality_threshold;
        let passes_authority =
            !filter.filter_by_authority || authority >= filter.authority_threshold;

        // Apply filter mode (AND/OR)
        let passes = match filter.filter_mode {
            FilterMode::And => passes_quality && passes_authority,
            FilterMode::Or => passes_quality || passes_authority,
        };

        if passes {
            candidates.push((node.id, quality, authority));
        }
    }

    trace!(
        "Filter applied: {} of {} nodes passed (mode: {:?})",
        candidates.len(),
        graph_data.nodes.len(),
        filter.filter_mode
    );

    // Populate filtered_node_ids
    for (node_id, _, _) in candidates {
        filter.filtered_node_ids.insert(node_id);
    }

    debug!(
        "Recomputed filtered nodes: {} nodes visible (quality_threshold={}, authority_threshold={}, mode={:?})",
        filter.filtered_node_ids.len(),
        filter.quality_threshold,
        filter.authority_threshold,
        filter.filter_mode
    );
}

/// Helper to check if a node passes the filter criteria without modifying state
pub fn node_passes_filter(
    filter: &ClientFilter,
    quality_score: Option<f64>,
    authority_score: Option<f64>,
) -> bool {
    if !filter.enabled {
        return true;
    }

    let quality = quality_score.unwrap_or(0.5);
    let authority = authority_score.unwrap_or(0.5);

    let passes_quality = !filter.filter_by_quality || quality >= filter.quality_threshold;
    let passes_authority = !filter.filter_by_authority || authority >= filter.authority_threshold;

    match filter.filter_mode {
        FilterMode::And => passes_quality && passes_authority,
        FilterMode::Or => passes_quality || passes_authority,
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
            filter_by_quality: true,
            filter_by_authority: false,
            quality_threshold: 0.7,
            ..Default::default()
        };
        // Use And mode for single-criterion filtering (Or mode with filter_by_authority=false would pass all)
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
            filter_by_quality: false,
            filter_by_authority: true,
            authority_threshold: 0.7,
            ..Default::default()
        };
        // Use And mode for single-criterion filtering (Or mode with filter_by_quality=false would pass all)
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
            ..Default::default()
        };

        recompute_filtered_nodes(&mut filter, &graph);

        // Nodes 1, 2, and 3 pass at least one threshold
        assert!(filter.filtered_node_ids.contains(&1));
        assert!(filter.filtered_node_ids.contains(&2));
        assert!(filter.filtered_node_ids.contains(&3));
    }

    #[test]
    fn test_default_values_for_missing_metadata() {
        let graph = create_test_graph();
        let mut filter = ClientFilter {
            enabled: true,
            filter_by_quality: true,
            filter_by_authority: false,
            quality_threshold: 0.6, // Above default 0.5
            ..Default::default()
        };
        // Use And mode for single-criterion filtering
        filter.filter_mode = FilterMode::And;

        recompute_filtered_nodes(&mut filter, &graph);

        // Node 4 has no metadata, should get defaults (0.5) and fail threshold (0.6)
        assert!(!filter.filtered_node_ids.contains(&4));
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
}
