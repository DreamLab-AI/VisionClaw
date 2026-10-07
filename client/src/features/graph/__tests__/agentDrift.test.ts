/**
 * The desktop agent drift (ADR-2135) must replay the Rust crate's scripted
 * run exactly: `crates/visionclaw-tri-layout/fixtures/tri_layout_fixture.json`
 * `drift` section, produced by `drift::DriftField` (which the XR client links).
 */
import { describe, it, expect } from 'vitest';
import {
  DriftField,
  HALF_LIFE_S,
  IDLE_WEIGHT,
  REACH,
  FOLLOW_S,
  SETTLED_EPSILON,
} from '../agentDrift';
import { triangleFrame, Vertex } from '../triLayout';

type Fs = typeof import('fs');
type PathMod = typeof import('path');
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;
const fs = getBuiltin('fs') as Fs;
const path = getBuiltin('path') as PathMod;
const FIXTURE = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, '../../../../../crates/visionclaw-tri-layout/fixtures/tri_layout_fixture.json'), 'utf8'),
).drift as {
  half_life_s: number; idle_weight: number; reach: number; follow_s: number; settled_epsilon: number;
  agents: number[]; step_s: number; steps: number;
  events: { t: number; kind: 'action' | 'memory'; agent: number | null; vertex: number }[];
  checkpoints: { t: number; coeffs: [number, number, number][] }[];
};

describe('agentDrift port matches the Rust DriftField', () => {
  it('constants', () => {
    // f32 constants serialise with f32 rounding (0.6 → 0.6000000238…)
    const ours = [HALF_LIFE_S, IDLE_WEIGHT, REACH, FOLLOW_S];
    const theirs = [FIXTURE.half_life_s, FIXTURE.idle_weight, FIXTURE.reach, FIXTURE.follow_s];
    ours.forEach((x, i) => expect(x).toBeCloseTo(theirs[i], 6));
    expect(SETTLED_EPSILON).toBeCloseTo(FIXTURE.settled_epsilon, 9);
  });

  it('replays the scripted run checkpoint for checkpoint', () => {
    const field = new DriftField<number>();
    const all = FIXTURE.agents;
    let next = 0;
    let checked = 0;
    for (let i = 0; i <= FIXTURE.steps; i++) {
      const t = i * FIXTURE.step_s;
      while (next < FIXTURE.events.length && FIXTURE.events[next].t <= t + 1e-9) {
        const e = FIXTURE.events[next];
        if (e.kind === 'action') field.recordAction(e.agent!, e.vertex as Vertex, e.t);
        else field.recordMemory(e.agent, all, e.t);
        next++;
      }
      field.step(t);
      if (i % 10 === 0) {
        const cp = FIXTURE.checkpoints[i / 10];
        all.forEach((a, k) => {
          const c = field.get(a)?.coeffs() ?? [0, 0, 0];
          c.forEach((x, v) => expect(Math.abs(x - cp.coeffs[k][v]), `t=${cp.t} agent ${a} v${v}`).toBeLessThan(2e-4));
        });
        checked++;
      }
    }
    expect(checked).toBe(FIXTURE.checkpoints.length);
  });
});

describe('agentDrift behaviour', () => {
  it('a new agent is at the centroid; work pulls it towards the vertex; idle returns it', () => {
    const f = triangleFrame(300);
    const field = new DriftField<string>();
    field.recordAction('a', Vertex.Ontology, 0);
    field.step(0);
    expect(field.offset('a', f)).toEqual([0, 0, 0]);
    for (let t = 0.1; t <= 3; t += 0.1) field.step(t);
    expect(field.offset('a', f)[0]).toBeGreaterThan(10);
    for (let t = 3; t <= 300; t += 0.5) field.step(t);
    expect(field.size).toBe(0);
    expect(field.offset('a', f)).toEqual([0, 0, 0]);
  });

  it('merged layout: every offset is zero', () => {
    const field = new DriftField<string>();
    field.step(0);
    field.recordAction('a', Vertex.Knowledge, 0);
    for (let t = 0.1; t <= 3; t += 0.1) field.step(t);
    expect(field.offset('a', triangleFrame(0))).toEqual([0, 0, 0]);
  });
});
