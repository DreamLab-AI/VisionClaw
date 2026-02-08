// src/services/ontology_enrichment_service.rs
//! Ontology Enrichment Service
//!
//! Enriches parsed graph data with ontology information (owl_class_iri, owl_property_iri)
//! AFTER parsing but BEFORE saving to database.

use std::sync:::Arc;
use log::{info, debug};

use crate::models::graph:::GraphData;
use crate::services::ontology_reasoner::OntologyReasoner;
use crate::services::edge_classifier::EdgeClassifier;

/// Service that enriches graph data with ontology classifications
pub struct OntologyEnrichmentService {
    reasoner: Arc<OntologyReasoner>,
    classifier: Arc<EdgeClassifier>,
}

impl OntologyEnrichmentService {
    /// Create a new enrichment service
    pub fn new(
        reasoner: Arc<OntologyReasoner>,
        classifier: Arc<EdgeClassifier>,
    ) -> Self {
        info!("Initializing OntologyEnrichmentService");
        Self {
            reasoner,
            classifier,
        }
    }

    /// Enrich a graph with ontology information
        /// This modifies the graph in-place, adding:
    /// - `owl_class_iri` to all nodes based on file path/content analysis
    /// - `owl_property_iri` to all edges based on context analysis
        /// # Arguments
    /// * `graph` - Mutable reference to graph data
    /// * `file_path` - Path to the source markdown file
    /// * `content` - Full markdown content
        /// # Returns
    /// Number of nodes and edges enriched
    pub async fn enrich_graph(
        &self,
        graph: &mut GraphData,
        file_path: &str,
        content: &str,
    ) -> Result<(usize, usize), String> {
        info!("Enriching graph from file: {}", file_path);

        let nodes_enriched = self.enrich_nodes(graph, file_path, content).await?;
        let edges_enriched = self.enrich_edges(graph, content).await?;

        info!(
            "Enriched {} nodes and {} edges with ontology data",
            nodes_enriched, edges_enriched
        );

        Ok((nodes_enriched, edges_enriched))
    }

    /// Enrich all nodes in the graph with owl_class_iri
    async fn enrich_nodes(
        &self,
        graph: &mut GraphData,
        file_path: &str,
        content: &str,
    ) -> Result<usize, String> {
        let mut enriched_count = 0;

        // Parse frontmatter/metadata if present
        let metadata = self.extract_frontmatter(content);

        for node in &mut graph.nodes {
            // Skip nodes that already have owl_class_iri
            if node.owl_class_iri.is_some() {
                continue;
            }

            // Infer class for this node
            let class_iri = self
                .reasoner
                .infer_class(file_path, content, metadata.as_ref())
                .await
                .map_err(|e| format!("Failed to infer class: {}", e))?;

            if let Some(iri) = class_iri {
                // Ensure the class exists in ontology
                self.reasoner
                    .ensure_class_exists(&iri)
                    .await
                    .map_err(|e| format!("Failed to ensure class exists: {}", e))?;

                node.owl_class_iri = Some(iri.clone());
                enriched_count += 1;

                debug!(
                    "Enriched node '{}' with class: {}",
                    node.label, iri
                );

                // Also update visual properties based on class
                self.update_node_visuals_by_class(node, &iri);
            }
        }

        Ok(enriched_count)
    }

    /// Enrich all edges in the graph with owl_property_iri
    async fn enrich_edges(
        &self,
        graph: &mut GraphData,
        content: &str,
    ) -> Result<usize, String> {
        let mut enriched_count = 0;

        // Build node ID to node map for lookups
        let node_map: std::collections::HashMap<u32, &crate::models::node::Node> =
            graph.nodes.iter().map(|n| (n.id, n)).collect();

        for edge in &mut graph.edges {
            // Skip edges that already have owl_property_iri
            if edge.owl_property_iri.is_some() {
                continue;
            }

            // Get source and target nodes
            let source_node = node_map.get(&edge.source);
            let target_node = node_map.get(&edge.target);

            if let (Some(src), Some(tgt)) = (source_node, target_node) {
                // Extract context around the link
                let context = self.extract_link_context(content, &tgt.label);

                // Classify the edge
                let property_iri = self.classifier.classify_edge(
                    &src.label,
                    &tgt.label,
                    src.owl_class_iri.as_deref(),
                    tgt.owl_class_iri.as_deref(),
                    &context,
                );

                if let Some(iri) = property_iri {
                    edge.owl_property_iri = Some(iri.clone());
                    enriched_count += 1;

                    debug!(
                        "Enriched edge {} -> {} with property: {}",
                        src.label, tgt.label, iri
                    );
                }
            }
        }

        Ok(enriched_count)
    }

    /// Extract frontmatter metadata from markdown
    fn extract_frontmatter(
        &self,
        content: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        // Simple frontmatter parser (YAML-style)
        // Looks for:
        // ---
        // key: value
        // ---

        let mut metadata = std::collections::HashMap::new();

        if !content.starts_with("---") {
            return None;
        }

        let lines: Vec<&str> = content.lines().collect();
        let mut in_frontmatter = false;
        let mut frontmatter_found = false;

        for line in lines.iter().skip(1) {
            if line.trim() == "---" {
                if !in_frontmatter && !frontmatter_found {
                    in_frontmatter = true;
                    frontmatter_found = true;
                } else {
                    break; // End of frontmatter
                }
            } else if in_frontmatter {
                if let Some((key, value)) = line.split_once(':') {
                    metadata.insert(
                        key.trim().to_string(),
                        value.trim().to_string(),
                    );
                }
            }
        }

        if metadata.is_empty() {
            None
        } else {
            Some(metadata)
        }
    }

    /// Extract context around a link in markdown content
    fn extract_link_context(&self, content: &str, link_target: &str) -> String {
        // Find the line containing the link
        for line in content.lines() {
            if line.contains(&format!("[[{}]]", link_target)) {
                return line.to_string();
            }
        }

        // If not found as [[link]], try with aliases
        for line in content.lines() {
            if line.contains(link_target) {
                return line.to_string();
            }
        }

        String::new()
    }

    /// Update node visual properties based on its OWL class
    fn update_node_visuals_by_class(&self, node: &mut crate::models::node::Node, class_iri: &str) {
        // Match the visual properties from OntologyConverter
        let (color, size) = if class_iri.contains("Person") || class_iri.contains("Individual") {
            ("#90EE90", 8.0) // Light green, small
        } else if class_iri.contains("Company") || class_iri.contains("Organization") {
            ("#4169E1", 12.0) // Royal blue, large
        } else if class_iri.contains("Project") || class_iri.contains("Work") {
            ("#FFA500", 10.0) // Orange, medium
        } else if class_iri.contains("Concept") || class_iri.contains("Idea") {
            ("#9370DB", 9.0) // Medium purple, small-medium
        } else if class_iri.contains("Technology") || class_iri.contains("Tool") {
            ("#00CED1", 11.0) // Dark turquoise, medium-large
        } else {
            ("#CCCCCC", 10.0) // Gray, default medium
        };

        node.color = Some(color.to_string());
        node.size = Some(size as f32);

        // Update node type to reflect ontology class
        node.node_type = Some("ontology_node".to_string());
    }

    /// Batch enrich multiple graphs
    pub async fn enrich_graphs_batch(
        &self,
        graphs: Vec<(GraphData, String, String)>, // (graph, path, content)
    ) -> Vec<Result<(usize, usize), String>> {
        let mut results = Vec::with_capacity(graphs.len());

        for (mut graph, path, content) in graphs {
            let result = self.enrich_graph(&mut graph, &path, &content).await;
            results.push(result);
        }

        results
    }
}

// TODO: Update tests to use Neo4j test containers
// Tests temporarily disabled - need to be updated to use Neo4j instead of SQLite
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_extract_frontmatter() {
//         let content = r#"---
// type: person
// category: engineer
// ---
//
// # Content here"#;
//
//         let service = OntologyEnrichmentService::new(
//             Arc::new(OntologyReasoner::new(
//                 Arc::new(crate::adapters::whelk_inference_engine::WhelkInferenceEngine::new()),
//                 Arc::new(crate::repositories::unified_ontology_repository::UnifiedOntologyRepository::new(":memory:").unwrap())
//             )),
//             Arc::new(EdgeClassifier::new()),
//         );
//
//         let metadata = service.extract_frontmatter(content);
//         assert!(metadata.is_some());
//         let meta = metadata.unwrap();
//         assert_eq!(meta.get("type"), Some(&"person".to_string()));
//         assert_eq!(meta.get("category"), Some(&"engineer".to_string()));
//     }
//
//     #[test]
//     fn test_extract_link_context() {
//         let content = "Tim Cook is the CEO of [[Apple Inc]].";
//
//         let service = OntologyEnrichmentService::new(
//             Arc::new(OntologyReasoner::new(
//                 Arc::new(crate::adapters::whelk_inference_engine::WhelkInferenceEngine::new()),
//                 Arc::new(crate::repositories::unified_ontology_repository::UnifiedOntologyRepository::new(":memory:").unwrap())
//             )),
//             Arc::new(EdgeClassifier::new()),
//         );
//
//         let context = service.extract_link_context(content, "Apple Inc");
//         assert_eq!(context, "Tim Cook is the CEO of [[Apple Inc]].");
//     }
// }

