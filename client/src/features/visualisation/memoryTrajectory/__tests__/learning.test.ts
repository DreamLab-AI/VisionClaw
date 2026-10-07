import { describe, it, expect } from 'vitest';
import {
  createLearningState,
  recallHints,
  learnFrom,
  serializeLearning,
  restoreLearning,
  breadthBounds,
} from '../learning';
import type { LearningState, SearchResult } from '../types';
import { seededRandom, normaliseInPlace } from './fixtures';

const DIM = 16;
const K = 10;

function vec(seed: number): Float32Array {
  const r = seededRandom(seed);
  const v = new Float32Array(DIM).map(() => r() - 0.5);
  normaliseInPlace(v);
  return v;
}

function result(top: number[], beamSize = 48, hops: number[] = [5, 6]): SearchResult {
  const beam = [...top];
  for (let i = 1000; beam.length < beamSize; i++) beam.push(i);
  return { top, beam, hops, distanceEvals: 100, trace: [] };
}

const TOP = Array.from({ length: K }, (_, i) => i);

describe('createLearningState', () => {
  it('uses the Explorer defaults', () => {
    const s = createLearningState();
    expect(s).toMatchObject({
      enabled: true,
      capacity: 256,
      hintsPerQuery: 2,
      targetRecall: 0.95,
      learningRate: 0.2,
      breadth: null,
      controller: 'step',
      ewma: null,
    });
    expect(s.memory).toEqual([]);
    expect(s.hubCounts.size).toBe(0);
  });

  it('accepts overrides', () => {
    const s = createLearningState({ controller: 'ewma', capacity: 4, targetRecall: 0.9 });
    expect(s.controller).toBe('ewma');
    expect(s.capacity).toBe(4);
    expect(s.targetRecall).toBe(0.9);
  });
});

describe('recallHints', () => {
  it('returns nothing without memory or when disabled', () => {
    const s = createLearningState();
    expect(recallHints(s, vec(1))).toEqual({ hints: [], nearestDistance: null });
    learnFrom(s, vec(1), result(TOP), 1, K);
    s.enabled = false;
    expect(recallHints(s, vec(1))).toEqual({ hints: [], nearestDistance: null });
  });

  it('returns the answers of the nearest remembered queries and counts hits', () => {
    const s = createLearningState({ hintsPerQuery: 1 });
    const q1 = vec(1);
    const q2 = vec(2);
    learnFrom(s, q1, result([7, 8, 9]), 1, 3);
    learnFrom(s, q2, result([20, 21, 22]), 1, 3);
    const r = recallHints(s, q2);
    expect(r.hints).toEqual([20, 21]);
    expect(r.nearestDistance).toBeCloseTo(0, 6);
    expect(s.memory[1].hits).toBe(1);
    expect(s.memory[0].hits).toBe(0);
  });

  it('deduplicates hints across memories and caps them at 2 x hintsPerQuery', () => {
    const s = createLearningState({ hintsPerQuery: 2 });
    const q = vec(3);
    learnFrom(s, q, result([1, 2, 3]), 1, 3);
    learnFrom(s, q, result([2, 1, 4]), 1, 3);
    // equal distances: the older memory comes first
    expect(recallHints(s, q).hints).toEqual([1, 2]);
    const one = createLearningState({ hintsPerQuery: 1 });
    learnFrom(one, q, result([5, 6, 7]), 1, 3);
    learnFrom(one, q, result([8, 9, 10]), 1, 3);
    expect(recallHints(one, q).hints).toEqual([5, 6]);
  });
});

describe('learnFrom', () => {
  it('stores the query copy, the first two answers and the hop hubs; evicts oldest beyond capacity', () => {
    const s = createLearningState({ capacity: 2 });
    const q = vec(4);
    learnFrom(s, q, result(TOP), 1, K);
    q[0] = 99;
    expect(s.memory[0].q[0]).not.toBe(99);
    expect(s.memory[0].ids).toEqual([0, 1]);
    learnFrom(s, vec(5), result([3, 4]), 1, 2);
    learnFrom(s, vec(6), result([5, 6]), 1, 2);
    expect(s.memory.map((m) => m.ids)).toEqual([[3, 4], [5, 6]]);
    expect(s.hubCounts.get(5)).toBe(3);
    expect(s.hubCounts.get(6)).toBe(3);
  });

  for (const controller of ['step', 'ewma'] as const) {
    describe(`${controller} controller`, () => {
      it('widens the beam when recall is below the target', () => {
        const s = createLearningState({ controller });
        learnFrom(s, vec(1), result(TOP, 48), 0.5, K);
        expect(s.breadth!).toBeGreaterThan(48);
      });

      it('narrows the beam when recall is above the target', () => {
        const s = createLearningState({ controller, targetRecall: 0.9 });
        learnFrom(s, vec(1), result(TOP, 48), 1, K);
        expect(s.breadth!).toBeLessThan(48);
      });

      it('stays within its bounds under sustained pressure', () => {
        const [lo, hi] = breadthBounds(K);
        const up = createLearningState({ controller });
        for (let i = 0; i < 400; i++) learnFrom(up, vec(i), result(TOP, Math.round(up.breadth ?? 48)), 0, K);
        expect(up.breadth).toBe(hi);
        const down = createLearningState({ controller, targetRecall: 0.5 });
        for (let i = 0; i < 400; i++) learnFrom(down, vec(i), result(TOP, Math.round(down.breadth ?? 48)), 1, K);
        expect(down.breadth).toBe(lo);
        expect(lo).toBe(Math.max(K, 8));
        expect(hi).toBe(400);
      });
    });
  }

  it('ewma holds steady inside its deadband', () => {
    const s = createLearningState({ controller: 'ewma', targetRecall: 0.95 });
    learnFrom(s, vec(1), result(TOP, 48), 0.96, K);
    expect(s.breadth).toBe(48);
    expect(s.ewma).toBeCloseTo(0.96, 9);
  });

  it('neither adapts breadth nor records memory when disabled', () => {
    const s = createLearningState({ enabled: false });
    learnFrom(s, vec(1), result(TOP, 48), 0, K);
    expect(s.breadth).toBeNull();
    expect(s.memory.length).toBe(0);
  });
});

describe('serializeLearning / restoreLearning', () => {
  function populated(): LearningState {
    const s = createLearningState({ controller: 'ewma', capacity: 8, hintsPerQuery: 3 });
    for (let i = 0; i < 5; i++) learnFrom(s, vec(i + 10), result([i, i + 1, i + 2], 40, [i % 2]), i % 2 ? 1 : 0.6, 3);
    recallHints(s, vec(10));
    return s;
  }

  it('round-trips the whole state', () => {
    const s = populated();
    const back = restoreLearning(serializeLearning(s), DIM)!;
    expect(back).not.toBeNull();
    expect(back.enabled).toBe(s.enabled);
    expect(back.capacity).toBe(8);
    expect(back.hintsPerQuery).toBe(3);
    expect(back.controller).toBe('ewma');
    expect(back.breadth).toBe(s.breadth);
    expect(back.ewma).toBe(s.ewma);
    expect(back.targetRecall).toBe(s.targetRecall);
    expect(back.learningRate).toBe(s.learningRate);
    expect([...back.hubCounts]).toEqual([...s.hubCounts]);
    expect(back.memory.length).toBe(s.memory.length);
    back.memory.forEach((m, i) => {
      expect(m.q).toBeInstanceOf(Float32Array);
      expect(Array.from(m.q)).toEqual(Array.from(s.memory[i].q));
      expect(m.ids).toEqual(s.memory[i].ids);
      expect(m.hits).toBe(s.memory[i].hits);
    });
  });

  it('rejects corrupt or mismatched input', () => {
    const good = serializeLearning(populated());
    const obj = JSON.parse(good);
    const bad = [
      '',
      'not json',
      'null',
      '[]',
      JSON.stringify({ ...obj, v: 2 }),
      JSON.stringify({ ...obj, memory: 'x' }),
      JSON.stringify({ ...obj, memory: [['@@@', [1], 0]] }),
      JSON.stringify({ ...obj, memory: [[obj.memory[0][0], [-1], 0]] }),
      JSON.stringify({ ...obj, memory: [[obj.memory[0][0], [1.5], 0]] }),
      JSON.stringify({ ...obj, breadth: 'wide' }),
      JSON.stringify({ ...obj, controller: 'pid' }),
      JSON.stringify({ ...obj, targetRecall: 3 }),
      JSON.stringify({ ...obj, capacity: -1 }),
      JSON.stringify({ ...obj, hub: [[1, 'x']] }),
    ];
    for (const s of bad) expect(restoreLearning(s, DIM), s.slice(0, 60)).toBeNull();
    expect(restoreLearning(good, DIM + 1)).toBeNull();
  });

  it('round-trips an empty state', () => {
    const s = createLearningState();
    const back = restoreLearning(serializeLearning(s), 384)!;
    expect(back.memory).toEqual([]);
    expect(back.breadth).toBeNull();
  });
});
