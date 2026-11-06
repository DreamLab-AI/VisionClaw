/**
 * useGraphFiltering
 *
 * Node filtering for the desktop graph, split by ADR-2183:
 *
 *  - REMOVAL (the render set, `visibleNodes`): population scope (the linked_page
 *    gate, mirroring the server's include_linked_pages) and hierarchy collapse.
 *    Access control (ADR-2003) and `?graph_type=` scoping happen on the server,
 *    before the client sees a node.
 *  - GREY-OUT (the highlight mask): the interactive node-filter predicates —
 *    quality / authority (AND / OR), minConnections, minMaturity — written as the
 *    `filter:settings` source. They no longer reflow instance buffers.
 *  - `visualisation.graphView.explore.hideFiltered` (default false) is the
 *    explicit opt-in that removes greyed nodes again, from every filter source.
 *
 * `nodeIdToIndexMap` stays over the FULL delivered set: it is the shared render
 * index (state/nodeIndex.ts) that the mask and encoding buffers are indexed by.
 * The predicates live in `explore/nodeFilter.ts`.
 */

import { useMemo, useEffect, useSyncExternalStore } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { useExpansionState, type ExpansionState } from '../hooks/useExpansionState';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData, Node as GraphNode, Edge } from '../managers/graphDataManager';
import type { HierarchyNode } from '../utils/hierarchyDetector';
import { highlightMask as sharedMask, type HighlightMask } from '../state/highlightMask';
import { resolveNodeFilter, isRemovedByScope, isGreyedByNodeFilter } from '../explore/nodeFilter';
import {
  SOURCE, writeFilter, isFilteredOut, subscribeFilterRegistry, filterRegistryVersion,
} from '../explore/filterRegistry';

const logger = createLogger('useGraphFiltering');

/**
 * Setting path of the ADR-2183 "hide filtered nodes" opt-in (default false):
 * `visualisation.graphView.explore.hideFiltered`. When on, nodes greyed by any
 * filter source are removed from the render set, as before ADR-2183.
 */
export const HIDE_FILTERED_SETTING_PATH = 'visualisation.graphView.explore.hideFiltered';

const noSubscribe = () => () => {};

// ---------------------------------------------------------------------------
// Hook return type
// ---------------------------------------------------------------------------

export interface GraphFilteringResult {
  /**
   * The render set: delivered nodes minus population scope (linked_page) and
   * hierarchy collapse. Interactive filters grey nodes through the highlight mask
   * instead (ADR-2183); they are removed only when `hideFiltered` is on.
   */
  visibleNodes: GraphNode[];
  /** Ids greyed out by the node-filter settings (written to the mask as `filter:settings`). */
  greyedNodeIds: ReadonlySet<string>;
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

export interface GraphFilteringOptions {
  /** The highlight mask to write (tests inject their own). */
  mask?: HighlightMask;
}

export function useGraphFiltering(
  graphData: GraphData,
  hierarchyMap: Map<string, HierarchyNode>,
  connectionCountMap: Map<string, number>,
  options: GraphFilteringOptions = {},
): GraphFilteringResult {
  const mask = options.mask ?? sharedMask;
  // Narrow selectors: only subscribe to nodeFilter and the hide toggle — prevents
  // re-renders when unrelated settings (glow, edges, physics, etc.) change.
  const storeNodeFilter = useSettingsStore(s => s.settings?.nodeFilter);
  const hideFiltered = useSettingsStore(s =>
    (s.settings?.visualisation?.graphView?.explore as { hideFiltered?: boolean } | undefined)?.hideFiltered ?? false);

  // --- nodeIdToIndexMap: the shared render index, over the FULL delivered set ---
  const nodeIdToIndexMap = useMemo(() =>
    new Map(graphData.nodes.map((n, i) => [String(n.id), i])),
    [graphData.nodes]
  );

  // --- Expansion state (per-client, no server persistence) ---
  const expansionState = useExpansionState(true); // Default: all expanded
  const f = resolveNodeFilter(storeNodeFilter);

  useEffect(() => {
    logger.info('[NodeFilter] Settings updated:', { ...f, hideFiltered, hasStoreFilter: !!storeNodeFilter });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [storeNodeFilter, hideFiltered]);

  // --- removal: population scope + hierarchy collapse (ADR-2183 D4) ---
  const scopedNodes = useMemo(() => {
    if (graphData.nodes.length === 0) return [];
    return graphData.nodes.filter(node => {
      if (isRemovedByScope(node, f)) return false;
      const h = hierarchyMap.get(node.id);
      if (h && !h.isRoot && !expansionState.isVisible(node.id, h.parentId)) return false;
      return true;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graphData.nodes, hierarchyMap, expansionState, f.includeLinkedPages]);

  // --- grey-out: the interactive node-filter predicates (ADR-2183 D2) ---
  const greyedNodeIds = useMemo(() => {
    const out = new Set<string>();
    for (const node of scopedNodes) {
      if (isGreyedByNodeFilter(node, f, connectionCountMap)) out.add(String(node.id));
    }
    if (f.enabled || f.minConnections > 0 || f.minMaturityRank > 0) {
      logger.debug(`[NodeFilter] ${out.size}/${scopedNodes.length} nodes greyed (quality>=${f.qualityThreshold}, authority>=${f.authorityThreshold}, mode=${f.filterMode})`);
    }
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopedNodes, connectionCountMap, f.enabled, f.qualityThreshold, f.authorityThreshold,
      f.filterByQuality, f.filterByAuthority, f.filterMode, f.minConnections, f.minMaturityRank]);

  // Write the mask after render; an empty set clears the source, so the default
  // (nothing greyed) shows no active filter. writeFilter skips unchanged sets.
  useEffect(() => {
    writeFilter(SOURCE.SETTINGS_FILTER, greyedNodeIds.size > 0 ? greyedNodeIds : null, 'exclude', mask);
  }, [greyedNodeIds, mask]);

  // --- hide filtered (opt-in): remove whatever any filter source greys ---
  const registryVersion = useSyncExternalStore(
    hideFiltered ? (cb) => subscribeFilterRegistry(cb, mask) : noSubscribe,
    () => (hideFiltered ? filterRegistryVersion(mask) : 0),
    () => 0,
  );
  const visibleNodes = useMemo(() => {
    if (!hideFiltered) return scopedNodes;
    // greyedNodeIds is checked directly so the first render already hides them,
    // before the effect above has written the mask.
    return scopedNodes.filter(n => {
      const id = String(n.id);
      return !greyedNodeIds.has(id) && !isFilteredOut(id, mask);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopedNodes, hideFiltered, greyedNodeIds, registryVersion, mask]);

  // --- filteredEdges (pass-through today) ---
  const filteredEdges = graphData.edges;

  return {
    visibleNodes,
    greyedNodeIds,
    nodeIdToIndexMap,
    filteredEdges,
    expansionState,
  };
}
