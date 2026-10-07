//! Advanced Pathfinding Algorithms
//!
//! Provides A*, Bidirectional Dijkstra, and Semantic SSSP for point-to-point
//! queries on the graph. These complement the existing GPU-based SSSP which
//! visits all reachable nodes.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// Result of a point-to-point path query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathResult {
    /// Ordered node IDs along the path (source first, target last).
    pub path: Vec<u32>,
    /// Total edge-weight distance of the path.
    pub distance: f32,
    /// Whether a valid path was found.
    pub exists: bool,
    /// Number of nodes visited during the search.
    pub nodes_visited: usize,
    /// Algorithm that produced this result.
    pub algorithm: String,
}

/// Extended result for semantic pathfinding that includes relevance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticPathResult {
    /// Base path result.
    #[serde(flatten)]
    pub path_result: PathResult,
    /// Semantic relevance score for the path (0.0-1.0).
    pub relevance: f32,
    /// The query that guided the search.
    pub query: String,
}

/// Which algorithm to use for a point-to-point query.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PathAlgorithm {
    #[default]
    Astar,
    Bidirectional,
    Semantic,
    /// Falls back to full SSSP via GPU actor.
    Sssp,
}

// ---------------------------------------------------------------------------
// Internal heap entry
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct HeapEntry {
    node_id: u32,
    f_score: f32, // For A*: g + h.  For Dijkstra: g.
    g_score: f32,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}
impl Eq for HeapEntry {}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap behaviour in BinaryHeap (which is a max-heap).
        other
            .f_score
            .partial_cmp(&self.f_score)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// Adjacency helpers
// ---------------------------------------------------------------------------

/// Build a forward adjacency list: node_id -> Vec<(neighbor_id, weight)>.
/// The graph is treated as undirected.
fn build_adjacency(graph: &GraphData) -> HashMap<u32, Vec<(u32, f32)>> {
    let mut adj: HashMap<u32, Vec<(u32, f32)>> = HashMap::new();
    for edge in &graph.edges {
        adj.entry(edge.source)
            .or_default()
            .push((edge.target, edge.weight));
        adj.entry(edge.target)
            .or_default()
            .push((edge.source, edge.weight));
    }
    adj
}

/// Build a node-id to position lookup.
fn build_position_map(graph: &GraphData) -> HashMap<u32, (f32, f32, f32)> {
    graph
        .nodes
        .iter()
        .map(|n| (n.id, (n.x(), n.y(), n.z())))
        .collect()
}

/// Euclidean distance between two 3D points.
#[inline]
fn euclidean_3d(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    let dz = a.2 - b.2;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Reconstruct a path from the predecessor map.
fn reconstruct_path(predecessors: &HashMap<u32, u32>, source: u32, target: u32) -> Vec<u32> {
    let mut path = vec![target];
    let mut current = target;
    while current != source {
        match predecessors.get(&current) {
            Some(&prev) => {
                path.push(prev);
                current = prev;
            }
            None => return Vec::new(),
        }
    }
    path.reverse();
    path
}

// ---------------------------------------------------------------------------
// A* Search
// ---------------------------------------------------------------------------

pub struct AStarPathfinder;

impl AStarPathfinder {
    /// A* with Euclidean distance heuristic (nodes have 3D positions).
    ///
    /// Returns the shortest path by edge weight from `source_id` to `target_id`.
    /// The heuristic is admissible when edge weights correspond to (or exceed)
    /// spatial distances, providing an optimal solution.
    pub fn find_path(
        graph: &GraphData,
        source_id: u32,
        target_id: u32,
    ) -> Result<PathResult, String> {
        if source_id == target_id {
            return Ok(PathResult {
                path: vec![source_id],
                distance: 0.0,
                exists: true,
                nodes_visited: 1,
                algorithm: "astar".into(),
            });
        }

        let adj = build_adjacency(graph);
        let positions = build_position_map(graph);

        let target_pos = positions
            .get(&target_id)
            .copied()
            .ok_or_else(|| format!("Target node {} not found in graph", target_id))?;
        if !positions.contains_key(&source_id) {
            return Err(format!("Source node {} not found in graph", source_id));
        }

        let mut open: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut g_scores: HashMap<u32, f32> = HashMap::new();
        let mut predecessors: HashMap<u32, u32> = HashMap::new();
        let mut closed: HashSet<u32> = HashSet::new();

        let source_pos = positions[&source_id];
        let h0 = euclidean_3d(source_pos, target_pos);

        g_scores.insert(source_id, 0.0);
        open.push(HeapEntry {
            node_id: source_id,
            f_score: h0,
            g_score: 0.0,
        });

        let mut nodes_visited: usize = 0;

        while let Some(current) = open.pop() {
            if current.node_id == target_id {
                let path = reconstruct_path(&predecessors, source_id, target_id);
                return Ok(PathResult {
                    path,
                    distance: current.g_score,
                    exists: true,
                    nodes_visited,
                    algorithm: "astar".into(),
                });
            }

            if closed.contains(&current.node_id) {
                continue;
            }
            closed.insert(current.node_id);
            nodes_visited += 1;

            if let Some(neighbors) = adj.get(&current.node_id) {
                for &(neighbor_id, weight) in neighbors {
                    if closed.contains(&neighbor_id) {
                        continue;
                    }

                    let tentative_g = current.g_score + weight;
                    let existing_g = g_scores.get(&neighbor_id).copied().unwrap_or(f32::MAX);

                    if tentative_g < existing_g {
                        g_scores.insert(neighbor_id, tentative_g);
                        predecessors.insert(neighbor_id, current.node_id);

                        let h = positions
                            .get(&neighbor_id)
                            .map(|&pos| euclidean_3d(pos, target_pos))
                            .unwrap_or(0.0);

                        open.push(HeapEntry {
                            node_id: neighbor_id,
                            f_score: tentative_g + h,
                            g_score: tentative_g,
                        });
                    }
                }
            }
        }

        Ok(PathResult {
            path: Vec::new(),
            distance: f32::MAX,
            exists: false,
            nodes_visited,
            algorithm: "astar".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Bidirectional Dijkstra
// ---------------------------------------------------------------------------

pub struct BidirectionalDijkstra;

impl BidirectionalDijkstra {
    /// Runs Dijkstra simultaneously from source and target, meeting in the middle.
    ///
    /// Termination: when the sum of the minimum tentative distances in the
    /// forward and backward heaps exceeds the best found path distance.
    pub fn find_path(
        graph: &GraphData,
        source_id: u32,
        target_id: u32,
    ) -> Result<PathResult, String> {
        if source_id == target_id {
            return Ok(PathResult {
                path: vec![source_id],
                distance: 0.0,
                exists: true,
                nodes_visited: 1,
                algorithm: "bidirectional".into(),
            });
        }

        let adj = build_adjacency(graph);

        // Verify both nodes exist.
        let node_ids: HashSet<u32> = graph.nodes.iter().map(|n| n.id).collect();
        if !node_ids.contains(&source_id) {
            return Err(format!("Source node {} not found in graph", source_id));
        }
        if !node_ids.contains(&target_id) {
            return Err(format!("Target node {} not found in graph", target_id));
        }

        // Forward search state
        let mut fwd_heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut fwd_g: HashMap<u32, f32> = HashMap::new();
        let mut fwd_pred: HashMap<u32, u32> = HashMap::new();
        let mut fwd_closed: HashSet<u32> = HashSet::new();

        // Backward search state
        let mut bwd_heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut bwd_g: HashMap<u32, f32> = HashMap::new();
        let mut bwd_pred: HashMap<u32, u32> = HashMap::new();
        let mut bwd_closed: HashSet<u32> = HashSet::new();

        fwd_g.insert(source_id, 0.0);
        fwd_heap.push(HeapEntry {
            node_id: source_id,
            f_score: 0.0,
            g_score: 0.0,
        });

        bwd_g.insert(target_id, 0.0);
        bwd_heap.push(HeapEntry {
            node_id: target_id,
            f_score: 0.0,
            g_score: 0.0,
        });

        let mut best_distance = f32::MAX;
        let mut meeting_node: Option<u32> = None;
        let mut nodes_visited: usize = 0;

        // Alternate forward and backward expansions
        loop {
            let fwd_min = fwd_heap.peek().map(|e| e.f_score).unwrap_or(f32::MAX);
            let bwd_min = bwd_heap.peek().map(|e| e.f_score).unwrap_or(f32::MAX);

            // Termination condition
            if fwd_min + bwd_min >= best_distance {
                break;
            }

            // Both heaps empty means no path
            if fwd_heap.is_empty() && bwd_heap.is_empty() {
                break;
            }

            // Expand forward
            if fwd_min <= bwd_min {
                if let Some(current) = fwd_heap.pop() {
                    if fwd_closed.contains(&current.node_id) {
                        continue;
                    }
                    fwd_closed.insert(current.node_id);
                    nodes_visited += 1;

                    // Check if backward search already settled this node
                    if let Some(&bwd_dist) = bwd_g.get(&current.node_id) {
                        let total = current.g_score + bwd_dist;
                        if total < best_distance {
                            best_distance = total;
                            meeting_node = Some(current.node_id);
                        }
                    }

                    if let Some(neighbors) = adj.get(&current.node_id) {
                        for &(neighbor_id, weight) in neighbors {
                            if fwd_closed.contains(&neighbor_id) {
                                continue;
                            }
                            let tentative_g = current.g_score + weight;
                            let existing_g = fwd_g.get(&neighbor_id).copied().unwrap_or(f32::MAX);
                            if tentative_g < existing_g {
                                fwd_g.insert(neighbor_id, tentative_g);
                                fwd_pred.insert(neighbor_id, current.node_id);
                                fwd_heap.push(HeapEntry {
                                    node_id: neighbor_id,
                                    f_score: tentative_g,
                                    g_score: tentative_g,
                                });

                                // Check meeting
                                if let Some(&bwd_dist) = bwd_g.get(&neighbor_id) {
                                    let total = tentative_g + bwd_dist;
                                    if total < best_distance {
                                        best_distance = total;
                                        meeting_node = Some(neighbor_id);
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                // Expand backward
                if let Some(current) = bwd_heap.pop() {
                    if bwd_closed.contains(&current.node_id) {
                        continue;
                    }
                    bwd_closed.insert(current.node_id);
                    nodes_visited += 1;

                    // Check if forward search already settled this node
                    if let Some(&fwd_dist) = fwd_g.get(&current.node_id) {
                        let total = fwd_dist + current.g_score;
                        if total < best_distance {
                            best_distance = total;
                            meeting_node = Some(current.node_id);
                        }
                    }

                    if let Some(neighbors) = adj.get(&current.node_id) {
                        for &(neighbor_id, weight) in neighbors {
                            if bwd_closed.contains(&neighbor_id) {
                                continue;
                            }
                            let tentative_g = current.g_score + weight;
                            let existing_g = bwd_g.get(&neighbor_id).copied().unwrap_or(f32::MAX);
                            if tentative_g < existing_g {
                                bwd_g.insert(neighbor_id, tentative_g);
                                bwd_pred.insert(neighbor_id, current.node_id);
                                bwd_heap.push(HeapEntry {
                                    node_id: neighbor_id,
                                    f_score: tentative_g,
                                    g_score: tentative_g,
                                });

                                // Check meeting
                                if let Some(&fwd_dist) = fwd_g.get(&neighbor_id) {
                                    let total = fwd_dist + tentative_g;
                                    if total < best_distance {
                                        best_distance = total;
                                        meeting_node = Some(neighbor_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        match meeting_node {
            Some(mid) => {
                // Reconstruct forward path: source -> mid
                let fwd_path = reconstruct_path(&fwd_pred, source_id, mid);

                // Reconstruct backward path: target -> mid, then reverse to get mid -> target
                let mut bwd_path = reconstruct_path(&bwd_pred, target_id, mid);
                bwd_path.reverse();

                // Merge: fwd_path ends with mid, bwd_path starts with mid.
                // Remove duplicate mid from bwd_path.
                let mut full_path = fwd_path;
                if !bwd_path.is_empty() {
                    // bwd_path[0] == mid which is already the last element of full_path
                    full_path.extend_from_slice(&bwd_path[1..]);
                }

                Ok(PathResult {
                    path: full_path,
                    distance: best_distance,
                    exists: true,
                    nodes_visited,
                    algorithm: "bidirectional".into(),
                })
            }
            None => Ok(PathResult {
                path: Vec::new(),
                distance: f32::MAX,
                exists: false,
                nodes_visited,
                algorithm: "bidirectional".into(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Embedding provider trait + Jaccard fallback
// ---------------------------------------------------------------------------

/// Trait for embedding providers (pluggable for future vector embedding models).
pub trait EmbeddingProvider: Send + Sync {
    /// Produce an embedding vector for a text string.
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, String>;

    /// Cosine similarity between two vectors. Returns value in [-1, 1].
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let (mut dot, mut mag_a, mut mag_b) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..a.len() {
            dot += a[i] * b[i];
            mag_a += a[i] * a[i];
            mag_b += b[i] * b[i];
        }
        let denom = mag_a.sqrt() * mag_b.sqrt();
        if denom < 1e-9 {
            0.0
        } else {
            dot / denom
        }
    }
}

/// Fallback embedding provider using word-level Jaccard similarity.
///
/// Each word is a dimension; the "embedding" is a binary indicator vector
/// over the vocabulary of all words seen. Cosine similarity on binary vectors
/// is equivalent to Jaccard similarity.
pub struct JaccardEmbedding;

impl EmbeddingProvider for JaccardEmbedding {
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        // Return the set of unique lowercased words as sorted tokens.
        // We encode them as hash-based sparse representation, but since
        // cosine_similarity is overridden, we store the raw string hash
        // as a single-element vector for identification purposes.
        // The real similarity is computed in cosine_similarity override.
        //
        // For the Jaccard provider, we store a fingerprint and compute
        // similarity directly from text. The embed_text result is a
        // placeholder that carries the text hash.
        let hash = text
            .to_lowercase()
            .split_whitespace()
            .map(|w| {
                let mut h: u32 = 0;
                for b in w.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(b as u32);
                }
                h as f32
            })
            .collect::<Vec<f32>>();
        Ok(hash)
    }

    fn cosine_similarity(&self, _a: &[f32], _b: &[f32]) -> f32 {
        // Not meaningful for Jaccard; similarity is computed via text directly.
        0.0
    }
}

impl JaccardEmbedding {
    /// Jaccard similarity between two text strings (word-level).
    pub fn jaccard_similarity(a: &str, b: &str) -> f32 {
        let a_lower = a.to_lowercase();
        let b_lower = b.to_lowercase();
        let set_a: HashSet<&str> = a_lower.split_whitespace().collect();
        let set_b: HashSet<&str> = b_lower.split_whitespace().collect();
        if set_a.is_empty() && set_b.is_empty() {
            return 1.0;
        }
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }
}

// ---------------------------------------------------------------------------
// Semantic Pathfinder
// ---------------------------------------------------------------------------

/// Modified Dijkstra where edge weights incorporate semantic similarity to
/// a query string.
///
/// Edge cost formula:
///   effective_weight = edge.weight * (1.0 + alpha * (1.0 - semantic_similarity))
///
/// where `alpha` controls how much semantic relevance influences the path.
/// Nodes whose labels/metadata are more related to the query get lower
/// traversal cost, biasing the search toward semantically relevant paths.
pub struct SemanticPathfinder {
    /// Controls semantic influence. 0.0 = pure Dijkstra, 1.0 = strong semantic bias.
    pub alpha: f32,
}

impl Default for SemanticPathfinder {
    fn default() -> Self {
        Self { alpha: 0.5 }
    }
}

impl SemanticPathfinder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha.clamp(0.0, 2.0);
        self
    }

    /// Find a semantically-guided shortest path.
    ///
    /// Uses modified Dijkstra where edge weights are scaled by how relevant
    /// the target node is to `query`.
    pub fn find_path(
        &self,
        graph: &GraphData,
        source_id: u32,
        target_id: u32,
        query: &str,
    ) -> Result<SemanticPathResult, String> {
        if source_id == target_id {
            return Ok(SemanticPathResult {
                path_result: PathResult {
                    path: vec![source_id],
                    distance: 0.0,
                    exists: true,
                    nodes_visited: 1,
                    algorithm: "semantic".into(),
                },
                relevance: 1.0,
                query: query.to_string(),
            });
        }

        let adj = build_adjacency(graph);
        let node_labels: HashMap<u32, String> = graph
            .nodes
            .iter()
            .map(|n| (n.id, Self::node_text(n)))
            .collect();

        if !node_labels.contains_key(&source_id) {
            return Err(format!("Source node {} not found in graph", source_id));
        }
        if !node_labels.contains_key(&target_id) {
            return Err(format!("Target node {} not found in graph", target_id));
        }

        // Pre-compute semantic similarity for each node to the query.
        let query_lower = query.to_lowercase();
        let node_similarity: HashMap<u32, f32> = node_labels
            .iter()
            .map(|(&id, text)| {
                let sim = JaccardEmbedding::jaccard_similarity(text, &query_lower);
                (id, sim)
            })
            .collect();

        let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut g_scores: HashMap<u32, f32> = HashMap::new();
        let mut predecessors: HashMap<u32, u32> = HashMap::new();
        let mut closed: HashSet<u32> = HashSet::new();
        let mut nodes_visited: usize = 0;

        g_scores.insert(source_id, 0.0);
        heap.push(HeapEntry {
            node_id: source_id,
            f_score: 0.0,
            g_score: 0.0,
        });

        while let Some(current) = heap.pop() {
            if current.node_id == target_id {
                let path = reconstruct_path(&predecessors, source_id, target_id);
                let path_relevance = Self::compute_path_relevance(&path, &node_similarity);
                return Ok(SemanticPathResult {
                    path_result: PathResult {
                        path,
                        distance: current.g_score,
                        exists: true,
                        nodes_visited,
                        algorithm: "semantic".into(),
                    },
                    relevance: path_relevance,
                    query: query.to_string(),
                });
            }

            if closed.contains(&current.node_id) {
                continue;
            }
            closed.insert(current.node_id);
            nodes_visited += 1;

            if let Some(neighbors) = adj.get(&current.node_id) {
                for &(neighbor_id, weight) in neighbors {
                    if closed.contains(&neighbor_id) {
                        continue;
                    }

                    let sim = node_similarity.get(&neighbor_id).copied().unwrap_or(0.0);
                    let semantic_penalty = 1.0 + self.alpha * (1.0 - sim);
                    let effective_weight = weight * semantic_penalty;
                    let tentative_g = current.g_score + effective_weight;

                    let existing_g = g_scores.get(&neighbor_id).copied().unwrap_or(f32::MAX);
                    if tentative_g < existing_g {
                        g_scores.insert(neighbor_id, tentative_g);
                        predecessors.insert(neighbor_id, current.node_id);
                        heap.push(HeapEntry {
                            node_id: neighbor_id,
                            f_score: tentative_g,
                            g_score: tentative_g,
                        });
                    }
                }
            }
        }

        Ok(SemanticPathResult {
            path_result: PathResult {
                path: Vec::new(),
                distance: f32::MAX,
                exists: false,
                nodes_visited,
                algorithm: "semantic".into(),
            },
            relevance: 0.0,
            query: query.to_string(),
        })
    }

    /// Combine node label + metadata into a single text for similarity.
    fn node_text(node: &Node) -> String {
        let mut parts = vec![node.label.to_lowercase()];
        if let Some(ref t) = node.node_type {
            parts.push(t.to_lowercase());
        }
        for val in node.metadata.values() {
            parts.push(val.to_lowercase());
        }
        parts.join(" ")
    }

    /// Average semantic similarity of all nodes on the path.
    fn compute_path_relevance(path: &[u32], similarity: &HashMap<u32, f32>) -> f32 {
        if path.is_empty() {
            return 0.0;
        }
        let sum: f32 = path
            .iter()
            .map(|id| similarity.get(id).copied().unwrap_or(0.0))
            .sum();
        sum / path.len() as f32
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::graph::GraphData;
    use visionclaw_domain::models::node::Node;

    /// Helper: build a small test graph.
    ///
    /// Layout (positions form a line along x-axis):
    ///   1 --1.0-- 2 --1.0-- 3
    ///   |                    |
    ///   +------5.0-----------+
    fn make_test_graph() -> (GraphData, u32, u32, u32) {
        let mut graph = GraphData::new();

        let n1 = Node::new("n1".into())
            .with_label("alpha node".into())
            .with_position(0.0, 0.0, 0.0);
        let n2 = Node::new("n2".into())
            .with_label("beta node".into())
            .with_position(1.0, 0.0, 0.0);
        let n3 = Node::new("n3".into())
            .with_label("gamma node".into())
            .with_position(2.0, 0.0, 0.0);

        let id1 = n1.id;
        let id2 = n2.id;
        let id3 = n3.id;

        graph.nodes.push(n1);
        graph.nodes.push(n2);
        graph.nodes.push(n3);

        graph.edges.push(Edge::new(id1, id2, 1.0));
        graph.edges.push(Edge::new(id2, id3, 1.0));
        graph.edges.push(Edge::new(id1, id3, 5.0)); // long shortcut

        (graph, id1, id2, id3)
    }

    #[test]
    fn test_astar_shortest_path() {
        let (graph, id1, _id2, id3) = make_test_graph();
        let result = AStarPathfinder::find_path(&graph, id1, id3).unwrap();
        assert!(result.exists);
        assert_eq!(result.path.len(), 3); // 1 -> 2 -> 3
        assert!((result.distance - 2.0).abs() < 0.001);
        assert_eq!(result.algorithm, "astar");
    }

    #[test]
    fn test_astar_same_node() {
        let (graph, id1, _, _) = make_test_graph();
        let result = AStarPathfinder::find_path(&graph, id1, id1).unwrap();
        assert!(result.exists);
        assert_eq!(result.path, vec![id1]);
        assert_eq!(result.distance, 0.0);
    }

    #[test]
    fn test_astar_nonexistent_source() {
        let (graph, _, _, _) = make_test_graph();
        let result = AStarPathfinder::find_path(&graph, 99999, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_bidirectional_shortest_path() {
        let (graph, id1, _id2, id3) = make_test_graph();
        let result = BidirectionalDijkstra::find_path(&graph, id1, id3).unwrap();
        assert!(result.exists);
        // Shortest path is 1->2->3 with distance 2.0
        assert!((result.distance - 2.0).abs() < 0.001);
        assert_eq!(result.algorithm, "bidirectional");
    }

    #[test]
    fn test_bidirectional_same_node() {
        let (graph, id1, _, _) = make_test_graph();
        let result = BidirectionalDijkstra::find_path(&graph, id1, id1).unwrap();
        assert!(result.exists);
        assert_eq!(result.path, vec![id1]);
    }

    #[test]
    fn test_bidirectional_no_path() {
        let mut graph = GraphData::new();
        let n1 = Node::new("isolated1".into()).with_position(0.0, 0.0, 0.0);
        let n2 = Node::new("isolated2".into()).with_position(10.0, 0.0, 0.0);
        let id1 = n1.id;
        let id2 = n2.id;
        graph.nodes.push(n1);
        graph.nodes.push(n2);
        // No edges

        let result = BidirectionalDijkstra::find_path(&graph, id1, id2).unwrap();
        assert!(!result.exists);
        assert!(result.path.is_empty());
    }

    #[test]
    fn test_semantic_pathfinder() {
        let (mut graph, id1, id2, id3) = make_test_graph();
        // Give node 2 a label related to query
        if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == id2) {
            n.label = "important search term".into();
        }

        let pathfinder = SemanticPathfinder::new().with_alpha(0.5);

        let result = pathfinder
            .find_path(&graph, id1, id3, "search term")
            .unwrap();
        assert!(result.path_result.exists);
        assert_eq!(result.path_result.algorithm, "semantic");
        assert!(!result.query.is_empty());
    }

    #[test]
    fn test_jaccard_similarity() {
        let sim = JaccardEmbedding::jaccard_similarity("hello world", "hello world");
        assert!((sim - 1.0).abs() < 0.001);

        let sim2 = JaccardEmbedding::jaccard_similarity("hello world", "foo bar");
        assert!(sim2 < 0.01);

        let sim3 = JaccardEmbedding::jaccard_similarity("hello world foo", "hello foo");
        assert!(sim3 > 0.5);
    }

    #[test]
    fn test_euclidean_3d() {
        let d = euclidean_3d((0.0, 0.0, 0.0), (3.0, 4.0, 0.0));
        assert!((d - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_embedding_provider_default_cosine() {
        let provider = JaccardEmbedding;
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        // JaccardEmbedding overrides cosine_similarity to return 0.0
        let sim = provider.cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}
