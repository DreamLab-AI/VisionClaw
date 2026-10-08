/**
 * Separated-layout triangle (ADR-2135) — TypeScript port of
 * `crates/visionclaw-tri-layout/src/lib.rs`.
 *
 * The knowledge graph, the formal ontology and the memory cloud are always
 * apart (operator decision 2026-10-08), on an equilateral triangle in the
 * ground plane (X–Z, Y up) at the fixed SEPARATION, centred on the scene
 * origin, with the agents at the centroid. The server places the two graphs
 * (display-only projection); the client places the memory cloud, ten graphs
 * wide, on the memory vertex's ray (`memoryCentre`) with this port. A vertex angle is a
 * yaw about +Y from +Z towards +X: knowledge −60° (front-left), ontology
 * +60° (front-right), memory 180° (back), so a camera on +Z sees all three.
 *
 * `__tests__/triLayout.test.ts` holds this file to the Rust constants and to
 * `crates/visionclaw-tri-layout/fixtures/tri_layout_fixture.json`.
 */

export type Vec3 = [number, number, number];

/** p99 radius of the larger graph body at live scale (2026-10-08, 9,473 nodes) */
export const LIVE_GRAPH_RADIUS = 152;
/** body centres sit at least CLEARANCE × (r₁ + r₂) apart */
export const CLEARANCE = 1.25;
/** smallest separation at which graph bodies of radii a and b clear each other */
export function separationForRadii(a: number, b: number): number {
  return (CLEARANCE * (a + b)) / 2;
}
/** the layout's one separation, derived from the live radii (190) */
export const SEPARATION = separationForRadii(LIVE_GRAPH_RADIUS, LIVE_GRAPH_RADIUS);
/** the memory cloud's robust radius relative to one graph's */
export const MEMORY_BODY_SCALE = 10;

/** separation at which the triangle reaches full strength */
export const FULL_STRENGTH_SEPARATION = 100;
/** circumradius per unit of separation, 2/√3: vertices sit 2 × separation apart */
export const RADIUS_PER_SEPARATION = 1.1547005;
/** vertex yaw angles in degrees: knowledge, ontology, memory */
export const VERTEX_ANGLES_DEG: readonly [number, number, number] = [-60, 60, 180];

/** vertex indices (a const object rather than an enum, for isolated-module builds) */
export const Vertex = { Knowledge: 0, Ontology: 1, Memory: 2 } as const;
export type Vertex = (typeof Vertex)[keyof typeof Vertex];

/** smoothstep from 0 (separation 0) to 1 (FULL_STRENGTH_SEPARATION); non-finite → 0 */
export function strength(separation: number): number {
  if (!Number.isFinite(separation)) return 0;
  const t = Math.min(1, Math.max(0, separation / FULL_STRENGTH_SEPARATION));
  return t * t * (3 - 2 * t);
}

/** circumradius; non-positive or non-finite separations are 0 (merged) */
export function circumradius(separation: number): number {
  return Number.isFinite(separation) && separation > 0 ? separation * RADIUS_PER_SEPARATION : 0;
}

/** rotate about +Y by `yaw` radians (+Z turns towards +X) */
export function rotateY(v: Vec3, yaw: number): Vec3 {
  const s = Math.sin(yaw);
  const c = Math.cos(yaw);
  return [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c];
}

export interface TriangleFrame {
  separation: number;
  radius: number;
  strength: number;
  /** vertex positions indexed by Vertex; y is 0 */
  vertices: [Vec3, Vec3, Vec3];
  /** yaw of each body's local frame, radians */
  yaws: [number, number, number];
}

export function triangleFrame(separation: number): TriangleFrame {
  const sep = Number.isFinite(separation) ? Math.max(0, separation) : 0;
  const radius = circumradius(sep);
  const s = strength(sep);
  const vertices = VERTEX_ANGLES_DEG.map((deg) => {
    const a = (deg * Math.PI) / 180;
    // `+ 0` turns the -0 of 0 × cos(180°) into +0, so a merged frame is all +0
    return [radius * Math.sin(a) + 0, 0, radius * Math.cos(a) + 0] as Vec3;
  }) as [Vec3, Vec3, Vec3];
  const yaws = VERTEX_ANGLES_DEG.map((deg) => ((deg * Math.PI) / 180) * s) as [number, number, number];
  return { separation: sep, radius, strength: s, vertices, yaws };
}

/** the frame every reader uses: the triangle at SEPARATION */
export function separatedFrame(): TriangleFrame {
  return triangleFrame(SEPARATION);
}

/**
 * the memory body's distance from the centroid as a fraction of
 * memoryClearDistance (operator decision 2026-10-08; it was 1)
 */
export const MEMORY_DISTANCE_FACTOR = 0.5;

/**
 * Centre of the memory body for one graph's robust radius and the cloud's
 * drawn radius: on the memory vertex's ray, MEMORY_DISTANCE_FACTOR of the way
 * out to memoryClearDistance. At live scale it overlaps the graphs.
 */
export function memoryCentre(f: TriangleFrame, graphRadius: number, memoryRadius: number): Vec3 {
  const dir = rotateY([0, 0, 1], (VERTEX_ANGLES_DEG[Vertex.Memory] * Math.PI) / 180);
  const d = MEMORY_DISTANCE_FACTOR * memoryClearDistance(f, graphRadius, memoryRadius);
  // `+ 0` keeps a 0 × sin(180°) at +0
  return [dir[0] * d + 0, 0, dir[2] * d + 0];
}

/**
 * Distance from the centroid along the memory vertex's ray at which the
 * cloud's sphere clears both graph spheres by CLEARANCE, and never less than
 * the vertex's own. Bad radii count as 0.
 */
export function memoryClearDistance(f: TriangleFrame, graphRadius: number, memoryRadius: number): number {
  const r = (x: number) => (Number.isFinite(x) ? Math.max(0, x) : 0);
  const need = CLEARANCE * (r(graphRadius) + r(memoryRadius));
  const dir = rotateY([0, 0, 1], (VERTEX_ANGLES_DEG[Vertex.Memory] * Math.PI) / 180);
  let dist = f.radius;
  for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
    const g = f.vertices[v];
    const b = g[0] * dir[0] + g[2] * dir[2];
    const disc = b * b - (g[0] * g[0] + g[2] * g[2]) + need * need;
    if (disc > 0) dist = Math.max(dist, b + Math.sqrt(disc));
  }
  return dist;
}

export function isMerged(f: TriangleFrame): boolean {
  return f.radius === 0 && f.strength === 0;
}

/** body-local point → scene: yaw, then move to the vertex */
export function place(f: TriangleFrame, v: Vertex, local: Vec3): Vec3 {
  const r = rotateY(local, f.yaws[v]);
  const c = f.vertices[v];
  return [r[0] + c[0], r[1] + c[1], r[2] + c[2]];
}

/** scene point → body-local, the inverse of place */
export function unplace(f: TriangleFrame, v: Vertex, world: Vec3): Vec3 {
  const c = f.vertices[v];
  return rotateY([world[0] - c[0], world[1] - c[1], world[2] - c[2]], -f.yaws[v]);
}

/** knowledge or ontology, whichever vertex is nearer in the ground plane (tie → knowledge) */
export function nearestGraphVertex(f: TriangleFrame, world: Vec3): Vertex {
  const d2 = (v: Vertex) => (world[0] - f.vertices[v][0]) ** 2 + (world[2] - f.vertices[v][2]) ** 2;
  return d2(Vertex.Ontology) < d2(Vertex.Knowledge) ? Vertex.Ontology : Vertex.Knowledge;
}

/** fold a projected graph position into its body's local frame (identity when merged) */
export function fold(f: TriangleFrame, world: Vec3): Vec3 {
  if (isMerged(f)) return world;
  return unplace(f, nearestGraphVertex(f, world), world);
}

/**
 * Fold a flat `[x, y, z, …]` buffer (first `count` rows) into body-local
 * frames, so a robust extent measured on it is one graph's, not the whole
 * triangle's. Returns the input untouched when merged.
 */
export function foldPositions(f: TriangleFrame, positions: ArrayLike<number>, count: number): ArrayLike<number> {
  if (isMerged(f)) return positions;
  const rows = Math.max(0, Math.min(Math.floor(count), Math.floor(positions.length / 3)));
  const out = new Float32Array(rows * 3);
  for (let i = 0; i < rows; i++) {
    const p = fold(f, [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]]);
    out[i * 3] = p[0];
    out[i * 3 + 1] = p[1];
    out[i * 3 + 2] = p[2];
  }
  return out;
}
