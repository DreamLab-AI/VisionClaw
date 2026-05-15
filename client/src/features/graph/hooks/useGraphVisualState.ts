/**
 * useGraphVisualState
 *
 * Extracts visual-mode classification, hierarchy detection, and
 * connection-count computation from GraphManager into a dedicated hook.
 *
 * Provides:
 *  - perNodeVisualModeMap: per-node visual mode assignment (binary protocol first, metadata fallback)
 *  - hierarchyMap: ontology depth hierarchy derived from node IDs
 *  - connectionCountMap: O(1) edge-connection lookup per node
 *  - dominantMode: the single GraphVisualMode governing the current graph population
 */

import { useMemo } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { NodeType } from '../../../types/binaryProtocol';
import { detectHierarchy, type HierarchyNode } from '../utils/hierarchyDetector';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData, Node as KGNode } from '../managers/graphDataManager';

const logger = createLogger('useGraphVisualState');

// === GRAPH VISUAL MODE ===
export type GraphVisualMode = 'knowledge_graph' | 'ontology' | 'agent';

// ---------------------------------------------------------------------------
// Pure helper functions (no React hooks — deterministic, testable)
// ---------------------------------------------------------------------------

/** Detect the dominant graph visual mode from node population (sampled for perf) */
const detectGraphMode = (nodes: KGNode[]): GraphVisualMode => {
  if (nodes.length === 0) return 'knowledge_graph';
  const sample = nodes.length > 50 ? nodes.slice(0, 50) : nodes;
  let ontologySignals = 0;
  let agentSignals = 0;
  for (const n of sample) {
    if ((n as unknown as { owlClassIri?: string }).owlClassIri || n.metadata?.hierarchyDepth !== undefined || n.metadata?.depth !== undefined) {
      ontologySignals++;
    }
    // Require agentType as the definitive agent signal.
    // `status` alone is too generic — knowledge graph nodes commonly have status fields.
    if (n.metadata?.agentType || n.metadata?.tokenRate !== undefined) {
      agentSignals++;
    }
  }
  const threshold = sample.length * 0.2;
  if (agentSignals > threshold && agentSignals >= ontologySignals) return 'agent';
  if (ontologySignals > threshold) return 'ontology';
  return 'knowledge_graph';
};

/** Map binary protocol NodeType to GraphVisualMode for per-node rendering */
export const nodeTypeToVisualMode = (nodeType: NodeType): GraphVisualMode => {
  switch (nodeType) {
    case NodeType.Agent:
      return 'agent';
    case NodeType.OntologyClass:
    case NodeType.OntologyIndividual:
    case NodeType.OntologyProperty:
      return 'ontology';
    case NodeType.Knowledge:
      return 'knowledge_graph';
    default:
      return 'knowledge_graph';
  }
};

// ---------------------------------------------------------------------------
// Hook return type
// ---------------------------------------------------------------------------

export interface GraphVisualStateResult {
  /** Per-node visual mode, keyed by string node ID */
  perNodeVisualModeMap: Map<string, GraphVisualMode>;
  /** Ontology hierarchy map, keyed by string node ID */
  hierarchyMap: Map<string, HierarchyNode>;
  /** Connection count per node, keyed by string node ID */
  connectionCountMap: Map<string, number>;
  /** The single dominant visual mode for the graph */
  dominantMode: GraphVisualMode;
}

// ---------------------------------------------------------------------------
// Hook implementation
// ---------------------------------------------------------------------------

export function useGraphVisualState(graphData: GraphData): GraphVisualStateResult {
  // Narrow selector: only the graph mode override matters for visual state.
  // Avoids re-renders when unrelated settings (glow, edges, physics) change.
  const settingsGraphMode = useSettingsStore(
    s => (s.settings?.visualisation?.graphs as unknown as { mode?: GraphVisualMode } | undefined)?.mode
  );

  // ADR-061: node type now ships via JSON `/api/graph/data` (`node.type`) at
  // session init — not via per-frame binary flag bits. The classification
  // priorities below cascade JSON-first → metadata heuristics; no wire-side
  // map is consulted.

  // --- hierarchyMap (built from hierarchical edges, not path heuristic) ---
  const hierarchyMap = useMemo(() => {
    if (graphData.nodes.length === 0) return new Map<string, HierarchyNode>();
    const hierarchy = detectHierarchy(graphData.nodes, graphData.edges);
    const maxDepth = hierarchy.size > 0
      ? Math.max(...Array.from(hierarchy.values()).map(n => n.depth))
      : 0;
    const parentCount = Array.from(hierarchy.values()).filter(n => n.childIds.length > 0).length;
    logger.debug(`Hierarchy: ${hierarchy.size} nodes, ${parentCount} parents, max depth ${maxDepth}`);
    return hierarchy;
  }, [graphData.nodes, graphData.edges]);

  // --- connectionCountMap ---
  const connectionCountMap = useMemo(() => {
    const map = new Map<string, number>();
    for (const edge of graphData.edges) {
      const src = String(edge.source);
      const tgt = String(edge.target);
      map.set(src, (map.get(src) || 0) + 1);
      map.set(tgt, (map.get(tgt) || 0) + 1);
    }
    return map;
  }, [graphData.edges]);

  // --- dominantMode ---
  const dominantMode: GraphVisualMode = useMemo(() => {
    if (settingsGraphMode && (settingsGraphMode === 'knowledge_graph' || settingsGraphMode === 'ontology' || settingsGraphMode === 'agent')) {
      return settingsGraphMode;
    }
    return detectGraphMode(graphData.nodes);
  }, [settingsGraphMode, graphData.nodes]);

  // --- perNodeVisualModeMap ---
  const perNodeVisualModeMap = useMemo(() => {
    const map = new Map<string, GraphVisualMode>();
    for (const node of graphData.nodes) {
      // Priority 1: Node type field from API (set by GraphStateActor classify_node).
      // Per ADR-061 this is the canonical source — node type rides JSON init,
      // not the per-frame binary stream.
      const nodeType = (node as unknown as { type?: string }).type || '';
      if (nodeType === 'ontology_node' || nodeType === 'owl_class' || nodeType === 'OwlClass'
          || nodeType.includes(':') // OWL class IRI like "mv:Avatar", "ai:BdiModel"
      ) {
        map.set(String(node.id), 'ontology');
        continue;
      }
      if (nodeType === 'page' || nodeType === 'linked_page') {
        map.set(String(node.id), 'knowledge_graph');
        continue;
      }
      if (nodeType === 'agent' || nodeType === 'bot') {
        map.set(String(node.id), 'agent');
        continue;
      }

      // Priority 2: Metadata heuristics (fallback for nodes without explicit type)
      const nt = node.metadata?.nodeType || (node as unknown as { nodeType?: string }).nodeType || '';
      const owlIri = (node as unknown as { owlClassIri?: string }).owlClassIri;
      if (node.metadata?.agentType || node.metadata?.tokenRate !== undefined) {
        map.set(String(node.id), 'agent');
      } else if (owlIri || nt === 'owl_class' || node.metadata?.hierarchyDepth !== undefined) {
        map.set(String(node.id), 'ontology');
      }
      // If no signals found, don't set -- will fall through to global dominantMode
    }
    return map;
  }, [graphData.nodes]);

  return {
    perNodeVisualModeMap,
    hierarchyMap,
    connectionCountMap,
    dominantMode,
  };
}
