/**
 * The sidecar top-k route on the desktop explorer (ADR-2136): the same
 * geometry and wording as the headset's (xr-client `memory_query::caption`,
 * `memory_route::agreement_line`), so the two clients describe one answer
 * the same way.
 *
 * The server projects the query vector and every hit's own embedding into
 * the snapshot's cloud coordinates with the snapshot's PCA basis and scale.
 * A hit inside the 6,000-row sample therefore lands exactly on its sampled
 * point; one outside the sample still has a place in the cloud, and is drawn
 * as a ghost. The route runs from the query point through every placed hit
 * in rank order. It shows the sidecar's ranking, not a path the search took.
 *
 * Older servers send neither position: the route then falls back to the
 * sampled rows, from the query point when there is one.
 */
import type { MemoryCloudHit, MemoryCloudQueryResponse } from './types';
import type { Vec3 } from '../memoryTrajectory/types';

/** One placed hit on the route. */
export interface RouteStop {
  /** 1-based sidecar rank */
  rank: number;
  pos: Vec3;
  /** the hit is a row of the current sample */
  sampled: boolean;
  sampleIndex: number | null;
}

export interface SidecarRoute {
  /** the query's point in the cloud, when the server sent one */
  origin: Vec3 | null;
  /** placed hits on the route, in rank order */
  stops: RouteStop[];
  /** hits drawn outside the sample (hollow, dimmer marks) */
  ghosts: RouteStop[];
  /** polyline: origin (if any) then the stops; empty when there is no route */
  points: Vec3[];
  total: number;
  inSample: number;
  /** hits placed by their server position (0 from an older server) */
  drawn: number;
  /** caption tail, worded as the headset's Query caption */
  caption: string;
}

const finite3 = (p: readonly number[] | null | undefined): p is [number, number, number] =>
  !!p && p.length === 3 && p.every((v) => Number.isFinite(v));

const rowPos = (i: number | null, positions: ArrayLike<number>): Vec3 | null =>
  i !== null && Number.isInteger(i) && i >= 0 && i * 3 + 2 < positions.length
    ? [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]]
    : null;

/**
 * Where a hit sits in the cloud: the position the server projected, else its
 * sampled row (an older server), else nowhere.
 */
export function placeHit(h: MemoryCloudHit, positions: ArrayLike<number>): Vec3 | null {
  if (finite3(h.position)) return [h.position[0], h.position[1], h.position[2]];
  return rowPos(h.sampleIndex, positions);
}

export function sidecarRoute(response: MemoryCloudQueryResponse | null, positions: ArrayLike<number>): SidecarRoute {
  const hits = response?.sidecar.results ?? [];
  const qp = response?.query.position;
  const origin: Vec3 | null = finite3(qp) ? [qp[0], qp[1], qp[2]] : null;
  // "in the sample" is the server's word (sampleIndex), as on the headset
  const inSample = hits.filter((h) => h.sampleIndex !== null && h.sampleIndex !== undefined).length;

  const placed: RouteStop[] = [];
  const sampledRows: RouteStop[] = [];
  hits.forEach((h, i) => {
    const row = rowPos(h.sampleIndex, positions);
    if (row) sampledRows.push({ rank: i + 1, pos: row, sampled: true, sampleIndex: h.sampleIndex });
    if (finite3(h.position)) {
      const sampled = h.sampleIndex !== null && h.sampleIndex !== undefined;
      placed.push({ rank: i + 1, pos: [h.position[0], h.position[1], h.position[2]], sampled, sampleIndex: h.sampleIndex });
    }
  });
  const drawn = placed.length;

  let stops: RouteStop[] = [];
  let caption: string;
  if (origin && drawn >= 1) {
    stops = placed;
    caption = `route: query point → sidecar top-k (${drawn} drawn, ${inSample} in sample; not a search path)`;
  } else if (origin && sampledRows.length >= 1) {
    stops = sampledRows;
    caption = 'route: query point → sidecar top-k (not a search path)';
  } else if (origin) {
    caption = 'no route: no hit in the sample';
  } else if (sampledRows.length >= 2) {
    stops = sampledRows;
    caption = 'route: sidecar top-k in rank order (not a search path)';
  } else {
    caption = 'no route: fewer than 2 hits in the sample';
  }
  const points = stops.length > 0 ? [...(origin ? [origin] : []), ...stops.map((s) => s.pos)] : [];
  return {
    origin,
    stops,
    ghosts: stops.filter((s) => !s.sampled),
    points,
    total: hits.length,
    inSample,
    drawn,
    caption,
  };
}

/** The headset's Memory-row route line (coverage is shown on its own line here). */
export function routeLegend(r: SidecarRoute): string {
  if (r.points.length < 2) return '';
  if (r.origin && r.drawn >= 1) return `Route: query point → sidecar top-k (${r.drawn} drawn, ${r.inSample} in sample)`;
  return r.origin ? 'Route: query point → sidecar top-k' : 'Route: sidecar top-k in rank order';
}
