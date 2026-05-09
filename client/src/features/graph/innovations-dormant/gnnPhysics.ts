/**
 * @deprecated DORMANT SERVICE -- imported only by gnnPhysicsConnector.ts,
 * whose sole export (checkAndApplyGNNPhysics) is never called from any
 * component, hook, or render loop. The settings UI toggle
 * (qualityGates.gnnPhysics) exists in unifiedSettingsConfig.ts and
 * settings.ts, but no code path reads that flag and invokes computation.
 * 335 lines of unused GAT implementation. Consider removing in the next
 * dead-code cleanup pass.  Audited 2026-05-09.
 *
 * GNN-Enhanced Physics Module
 *
 * Implements a simplified Graph Attention Network (GAT) for computing
 * edge attention weights that modulate physics spring forces.
 *
 * Architecture: Single-layer GAT with message passing
 * - Node features: [degree_normalized, cluster_id_onehot, position_normalized]
 * - Edge attention: LeakyReLU(a^T [Wh_i || Wh_j])
 * - Output: Per-edge attention weight (0-1)
 *
 * Compatible with RuVector's GNN attention mechanism.
 * When ruvectorEnabled=true, uses HNSW-based neighbor lookup for
 * adaptive edge weight computation.
 */

import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GNNPhysics');

/** Node feature vector for GNN computation */
interface NodeFeatures {
  degree: number;
  clusterCoeff: number;
  posX: number;
  posY: number;
  posZ: number;
}

/** Edge with computed attention weight */
export interface WeightedEdge {
  source: string;
  target: string;
  weight: number; // 0-1 attention weight
}

/** GNN computation result */
export interface GNNResult {
  edgeWeights: WeightedEdge[];
  computeTimeMs: number;
  nodeCount: number;
  edgeCount: number;
}

/**
 * Simple node/edge graph structure for GNN input
 */
interface GraphInput {
  nodes: Array<{ id: string; degree: number; position?: { x: number; y: number; z: number } }>;
  edges: Array<{ source: string; target: string }>;
}

// ============================================================================
// GNN Attention Computation
// ============================================================================

/**
 * Compute node feature vectors from graph structure.
 * Features: [normalized_degree, clustering_coefficient_approx, norm_x, norm_y, norm_z]
 */
function computeNodeFeatures(graph: GraphInput): Map<string, Float32Array> {
  const features = new Map<string, Float32Array>();

  // Find max degree for normalization
  const maxDegree = Math.max(1, ...graph.nodes.map(n => n.degree));

  // Compute position bounds for normalization
  let minX = Infinity, maxX = -Infinity;
  let minY = Infinity, maxY = -Infinity;
  let minZ = Infinity, maxZ = -Infinity;

  for (const node of graph.nodes) {
    if (node.position) {
      minX = Math.min(minX, node.position.x);
      maxX = Math.max(maxX, node.position.x);
      minY = Math.min(minY, node.position.y);
      maxY = Math.max(maxY, node.position.y);
      minZ = Math.min(minZ, node.position.z);
      maxZ = Math.max(maxZ, node.position.z);
    }
  }

  const rangeX = maxX - minX || 1;
  const rangeY = maxY - minY || 1;
  const rangeZ = maxZ - minZ || 1;

  for (const node of graph.nodes) {
    const feat = new Float32Array(5);
    feat[0] = node.degree / maxDegree; // normalized degree
    feat[1] = Math.min(node.degree / 10, 1); // rough clustering proxy
    feat[2] = node.position ? (node.position.x - minX) / rangeX : 0.5;
    feat[3] = node.position ? (node.position.y - minY) / rangeY : 0.5;
    feat[4] = node.position ? (node.position.z - minZ) / rangeZ : 0.5;
    features.set(node.id, feat);
  }

  return features;
}

/**
 * Compute attention weights using simplified GAT mechanism.
 *
 * attention(i,j) = LeakyReLU(a . [W.h_i || W.h_j])
 * where W is a learnable weight matrix (here: identity for simplicity)
 * and a is the attention vector (here: dot product of concatenated features)
 */
function computeAttentionWeights(
  features: Map<string, Float32Array>,
  edges: Array<{ source: string; target: string }>
): WeightedEdge[] {
  const FEATURE_DIM = 5;
  const LEAK = 0.2; // LeakyReLU negative slope

  // Compute raw attention scores for each edge
  const rawScores: number[] = [];

  for (const edge of edges) {
    const srcFeat = features.get(edge.source);
    const tgtFeat = features.get(edge.target);

    if (!srcFeat || !tgtFeat) {
      rawScores.push(0);
      continue;
    }

    // Concatenated feature dot product (simplified attention)
    let score = 0;
    for (let d = 0; d < FEATURE_DIM; d++) {
      score += srcFeat[d] * tgtFeat[d];
    }

    // LeakyReLU
    score = score > 0 ? score : LEAK * score;
    rawScores.push(score);
  }

  // Softmax normalization per source node
  const sourceEdges = new Map<string, number[]>(); // source -> edge indices
  for (let i = 0; i < edges.length; i++) {
    const src = edges[i].source;
    if (!sourceEdges.has(src)) sourceEdges.set(src, []);
    sourceEdges.get(src)!.push(i);
  }

  const normalizedWeights = new Float32Array(edges.length);

  for (const [, edgeIndices] of sourceEdges) {
    // Find max for numerical stability
    let maxScore = -Infinity;
    for (const idx of edgeIndices) {
      maxScore = Math.max(maxScore, rawScores[idx]);
    }

    // Compute softmax
    let sumExp = 0;
    for (const idx of edgeIndices) {
      sumExp += Math.exp(rawScores[idx] - maxScore);
    }

    for (const idx of edgeIndices) {
      normalizedWeights[idx] = Math.exp(rawScores[idx] - maxScore) / (sumExp || 1);
    }
  }

  // Build result
  return edges.map((edge, i) => ({
    source: edge.source,
    target: edge.target,
    weight: normalizedWeights[i],
  }));
}

// ============================================================================
// HNSW Similarity (RuVector-compatible)
// ============================================================================

/**
 * Simple HNSW-inspired nearest neighbor lookup for position-based similarity.
 * When ruvectorEnabled=true, this enhances edge weights with spatial proximity.
 *
 * In production, this would be replaced by RuVector's WASM HNSW implementation
 * for O(log n) approximate nearest neighbor search.
 */
function computeHNSWSimilarity(
  features: Map<string, Float32Array>,
  nodeIds: string[],
  k: number = 5
): Map<string, Array<{ id: string; similarity: number }>> {
  const result = new Map<string, Array<{ id: string; similarity: number }>>();

  // For each node, find k nearest neighbors by feature cosine similarity
  for (const nodeId of nodeIds) {
    const feat = features.get(nodeId);
    if (!feat) continue;

    const similarities: Array<{ id: string; similarity: number }> = [];

    for (const otherId of nodeIds) {
      if (otherId === nodeId) continue;
      const otherFeat = features.get(otherId);
      if (!otherFeat) continue;

      // Cosine similarity
      let dot = 0, normA = 0, normB = 0;
      for (let d = 0; d < feat.length; d++) {
        dot += feat[d] * otherFeat[d];
        normA += feat[d] * feat[d];
        normB += otherFeat[d] * otherFeat[d];
      }
      const sim = dot / (Math.sqrt(normA * normB) || 1);
      similarities.push({ id: otherId, similarity: sim });
    }

    // Sort by similarity descending, take top k
    similarities.sort((a, b) => b.similarity - a.similarity);
    result.set(nodeId, similarities.slice(0, k));
  }

  return result;
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Run GNN-enhanced physics computation on graph data.
 *
 * @param nodes - Graph nodes with IDs and positions
 * @param edges - Graph edges
 * @param options - Configuration options
 * @returns GNN computation result with edge weights
 */
export function computeGNNWeights(
  nodes: Array<{ id: string; position?: { x: number; y: number; z: number } }>,
  edges: Array<{ source: string; target: string }>,
  options: { useHNSW?: boolean } = {}
): GNNResult {
  const startTime = performance.now();

  // Build degree map
  const degreeMap = new Map<string, number>();
  for (const node of nodes) degreeMap.set(node.id, 0);
  for (const edge of edges) {
    degreeMap.set(edge.source, (degreeMap.get(edge.source) ?? 0) + 1);
    degreeMap.set(edge.target, (degreeMap.get(edge.target) ?? 0) + 1);
  }

  // Build graph input
  const graphInput: GraphInput = {
    nodes: nodes.map(n => ({
      id: n.id,
      degree: degreeMap.get(n.id) ?? 0,
      position: n.position,
    })),
    edges,
  };

  // Step 1: Compute node features
  const features = computeNodeFeatures(graphInput);

  // Step 2: Compute attention weights
  let edgeWeights = computeAttentionWeights(features, edges);

  // Step 3: If HNSW enabled, modulate weights with spatial similarity
  if (options.useHNSW) {
    const hnswNeighbors = computeHNSWSimilarity(features, nodes.map(n => n.id));

    // Boost edges that connect HNSW-similar nodes
    edgeWeights = edgeWeights.map(ew => {
      const neighbors = hnswNeighbors.get(ew.source);
      if (!neighbors) return ew;

      const neighborMatch = neighbors.find(n => n.id === ew.target);
      if (neighborMatch) {
        // Blend GNN attention with HNSW similarity (70/30 split)
        return {
          ...ew,
          weight: ew.weight * 0.7 + neighborMatch.similarity * 0.3,
        };
      }
      return ew;
    });
  }

  const computeTimeMs = performance.now() - startTime;

  logger.info(`GNN computed ${edgeWeights.length} edge weights in ${computeTimeMs.toFixed(1)}ms` +
    (options.useHNSW ? ' (with HNSW)' : ''));

  return {
    edgeWeights,
    computeTimeMs,
    nodeCount: nodes.length,
    edgeCount: edges.length,
  };
}

/**
 * Apply GNN edge weights to physics parameters by sending them to the server.
 * The server will use these as spring force multipliers.
 */
export async function applyGNNWeightsToPhysics(
  result: GNNResult,
  baseUrl: string,
  authHeaders: Record<string, string> = {}
): Promise<boolean> {
  try {
    // Compute aggregate spring weight adjustment
    // Higher attention = stronger spring force
    const avgWeight = result.edgeWeights.reduce((sum, e) => sum + e.weight, 0) / (result.edgeWeights.length || 1);

    // Scale spring constant by average GNN attention (range: 0.5x to 2.0x)
    const springMultiplier = 0.5 + avgWeight * 1.5;

    logger.info(`GNN spring multiplier: ${springMultiplier.toFixed(3)} (avg attention: ${avgWeight.toFixed(3)})`);

    // Send as physics parameter adjustment
    const response = await fetch(`${baseUrl}/api/physics/parameters`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
        ...authHeaders,
      },
      body: JSON.stringify({
        attractionK: springMultiplier * 2.0, // base attractionK scaled by GNN
      }),
    });

    return response.ok;
  } catch (err) {
    logger.warn('Failed to apply GNN weights to physics:', err);
    return false;
  }
}
