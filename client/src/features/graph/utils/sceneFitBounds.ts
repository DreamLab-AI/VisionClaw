/**
 * What the desktop camera auto-fit frames (ADR-2135).
 *
 * Merged (Graph Separation 0) it frames the graph's robust bounds, exactly as
 * before. Separated, the graph broadcast spans only the knowledge and ontology
 * vertices, and the memory cloud sits at the third vertex behind them, so a
 * graph-only fit leaves the cloud at the edge of the frame or outside it. The
 * fit then frames the sphere enclosing all three bodies: each graph is one
 * graph's folded extent (`graphBoundsFor`) placed at its vertex, and the cloud
 * is where and as large as `cloudPlacement` puts it.
 *
 * The fit runs once the slider settles (`createSettleTrigger`), never on every
 * tick of a drag.
 */

import { robustBounds, type RobustBounds } from '../../../utils/robustBounds';
import { triangleFrame, place, Vertex, type Vec3 } from '../triLayout';
import { cloudPlacement, graphBoundsFor } from '../../visualisation/memoryCloud/cloudFrame';

/** quiet time after the last slider change before the camera refits */
export const SEPARATION_SETTLE_MS = 900;

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
  separation: number,
  cloud: CloudFitInput | null,
): RobustBounds | null {
  const frame = triangleFrame(separation);
  if (frame.radius === 0) return robustBounds(positions, count);
  const local = graphBoundsFor(positions, count, separation);
  if (!local) return null;
  const spheres: Sphere[] = [Vertex.Knowledge, Vertex.Ontology].map((v) => ({
    c: place(frame, v, local.centre),
    r: local.radius,
  }));
  if (cloud) {
    const p = cloudPlacement(cloud.bounds, local, cloud.cloudScale, separation);
    spheres.push({ c: p.position, r: p.scale * cloud.bounds.radius });
  }
  return enclose(spheres);
}

/**
 * Fires once when a value has stayed unchanged for `delayMs` after a change.
 * The first value seen is the baseline, not a change.
 */
export function createSettleTrigger(delayMs: number) {
  let last: number | undefined;
  let changedAt = 0;
  let armed = false;
  return {
    update(value: number, nowMs: number): boolean {
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
