/**
 * cameraFocus — click-to-fly focus bridge (LANE D1).
 *
 * A tiny, coordinate-free module that lets HTML panels outside the R3F Canvas
 * (e.g. the live agent-action transcript in ActivityLogPanel) fly the camera to
 * a graph node and highlight it, without importing any Three.js / graph
 * internals.
 *
 * It reuses the codebase's established focus utility: the `visionclaw:search`
 * CustomEvent already consumed by useGraphSelection, which sets the selection
 * (a pulsing wave-scale highlight in GemNodes + highlight edges) and eases the
 * camera along `flyToTargetRef`. Producers already using that contract:
 * CommandInput, NodeDetailPanel, OntologyExplorationControls. This module adds
 * the transcript as one more producer, keyed by a numeric wire id.
 *
 * Two concerns live here:
 *   1. `focusNodeById(id)` — mask the wire id and dispatch the focus event.
 *   2. `resolveNodeWorldPosition(...)` — the pure id-masking + SAB position
 *      lookup that GraphManager uses for beam targets, extracted so it can be
 *      shared (beam resolver) and unit-tested in isolation.
 *
 * Node-id spaces reconcile via `getActualNodeId`: the wire `targetNodeId` may
 * carry the high type-flag bits (AGENT / KNOWLEDGE / ontology, bits 26-31),
 * whereas `graphData.nodes` / `nodeIdToIndexMap` are keyed by the bare base id.
 */

import { getActualNodeId } from '@/types/binaryProtocol';

/**
 * The established focus event name. useGraphSelection listens for this and,
 * given `{ nodeId }`, selects the node (pulse highlight) and starts the fly-to.
 */
export const CAMERA_FOCUS_EVENT = 'visionclaw:search';

/** Detail payload for the focus event (mirrors useGraphSelection's reader). */
export interface CameraFocusDetail {
  /** Bare (masked) node id, stringified to match `String(node.id)`. */
  nodeId: string;
  /** Free-text label fallback the search handler can resolve by name. */
  query?: string;
}

/**
 * Resolve a wire id to its index in the flat SAB position buffer.
 *
 * Tries the raw id first (KG nodes may already be keyed raw), then the masked
 * id (strip AGENT / KNOWLEDGE / ontology flag bits). Mirrors GraphManager's
 * beam `resolveNodePosition` exactly so both paths agree.
 *
 * @returns the instance index, or `undefined` when the id is unknown.
 */
export function resolveNodeIndex(
  id: number,
  nodeIdToIndexMap: Map<string, number>,
): number | undefined {
  let index = nodeIdToIndexMap.get(String(id));
  if (index === undefined) index = nodeIdToIndexMap.get(String(getActualNodeId(id)));
  return index;
}

/**
 * Resolve a wire id to a world position from the live SAB position buffer.
 *
 * Pure: no Three.js, no DOM, no globals. Returns a plain `{x,y,z}` (or `null`
 * when unresolvable — no buffer, unknown id, or index out of range) so the
 * caller decides how to represent it (Vector3, event detail, …).
 */
export function resolveNodeWorldPosition(
  id: number,
  nodeIdToIndexMap: Map<string, number>,
  positions: Float32Array | null | undefined,
): { x: number; y: number; z: number } | null {
  if (!positions) return null;
  const index = resolveNodeIndex(id, nodeIdToIndexMap);
  if (index === undefined) return null;
  const i3 = index * 3;
  if (i3 < 0 || i3 + 2 >= positions.length) return null;
  return { x: positions[i3], y: positions[i3 + 1], z: positions[i3 + 2] };
}

/**
 * Fly the camera to a node and highlight it, by wire id.
 *
 * Masks the id to its base form and dispatches the established focus event so
 * the in-Canvas graph (useGraphSelection) performs the eased fly-to + pulse
 * selection. Silent no-op outside a browser (SSR / test without a window).
 *
 * The graph resolves the target itself; an id with no matching node simply does
 * nothing (the handler returns early), so callers need not pre-check
 * resolvability.
 *
 * @returns `true` when the event was dispatched, `false` when there is no window.
 */
export function focusNodeById(id: number): boolean {
  if (typeof window === 'undefined' || typeof window.dispatchEvent !== 'function') {
    return false;
  }
  const detail: CameraFocusDetail = { nodeId: String(getActualNodeId(id)) };
  window.dispatchEvent(new CustomEvent<CameraFocusDetail>(CAMERA_FOCUS_EVENT, { detail }));
  return true;
}

// ── Memory cloud focus ──────────────────────────────────────────────────────
//
// The memory explorer's result list sits outside the Canvas too, but its
// targets are rows of the memory-cloud sample rather than graph nodes, so they
// travel on their own event. The cloud layer (inside the Canvas) owns the
// cloud's world transform and performs the fly-to with `flyPose`.

/** Event the memory cloud layer listens for; detail `{ sampleIndex }`. */
export const MEMORY_FOCUS_EVENT = 'visionclaw:memory-focus';

export interface MemoryFocusDetail {
  /** row in the current memory-cloud snapshot */
  sampleIndex: number;
}

/**
 * Fly the camera to a memory-cloud point by snapshot row. Returns false
 * outside a browser or for an invalid row.
 */
export function focusMemoryPoint(sampleIndex: number): boolean {
  if (!Number.isInteger(sampleIndex) || sampleIndex < 0) return false;
  if (typeof window === 'undefined' || typeof window.dispatchEvent !== 'function') return false;
  window.dispatchEvent(new CustomEvent<MemoryFocusDetail>(MEMORY_FOCUS_EVENT, { detail: { sampleIndex } }));
  return true;
}

export type PoseVec = [number, number, number];

export interface CameraPoseLike {
  position: PoseVec;
  target: PoseVec;
}

/** Camera pose `u` ∈ [0, 1] of the way from `from` to `to`, eased in and out. */
export function flyPose(from: CameraPoseLike, to: CameraPoseLike, u: number): CameraPoseLike {
  const t = u <= 0 ? 0 : u >= 1 ? 1 : u < 0.5 ? 4 * u * u * u : 1 - Math.pow(-2 * u + 2, 3) / 2;
  if (t === 0) return { position: [...from.position], target: [...from.target] };
  if (t === 1) return { position: [...to.position], target: [...to.target] };
  const mix = (a: PoseVec, b: PoseVec): PoseVec => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
  return { position: mix(from.position, to.position), target: mix(from.target, to.target) };
}
