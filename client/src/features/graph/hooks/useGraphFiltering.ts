/**
 * useGraphFiltering
 *
 * Extracts node filtering logic (quality/authority thresholds, hierarchy-based
 * visibility, expansion state) and derived lookup maps from GraphManager.
 *
 * Provides:
 *  - visibleNodes: the filtered subset of graphData.nodes that should be rendered
 *  - nodeIdToIndexMap: O(1) lookup from string node ID to its index in graphData.nodes
 *  - filteredEdges: edges whose both endpoints survive filtering (same as graphData.edges
 *    today since edge filtering is position-based, but provided for future use)
 */

import { useMemo, useEffect } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { useExpansionState, type ExpansionState } from '../hooks/useExpansionState';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData, Node as KGNode, Edge } from '../managers/graphDataManager';
import { debugState } from '../../../utils/clientDebugState';
import type { HierarchyNode } from '../utils/hierarchyDetector';

const logger = createLogger('useGraphFiltering');

// ---------------------------------------------------------------------------
// Hook return type
// ---------------------------------------------------------------------------

export interface GraphFilteringResult {
  /** Nodes that pass hierarchy-expansion and quality/authority filters */
  visibleNodes: KGNode[];
  /** O(1) lookup: string node-id -> index in graphData.nodes (full set, not filtered) */
  nodeIdToIndexMap: Map<string, number>;
  /** Edges from graphData (pass-through today; hook owns the contract for future filtering) */
  filteredEdges: Edge[];
  /** Expansion state controller (for toggling node expansion in hierarchy view) */
  expansionState: ExpansionState;
}

// ---------------------------------------------------------------------------
// Hook implementation
// ---------------------------------------------------------------------------

export function useGraphFiltering(
  graphData: GraphData,
  hierarchyMap: Map<string, HierarchyNode>,
  connectionCountMap: Map<string, number>,
): GraphFilteringResult {
  // Narrow selector: only subscribe to nodeFilter — prevents re-renders when
  // unrelated settings (glow, edges, physics, etc.) change.
  const storeNodeFilter = useSettingsStore(s => s.settings?.nodeFilter);

  // --- nodeIdToIndexMap ---
  const nodeIdToIndexMap = useMemo(() =>
    new Map(graphData.nodes.map((n, i) => [String(n.id), i])),
    [graphData.nodes]
  );

  // --- Expansion state (per-client, no server persistence) ---
  const expansionState = useExpansionState(true); // Default: all expanded
  const filterEnabled = storeNodeFilter?.enabled ?? false;
  const qualityThreshold = storeNodeFilter?.qualityThreshold ?? 0.7;
  const authorityThreshold = storeNodeFilter?.authorityThreshold ?? 0.5;
  const filterByQuality = storeNodeFilter?.filterByQuality ?? true;
  const filterByAuthority = storeNodeFilter?.filterByAuthority ?? false;
  const filterMode = storeNodeFilter?.filterMode ?? 'or';
  // Tier-depth collapse: client-only, default 999 (= filter off).
  const tierDepth = storeNodeFilter?.tierDepth ?? 999;
  // Hide kg_stub placeholder nodes — typically 85% of the dataset on a content-
  // sparse graph. Operates BEFORE the hierarchy/quality gates so it removes
  // load before everything else.
  const hideStubs = storeNodeFilter?.hideStubs ?? false;

  useEffect(() => {
    if (debugState.isEnabled()) {
      logger.debug('[NodeFilter] Settings updated:', {
        enabled: filterEnabled,
        qualityThreshold,
        authorityThreshold,
        filterMode,
        tierDepth,
      });
    }
  }, [filterEnabled, qualityThreshold, authorityThreshold, filterByQuality, filterByAuthority, filterMode, tierDepth, storeNodeFilter]);

  // --- Tier-depth driven mass-collapse ---
  // When the slider moves, repopulate expansionState.collapsedNodes with every
  // parent at depth >= tierDepth. The effect intentionally does NOT depend on
  // expansionState (would cause an infinite loop) — it overwrites the entire
  // set, which means manual click-toggles get blown away whenever the slider
  // moves. That trade-off matches WebVOWL's UX: tier slider is the dominant
  // control; per-node toggles are for fine-tuning between slider moves.
  useEffect(() => {
    expansionState.setCollapsedFromTierDepth(tierDepth, hierarchyMap);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tierDepth, hierarchyMap]);

  // --- visibleNodes ---
  const visibleNodes = useMemo(() => {
    if (graphData.nodes.length === 0) return [];

    logger.debug(`[NodeFilter] Computing visible nodes: filterEnabled=${filterEnabled}, qualityThreshold=${qualityThreshold}, authorityThreshold=${authorityThreshold}`);

    const visible = graphData.nodes.filter(node => {
      // Hide kg_stub placeholder nodes. The backend marks orphan wikilink
      // targets (no source file, empty label, empty metadataId) as `type:
      // 'kg_stub'`. Two access shapes seen in the wild — top-level `node.type`
      // from /api/graph/data, and `node.metadata.type` from the worker pipeline
      // — so check both.
      if (hideStubs) {
        const topType = (node as unknown as { type?: string }).type;
        const metaType = node.metadata?.type;
        if (topType === 'kg_stub' || metaType === 'kg_stub') return false;
      }

      // First apply hierarchy/expansion filtering
      const hierarchyNode = hierarchyMap.get(node.id);
      if (hierarchyNode) {
        // Root nodes always pass hierarchy check
        if (!hierarchyNode.isRoot) {
          // Child nodes visible only if parent is expanded
          if (!expansionState.isVisible(node.id, hierarchyNode.parentId)) {
            return false;
          }
        }
      }

      // Then apply quality/authority filtering if enabled
      if (filterEnabled) {
        // Get quality score - use metadata if available, otherwise compute from connections
        let quality = node.metadata?.quality ?? node.metadata?.qualityScore;
        if (quality === undefined || quality === null) {
          // Compute quality from node connections (normalized 0-1) using pre-built map
          const connectionCount = connectionCountMap.get(node.id) || 0;
          // Map connections to 0-1 range: 0 connections = 0, 10+ connections = 1
          quality = Math.min(1.0, connectionCount / 10);
        }

        // Get authority score - use metadata if available, otherwise compute from hierarchy
        let authority = node.metadata?.authority ?? node.metadata?.authorityScore;
        if (authority === undefined || authority === null) {
          // Compute authority from hierarchy depth and connections
          const hierarchyNode = hierarchyMap.get(node.id);
          const depth = hierarchyNode?.depth ?? 0;
          // Root nodes (depth 0) have high authority, deeper nodes have less
          authority = Math.max(0, 1.0 - (depth * 0.2));
        }

        const passesQuality = !filterByQuality || quality >= qualityThreshold;
        const passesAuthority = !filterByAuthority || authority >= authorityThreshold;

        // Apply filter mode (AND requires both, OR requires at least one)
        if (filterMode === 'and') {
          if (!passesQuality || !passesAuthority) {
            return false;
          }
        } else {
          // OR mode - but only if at least one filter is active
          if (filterByQuality || filterByAuthority) {
            if (!passesQuality && !passesAuthority) {
              return false;
            }
          }
        }
      }

      return true;
    });

    if (filterEnabled && debugState.isEnabled()) {
      logger.debug(`[NodeFilter] Result: ${visible.length}/${graphData.nodes.length} nodes visible`);
    }

    return visible;
  }, [graphData.nodes, connectionCountMap, hierarchyMap, expansionState, filterEnabled, qualityThreshold, authorityThreshold, filterByQuality, filterByAuthority, filterMode]);

  // --- filteredEdges (pass-through today) ---
  const filteredEdges = graphData.edges;

  return {
    visibleNodes,
    nodeIdToIndexMap,
    filteredEdges,
    expansionState,
  };
}
