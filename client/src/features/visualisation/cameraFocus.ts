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

/** Fractions of the canvas covered by overlays (0..1 of width / height). */
export interface ScreenInsets {
  right?: number;
  bottom?: number;
}

/**
 * A pose that frames `points` (world space). It keeps the camera's current
 * viewing direction, backs off until the points' bounding sphere (radius
 * floored at `minRadius`) fits the part of the screen that `insets` leave
 * free, with 25% padding, and shifts sideways so the sphere's centre lands in
 * the middle of that free area. World up is +y, as for OrbitControls. Used to
 * frame a memory query's route when it is drawn.
 */
export function frameRoutePose(
  from: CameraPoseLike,
  points: ReadonlyArray<PoseVec>,
  fovDeg: number,
  aspect: number,
  minRadius = 1,
  insets: ScreenInsets = {},
): CameraPoseLike {
  if (points.length === 0) return { position: [...from.position], target: [...from.target] };
  const lo: PoseVec = [Infinity, Infinity, Infinity];
  const hi: PoseVec = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let a = 0; a < 3; a++) {
      lo[a] = Math.min(lo[a], p[a]);
      hi[a] = Math.max(hi[a], p[a]);
    }
  }
  const centre: PoseVec = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
  let radius = minRadius;
  for (const p of points) radius = Math.max(radius, Math.hypot(p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]));

  const right = Math.min(0.8, Math.max(0, insets.right ?? 0));
  const bottom = Math.min(0.8, Math.max(0, insets.bottom ?? 0));
  const tanV = Math.tan((fovDeg * Math.PI) / 360);
  const tanH = tanV * (aspect > 0 ? aspect : 1);
  // half-angles of the free area, which spans 1 - inset of each NDC half-axis
  const halfV = Math.atan(tanV * (1 - bottom));
  const halfH = Math.atan(tanH * (1 - right));
  const distance = (radius / Math.sin(Math.min(halfV, halfH))) * 1.25;

  let dir: PoseVec = [
    from.position[0] - from.target[0],
    from.position[1] - from.target[1],
    from.position[2] - from.target[2],
  ];
  let n = Math.hypot(dir[0], dir[1], dir[2]);
  if (n < 1e-6) {
    dir = [0, 0.3, 1];
    n = Math.hypot(0, 0.3, 1);
  }
  dir = [dir[0] / n, dir[1] / n, dir[2] / n];

  // camera basis: forward = -dir, right = forward x up(+y), up' = right x forward
  const fw: PoseVec = [-dir[0], -dir[1], -dir[2]];
  let rx = -fw[2];
  let rz = fw[0];
  let rl = Math.hypot(rx, rz);
  if (rl < 1e-6) {
    rx = 1;
    rz = 0;
    rl = 1;
  }
  const r: PoseVec = [rx / rl, 0, rz / rl];
  const u: PoseVec = [r[1] * fw[2] - r[2] * fw[1], r[2] * fw[0] - r[0] * fw[2], r[0] * fw[1] - r[1] * fw[0]];

  // move the target so the centre projects to the free area's centre: NDC (-right, +bottom)
  const sx = right * distance * tanH;
  const sy = -bottom * distance * tanV;
  const target: PoseVec = [
    centre[0] + r[0] * sx + u[0] * sy,
    centre[1] + r[1] * sx + u[1] * sy,
    centre[2] + r[2] * sx + u[2] * sy,
  ];
  return {
    position: [target[0] + dir[0] * distance, target[1] + dir[1] * distance, target[2] + dir[2] * distance],
    target,
  };
}
