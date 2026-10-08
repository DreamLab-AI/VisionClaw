/// <reference lib="webworker" />
/**
 * Vectors worker: fetch, read and decode one memory-cloud vectors blob away
 * from the busy main thread, then transfer the decoded floats back.
 * Protocol and rationale: vectorsFetch.ts. One request per worker; the page
 * terminates it after the answer, or to abort.
 */

import { runVectorsFetch, type VectorsFetchRequest, type VectorsWorkerMessage } from './vectorsFetch';

const scope = self as unknown as DedicatedWorkerGlobalScope;

scope.onmessage = async (e: MessageEvent<VectorsFetchRequest>) => {
  scope.postMessage({ type: 'started' } satisfies VectorsWorkerMessage);
  const result = await runVectorsFetch(e.data, (input, init) => fetch(input, init));
  scope.postMessage(result satisfies VectorsWorkerMessage, result.type === 'ok' ? [result.data.buffer] : []);
};
