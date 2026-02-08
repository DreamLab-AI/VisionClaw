//! Semantic Pathfinding Service
//!
//! Intelligent graph traversal with query-aware and type-aware pathfinding

use crate::models::graph::GraphData;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, BinaryHeap, VecDeque};
use std::cmp::Ordering;

/// Path search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathResult {
    /// Node IDs in the path
    pub path: Vec<u32>,
    /// Total path cost/distance
    pub cost: f32,
    /// Path relevance score (0.0-1.0)
    pub relevance: f32,
    /// Explanation of path
    pub explanation: String,
}

/// Pathfinding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathfindingConfig {
    /// Maximum path length
    pub max_length: usize,
    /// Maximum nodes to explore
    pub max_explored: usize,
    /// Weight for edge weights (0.0-1.0)
    pub edge_weight_factor: f32,
    /// Weight for semantic similarity (0.0-1.0)
    pub semantic_weight_factor: f32,
    /// Weight for type compatibility (0.0-1.0)
    pub type_weight_factor: f32,
}

impl Default for PathfindingConfig {
    fn default() -> Self {
        Self {
            max_length: 10,
            max_explored: 1000,
            edge_weight_factor: 0.4,
            semantic_weight_factor: 0.4,
            type_weight_factor: 0.2,
        }
    }
}

/// Node state for pathfinding
#[derive(Clone)]
struct PathNode {
    node_id: u32,
    cost: f32,
    relevance: f32,
    path: Vec<u32>,
}

impl Eq for PathNode {}

impl PartialEq for PathNode {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (prioritize lower cost + higher relevance)
        let self_score = self.cost - self.relevance;
        let other_score = other.cost - other.relevance;
        other_score.partial_cmp(&self_score).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Semantic pathfinding service
pub struct SemanticPathfindingService {
    config: PathfindingConfig,
}

impl SemanticPathfindingService {
    /// Create a new semantic pathfinding service
    pub fn new(config: PathfindingConfig) -> Self {
        Self { config }
    }

    /// Find shortest semantic path between two nodes
    /// Uses enhanced A* with semantic weighting
    pub fn find_semantic_path(
        &self,
        graph: &GraphData,
        start_id: u32,
        end_id: u32,
        query: Option<&str>,
    ) -> Option<PathResult> {
        info!("Finding semantic path from {} to {}", start_id, end_id);

        // Build adjacency list
        let adjacency = self.build_adjacency_list(graph);

        // A* search with semantic weighting
        let mut open_set = BinaryHeap::new();
        let mut closed_set = HashSet::new();
        let mut g_scores: HashMap<u32, f32> = HashMap::new();

        // Initialize
        g_scores.insert(start_id, 0.0);
        open_set.push(PathNode {
            node_id: start_id,
            cost: 0.0,
            relevance: 1.0,
            path: vec![start_id],
        });

        let mut explored = 0;

        while let Some(current) = open_set.pop() {
            if current.node_id == end_id {
                return Some(PathResult {
                    path: current.path.clone(),
                    cost: current.cost,
                    relevance: current.relevance,
                    explanation: format!("Found path with {} hops", current.path.len() - 1),
                });
            }

            if explored >= self.config.max_explored {
                break;
            }

            if closed_set.contains(&current.node_id) {
                continue;
            }

            closed_set.insert(current.node_id);
            explored += 1;

            // Explore neighbors
            if let Some(neighbors) = adjacency.get(&current.node_id) {
                for &(neighbor_id, _edge_weight, edge_type) in neighbors {
                    if closed_set.contains(&neighbor_id) {
                        continue;
                    }

                    if current.path.len() >= self.config.max_length {
                        continue;
                    }

                    // Calculate semantic weight
                    let semantic_cost = self.calculate_semantic_cost(
                        graph,
                        current.node_id,
                        neighbor_id,
                        edge_type,
                        query,
                    );

                    let tentative_g = current.cost + semantic_cost;

                    if let Some(&existing_g) = g_scores.get(&neighbor_id) {
                        if tentative_g >= existing_g {
                            continue;
                        }
                    }

                    g_scores.insert(neighbor_id, tentative_g);

                    let mut new_path = current.path.clone();
                    new_path.push(neighbor_id);

                    // Calculate relevance based on query similarity if provided
                    let relevance = if let Some(q) = query {
                        self.calculate_query_relevance(graph, neighbor_id, q)
                    } else {
                        current.relevance * 0.95 // Decay relevance with distance
                    };

                    open_set.push(PathNode {
                        node_id: neighbor_id,
                        cost: tentative_g,
                        relevance,
                        path: new_path,
                    });
                }
            }
        }

        None
    }

    /// Query-guided traversal
    /// Prioritizes nodes similar to the query at each step
    pub fn query_traversal(
        &self,
        graph: &GraphData,
        start_id: u32,
        query: &str,
        max_nodes: usize,
    ) -> Vec<PathResult> {
        info!("Starting query-guided traversal from {}", start_id);

        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(start_id);
        queue.push_back(PathNode {
            node_id: start_id,
            cost: 0.0,
            relevance: 1.0,
            path: vec![start_id],
        });

        let adjacency = self.build_adjacency_list(graph);

        while let Some(current) = queue.pop_front() {
            if results.len() >= max_nodes {
                break;
            }

            // Calculate relevance to query
            let relevance = self.calculate_query_relevance(graph, current.node_id, query);

            if relevance > 0.3 {
                // Threshold for inclusion
                results.push(PathResult {
                    path: current.path.clone(),
                    cost: current.cost,
                    relevance,
                    explanation: format!("Query match score: {:.2}", relevance),
                });
            }

            // Explore neighbors, prioritizing by query similarity
            if let Some(neighbors) = adjacency.get(&current.node_id) {
                let mut neighbor_scores: Vec<_> = neighbors
                    .iter()
                    .filter(|(nid, _, _)| !visited.contains(nid))
                    .map(|&(nid, weight, edge_type)| {
                        let rel = self.calculate_query_relevance(graph, nid, query);
                        (nid, weight, edge_type, rel)
                    })
                    .collect();

                // Sort by relevance (descending)
                neighbor_scores.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(Ordering::Equal));

                for (neighbor_id, _edge_weight, edge_type, neighbor_relevance) in neighbor_scores.iter().take(5) {
                    if visited.contains(neighbor_id) {
                        continue;
                    }

                    visited.insert(*neighbor_id);

                    let semantic_cost = self.calculate_semantic_cost(
                        graph,
                        current.node_id,
                        *neighbor_id,
                        *edge_type,
                        Some(query),
                    );

                    let mut new_path = current.path.clone();
                    new_path.push(*neighbor_id);

                    queue.push_back(PathNode {
                        node_id: *neighbor_id,
                        cost: current.cost + semantic_cost,
                        relevance: *neighbor_relevance,
                        path: new_path,
                    });
                }
            }
        }

        // Sort results by relevance
        results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(Ordering::Equal));

        results
    }

    /// Chunk-based local traversal
    /// Explores locally similar nodes without query context
    pub fn chunk_traversal(
        &self,
        graph: &GraphData,
        start_id: u32,
        max_nodes: usize,
    ) -> Vec<PathResult> {
        info!("Starting chunk traversal from {}", start_id);

        let mut results = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = BinaryHeap::new();

        visited.insert(start_id);
        queue.push(PathNode {
            node_id: start_id,
            cost: 0.0,
            relevance: 1.0,
            path: vec![start_id],
        });

        let adjacency = self.build_adjacency_list(graph);

        // Get start node type for similarity calculation
        let start_node_type = graph.nodes
            .iter()
            .find(|n| n.id == start_id)
            .and_then(|n| n.node_type.as_ref().map(|s| s.as_str()));

        while let Some(current) = queue.pop() {
            if results.len() >= max_nodes {
                break;
            }

            results.push(PathResult {
                path: current.path.clone(),
                cost: current.cost,
                relevance: current.relevance,
                explanation: format!("Chunk member at distance {:.2}", current.cost),
            });

            // Explore neighbors
            if let Some(neighbors) = adjacency.get(&current.node_id) {
                for &(neighbor_id, _edge_weight, _edge_type) in neighbors {
                    if visited.contains(&neighbor_id) {
                        continue;
                    }

                    visited.insert(neighbor_id);

                    // Calculate local similarity
                    let similarity = self.calculate_local_similarity(
                        graph,
                        start_id,
                        neighbor_id,
                        start_node_type,
                    );

                    let mut new_path = current.path.clone();
                    new_path.push(neighbor_id);

                    queue.push(PathNode {
                        node_id: neighbor_id,
                        cost: current.cost + (1.0 - similarity),
                        relevance: similarity,
                        path: new_path,
                    });
                }
            }
        }

        results
    }

    // Helper methods

    fn build_adjacency_list(&self, graph: &GraphData) -> HashMap<u32, Vec<(u32, f32, i32)>> {
        let mut adjacency: HashMap<u32, Vec<(u32, f32, i32)>> = HashMap::new();

        for edge in &graph.edges {
            let edge_type = self.edge_type_to_int(&edge.edge_type);

            adjacency
                .entry(edge.source)
                .or_insert_with(Vec::new)
                .push((edge.target, edge.weight, edge_type));

            // Undirected - add reverse edge
            adjacency
                .entry(edge.target)
                .or_insert_with(Vec::new)
                .push((edge.source, edge.weight, edge_type));
        }

        adjacency
    }

    fn calculate_semantic_cost(
        &self,
        graph: &GraphData,
        from_id: u32,
        to_id: u32,
        _edge_type: i32,
        query: Option<&str>,
    ) -> f32 {
        let mut cost = 1.0;

        // Edge weight factor
        if let Some(edge) = graph.edges.iter().find(|e| {
            (e.source == from_id && e.target == to_id) || (e.source == to_id && e.target == from_id)
        }) {
            cost *= 1.0 + (1.0 - edge.weight) * self.config.edge_weight_factor;
        }

        // Type compatibility factor
        let from_node = graph.nodes.iter().find(|n| n.id == from_id);
        let to_node = graph.nodes.iter().find(|n| n.id == to_id);

        if let (Some(from), Some(to)) = (from_node, to_node) {
            if from.node_type != to.node_type {
                cost *= 1.0 + self.config.type_weight_factor;
            }
        }

        // Query relevance factor
        if let Some(q) = query {
            if let Some(to_node) = to_node {
                let relevance = self.calculate_node_query_relevance(to_node, q);
                cost *= 1.0 + (1.0 - relevance) * self.config.semantic_weight_factor;
            }
        }

        cost
    }

    fn calculate_query_relevance(&self, graph: &GraphData, node_id: u32, query: &str) -> f32 {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
            self.calculate_node_query_relevance(node, query)
        } else {
            0.0
        }
    }

    fn calculate_node_query_relevance(&self, node: &crate::models::node::Node, query: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let label_lower = node.label.to_lowercase();

        // Simple keyword matching
        let label_match = if label_lower.contains(&query_lower) {
            1.0
        } else {
            // Partial word matching
            let query_words: HashSet<&str> = query_lower.split_whitespace().collect();
            let label_words: HashSet<&str> = label_lower.split_whitespace().collect();
            let intersection = query_words.intersection(&label_words).count();
            let union = query_words.union(&label_words).count();

            if union > 0 {
                intersection as f32 / union as f32
            } else {
                0.0
            }
        };

        // Check metadata for matches
        let metadata_match = node.metadata.values().any(|v| {
            v.to_lowercase().contains(&query_lower)
        });

        if metadata_match {
            (label_match + 0.5).min(1.0)
        } else {
            label_match
        }
    }

    fn calculate_local_similarity(
        &self,
        graph: &GraphData,
        _ref_id: u32,
        node_id: u32,
        ref_node_type: Option<&str>,
    ) -> f32 {
        if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
            // Type similarity
            let type_similarity = if let Some(ref_type) = ref_node_type {
                if node.node_type.as_deref() == Some(ref_type) {
                    1.0
                } else {
                    0.3
                }
            } else {
                0.5
            };

            // Could add more similarity metrics (position, metadata, etc.)
            type_similarity
        } else {
            0.0
        }
    }

    fn edge_type_to_int(&self, edge_type: &Option<String>) -> i32 {
        match edge_type.as_deref() {
            None | Some("generic") => 0,
            Some("dependency") => 1,
            Some("hierarchy") => 2,
            Some("association") => 3,
            Some("sequence") => 4,
            Some("subClassOf") => 5,
            Some("instanceOf") => 6,
            Some(_) => 7,
        }
    }
}

impl Default for SemanticPathfindingService {
    fn default() -> Self {
        Self::new(PathfindingConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::node::Node;
    use crate::models::edge::Edge;

    #[test]
    fn test_semantic_pathfinding_creation() {
        let config = PathfindingConfig::default();
        let _service = SemanticPathfindingService::new(config);
    }

    #[test]
    fn test_query_relevance() {
        let service = SemanticPathfindingService::default();

        let mut node = Node::new("test".to_string());
        node.label = "Machine Learning Project".to_string();

        let relevance = service.calculate_node_query_relevance(&node, "machine learning");
        assert!(relevance > 0.5);

        let relevance2 = service.calculate_node_query_relevance(&node, "unrelated query");
        assert!(relevance2 < 0.5);
    }

    #[test]
    fn test_adjacency_list() {
        let service = SemanticPathfindingService::default();

        let mut graph = GraphData::new();
        let node1 = Node::new("n1".to_string());
        let node2 = Node::new("n2".to_string());
        let id1 = node1.id;
        let id2 = node2.id;
        graph.nodes.push(node1);
        graph.nodes.push(node2);

        let edge = Edge::new(id1, id2, 1.0);
        graph.edges.push(edge);

        let adjacency = service.build_adjacency_list(&graph);

        assert!(adjacency.contains_key(&id1));
        assert!(adjacency.contains_key(&id2));
        assert_eq!(adjacency[&id1].len(), 1);
        assert_eq!(adjacency[&id2].len(), 1);
    }
}
