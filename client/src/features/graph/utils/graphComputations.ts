/**
 * Pure computation functions extracted from GraphManager.tsx for testability.
 *
 * These functions contain no React, Three.js, or side-effect dependencies.
 * They operate on plain JS objects and arrays.
 */

import type { GraphVisualMode } from '../hooks/useGraphVisualState';
import type { Node as GraphNode, Edge } from '../managers/graphDataManager';
import { computeNodeScale } from './nodeScaling';
import type { GraphTypeVisualsSettings } from '../../settings/config/settings';

// ---------- Domain / type color constants ----------

export { getDomainColor } from './domainColors';

const TYPE_COLORS: Record<string, string> = {
  'folder': '#FFD700',
  'file': '#00CED1',
  'function': '#FF6B6B',
  'class': '#4ECDC4',
  'variable': '#95E1D3',
  'import': '#F38181',
  'export': '#AA96DA',
  'default': '#00ffff',
};

const ONTOLOGY_DEPTH_COLORS = ['#FF6B6B', '#FFD93D', '#4ECDC4', '#AA96DA', '#95E1D3'];
const ONTOLOGY_PROPERTY_COLOR = '#F38181';
const ONTOLOGY_INSTANCE_COLOR = '#B8D4E3';

const AGENT_STATUS_COLORS: Record<string, string> = {
  'active': '#2ECC71',
  'busy': '#F39C12',
  'idle': '#95A5A6',
  'error': '#E74C3C',
  'default': '#2ECC71',
};

const AGENT_TYPE_COLORS: Record<string, string> = {
  'queen': '#FFD700',
  'coordinator': '#E67E22',
};

// ---------- Edge position computation ----------

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface EdgePositionResult {
  source: Vec3;
  target: Vec3;
}

/**
 * Compute edge endpoint positions (surface-to-surface) for a set of edges.
 *
 * Given flat position data (x,y,z per node in index order), node index mapping,
 * and per-node visual scale, returns an array of source/target position pairs
 * offset to the visual surface of each node.
 *
 * Edges referencing missing nodes or zero-length edges are skipped.
 */
export function computeEdgePositions(
  edges: Edge[],
  positions: Float32Array | number[],
  nodeIdToIndexMap: Map<string, number>,
  nodeSize: number,
  nodes: GraphNode[],
  connectionCountMap: Map<string, number>,
  graphMode: GraphVisualMode,
  hierarchyMap?: Map<string, any>,
  graphTypeVisuals?: GraphTypeVisualsSettings,
  perNodeVisualModeMap?: Map<string, GraphVisualMode>,
): EdgePositionResult[] {
  const results: EdgePositionResult[] = [];

  for (const edge of edges) {
    const sourceStr = String(edge.source);
    const targetStr = String(edge.target);
    const sourceIndex = nodeIdToIndexMap.get(sourceStr);
    const targetIndex = nodeIdToIndexMap.get(targetStr);

    if (sourceIndex === undefined || targetIndex === undefined) continue;

    const i3s = sourceIndex * 3;
    const i3t = targetIndex * 3;

    if (i3s + 2 >= positions.length || i3t + 2 >= positions.length) continue;

    const sx = positions[i3s], sy = positions[i3s + 1], sz = positions[i3s + 2];
    const tx = positions[i3t], ty = positions[i3t + 1], tz = positions[i3t + 2];

    // Direction vector
    const dx = tx - sx, dy = ty - sy, dz = tz - sz;
    const edgeLength = Math.sqrt(dx * dx + dy * dy + dz * dz);

    if (edgeLength <= 0.001) continue;

    // Normalize
    const nx = dx / edgeLength, ny = dy / edgeLength, nz = dz / edgeLength;

    const sourceNode = nodes[sourceIndex];
    const targetNode = nodes[targetIndex];
    if (!sourceNode || !targetNode) continue;

    const sourceVisualMode = perNodeVisualModeMap?.get(sourceStr) || graphMode;
    const targetVisualMode = perNodeVisualModeMap?.get(targetStr) || graphMode;

    const sourceRadius = computeNodeScale(sourceNode, connectionCountMap, sourceVisualMode, hierarchyMap, graphTypeVisuals) * nodeSize;
    const targetRadius = computeNodeScale(targetNode, connectionCountMap, targetVisualMode, hierarchyMap, graphTypeVisuals) * nodeSize;

    const srcX = sx + nx * sourceRadius;
    const srcY = sy + ny * sourceRadius;
    const srcZ = sz + nz * sourceRadius;
    const tgtX = tx - nx * targetRadius;
    const tgtY = ty - ny * targetRadius;
    const tgtZ = tz - nz * targetRadius;

    // Check remaining gap
    const gapDx = tgtX - srcX, gapDy = tgtY - srcY, gapDz = tgtZ - srcZ;
    const gap = Math.sqrt(gapDx * gapDx + gapDy * gapDy + gapDz * gapDz);
    if (gap <= 0.1) continue;

    results.push({
      source: { x: srcX, y: srcY, z: srcZ },
      target: { x: tgtX, y: tgtY, z: tgtZ },
    });
  }

  return results;
}

// ---------- Node color computation ----------

/**
 * Compute the display color hex string for a node based on its type and visual mode.
 *
 * This is a pure-function equivalent of the GraphManager getNodeColor(),
 * returning a hex string instead of mutating a shared THREE.Color instance.
 */
export function computeNodeColor(
  node: GraphNode,
  graphMode: GraphVisualMode = 'knowledge_graph',
): string {
  // Ontology mode
  if (graphMode === 'ontology') {
    const nodeType = node.metadata?.type?.toLowerCase() || '';
    if (nodeType === 'property' || nodeType === 'datatype_property' || nodeType === 'object_property') {
      return ONTOLOGY_PROPERTY_COLOR;
    }
    if (nodeType === 'instance' || nodeType === 'individual') {
      return ONTOLOGY_INSTANCE_COLOR;
    }
    const depth = node.metadata?.depth ?? 0;
    const depthIndex = Math.min(depth, ONTOLOGY_DEPTH_COLORS.length - 1);
    return ONTOLOGY_DEPTH_COLORS[depthIndex];
  }

  // Agent mode
  if (graphMode === 'agent') {
    const agentType = node.metadata?.agentType?.toLowerCase() || '';
    const agentStatus = node.metadata?.status?.toLowerCase() || 'active';
    if (AGENT_TYPE_COLORS[agentType]) {
      return AGENT_TYPE_COLORS[agentType];
    }
    return AGENT_STATUS_COLORS[agentStatus] || AGENT_STATUS_COLORS['default'];
  }

  // Knowledge graph mode (default)
  const nodeType = node.metadata?.type || 'default';
  return TYPE_COLORS[nodeType] || TYPE_COLORS['default'];
}

// ---------- Node visibility filtering ----------

export interface NodeTypeVisibility {
  knowledge?: boolean;
  ontology?: boolean;
  agent?: boolean;
}

/**
 * Determine whether a node is visible based on type visibility toggles.
 *
 * Returns true if the node should be shown, false if hidden.
 * When all toggles are true (or visibility config is null), always returns true.
 */
export function isNodeVisible(
  node: GraphNode,
  visibility: NodeTypeVisibility | null | undefined,
  graphMode: GraphVisualMode = 'knowledge_graph',
  perNodeVisualModeMap?: Map<string, GraphVisualMode>,
): boolean {
  if (!visibility) return true;

  // If all types are visible, skip check
  if (visibility.knowledge !== false && visibility.ontology !== false && visibility.agent !== false) {
    return true;
  }

  const nodeMode = perNodeVisualModeMap?.get(String(node.id)) || graphMode;

  if (nodeMode === 'knowledge_graph') return visibility.knowledge !== false;
  if (nodeMode === 'ontology') return visibility.ontology !== false;
  if (nodeMode === 'agent') return visibility.agent !== false;

  return true;
}
