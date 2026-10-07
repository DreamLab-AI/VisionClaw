/**
 * Agent drift in the separated layout (ADR-2135) — TypeScript port of
 * `crates/visionclaw-tri-layout/src/drift.rs`, which the XR client links.
 *
 * Each agent starts at the triangle's centroid. An action on a knowledge or
 * ontology node credits that vertex; a memory access credits the memory
 * vertex (the named agent, or every agent equally when the flash names
 * none). Credit halves every HALF_LIFE_S. The target
 *
 *   REACH × Σ_v w_v · vertex_v / (IDLE_WEIGHT + Σ_v w_v)
 *
 * stays inside the triangle and short of the graphs, and falls back to the
 * centroid as credit decays; the shown offset follows it through a first-order
 * lag (FOLLOW_S), so it never overshoots or oscillates. State is kept as vertex
 * coefficients, so moving the slider carries agents with the triangle.
 *
 * `__tests__/agentDrift.test.ts` replays the crate's scripted run from
 * `tri_layout_fixture.json`.
 */

import { Vertex, type TriangleFrame, type Vec3 } from './triLayout';

export const HALF_LIFE_S = 6.0;
export const IDLE_WEIGHT = 1.0;
export const REACH = 0.6;
export const FOLLOW_S = 1.5;
export const SETTLED_EPSILON = 1e-3;

type Triple = [number, number, number];

export class AgentActivity {
  private weights: Triple = [0, 0, 0];
  private weightsAt: number;
  private c: Triple = [0, 0, 0];
  private steppedAt: number | null = null;

  constructor(now: number) {
    this.weightsAt = now;
  }

  /** credit `credit` to vertex `v` at `now`; non-positive or non-finite credit is ignored */
  record(v: Vertex, credit: number, now: number): void {
    if (!(Number.isFinite(credit) && credit > 0)) return;
    this.weights = this.weightsAtTime(now);
    this.weightsAt = Math.max(this.weightsAt, now);
    this.weights[v] += credit;
  }

  /** credit per vertex decayed to `now` (an earlier `now` never grows it) */
  weightsAtTime(now: number): Triple {
    const dt = Math.max(0, now - this.weightsAt);
    const k = Math.pow(0.5, dt / HALF_LIFE_S);
    return [this.weights[0] * k, this.weights[1] * k, this.weights[2] * k];
  }

  targetCoeffs(now: number): Triple {
    const w = this.weightsAtTime(now);
    const denom = IDLE_WEIGHT + w[0] + w[1] + w[2];
    return [(REACH * w[0]) / denom, (REACH * w[1]) / denom, (REACH * w[2]) / denom];
  }

  /** advance the eased coefficients to `now`; the first step only starts the clock */
  step(now: number): Triple {
    const target = this.targetCoeffs(now);
    const alpha = this.steppedAt === null ? 0 : 1 - Math.exp(-Math.max(0, now - this.steppedAt) / FOLLOW_S);
    for (let v = 0; v < 3; v++) this.c[v] += (target[v] - this.c[v]) * alpha;
    this.steppedAt = this.steppedAt === null ? now : Math.max(this.steppedAt, now);
    return this.coeffs();
  }

  coeffs(): Triple {
    return [this.c[0], this.c[1], this.c[2]];
  }

  /** display offset from the centroid in `frame` */
  offset(frame: TriangleFrame): Vec3 {
    const out: Triple = [0, 0, 0];
    for (let v = 0; v < 3; v++) {
      const p = frame.vertices[v];
      for (let k = 0; k < 3; k++) out[k] += this.c[v] * p[k];
    }
    return [out[0] + 0, out[1] + 0, out[2] + 0];
  }

  isSettled(now: number): boolean {
    const w = this.weightsAtTime(now);
    return w[0] + w[1] + w[2] < SETTLED_EPSILON && this.c[0] + this.c[1] + this.c[2] < SETTLED_EPSILON;
  }
}

/** activity of every agent, keyed by any stable agent key */
export class DriftField<K> {
  private agents = new Map<K, AgentActivity>();

  recordAction(agent: K, target: Vertex, now: number): void {
    this.entry(agent, now).record(target, 1, now);
  }

  /** a named agent gets full credit; an anonymous flash is shared by `allAgents` */
  recordMemory(agent: K | null, allAgents: readonly K[], now: number): void {
    if (agent !== null) {
      this.entry(agent, now).record(Vertex.Memory, 1, now);
      return;
    }
    if (allAgents.length === 0) return;
    const credit = 1 / allAgents.length;
    for (const a of allAgents) this.entry(a, now).record(Vertex.Memory, credit, now);
  }

  /** ease every agent to `now` and drop those that are home */
  step(now: number): void {
    for (const [k, a] of this.agents) {
      a.step(now);
      if (a.isSettled(now)) this.agents.delete(k);
    }
  }

  offset(agent: K, frame: TriangleFrame): Vec3 {
    return this.agents.get(agent)?.offset(frame) ?? [0, 0, 0];
  }

  get(agent: K): AgentActivity | undefined {
    return this.agents.get(agent);
  }

  get size(): number {
    return this.agents.size;
  }

  clear(): void {
    this.agents.clear();
  }

  private entry(agent: K, now: number): AgentActivity {
    let a = this.agents.get(agent);
    if (!a) {
      a = new AgentActivity(now);
      this.agents.set(agent, a);
    }
    return a;
  }
}
