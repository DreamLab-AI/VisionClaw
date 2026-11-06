/**
 * Whether the overview layers are actually mounted with a built material, and
 * whether one of them has faulted.
 *
 * Gems beyond gemLodDistance (GemNodes) and glass tubes beyond close zoom (the
 * tube subset, overview/tubeSelect.ts) are hidden ONLY while the sprites /
 * overview edges that replace them are live. While a material is still
 * building (TSL materials build asynchronously), or once a layer has faulted
 * (its material failed to build, the renderObject guard caught a throwing
 * draw, or its shader failed on the GPU: useRenderFaultWatch), the gems and
 * tubes draw everything again, so the graph never vanishes at fit, and
 * OverviewEdgeNotice says why.
 *
 * `labels` has no gem fallback: a failed glyph material hides the labels and
 * the notice says so (ADR-2186 D4, one label path).
 *
 * A layer is also not live until the node position texture has received real
 * positions (`setOverviewPositions`, from OverviewLayer's first upload). Before
 * the position stream delivers, or without one, the texture is all zeros and the
 * sprites and edges would collapse onto the origin while the gems are hidden.
 *
 * A fault is sticky until the overview layer unmounts (clearOverviewFaults):
 * retrying a broken pipeline every frame would only throw again.
 */
import { useSyncExternalStore } from 'react';

export type OverviewLayerName = 'nodes' | 'edges' | 'inferred' | 'labels';

export interface LiveState {
  nodes: boolean;
  edges: boolean;
  /** Fault text per layer; absent = healthy. */
  faults: Partial<Record<OverviewLayerName, string>>;
}

const mounted = { nodes: false, edges: false };
/** The position texture has had at least one upload of drawn positions. */
let positionsReady = false;
const faults: Partial<Record<OverviewLayerName, string>> = {};
let snapshot: LiveState = { nodes: false, edges: false, faults: {} };
const listeners = new Set<() => void>();

function publish(): void {
  snapshot = {
    nodes: positionsReady && mounted.nodes && !faults.nodes,
    edges: positionsReady && mounted.edges && !faults.edges,
    faults: { ...faults },
  };
  for (const l of listeners) l();
}

export function setOverviewLive(layer: 'nodes' | 'edges', live: boolean): void {
  if (mounted[layer] === live) return;
  mounted[layer] = live;
  publish();
}

/** OverviewLayer: true once drawn positions reached the texture; false on unmount. */
export function setOverviewPositions(ready: boolean): void {
  if (positionsReady === ready) return;
  positionsReady = ready;
  publish();
}

/** Frame-loop read: whether the position texture holds real positions yet. */
export function overviewPositionsReady(): boolean {
  return positionsReady;
}

/** Record a fault for a layer (first one wins). */
export function markOverviewFault(layer: OverviewLayerName, reason: string): void {
  if (faults[layer]) return;
  faults[layer] = reason || 'render fault';
  publish();
}

/** An `onError` for useOverviewMaterial / the glyph material: a failed build faults the layer. */
export function buildFault(layer: OverviewLayerName): (err: unknown) => void {
  return (err) => markOverviewFault(layer, err instanceof Error ? err.message : String(err));
}

export function isOverviewFaulted(layer: OverviewLayerName): boolean {
  return faults[layer] !== undefined;
}

export function clearOverviewFaults(): void {
  let had = false;
  for (const k of Object.keys(faults) as OverviewLayerName[]) { delete faults[k]; had = true; }
  if (had) publish();
}

/** Frame-loop read (no React): mounted with a material and not faulted. */
export function isOverviewLive(layer: 'nodes' | 'edges'): boolean {
  return positionsReady && mounted[layer] && !faults[layer];
}

export function useOverviewLive(): LiveState {
  return useSyncExternalStore(
    (l) => { listeners.add(l); return () => { listeners.delete(l); }; },
    () => snapshot,
    () => snapshot,
  );
}

/** One line for OverviewEdgeNotice, or null while every layer is healthy. */
export function overviewFaultNoticeText(f: LiveState['faults']): string | null {
  const names: Record<OverviewLayerName, string> = { nodes: 'node sprites', edges: 'overview edges', inferred: 'inferred edges', labels: 'labels' };
  const bad = (Object.keys(f) as OverviewLayerName[]).filter((k) => f[k]);
  if (bad.length === 0) return null;
  const fallback = bad.some((k) => k === 'nodes' || k === 'edges') ? '; drawing gems and glass tubes instead' : '';
  return `Overview ${bad.map((k) => names[k]).join(' and ')} failed on this GPU${fallback}`;
}
