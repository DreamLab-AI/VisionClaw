/**
 * Readiness wait for the graph worker.
 *
 * The worker is started only after sign-in (AppInitializer), while the data
 * manager is constructed at module load, on the sign-in screen. The readiness
 * timeout therefore starts when initialisation is REQUESTED, not when the wait
 * begins: time spent signing in never counts against the worker.
 */
export interface WorkerReadiness {
  /** True once someone has asked the worker to initialise. */
  isInitRequested(): boolean;
  isReady(): boolean;
}

export interface WorkerWaitOptions {
  pollMs: number;
  /** How long the worker has to become ready after initialisation is requested. */
  readyTimeoutMs: number;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export async function waitForWorkerReady(
  proxy: WorkerReadiness,
  { pollMs, readyTimeoutMs }: WorkerWaitOptions,
): Promise<'ready' | 'timeout'> {
  // No deadline here: nothing is wrong while the user is still signing in.
  while (!proxy.isReady() && !proxy.isInitRequested()) {
    await sleep(pollMs);
  }
  let waited = 0;
  while (!proxy.isReady() && waited < readyTimeoutMs) {
    await sleep(pollMs);
    waited += pollMs;
  }
  return proxy.isReady() ? 'ready' : 'timeout';
}
