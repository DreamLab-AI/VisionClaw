/**
 * Drift guard for the TypeScript port of the separated-layout triangle
 * (ADR-2135). The Rust crate `crates/visionclaw-tri-layout` is the source:
 * its constants are parsed from `src/lib.rs`, and its committed fixture
 * (`fixtures/tri_layout_fixture.json`, checked by the crate's own
 * `fixture_is_current` test) must be reproduced by the port.
 */

import { describe, it, expect } from 'vitest';
import {
  CLEARANCE,
  LIVE_GRAPH_RADIUS,
  MEMORY_BODY_SCALE,
  MEMORY_DISTANCE_FACTOR,
  SEPARATION,
  memoryCentre,
  memoryClearDistance,
  separatedFrame,
  separationForRadii,
  FULL_STRENGTH_SEPARATION,
  RADIUS_PER_SEPARATION,
  VERTEX_ANGLES_DEG,
  Vertex,
  triangleFrame,
  place,
  fold,
  foldPositions,
  isMerged,
  strength,
  type Vec3,
} from '../triLayout';

// vite-node here cannot resolve static `fs`/`path` imports; take them from the runtime.
type Fs = typeof import('fs');
type PathMod = typeof import('path');
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;
const fs = getBuiltin('fs') as Fs;
const path = getBuiltin('path') as PathMod;

const CRATE = path.resolve(__dirname, '../../../../../crates/visionclaw-tri-layout');
const LIB_RS = fs.readFileSync(path.join(CRATE, 'src/lib.rs'), 'utf8');

interface FixtureCase {
  separation: number;
  radius: number;
  strength: number;
  vertices: Vec3[];
  yaws: number[];
  place: { vertex: number; local: Vec3; world: Vec3 }[];
  fold: { world: Vec3; local: Vec3 }[];
}
const FIXTURE = JSON.parse(fs.readFileSync(path.join(CRATE, 'fixtures/tri_layout_fixture.json'), 'utf8')) as {
  live_graph_radius: number;
  clearance: number;
  separation: number;
  memory_body_scale: number;
  memory_distance_factor: number;
  memory_centre: { graph_radius: number; memory_radius: number; centre: Vec3 }[];
  full_strength_separation: number;
  radius_per_separation: number;
  vertex_angles_deg: number[];
  cases: FixtureCase[];
};

/** the numeric literal of `pub const NAME: T = <literal>;` in lib.rs */
function rustConst(name: string): string {
  const m = LIB_RS.match(new RegExp(`pub const ${name}: [^=]+= ([^;]+);`));
  if (!m) throw new Error(`${name} not found in lib.rs`);
  return m[1].replace(/_/g, '');
}

const near = (a: number, b: number, eps = 2e-3) => Math.abs(a - b) <= eps;
const nearV = (a: Vec3, b: Vec3, eps = 2e-3) => a.every((x, k) => near(x, b[k], eps));

describe('triLayout port matches the Rust crate', () => {
  it('constants are the Rust constants', () => {
    expect(Number(rustConst('FULL_STRENGTH_SEPARATION'))).toBe(FULL_STRENGTH_SEPARATION);
    expect(Number(rustConst('RADIUS_PER_SEPARATION'))).toBeCloseTo(RADIUS_PER_SEPARATION, 7);
    expect(JSON.parse(rustConst('VERTEX_ANGLES_DEG'))).toEqual([...VERTEX_ANGLES_DEG]);
    expect(FIXTURE.full_strength_separation).toBe(FULL_STRENGTH_SEPARATION);
    expect(FIXTURE.vertex_angles_deg).toEqual([...VERTEX_ANGLES_DEG]);
    expect(Number(rustConst('LIVE_GRAPH_RADIUS'))).toBe(LIVE_GRAPH_RADIUS);
    expect(Number(rustConst('CLEARANCE'))).toBe(CLEARANCE);
    expect(Number(rustConst('MEMORY_BODY_SCALE'))).toBe(MEMORY_BODY_SCALE);
    expect(rustConst('SEPARATION')).toBe('separationforradii(LIVEGRAPHRADIUS, LIVEGRAPHRADIUS)');
    expect(FIXTURE.separation).toBeCloseTo(SEPARATION, 4);
    expect(FIXTURE.live_graph_radius).toBe(LIVE_GRAPH_RADIUS);
    expect(FIXTURE.clearance).toBe(CLEARANCE);
    expect(FIXTURE.memory_body_scale).toBe(MEMORY_BODY_SCALE);
    expect(Number(rustConst('MEMORY_DISTANCE_FACTOR'))).toBe(MEMORY_DISTANCE_FACTOR);
    expect(FIXTURE.memory_distance_factor).toBe(MEMORY_DISTANCE_FACTOR);
  });

  it('places the memory body where the crate does', () => {
    expect(FIXTURE.memory_centre.length).toBeGreaterThanOrEqual(3);
    const f = separatedFrame();
    for (const m of FIXTURE.memory_centre) {
      expect(nearV(memoryCentre(f, m.graph_radius, m.memory_radius), m.centre), `${m.graph_radius}/${m.memory_radius}`).toBe(
        true,
      );
    }
  });

  it('reproduces every fixture case', () => {
    expect(FIXTURE.cases.length).toBeGreaterThanOrEqual(5);
    for (const c of FIXTURE.cases) {
      const f = triangleFrame(c.separation);
      expect(near(f.radius, c.radius), `radius @${c.separation}`).toBe(true);
      expect(near(f.strength, c.strength), `strength @${c.separation}`).toBe(true);
      f.vertices.forEach((v, i) => expect(nearV(v, c.vertices[i]), `vertex ${i} @${c.separation}`).toBe(true));
      f.yaws.forEach((y, i) => expect(near(y, c.yaws[i]), `yaw ${i} @${c.separation}`).toBe(true));
      for (const p of c.place) {
        expect(nearV(place(f, p.vertex as Vertex, p.local), p.world), `place @${c.separation}`).toBe(true);
      }
      for (const p of c.fold) {
        expect(nearV(fold(f, p.world), p.local), `fold @${c.separation}`).toBe(true);
      }
    }
  });
});

describe('triLayout behaviour', () => {
  it('the layout is always separated, from the live radii', () => {
    expect(SEPARATION).toBe(separationForRadii(LIVE_GRAPH_RADIUS, LIVE_GRAPH_RADIUS));
    const f = separatedFrame();
    expect(isMerged(f)).toBe(false);
    expect(f.strength).toBe(1);
    const k = f.vertices[Vertex.Knowledge];
    const o = f.vertices[Vertex.Ontology];
    expect(Math.hypot(k[0] - o[0], k[2] - o[2])).toBeGreaterThanOrEqual(CLEARANCE * 2 * LIVE_GRAPH_RADIUS - 1e-3);
  });

  it('the clear distance clears both graphs; the memory body sits at half of it', () => {
    expect(MEMORY_DISTANCE_FACTOR).toBe(0.5);
    const f = separatedFrame();
    const g = 93;
    const m = MEMORY_BODY_SCALE * g;
    const d = memoryClearDistance(f, g, m);
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
      const p = f.vertices[v];
      expect(Math.hypot(p[0], p[1], p[2] + d)).toBeGreaterThanOrEqual(CLEARANCE * (g + m) - 1e-6);
    }
    const c = memoryCentre(f, g, m);
    expect(c[2]).toBeLessThan(f.vertices[Vertex.Memory][2]);
    expect(Math.hypot(...c)).toBeCloseTo(MEMORY_DISTANCE_FACTOR * d, 6);
  });

  it('at live scale the closer memory body encloses both graphs (measured, operator choice)', () => {
    const f = separatedFrame();
    const m = MEMORY_BODY_SCALE * LIVE_GRAPH_RADIUS;
    const c = memoryCentre(f, LIVE_GRAPH_RADIUS, m);
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) {
      const p = f.vertices[v];
      const d = Math.hypot(c[0] - p[0], c[1] - p[1], c[2] - p[2]);
      expect(d + LIVE_GRAPH_RADIUS).toBeLessThan(m);
    }
  });

  it('separation 0 is the merged identity', () => {
    const f = triangleFrame(0);
    expect(place(f, Vertex.Memory, [1, 2, 3])).toEqual([1, 2, 3]);
    const buf = new Float32Array([1, 2, 3, 4, 5, 6]);
    expect(foldPositions(f, buf, 2)).toBe(buf);
  });

  it('the memory vertex is behind the centre for a camera on +Z', () => {
    const m = triangleFrame(200).vertices[Vertex.Memory];
    expect(Math.abs(m[0])).toBeLessThan(1e-9);
    expect(m[2]).toBeLessThan(0);
  });

  it('foldPositions brings both graphs back onto one local extent', () => {
    const f = triangleFrame(400);
    const local: Vec3[] = [[10, 20, 30], [-50, 0, 15]];
    const flat: number[] = [];
    for (const v of [Vertex.Knowledge, Vertex.Ontology]) for (const p of local) flat.push(...place(f, v, p));
    const out = foldPositions(f, flat, 4);
    for (let i = 0; i < 4; i++) {
      const want = local[i % 2];
      expect(nearV([out[i * 3], out[i * 3 + 1], out[i * 3 + 2]], want)).toBe(true);
    }
  });

  it('strength is a smoothstep and ignores garbage', () => {
    expect(strength(NaN)).toBe(0);
    expect(strength(-1)).toBe(0);
    expect(strength(FULL_STRENGTH_SEPARATION / 2)).toBeCloseTo(0.5, 9);
    expect(strength(1e6)).toBe(1);
  });
});
