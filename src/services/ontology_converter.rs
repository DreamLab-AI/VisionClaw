// src/services/ontology_converter.rs
//! Ontology to Graph Converter (SIMPLIFIED VERSION)
//!
//! Converts OWL ontology classes to graph nodes for visualization.
//! Uses hornedowl/whelk-rs for advanced ontology reasoning.
//! This is the critical bridge that populates owl_class_iri fields.

use log::{info, warn};
use std::collections::HashMap;
use std::sync::Arc;

use crate::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;
use visionclaw_domain::ports::ontology_repository::{OntologyRepository, OwlClass};

/// Statistics from ontology conversion
#[derive(Default, Debug)]
pub struct ConversionStats {
    pub nodes_created: usize,
    pub edges_created: usize,
}

/// Converts OWL ontology classes to graph nodes
pub struct OntologyConverter {
    ontology_repo: Arc<dyn OntologyRepository>,
    graph_repo: Arc<dyn KnowledgeGraphRepository>,
}

impl OntologyConverter {
    pub fn new(
        ontology_repo: Arc<dyn OntologyRepository>,
        graph_repo: Arc<dyn KnowledgeGraphRepository>,
    ) -> Self {
        Self {
            ontology_repo,
            graph_repo,
        }
    }

    /// Convert all OWL classes to graph nodes (simplified for sprint)
    pub async fn convert_all(&self) -> Result<ConversionStats, Box<dyn std::error::Error>> {
        let mut stats = ConversionStats::default();

        info!("Starting ontology to graph conversion...");

        // 1. Load all OWL classes using trait method
        let classes = self.ontology_repo.get_classes().await?;
        info!("Found {} OWL classes to convert", classes.len());

        if classes.is_empty() {
            warn!("No OWL classes found in ontology repository");
            return Ok(stats);
        }

        // 2. Convert each class to a node
        let mut nodes = Vec::new();
        for class in &classes {
            match self.create_node_from_class(class) {
                Ok(node) => {
                    nodes.push(node);
                    stats.nodes_created += 1;
                    if stats.nodes_created % 100 == 0 {
                        info!(
                            "  Converted {} / {} nodes...",
                            stats.nodes_created,
                            classes.len()
                        );
                    }
                }
                Err(e) => {
                    warn!("Failed to create node from class {}: {}", class.iri, e);
                }
            }
        }

        // 3. Save as graph
        if !nodes.is_empty() {
            let mut graph = GraphData::new();
            graph.nodes = nodes;
            self.graph_repo.save_graph(&graph).await?;
            info!("Saved {} nodes to graph repository", stats.nodes_created);
        }

        info!("Conversion complete! Nodes: {}", stats.nodes_created);

        Ok(stats)
    }

    /// Create a graph node from an OWL class
    fn create_node_from_class(&self, class: &OwlClass) -> Result<Node, Box<dyn std::error::Error>> {
        // Extract IRI suffix as metadata_id
        let metadata_id = class
            .iri
            .split(':')
            .next_back()
            .or(class.iri.split('/').next_back())
            .unwrap_or(&class.iri)
            .to_string();

        // Create metadata HashMap with ontology properties
        let mut metadata = HashMap::new();
        metadata.insert("owl_class_iri".to_string(), class.iri.clone());

        if let Some(label) = &class.label {
            metadata.insert("ontology_label".to_string(), label.clone());
        }

        if let Some(desc) = &class.description {
            metadata.insert("description".to_string(), desc.clone());
        }

        // Determine visual properties based on ontology
        let (color, size) = self.get_class_visual_properties(&class.iri);

        let label = class.label.clone().unwrap_or_else(|| metadata_id.clone());

        // Create BinaryNodeData for GPU physics
        use crate::utils::socket_flow_messages::BinaryNodeData;
        let data: visionclaw_domain::BinaryNodeData = BinaryNodeData {
            node_id: 0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        }
        .into();

        Ok(Node {
            id: 0, // Auto-assigned by database
            metadata_id,
            label,
            data,

            // CRITICAL: Populate owl_class_iri - THIS IS THE KEY FIELD
            owl_class_iri: Some(class.iri.clone()),

            // Initial physics state (Option<f32>)
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),

            // Physical properties (Option<f32>)
            mass: Some(1.0),

            // Visual properties
            color: Some(color),
            size: Some(size as f32), // Convert f64 to f32
            node_type: Some("ontology_class".to_string()),
            weight: Some(1.0),
            group: None,

            // Metadata as HashMap (not JSON string)
            metadata,
            file_size: 0,
            user_data: None,
        })
    }

    /// Determine visual properties based on class IRI
    fn get_class_visual_properties(&self, iri: &str) -> (String, f64) {
        // Classification rules based on IRI patterns
        if iri.contains("Person") || iri.contains("User") || iri.contains("Individual") {
            ("#90EE90".to_string(), 8.0) // Light green, small
        } else if iri.contains("Company") || iri.contains("Organization") || iri.contains("Corp") {
            ("#4169E1".to_string(), 12.0) // Royal blue, large
        } else if iri.contains("Project") || iri.contains("Work") || iri.contains("Task") {
            ("#FFA500".to_string(), 10.0) // Orange, medium
        } else if iri.contains("Concept") || iri.contains("Idea") || iri.contains("Notion") {
            ("#9370DB".to_string(), 9.0) // Medium purple, small-medium
        } else if iri.contains("Technology") || iri.contains("Tool") || iri.contains("System") {
            ("#00CED1".to_string(), 11.0) // Dark turquoise, medium-large
        } else {
            ("#CCCCCC".to_string(), 10.0) // Gray, default medium
        }
    }
}

// CustomReasoner now provides EL++ reasoning capabilities
// - EL++ tractable reasoning via CustomReasoner
// - Infers class hierarchy (SubClassOf relationships)
// - Detects disjoint classes for separation forces
// - Class inference integrated into OntologyPipelineService
