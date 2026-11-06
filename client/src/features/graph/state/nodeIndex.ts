/**
 * The node render index shared by the graph-view contracts (ADR-2183, ADR-2184).
 *
 * There is ONE node ordering on the desktop client: the index of a node in
 * `graphData.nodes`, built by `useGraphFiltering` over the full delivered set
 * (not the filtered `visibleNodes`) and published by GraphManager as
 * `sharedNodeIdToIndexMap`. The drawn-positions array, the analytics buffer
 * (`nodeAnalyticsStore.getIndexedBuffer`) and GemNodes' position lookups are all
 * indexed by it. The highlight mask and the encoding buffers use the same index,
 * so a consumer reads slot `i` of every buffer for the same node.
 *
 * Nodes the server withheld (visibility filter, population scope) are absent from
 * the map, so they have no slot in any buffer (ADR-2183 D4).
 */
import { sharedNodeIdToIndexMap } from '../contexts/NodePositionContext';

/** Returns the current id → render-index map. */
export type NodeIndexResolver = () => ReadonlyMap<string, number>;

/**
 * The default resolver: reads GraphManager's published map through the ES module
 * live binding, so a rebuilt map (new graph load) is seen without a subscription.
 */
export const sharedNodeIndexResolver: NodeIndexResolver = () => sharedNodeIdToIndexMap;

/** Buffer length needed for a map: one past the highest index (0 for an empty map). */
export function slotCountFor(map: ReadonlyMap<string, number>): number {
  let max = -1;
  for (const idx of map.values()) if (idx > max) max = idx;
  return max + 1;
}
