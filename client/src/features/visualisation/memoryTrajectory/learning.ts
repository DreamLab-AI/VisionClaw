/**
 * The self-learning loop: remember past queries and their answers, inject
 * those answers as hints into later searches, and adapt the beam breadth
 * (ef) toward a target recall with either a per-query step controller or a
 * smoothed (EWMA) controller with a deadband.
 *
 * Adapted from the RuVector Explorer's `recallMemory`, `learnFrom`, `bFloor`,
 * `learnSave` and `learnRestore` (https://github.com/ruvnet/RuVector,
 * docs/explorer, MIT licence). Distances are `1 - dot` on normalised rows.
 */
import type { LearningMemoryEntry, LearningState, SearchResult } from './types';
import { dot } from './vec';

/** Upper bound of the learned breadth, as in the Explorer. */
export const MAX_BREADTH = 400;
/** Most hub counters kept in a serialised state. */
const MAX_SERIALISED_HUBS = 512;
const FORMAT_VERSION = 1;

/** The [floor, ceiling] the controller keeps the breadth within for a given k. */
export function breadthBounds(k: number): [number, number] {
  return [Math.max(Math.floor(k), 8), MAX_BREADTH];
}

export function createLearningState(opts: Partial<Omit<LearningState, 'memory' | 'hubCounts'>> = {}): LearningState {
  return {
    enabled: opts.enabled ?? true,
    memory: [],
    capacity: opts.capacity ?? 256,
    hintsPerQuery: opts.hintsPerQuery ?? 2,
    targetRecall: opts.targetRecall ?? 0.95,
    learningRate: opts.learningRate ?? 0.2,
    breadth: opts.breadth ?? null,
    controller: opts.controller ?? 'step',
    ewma: opts.ewma ?? null,
    hubCounts: new Map(),
  };
}

/**
 * Hints from the `hintsPerQuery` nearest remembered queries (their stored
 * answers, deduplicated, at most `2 * hintsPerQuery`). Counts a hit on each
 * consulted memory. `nearestDistance` is `1 - dot` to the closest one.
 */
export function recallHints(state: LearningState, q: Float32Array): { hints: number[]; nearestDistance: number | null } {
  if (!state.enabled || !state.memory.length || state.hintsPerQuery <= 0) return { hints: [], nearestDistance: null };
  const scored = state.memory
    .map((m, i) => [1 - dot(q, m.q), i] as const)
    .sort((a, b) => a[0] - b[0] || a[1] - b[1])
    .slice(0, state.hintsPerQuery);
  const hints: number[] = [];
  for (const [, i] of scored) {
    const m = state.memory[i];
    m.hits = (m.hits || 0) + 1;
    for (const id of m.ids) if (!hints.includes(id)) hints.push(id);
  }
  return { hints: hints.slice(0, state.hintsPerQuery * 2), nearestDistance: scored[0][0] };
}

/**
 * Stores the query and its first two answers, counts the upper-layer hops as
 * hubs, and moves the breadth toward the target recall. The breadth starts
 * from the beam this search actually ran (`max(beam.length, k)`) the first
 * time. Does nothing while learning is disabled.
 */
export function learnFrom(state: LearningState, q: Float32Array, result: SearchResult, recall: number, k: number): void {
  if (!state.enabled) return;
  const entry: LearningMemoryEntry = { q: Float32Array.from(q), ids: result.top.slice(0, 2), hits: 0 };
  state.memory.push(entry);
  while (state.memory.length > Math.max(0, state.capacity)) state.memory.shift();
  for (const h of result.hops) state.hubCounts.set(h, (state.hubCounts.get(h) ?? 0) + 1);

  const [lo, hi] = breadthBounds(k);
  if (state.breadth == null) state.ewma = null;
  const b = state.breadth ?? Math.max(result.beam.length, Math.floor(k));
  let nb = b;
  const target = state.targetRecall;
  const lr = state.learningRate;
  if (state.controller === 'ewma') {
    // smoothed recall with a deadband: move only when the average leaves the band around the target
    state.ewma = state.ewma == null ? recall : state.ewma * 0.9 + recall * 0.1;
    const lower = target - 0.005;
    const upper = Math.min(target + 0.02, (1 + target) / 2);
    // the step scales with how far the average sits outside the band, so the lagging average does not overshoot
    if (state.ewma < lower) nb = b * (1 + lr * Math.min(1, (lower - state.ewma) / 0.02));
    else if (state.ewma > upper) nb = b * (1 - lr * 0.25 * Math.min(1, (state.ewma - upper) / Math.max(0.003, 1 - upper)));
  } else {
    // per query, the step scales with the recall gap (one missed neighbour = 1/k); widening outweighs
    // narrowing 3:1, so mean recall settles at or above the target
    const gap = (target - recall) * k;
    if (gap > 1e-9) nb = b * (1 + lr * Math.min(1, gap));
    else if (gap < -1e-9) nb = b * (1 - (lr / 3) * Math.min(1, -gap));
  }
  state.breadth = Math.max(lo, Math.min(hi, nb));
}

// ---------------------------------------------------------------------------
// persistence

function f32ToBase64(a: Float32Array): string {
  const u = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
  let s = '';
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
  return btoa(s);
}

function base64ToF32(s: string): Float32Array | null {
  let bin: string;
  try {
    bin = atob(s);
  } catch {
    return null;
  }
  if (bin.length % 4 !== 0) return null;
  const u = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u[i] = bin.charCodeAt(i);
  return new Float32Array(u.buffer);
}

interface SerialisedLearning {
  v: number;
  dim: number;
  enabled: boolean;
  capacity: number;
  hintsPerQuery: number;
  targetRecall: number;
  learningRate: number;
  breadth: number | null;
  controller: 'step' | 'ewma';
  ewma: number | null;
  memory: [string, number[], number][];
  hub: [number, number][];
}

export function serializeLearning(state: LearningState): string {
  const dim = state.memory.length ? state.memory[0].q.length : 0;
  const o: SerialisedLearning = {
    v: FORMAT_VERSION,
    dim,
    enabled: state.enabled,
    capacity: state.capacity,
    hintsPerQuery: state.hintsPerQuery,
    targetRecall: state.targetRecall,
    learningRate: state.learningRate,
    breadth: state.breadth,
    controller: state.controller,
    ewma: state.ewma,
    memory: state.memory.map((m) => [f32ToBase64(m.q instanceof Float32Array ? m.q : Float32Array.from(m.q)), m.ids.slice(), m.hits | 0]),
    hub: [...state.hubCounts.entries()].sort((a, b) => b[1] - a[1] || a[0] - b[0]).slice(0, MAX_SERIALISED_HUBS),
  };
  return JSON.stringify(o);
}

const isFiniteNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const isNodeId = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v >= 0;

/**
 * Restores a state serialised by {@link serializeLearning}. Returns null
 * when the JSON is corrupt, has another version, or any remembered query
 * does not have `dim` components.
 */
export function restoreLearning(json: string, dim: number): LearningState | null {
  let o: unknown;
  try {
    o = JSON.parse(json);
  } catch {
    return null;
  }
  if (!o || typeof o !== 'object' || Array.isArray(o)) return null;
  const s = o as Partial<SerialisedLearning>;
  if (s.v !== FORMAT_VERSION) return null;
  if (typeof s.enabled !== 'boolean') return null;
  if (!isNodeId(s.capacity) || !isNodeId(s.hintsPerQuery)) return null;
  if (!isFiniteNum(s.targetRecall) || s.targetRecall < 0 || s.targetRecall > 1) return null;
  if (!isFiniteNum(s.learningRate) || s.learningRate < 0 || s.learningRate > 1) return null;
  if (s.breadth !== null && !(isFiniteNum(s.breadth) && s.breadth > 0)) return null;
  if (s.ewma !== null && !(isFiniteNum(s.ewma) && s.ewma >= 0 && s.ewma <= 1)) return null;
  if (s.controller !== 'step' && s.controller !== 'ewma') return null;
  if (!Array.isArray(s.memory) || !Array.isArray(s.hub)) return null;

  const memory: LearningMemoryEntry[] = [];
  for (const m of s.memory) {
    if (!Array.isArray(m) || m.length !== 3) return null;
    const [qb, ids, hits] = m as unknown[];
    if (typeof qb !== 'string') return null;
    const q = base64ToF32(qb);
    if (!q || q.length !== dim || !q.every(Number.isFinite)) return null;
    if (!Array.isArray(ids) || !ids.every(isNodeId)) return null;
    if (!isNodeId(hits)) return null;
    memory.push({ q, ids: ids.slice(), hits });
  }
  const hubCounts = new Map<number, number>();
  for (const h of s.hub) {
    if (!Array.isArray(h) || h.length !== 2 || !isNodeId(h[0]) || !isNodeId(h[1])) return null;
    hubCounts.set(h[0], h[1]);
  }
  while (memory.length > s.capacity) memory.shift();
  return {
    enabled: s.enabled,
    memory,
    capacity: s.capacity,
    hintsPerQuery: s.hintsPerQuery,
    targetRecall: s.targetRecall,
    learningRate: s.learningRate,
    breadth: s.breadth ?? null,
    controller: s.controller,
    ewma: s.ewma ?? null,
    hubCounts,
  };
}
