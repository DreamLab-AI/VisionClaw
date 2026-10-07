import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createTrajectoryEngine, learningStorageKey } from '../engine';
import { exactTopK } from '../hnsw';
import { clusteredVectors, perturbedQueries } from './fixtures';

describe('createTrajectoryEngine (main-thread path)', () => {
  const vs = clusteredVectors(1500, 32, 12, 2024);
  const queries = perturbedQueries(vs, 30, 6);

  beforeEach(() => {
    try {
      localStorage.clear();
    } catch {
      /* storage unavailable: nothing to clear */
    }
  });

  it('falls back to the main thread when Worker is unavailable and reports progress', async () => {
    expect(typeof (globalThis as { Worker?: unknown }).Worker).toBe('undefined');
    const engine = createTrajectoryEngine(vs, { storageKey: 'engine-a' });
    const seen: number[] = [];
    engine.onProgress((d, t) => {
      expect(t).toBe(vs.count);
      seen.push(d);
    });
    expect(engine.graph()).toBeNull();
    await engine.ready;
    expect(seen[seen.length - 1]).toBe(vs.count);
    const g = engine.graph()!;
    expect(g.levels.length).toBe(vs.count);
    expect(g.params.M).toBe(12);
    expect(g.params.efConstruction).toBe(80);
    engine.dispose();
  });

  it('derives a stable seed from the storage key', async () => {
    const a = createTrajectoryEngine(vs, { storageKey: 'same', useWorker: false });
    const b = createTrajectoryEngine(vs, { storageKey: 'same', useWorker: false });
    const c = createTrajectoryEngine(vs, { storageKey: 'other', useWorker: false });
    await Promise.all([a.ready, b.ready, c.ready]);
    expect(a.graph()!.params.seed).toBe(b.graph()!.params.seed);
    expect(a.graph()!.params.seed).not.toBe(c.graph()!.params.seed);
    expect(a.graph()!.links).toEqual(b.graph()!.links);
    [a, b, c].forEach((e) => e.dispose());
  });

  it('runs a traced query end to end', async () => {
    const engine = createTrajectoryEngine(vs, { useWorker: false, params: { seed: 3 } });
    const q = queries[0];
    const run = await engine.query(q, { k: 10, learn: false });
    const g = engine.graph()!;
    expect(run.result.top.length).toBe(10);
    expect(run.result.trace.length).toBeGreaterThan(0);
    expect(run.exactTop).toEqual(exactTopK(vs, q, 10));
    expect(run.recall).toBeGreaterThanOrEqual(0);
    expect(run.recall).toBeLessThanOrEqual(1);
    expect(run.breadth).toBe(48);
    expect(run.hintsUsed).toEqual([]);
    expect(run.tree.root).toBe(g.ep);
    expect(run.tree.path[run.tree.path.length - 1]).toBe(run.result.top[0]);
    expect(engine.learning.memory.length).toBe(0);
    const wider = await engine.query(q, { k: 5, ef: 100, learn: false });
    expect(wider.breadth).toBe(100);
    expect(wider.result.beam.length).toBe(100);
    engine.dispose();
  });

  it('learns across queries: hints, breadth and memory', async () => {
    const engine = createTrajectoryEngine(vs, { useWorker: false, params: { seed: 4 } });
    let mean = 0;
    for (const q of queries) {
      const run = await engine.query(q, { k: 10 });
      mean += run.recall / queries.length;
    }
    expect(engine.learning.memory.length).toBe(queries.length);
    expect(engine.learning.breadth).not.toBeNull();
    expect(engine.learning.hubCounts.size).toBeGreaterThan(0);
    expect(mean).toBeGreaterThan(0.9);
    const learnedBreadth = engine.learning.breadth!;
    const again = await engine.query(queries[0], { k: 10, ef: 300 });
    expect(again.hintsUsed.length).toBeGreaterThan(0);
    // the learned breadth outranks the caller's ef
    expect(again.breadth).toBe(Math.max(10, Math.round(learnedBreadth)));
    const learningRef = engine.learning;
    engine.resetLearning();
    expect(engine.learning).toBe(learningRef);
    expect(engine.learning.memory.length).toBe(0);
    expect(engine.learning.breadth).toBeNull();
    engine.dispose();
  });

  it('persists learning under the storage key and restores it', async () => {
    const a = createTrajectoryEngine(vs, { useWorker: false, storageKey: 'persist' });
    for (const q of queries.slice(0, 5)) await a.query(q, { k: 10 });
    a.dispose();
    const raw = localStorage.getItem(learningStorageKey('persist'));
    expect(raw).toBeTruthy();
    const b = createTrajectoryEngine(vs, { useWorker: false, storageKey: 'persist' });
    expect(b.learning.memory.length).toBe(5);
    b.resetLearning();
    expect(localStorage.getItem(learningStorageKey('persist'))).toBeNull();
    b.dispose();
  });

  it('ignores corrupt or foreign stored learning', async () => {
    localStorage.setItem(learningStorageKey('corrupt'), '{"v":1,"memory":"oops"}');
    const e = createTrajectoryEngine(vs, { useWorker: false, storageKey: 'corrupt' });
    expect(e.learning.memory.length).toBe(0);
    e.dispose();
  });

  it('survives storage that throws', async () => {
    const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const spySet = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const e = createTrajectoryEngine(vs, { useWorker: false, storageKey: 'blocked' });
    await e.query(queries[1], { k: 10 });
    e.dispose();
    spy.mockRestore();
    spySet.mockRestore();
  });

  it('rejects ready on a build failure and queries after dispose', async () => {
    const broken = createTrajectoryEngine({ count: 5, dim: 4, data: new Float32Array(3) }, { useWorker: false });
    await expect(broken.ready).rejects.toThrow();
    await expect(broken.query(new Float32Array(4))).rejects.toThrow();
    const e = createTrajectoryEngine(vs, { useWorker: false });
    e.dispose();
    await expect(e.ready).rejects.toThrow(/dispos|abort/i);
    await expect(e.query(queries[0])).rejects.toThrow();
  });

  it('rejects a query of the wrong dimension', async () => {
    const e = createTrajectoryEngine(vs, { useWorker: false });
    await expect(e.query(new Float32Array(7))).rejects.toThrow(/dimension/i);
    e.dispose();
  });

  it('unsubscribes progress listeners', async () => {
    const e = createTrajectoryEngine(vs, { useWorker: false });
    const cb = vi.fn();
    const off = e.onProgress(cb);
    off();
    await e.ready;
    expect(cb).not.toHaveBeenCalled();
    e.dispose();
  });
});

describe('worker build protocol', () => {
  it('posts monotone progress, then a packed graph equal to the main-thread build', async () => {
    const { runBuildRequest } = await import('../workerProtocol');
    const { buildHnsw } = await import('../hnsw');
    const { unpackGraph } = await import('../graphCodec');
    const vs = clusteredVectors(500, 16, 5, 8);
    const params = { M: 8, efConstruction: 40, seed: 12 };
    const msgs: import('../workerProtocol').BuildResponse[] = [];
    const transfers: (Transferable[] | undefined)[] = [];
    runBuildRequest({ type: 'build', count: vs.count, dim: vs.dim, data: vs.data.slice(), params }, (m, t) => {
      msgs.push(m);
      transfers.push(t);
    });
    const progress = msgs.filter((m) => m.type === 'progress') as { done: number; total: number }[];
    expect(progress.length).toBeGreaterThan(10);
    for (let i = 1; i < progress.length; i++) expect(progress[i].done).toBeGreaterThan(progress[i - 1].done);
    expect(progress[progress.length - 1]).toMatchObject({ done: 500, total: 500 });
    const done = msgs[msgs.length - 1];
    expect(done.type).toBe('done');
    expect(transfers[transfers.length - 1]?.length).toBe(2);
    if (done.type !== 'done') return;
    const expected = await buildHnsw(vs, params);
    expect(unpackGraph(done.graph).links).toEqual(expected.links);
  });

  it('reports invalid input as an error message', async () => {
    const { runBuildRequest } = await import('../workerProtocol');
    const msgs: unknown[] = [];
    runBuildRequest(
      { type: 'build', count: 4, dim: 4, data: new Float32Array(3), params: { M: 8, efConstruction: 40, seed: 1 } },
      (m) => msgs.push(m),
    );
    expect(msgs).toHaveLength(1);
    expect(msgs[0]).toMatchObject({ type: 'error' });
  });
});
