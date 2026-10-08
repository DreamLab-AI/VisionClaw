/**
 * What the desktop camera auto-fit frames (ADR-2135).
 *
 * The layout is always separated: the graph broadcast spans the knowledge and
 * ontology vertices, and the memory cloud, ten graphs wide, sits behind them
 * on the memory vertex's ray, so a graph-only fit leaves the cloud out of the
 * frame. The fit frames the sphere enclosing all three bodies: each graph is
 * one graph's folded extent (`graphBoundsFor`) placed at its vertex, and the
 * cloud is where and as large as `cloudPlacement` puts it.
 *
 * Besides the first-data fit and explicit requests, the camera refits once
 * when the cloud's framing input settles (`createSettleTrigger` on
 * `cloudFitKey`): the cloud turning on, a new snapshot, or a new cloudScale.
 */

import { type RobustBounds } from '../../../utils/robustBounds';
import { separatedFrame, place, Vertex, type Vec3 } from '../triLayout';
import { cloudPlacement, graphBoundsFor } from '../../visualisation/memoryCloud/cloudFrame';

/** quiet time after the cloud's framing input changes before the camera refits */
export const CLOUD_SETTLE_MS = 900;

export interface CloudFitInput {
  /** the cloud's own robust bounds (cloud-local), from its snapshot */
  bounds: RobustBounds;
  /** `embeddingCloud.cloudScale` */
  cloudScale: number;
}

interface Sphere {
  c: Vec3;
  r: number;
}

/** smallest-box-centred sphere enclosing every sphere */
function enclose(spheres: Sphere[]): RobustBounds {
  const lo: Vec3 = [Infinity, Infinity, Infinity];
  const hi: Vec3 = [-Infinity, -Infinity, -Infinity];
  for (const s of spheres) {
    for (let k = 0; k < 3; k++) {
      lo[k] = Math.min(lo[k], s.c[k] - s.r);
      hi[k] = Math.max(hi[k], s.c[k] + s.r);
    }
  }
  const centre: [number, number, number] = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
  let radius = 1;
  for (const s of spheres) {
    radius = Math.max(radius, Math.hypot(s.c[0] - centre[0], s.c[1] - centre[1], s.c[2] - centre[2]) + s.r);
  }
  return { centre, radius };
}

/**
 * Bounds the camera should frame. `cloud` is null when the memory cloud is
 * off or has no snapshot yet; the fit then covers the two graphs only.
 */
export function sceneFitBounds(
  positions: ArrayLike<number>,
  count: number,
  cloud: CloudFitInput | null,
): RobustBounds | null {
  const frame = separatedFrame();
  const local = graphBoundsFor(positions, count);
  if (!local) return null;
  const spheres: Sphere[] = [Vertex.Knowledge, Vertex.Ontology].map((v) => ({
    c: place(frame, v, local.centre),
    r: local.radius,
  }));
  if (cloud) {
    const p = cloudPlacement(cloud.bounds, local, cloud.cloudScale);
    spheres.push({ c: p.position, r: p.scale * cloud.bounds.radius });
  }
  return enclose(spheres);
}

/**
 * What the cloud contributes to the fit, as one comparable key: empty while
 * the cloud is off or has no snapshot, else its snapshot and scale.
 */
export function cloudFitKey(enabled: boolean, snapshotId: string | null | undefined, cloudScale: number): string {
  return enabled && snapshotId ? `${snapshotId}|${cloudScale}` : '';
}

/**
 * Fires once when a value has stayed unchanged for `delayMs` after a change.
 * The first value seen is the baseline, not a change.
 */
export function createSettleTrigger(delayMs: number) {
  let last: number | string | undefined;
  let changedAt = 0;
  let armed = false;
  return {
    update(value: number | string, nowMs: number): boolean {
      if (last === undefined) {
        last = value;
        return false;
      }
      if (value !== last) {
        last = value;
        changedAt = nowMs;
        armed = true;
        return false;
      }
      if (armed && nowMs - changedAt >= delayMs) {
        armed = false;
        return true;
      }
      return false;
    },
  };
}

/** a canvas-relative rectangle in CSS pixels */
export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * The largest axis-aligned rectangle of the view that no occluder covers
 * (the memory explorer panel, the control dock). Occluders are clipped to
 * the view; empty ones are ignored. Candidates are every combination of the
 * view's and the occluders' edges, so a handful of overlays costs nothing.
 */
export function unoccludedRect(view: { width: number; height: number }, occluders: Rect[]): Rect {
  const W = view.width;
  const H = view.height;
  const occ = occluders
    .map((o) => ({
      left: Math.max(0, o.left),
      top: Math.max(0, o.top),
      right: Math.min(W, o.right),
      bottom: Math.min(H, o.bottom),
    }))
    .filter((o) => o.right > o.left && o.bottom > o.top);
  const xs = [...new Set([0, W, ...occ.flatMap((o) => [o.left, o.right])])].sort((a, b) => a - b);
  const ys = [...new Set([0, H, ...occ.flatMap((o) => [o.top, o.bottom])])].sort((a, b) => a - b);
  let best: Rect = { left: 0, top: 0, right: W, bottom: H };
  let bestArea = occ.length === 0 ? W * H : -1;
  if (occ.length === 0) return best;
  for (let a = 0; a < xs.length; a++) {
    for (let b = a + 1; b < xs.length; b++) {
      for (let c = 0; c < ys.length; c++) {
        for (let d = c + 1; d < ys.length; d++) {
          const r = { left: xs[a], right: xs[b], top: ys[c], bottom: ys[d] };
          const area = (r.right - r.left) * (r.bottom - r.top);
          if (area <= bestArea) continue;
          const hit = occ.some((o) => o.left < r.right && o.right > r.left && o.top < r.bottom && o.bottom > r.top);
          if (!hit) {
            best = r;
            bestArea = area;
          }
        }
      }
    }
  }
  return best;
}

/** where the fit puts the camera and the orbit target */
export interface FitPose {
  position: Vec3;
  target: Vec3;
}

/** view direction of the fit, target → camera: a slight elevation, from +Z */
const FIT_DIR = (() => {
  const l = Math.hypot(0, 0.3, 1);
  return [0, 0.3 / l, 1 / l] as Vec3;
})();

/** margin around the fitted sphere */
export const FIT_PADDING = 1.15;

/**
 * Camera pose that puts the sphere `bounds` inside `free`, the part of the
 * view no overlay covers, seen along the usual elevated view from +Z.
 *
 * In camera space the free rectangle is a sub-frustum with side planes at
 * x/−z ∈ [a0, a1] and y/−z ∈ [b0, b1]. A sphere of radius r at depth d clears
 * the left and right planes when (a1 − a0)·d ≥ r(√(1 + a0²) + √(1 + a1²)),
 * and likewise vertically, so the nearest depth that fits is the larger of
 * the two; the centre then goes midway between the bounds the planes leave.
 * The orbit target sits on the view axis at the sphere's depth, so orbiting
 * turns about a point beside the scene rather than re-centring it under the
 * panel.
 */
export function fitPose(
  bounds: RobustBounds,
  cam: { fovDeg: number; aspect: number },
  view: { width: number; height: number },
  free: Rect,
  padding = FIT_PADDING,
): FitPose {
  const r = Math.max(1, bounds.radius) * padding;
  const tanV = Math.tan((cam.fovDeg * Math.PI) / 360);
  const tanH = tanV * (cam.aspect > 0 ? cam.aspect : 1);
  const W = view.width > 0 ? view.width : 1;
  const H = view.height > 0 ? view.height : 1;
  const ok = free.right - free.left > 1 && free.bottom - free.top > 1;
  const f = ok ? free : { left: 0, top: 0, right: W, bottom: H };
  // NDC → tangent of the view angle
  const a0 = ((2 * f.left) / W - 1) * tanH;
  const a1 = ((2 * f.right) / W - 1) * tanH;
  const b0 = (1 - (2 * f.bottom) / H) * tanV;
  const b1 = (1 - (2 * f.top) / H) * tanV;
  const sa0 = Math.hypot(1, a0);
  const sa1 = Math.hypot(1, a1);
  const sb0 = Math.hypot(1, b0);
  const sb1 = Math.hypot(1, b1);
  const d = Math.max((r * (sa0 + sa1)) / (a1 - a0), (r * (sb0 + sb1)) / (b1 - b0));
  // camera-space centre: midway between what the planes allow at depth d
  const x = (a0 * d + r * sa0 + (a1 * d - r * sa1)) / 2;
  const y = (b0 * d + r * sb0 + (b1 * d - r * sb1)) / 2;
  // camera basis, as Object3D.lookAt builds it for a camera with +Y up
  const back = FIT_DIR; // camera +Z (towards the viewer)
  const right: Vec3 = [back[2], 0, -back[0]]; // normalize(up × back) with up = +Y
  const rl = Math.hypot(...right) || 1;
  right[0] /= rl;
  right[2] /= rl;
  const up: Vec3 = [
    back[1] * right[2] - back[2] * right[1],
    back[2] * right[0] - back[0] * right[2],
    back[0] * right[1] - back[1] * right[0],
  ];
  // camera = centre − (x·right + y·up − d·back)
  const c = bounds.centre;
  const position: Vec3 = [0, 1, 2].map((k) => c[k] - x * right[k] - y * up[k] + d * back[k]) as Vec3;
  const target: Vec3 = [0, 1, 2].map((k) => position[k] - d * back[k]) as Vec3;
  return { position, target };
}
