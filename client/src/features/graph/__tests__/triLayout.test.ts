/**
 * Drift guard for the TypeScript port of the separated-layout triangle
 * (ADR-2135). The Rust crate `crates/visionclaw-tri-layout` is the source:
 * its constants are parsed from `src/lib.rs`, and its committed fixture
 * (`fixtures/tri_layout_fixture.json`, checked by the crate's own
 * `fixture_is_current` test) must be reproduced by the port.
 */

import { describe, it, expect } from 'vitest';
import {
  FULL_STRENGTH_SEPARATION,
  RADIUS_PER_SEPARATION,
  VERTEX_ANGLES_DEG,
  Vertex,
  triangleFrame,
  place,
  fold,
  foldPositions,
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
