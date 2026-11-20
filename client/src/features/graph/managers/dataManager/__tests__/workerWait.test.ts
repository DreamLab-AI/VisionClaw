// @ts-ignore - vitest types may not be available in all environments
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { waitForWorkerReady } from '../workerWait';

/**
 * The graph worker is started only after sign-in (AppInitializer). The data
 * manager is constructed at module load, on the sign-in screen. Its readiness
 * timeout must therefore start when worker initialisation is REQUESTED, not at
 * construction: a user who takes longer than the timeout to sign in must not
 * see "Graph Engine Initialization Failed".
 */
describe('waitForWorkerReady', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('does not time out while worker initialisation has not been requested (slow sign-in)', async () => {
    let requested = false;
    let ready = false;
    const proxy = { isInitRequested: () => requested, isReady: () => ready };
    const result = waitForWorkerReady(proxy, { pollMs: 10, readyTimeoutMs: 12_000 });
    let settled: string | null = null;
    result.then((r) => { settled = r; });

    // A minute on the sign-in screen: far past the readiness timeout.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(settled).toBeNull();

    // The user signs in; the worker is requested and comes up two seconds later.
    requested = true;
    await vi.advanceTimersByTimeAsync(2_000);
    expect(settled).toBeNull();
    ready = true;
    await vi.advanceTimersByTimeAsync(50);
    expect(settled).toBe('ready');
  });

  it('times out when initialisation was requested but the worker never becomes ready', async () => {
    const proxy = { isInitRequested: () => true, isReady: () => false };
    let settled: string | null = null;
    waitForWorkerReady(proxy, { pollMs: 10, readyTimeoutMs: 12_000 }).then((r) => { settled = r; });
    await vi.advanceTimersByTimeAsync(11_900);
    expect(settled).toBeNull();
    await vi.advanceTimersByTimeAsync(200);
    expect(settled).toBe('timeout');
  });

  it('resolves immediately when the worker is already ready', async () => {
    const proxy = { isInitRequested: () => true, isReady: () => true };
    let settled: string | null = null;
    waitForWorkerReady(proxy, { pollMs: 10, readyTimeoutMs: 12_000 }).then((r) => { settled = r; });
    await vi.advanceTimersByTimeAsync(1);
    expect(settled).toBe('ready');
  });
});
