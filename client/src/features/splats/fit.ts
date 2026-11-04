import type { SplatBounds, Vec3 } from './types';

/** A bounding sphere in scene units. */
export interface Sphere {
  centre: Vec3;
  radius: number;
}

/** Where a splat cloud sits: its bounds-centre lands at the target, uniformly scaled. */
export interface Placement {
  /** position of the layer root (the asset's origin, not its centre) */
  position: Vec3;
  scale: number;
}

/** how the viewer sees the cloud: a framed volume beside the graph, or the room around it */
export type SplatMode = 'framed' | 'entered';

/** Framed volume radius as a fraction of the graph's bounding radius. */
export const FRAMED_RADIUS_FACTOR = 0.5;
/** Clearance between the framed volume and the graph, as a multiple of the two radii. */
export const FRAMED_CLEARANCE = 1.25;
/**
 * Entered backdrop radius as a multiple of the graph's: the graph then sits
 * inside the cloud, the desktop's counterpart of the ADR-2166 room-scale fit.
 */
export const ENTERED_RADIUS_FACTOR = 3;
/** Used when no graph is loaded yet. */
export const DEFAULT_GRAPH: Sphere = { centre: [0, 0, 0], radius: 50 };

/**
 * Entered clouds draw below every graph transparent layer (hulls 1, rings 4,
 * labels 10) so they act as a backdrop (ADR-2170 decision 2). Framed clouds are
 * disjoint from the graph, so they keep the default order.
 */
export const BACKDROP_RENDER_ORDER = -1000;
export const FRAMED_RENDER_ORDER = 0;

/** Smallest enclosing sphere of the box centred on it: the half-diagonal. */
export function boundsSphere(b: SplatBounds): Sphere {
  const centre: Vec3 = [(b.min[0] + b.max[0]) / 2, (b.min[1] + b.max[1]) / 2, (b.min[2] + b.max[2]) / 2];
  const radius = Math.hypot(b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]) / 2;
  return { centre, radius: Math.max(radius, 1e-6) };
}

/** Uniformly scale the bounds' sphere onto `target`. */
export function fitPlacement(bounds: SplatBounds, target: Sphere): Placement {
  const src = boundsSphere(bounds);
  const scale = target.radius / src.radius;
  return {
    scale,
    position: [
      target.centre[0] - src.centre[0] * scale,
      target.centre[1] - src.centre[1] * scale,
      target.centre[2] - src.centre[2] * scale,
    ],
  };
}

/**
 * Un-entered: a sphere whose radius follows the graph's and whose centre sits
 * on +X past the graph's sphere, so the two boxes cannot intersect (the
 * sphere bounds both).
 */
export function framedPlacement(bounds: SplatBounds, graph: Sphere = DEFAULT_GRAPH): Placement {
  const radius = Math.max(graph.radius * FRAMED_RADIUS_FACTOR, 1);
  const gap = (graph.radius + radius) * FRAMED_CLEARANCE;
  return fitPlacement(bounds, {
    radius,
    centre: [graph.centre[0] + gap, graph.centre[1], graph.centre[2]],
  });
}

/** Entered: the backdrop, centred on the graph and enclosing it. */
export function enteredPlacement(bounds: SplatBounds, graph: Sphere = DEFAULT_GRAPH): Placement {
  return fitPlacement(bounds, { radius: Math.max(graph.radius * ENTERED_RADIUS_FACTOR, 1), centre: graph.centre });
}

export function placementFor(mode: SplatMode, bounds: SplatBounds, graph: Sphere | null): Placement & { renderOrder: number } {
  const g = graph ?? DEFAULT_GRAPH;
  return mode === 'entered'
    ? { ...enteredPlacement(bounds, g), renderOrder: BACKDROP_RENDER_ORDER }
    : { ...framedPlacement(bounds, g), renderOrder: FRAMED_RENDER_ORDER };
}
