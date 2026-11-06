/**
 * Whether the overview layers are actually mounted with a built material.
 *
 * Gems beyond gemLodDistance (GemNodes) and glass tubes beyond close zoom
 * (GlassEdges' tube LOD) are hidden ONLY while the sprites / overview edges
 * that replace them are live. While a material is still building (the TSL
 * path is async), or if it failed to build, the gems and tubes keep drawing
 * everything, so the graph never vanishes at fit.
 */
import { useSyncExternalStore } from 'react';

const state = { nodes: false, edges: false };
let snapshot = { ...state };
const listeners = new Set<() => void>();

export function setOverviewLive(layer: 'nodes' | 'edges', live: boolean): void {
  if (state[layer] === live) return;
  state[layer] = live;
  snapshot = { ...state };
  for (const l of listeners) l();
}

/** Frame-loop read (no React). */
export function isOverviewLive(layer: 'nodes' | 'edges'): boolean {
  return state[layer];
}

export function useOverviewLive(): { nodes: boolean; edges: boolean } {
  return useSyncExternalStore(
    (l) => { listeners.add(l); return () => { listeners.delete(l); }; },
    () => snapshot,
    () => snapshot,
  );
}
