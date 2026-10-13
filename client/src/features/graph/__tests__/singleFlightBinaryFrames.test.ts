/**
 * T7 — ordered single-flight binary-frame test (ADR-03 D2, amended by ADR-2141).
 *
 * The proxy enforces:
 *   1. At most one frame is being processed across the Comlink await.
 *   2. Frames that arrive meanwhile queue in arrival order; NONE is collapsed or dropped (each is
 *      one chunk of the client's acknowledged stream, so a newer chunk never supersedes an older).
 *   3. Each call's promise resolves when THAT frame has been applied (when it may be acknowledged).
 *
 * We mock the Comlink-wrapped worker so processBinaryFrame() resolves after a controlled delay,
 * fire N frames rapidly and assert the resulting call sequence on the underlying API.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock comlink before importing the proxy so wrap/transfer are controllable.
vi.mock('comlink', () => ({
  wrap: vi.fn(),
  transfer: vi.fn((value: unknown, _transferList: unknown[]) => value),
  expose: vi.fn(),
}));

// Mock the worker module URL resolution (Vite import.meta.url is unavailable in jsdom).
vi.mock('../workers/graph.worker.ts?worker', () => ({
  default: class FakeWorker {
    onerror: ((e: unknown) => void) | null = null;
    terminate = vi.fn();
  },
}), { virtual: true });

// Mock Worker constructor for the `new Worker(new URL(...))` call.
class StubWorker {
  onerror: ((e: unknown) => void) | null = null;
  terminate = vi.fn();
}
(globalThis as unknown as { Worker: typeof StubWorker }).Worker = StubWorker;

// Pretend cross-origin isolation is unavailable so the proxy uses the Comlink path.
Object.defineProperty(globalThis, 'crossOriginIsolated', {
  value: false,
  writable: true,
  configurable: true,
});

// Import after mocks are wired.
import { wrap } from 'comlink';
import { graphWorkerProxy, WORKER_USES_SAB } from '../managers/graphWorkerProxy';

interface FakeWorkerApi {
  initialize: ReturnType<typeof vi.fn>;
  setupSharedPositions: ReturnType<typeof vi.fn>;
  processBinaryFrame: ReturnType<typeof vi.fn>;
  setGraphData: ReturnType<typeof vi.fn>;
}

describe('single-flight binary frame discipline (ADR-03 D2)', () => {
  let fakeApi: FakeWorkerApi;
  let resolvers: Array<() => void>;

  beforeEach(async () => {
    // Reset singleton state (the proxy is a singleton; we re-initialise).
    await graphWorkerProxy.dispose();

    resolvers = [];
    fakeApi = {
      initialize: vi.fn().mockResolvedValue(undefined),
      setupSharedPositions: vi.fn().mockResolvedValue(undefined),
      processBinaryFrame: vi.fn((_buf: ArrayBuffer) => {
        return new Promise<ArrayBuffer | void>(resolve => {
          resolvers.push(() => resolve(new ArrayBuffer(0)));
        });
      }),
      setGraphData: vi.fn().mockResolvedValue(undefined),
    };

    (wrap as ReturnType<typeof vi.fn>).mockReturnValue(fakeApi);

    await graphWorkerProxy.initialize();
  });

  it('forces Comlink path when crossOriginIsolated is false', () => {
    expect(WORKER_USES_SAB).toBe(false);
  });

  const settle = async () => {
    for (let i = 0; i < 6; i++) await new Promise((r) => queueMicrotask(() => r(undefined)));
  };

  it('processes every one of 10 rapid frames, in order, one at a time', async () => {
    const frames = Array.from({ length: 10 }, (_, i) => new Uint8Array([i]));
    const done = frames.map((f) => graphWorkerProxy.processBinaryFrame(f));

    await settle();
    // Only the first has reached the worker (single-flight).
    expect(fakeApi.processBinaryFrame).toHaveBeenCalledTimes(1);

    for (let i = 0; i < 10; i++) {
      resolvers[i]();
      await settle();
    }
    await Promise.all(done);

    expect(fakeApi.processBinaryFrame).toHaveBeenCalledTimes(10);
    const order = fakeApi.processBinaryFrame.mock.calls.map((c) => new Uint8Array(c[0] as ArrayBufferLike)[0]);
    expect(order).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(graphWorkerProxy.getStats().framesDropped).toBe(0);
  });

  it('a frame is only reported applied after the worker has applied it', async () => {
    let applied = false;
    const p = graphWorkerProxy.processBinaryFrame(new Uint8Array([1])).then(() => { applied = true; });
    await settle();
    expect(applied).toBe(false);
    resolvers[0]();
    await p;
    expect(applied).toBe(true);
  });

  it('a failing frame rejects only its own promise; the next frame still runs', async () => {
    fakeApi.processBinaryFrame.mockImplementationOnce(() => Promise.reject(new Error('worker error')));
    const bad = graphWorkerProxy.processBinaryFrame(new Uint8Array([1]));
    const good = graphWorkerProxy.processBinaryFrame(new Uint8Array([2]));
    await expect(bad).rejects.toThrow('worker error');
    await settle();
    resolvers[0]();
    await expect(good).resolves.toBeUndefined();
    expect(fakeApi.processBinaryFrame).toHaveBeenCalledTimes(2);
  });

  it('reports stats: framesProcessed counter increments on success', async () => {
    const f1 = new Uint8Array([1]);
    void graphWorkerProxy.processBinaryFrame(f1);
    resolvers[0]();
    await new Promise(r => queueMicrotask(() => r(undefined)));
    await new Promise(r => queueMicrotask(() => r(undefined)));

    expect(graphWorkerProxy.getStats().framesProcessed).toBeGreaterThanOrEqual(1);
  });
});
