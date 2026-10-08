/**
 * Where the memory cloud sits in the scene, and the sprite its points use.
 *
 * The snapshot's PCA coordinates span roughly ±100 around the sample mean,
 * but the dense core is skewed off that mean, and the graph lives wherever
 * its layout put it (here centred near (90, −3, −14), radius ≈ 300). Placing
 * the cloud at the world origin at a fixed ×5 therefore left it beside the
 * graph, partly behind the explorer panel, at about twice the graph's size.
 *
 * Instead the cloud is framed on the graph: an outer group sits at the
 * graph's robust centre and scales the cloud's robust radius to the graph's,
 * and an inner group shifts the cloud so its own robust centre is the pivot
 * (rotation turns the core in place). `cloudScale` keeps its meaning as the
 * size knob: the default 5 makes the two radii equal, and it scales linearly
 * from there.
 *
 * The layout is always separated (ADR-2135, operator decision 2026-10-08):
 * the knowledge graph and ontology sit on two vertices of a ground-plane
 * triangle and the cloud takes the third (`triLayout.ts`, the memory vertex
 * at the back). The graph extent is measured on positions folded back into
 * each graph's own frame (`graphBoundsFor`), so it is one graph's. The cloud
 * is drawn MEMORY_BODY_SCALE (10) times that size, and the outer group sits
 * on the memory vertex's ray far enough out that the enlarged cloud clears
 * both graphs (`memoryCentre`). The route, beads and rings live inside the
 * groups, so they grow with the cloud.
 */

import { robustBounds, type RobustBounds } from '@/utils/robustBounds';
import { separatedFrame, foldPositions, memoryCentre, LIVE_GRAPH_RADIUS, MEMORY_BODY_SCALE } from '../../graph/triLayout';

export type Vec3 = [number, number, number];

/** settings default for `embeddingCloud.cloudScale`; this value draws the cloud MEMORY_BODY_SCALE graphs wide */
export const DEFAULT_CLOUD_SCALE = 5;

export interface CloudPlacement {
  /** outer group position: the graph's centre (world) */
  position: Vec3;
  /** outer group uniform scale */
  scale: number;
  /** inner group position: minus the cloud's own centre (cloud-local) */
  offset: Vec3;
}

/**
 * Placement of the cloud given its own bounds (cloud-local) and the graph's
 * (world, measured with `graphBoundsFor`). At the default `cloudScale` the
 * cloud's radius is MEMORY_BODY_SCALE times the graph's, linear from there.
 * Without a graph the cloud keeps the old size rule (`cloudScale` times
 * MEMORY_BODY_SCALE) and clears a live-sized graph. Either way it sits at
 * `memoryCentre`: on the memory vertex's ray, clear of both graphs.
 */
export function cloudPlacement(
  cloud: RobustBounds | null,
  graph: RobustBounds | null,
  cloudScale: number,
): CloudPlacement {
  const k = Number.isFinite(cloudScale) ? Math.max(0.1, cloudScale) : DEFAULT_CLOUD_SCALE;
  const offset: Vec3 = cloud ? [-cloud.centre[0], -cloud.centre[1], -cloud.centre[2]] : [0, 0, 0];
  const framed = cloud !== null && graph !== null;
  const scale = framed ? MEMORY_BODY_SCALE * (k / DEFAULT_CLOUD_SCALE) * (graph.radius / cloud.radius) : MEMORY_BODY_SCALE * k;
  const m = memoryCentre(separatedFrame(), graph ? graph.radius : LIVE_GRAPH_RADIUS, cloud ? scale * cloud.radius : 0);
  const base: Vec3 = framed ? graph.centre : [0, 0, 0];
  return { position: [base[0] + m[0], base[1] + m[1], base[2] + m[2]], scale, offset };
}

/**
 * Robust bounds of the graph's broadcast positions with the separated
 * layout folded out: each position is mapped back into its graph's frame
 * (nearest of the knowledge and ontology vertices), so the result is one
 * graph's centre and radius.
 */
export function graphBoundsFor(positions: ArrayLike<number>, count: number): RobustBounds | null {
  return robustBounds(foldPositions(separatedFrame(), positions, count), count);
}

/**
 * World point size for the cloud's points. `sizeAttenuation` ignores object
 * scale, so a fixed size would turn into blobs as the cloud shrinks to fit a
 * small graph. Scaling it with the placement keeps the `pointSize` setting's
 * look relative to the cloud's spread (the old fixed placement was ×5).
 */
export function cloudPointSize(pointSize: number, placementScale: number): number {
  return Math.max(0.5, (pointSize * placementScale) / DEFAULT_CLOUD_SCALE);
}

/** smallest on-screen point diameter, CSS pixels, however far the camera is */
export const MIN_POINT_PX = 2;

/**
 * World size the cloud's points draw at this frame. Points are
 * size-attenuated (on screen: size × halfHeightPx / distance), so they keep
 * their size relative to the cloud (`cloudPointSize`: the `pointSize`
 * setting, scaled with the placement) while the camera is near; when the
 * camera is far enough that a point would drop under MIN_POINT_PX, the size
 * grows to hold that floor, so a zoomed-out cloud still reads as a field
 * rather than vanishing. `distance` is camera to cloud centre, `halfHeightPx`
 * half the canvas height in CSS pixels; bad values skip the floor.
 */
export function cloudPointWorldSize(
  pointSize: number,
  placementScale: number,
  distance: number,
  halfHeightPx: number,
): number {
  const world = cloudPointSize(pointSize, placementScale);
  if (!(distance > 0) || !(halfHeightPx > 0) || !Number.isFinite(distance) || !Number.isFinite(halfHeightPx)) return world;
  return Math.max(world, (MIN_POINT_PX * distance) / halfHeightPx);
}

/**
 * RGBA pixels of a round point sprite: white, so `vertexColors` pass through
 * unchanged, with alpha 1 inside 70% of the radius falling smoothly to 0 at
 * the rim. Used as the points material's `map`, which turns the default
 * square points into discs on both the WebGL and WebGPU renderers.
 */
export function discSpritePixels(size = 64): Uint8Array {
  const px = new Uint8Array(size * size * 4);
  const c = (size - 1) / 2;
  const inner = 0.7;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const r = Math.hypot(x - c, y - c) / (size / 2);
      const t = Math.min(1, Math.max(0, (r - inner) / (1 - inner)));
      const a = r >= 1 ? 0 : 1 - t * t * (3 - 2 * t);
      const i = (y * size + x) * 4;
      px[i] = px[i + 1] = px[i + 2] = 255;
      px[i + 3] = Math.round(a * 255);
    }
  }
  return px;
}

export interface RouteFrameInput {
  /** store `query.seq`: bumps on every completed query */
  querySeq: number;
  /** store `query.morphSeq`: bumps on every view change */
  morphSeq: number;
  hasRun: boolean;
  /** `routeChannel.seq`: bumps whenever TrajectoryLayer publishes a route */
  routeSeq: number;
  routeLength: number;
}

/**
 * Decides when the camera frames the route: once per completed query and
 * once per view change, and only after TrajectoryLayer has published the
 * route for that state (the channel lags the store by a frame or more, and
 * holds the old route until then). Between those moments the user orbits
 * freely.
 */
export function createRouteFramer() {
  let key = '';
  let seqAtKey = -1;
  let done = true;
  /** channel seq seen on the previous call; null before the first */
  let prevSeq: number | null = null;
  return {
    update(i: RouteFrameInput): boolean {
      const before = prevSeq;
      prevSeq = i.routeSeq;
      if (!i.hasRun) {
        key = '';
        done = true;
        return false;
      }
      const k = `${i.querySeq}|${i.morphSeq}`;
      if (k !== key) {
        key = k;
        // TrajectoryLayer's frame callback runs before the rig's, so the new
        // route may already be on the channel: compare against the seq from
        // the previous frame, not this one.
        seqAtKey = before ?? i.routeSeq;
        done = false;
      }
      if (done || i.routeSeq === seqAtKey || i.routeLength < 2) return false;
      done = true;
      return true;
    },
  };
}
