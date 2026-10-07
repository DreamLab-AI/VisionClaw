/**
 * Module worker that builds the trajectory HNSW off the main thread. It
 * receives the vectors by transfer and returns the graph as flat typed
 * arrays (see graphCodec.ts). HNSW build adapted from the RuVector Explorer
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence).
 */
import { runBuildRequest, type BuildRequest, type BuildResponse } from './workerProtocol';

interface BuildWorkerScope {
  onmessage: ((e: MessageEvent<BuildRequest>) => void) | null;
  postMessage(msg: BuildResponse, transfer?: Transferable[]): void;
}

const scope = self as unknown as BuildWorkerScope;

scope.onmessage = (e) => {
  runBuildRequest(e.data, (msg, transfer) => scope.postMessage(msg, transfer ?? []));
};
