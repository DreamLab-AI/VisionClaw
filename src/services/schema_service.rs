//! Schema Service - Graph Schema Extraction for Natural Language Queries
//!
//! Provides graph schema information to LLMs for natural language to Cypher translation.
//! Extracts node types, edge types, properties, and relationships from the knowledge graph.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::models::graph::GraphData;

/// Graph schema metadata for LLM context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSchema {
    /// Available node types with counts
    pub node_types: HashMap<String, usize>,
    /// Available edge types with counts
    pub edge_types: HashMap<String, usize>,
    /// Node properties with example values
    pub node_properties: HashMap<String, Vec<String>>,
    /// Edge properties with example values
    pub edge_properties: HashMap<String, Vec<String>>,
    /// Sample node labels
    pub sample_labels: Vec<String>,
    /// OWL classes present in graph
    pub owl_classes: HashSet<String>,
    /// OWL properties present in graph
    pub owl_properties: HashSet<String>,
    /// Total node count
    pub total_nodes: usize,
    /// Total edge count
    pub total_edges: usize,
}

impl GraphSchema {
    /// Create an empty schema
    pub fn new() -> Self {
        Self {
            node_types: HashMap::new(),
            edge_types: HashMap::new(),
            node_properties: HashMap::new(),
            edge_properties: HashMap::new(),
            sample_labels: Vec::new(),
            owl_classes: HashSet::new(),
            owl_properties: HashSet::new(),
            total_nodes: 0,
            total_edges: 0,
        }
    }

    /// Convert schema to human-readable text for LLM context
    pub fn to_llm_context(&self) -> String {
        let mut context = String::new();

        context.push_str("# Graph Schema\n\n");

        context.push_str(&format!("Total Nodes: {}\n", self.total_nodes));
        context.push_str(&format!("Total Edges: {}\n\n", self.total_edges));

        context.push_str("## Node Types\n");
        for (node_type, count) in &self.node_types {
            context.push_str(&format!("- {} ({} nodes)\n", node_type, count));
        }
        context.push_str("\n");

        context.push_str("## Edge Types\n");
        for (edge_type, count) in &self.edge_types {
            context.push_str(&format!("- {} ({} edges)\n", edge_type, count));
        }
        context.push_str("\n");

        context.push_str("## Available Node Properties\n");
        for (prop, examples) in &self.node_properties {
            let example_str = examples.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            context.push_str(&format!("- {} (examples: {})\n", prop, example_str));
        }
        context.push_str("\n");

        if !self.owl_classes.is_empty() {
            context.push_str("## OWL Classes\n");
            for class in self.owl_classes.iter().take(10) {
                context.push_str(&format!("- {}\n", class));
            }
            if self.owl_classes.len() > 10 {
                context.push_str(&format!("... and {} more\n", self.owl_classes.len() - 10));
            }
            context.push_str("\n");
        }

        context.push_str("## Sample Cypher Queries\n");
        context.push_str("```cypher\n");
        context.push_str("// Find all nodes of a specific type\n");
        context.push_str("MATCH (n:GraphNode {node_type: 'person'}) RETURN n\n\n");
        context.push_str("// Find relationships of a specific type\n");
        context.push_str("MATCH (a)-[r:EDGE {relation_type: 'dependency'}]->(b) RETURN a, r, b\n\n");
        context.push_str("// Find paths between nodes\n");
        context.push_str("MATCH path = (a:GraphNode)-[*1..3]->(b:GraphNode) WHERE a.label = 'Start' AND b.label = 'End' RETURN path\n");
        context.push_str("```\n");

        context
    }
}

impl Default for GraphSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Service for extracting and managing graph schema
pub struct SchemaService {
    schema: Arc<tokio::sync::RwLock<GraphSchema>>,
}

impl SchemaService {
    /// Create a new SchemaService
    pub fn new() -> Self {
        Self {
            schema: Arc::new(tokio::sync::RwLock::new(GraphSchema::new())),
        }
    }

    /// Extract schema from graph data
    pub async fn extract_schema(&self, graph: &GraphData) -> GraphSchema {
        let mut schema = GraphSchema::new();

        schema.total_nodes = graph.nodes.len();
        schema.total_edges = graph.edges.len();

        // Extract node types
        let mut node_type_counts: HashMap<String, usize> = HashMap::new();
        for node in &graph.nodes {
            let node_type = node.node_type.as_ref()
                .map(|s| s.clone())
                .unwrap_or_else(|| "generic".to_string());
            *node_type_counts.entry(node_type).or_insert(0) += 1;

            // Collect OWL classes
            if let Some(ref iri) = node.owl_class_iri {
                schema.owl_classes.insert(iri.clone());
            }

            // Sample labels (up to 20)
            if schema.sample_labels.len() < 20 && !node.label.is_empty() {
                schema.sample_labels.push(node.label.clone());
            }
        }
        schema.node_types = node_type_counts;

        // Extract edge types
        let mut edge_type_counts: HashMap<String, usize> = HashMap::new();
        for edge in &graph.edges {
            let edge_type = edge.edge_type.as_ref()
                .map(|s| s.clone())
                .unwrap_or_else(|| "generic".to_string());
            *edge_type_counts.entry(edge_type).or_insert(0) += 1;

            // Collect OWL properties
            if let Some(ref iri) = edge.owl_property_iri {
                schema.owl_properties.insert(iri.clone());
            }
        }
        schema.edge_types = edge_type_counts;

        // Extract node properties
        let mut node_props: HashMap<String, HashSet<String>> = HashMap::new();
        for node in graph.nodes.iter().take(100) {  // Sample first 100 nodes
            if !node.label.is_empty() {
                node_props.entry("label".to_string())
                    .or_insert_with(HashSet::new)
                    .insert(node.label.clone());
            }
            if let Some(ref color) = node.color {
                node_props.entry("color".to_string())
                    .or_insert_with(HashSet::new)
                    .insert(color.clone());
            }
            if let Some(ref group) = node.group {
                node_props.entry("group".to_string())
                    .or_insert_with(HashSet::new)
                    .insert(group.clone());
            }
            for (key, value) in &node.metadata {
                node_props.entry(key.clone())
                    .or_insert_with(HashSet::new)
                    .insert(value.clone());
            }
        }

        // Convert to Vec for easier serialization
        schema.node_properties = node_props.into_iter()
            .map(|(k, v)| (k, v.into_iter().take(5).collect()))
            .collect();

        // Extract edge properties (weight is common)
        let mut edge_props: HashMap<String, HashSet<String>> = HashMap::new();
        edge_props.insert("weight".to_string(),
            graph.edges.iter().take(5).map(|e| e.weight.to_string()).collect());
        schema.edge_properties = edge_props.into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();

        schema
    }

    /// Update cached schema from graph
    pub async fn update_schema(&self, graph: &GraphData) {
        let schema = self.extract_schema(graph).await;
        let mut cached_schema = self.schema.write().await;
        *cached_schema = schema;
    }

    /// Get cached schema
    pub async fn get_schema(&self) -> GraphSchema {
        self.schema.read().await.clone()
    }

    /// Get LLM context string
    pub async fn get_llm_context(&self) -> String {
        let schema = self.schema.read().await;
        schema.to_llm_context()
    }

    /// Get schema for specific node type
    pub async fn get_node_type_info(&self, node_type: &str) -> Option<usize> {
        let schema = self.schema.read().await;
        schema.node_types.get(node_type).copied()
    }

    /// Get schema for specific edge type
    pub async fn get_edge_type_info(&self, edge_type: &str) -> Option<usize> {
        let schema = self.schema.read().await;
        schema.edge_types.get(edge_type).copied()
    }

    /// Get all available node types
    pub async fn get_node_types(&self) -> Vec<String> {
        let schema = self.schema.read().await;
        schema.node_types.keys().cloned().collect()
    }

    /// Get all available edge types
    pub async fn get_edge_types(&self) -> Vec<String> {
        let schema = self.schema.read().await;
        schema.edge_types.keys().cloned().collect()
    }
}

impl Default for SchemaService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::node::Node;
    use crate::models::edge::Edge;

    #[tokio::test]
    async fn test_schema_extraction() {
        let mut graph = GraphData::new();

        // Add nodes with different types
        let mut node1 = Node::new("node1".to_string());
        node1.node_type = Some("person".to_string());
        node1.label = "Alice".to_string();

        let mut node2 = Node::new("node2".to_string());
        node2.node_type = Some("person".to_string());
        node2.label = "Bob".to_string();

        let mut node3 = Node::new("node3".to_string());
        node3.node_type = Some("organization".to_string());
        node3.label = "Acme Corp".to_string();

        graph.nodes = vec![node1, node2, node3];

        // Add edges with different types
        let mut edge1 = Edge::new(1, 2, 1.0);
        edge1.edge_type = Some("dependency".to_string());

        let mut edge2 = Edge::new(2, 3, 1.0);
        edge2.edge_type = Some("hierarchy".to_string());

        graph.edges = vec![edge1, edge2];

        // Extract schema
        let service = SchemaService::new();
        let schema = service.extract_schema(&graph).await;

        // Verify node types
        assert_eq!(schema.total_nodes, 3);
        assert_eq!(schema.node_types.get("person"), Some(&2));
        assert_eq!(schema.node_types.get("organization"), Some(&1));

        // Verify edge types
        assert_eq!(schema.total_edges, 2);
        assert_eq!(schema.edge_types.get("dependency"), Some(&1));
        assert_eq!(schema.edge_types.get("hierarchy"), Some(&1));

        // Verify labels
        assert!(schema.sample_labels.contains(&"Alice".to_string()));
        assert!(schema.sample_labels.contains(&"Bob".to_string()));
        assert!(schema.sample_labels.contains(&"Acme Corp".to_string()));
    }

    #[tokio::test]
    async fn test_llm_context_generation() {
        let mut graph = GraphData::new();

        let mut node1 = Node::new("node1".to_string());
        node1.node_type = Some("person".to_string());
        node1.label = "Alice".to_string();
        graph.nodes.push(node1);

        let service = SchemaService::new();
        service.update_schema(&graph).await;

        let context = service.get_llm_context().await;

        // Verify context contains expected information
        assert!(context.contains("Graph Schema"));
        assert!(context.contains("Total Nodes: 1"));
        assert!(context.contains("person"));
        assert!(context.contains("Sample Cypher Queries"));
    }
}
