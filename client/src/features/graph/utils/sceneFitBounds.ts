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
