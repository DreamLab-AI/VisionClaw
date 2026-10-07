/**
 * Messages between the trajectory engine and the HNSW build worker, and the
 * worker-side handler (kept here, outside the worker entry, so it can be
 * tested without a Worker).
 */
import type { HnswParams } from './types';
import { HnswBuilder } from './hnsw';
import { packGraph, type PackedGraph } from './graphCodec';

export interface BuildRequest {
  type: 'build';
  count: number;
  dim: number;
  /** row-major vectors; transferred to the worker */
  data: Float32Array;
  params: HnswParams;
}

export type BuildResponse =
  | { type: 'progress'; done: number; total: number }
  | { type: 'done'; graph: PackedGraph }
  | { type: 'error'; message: string };

export type PostResponse = (msg: BuildResponse, transfer?: Transferable[]) => void;

/** Number of progress messages a build sends at most (plus the final one). */
const PROGRESS_STEPS = 100;

/** Runs a build request to completion, posting progress, then the packed graph (or an error). */
export function runBuildRequest(req: BuildRequest, post: PostResponse): void {
  try {
    if (!req || req.type !== 'build') throw new Error('unknown build request');
    const builder = new HnswBuilder({ count: req.count, dim: req.dim, data: req.data }, req.params);
    const total = builder.total;
    const every = Math.max(1, Math.floor(total / PROGRESS_STEPS));
    while (builder.step()) {
      const done = builder.inserted;
      if (done % every === 0 && done < total) post({ type: 'progress', done, total });
    }
    post({ type: 'progress', done: total, total });
    const graph = packGraph(builder.finish());
    post({ type: 'done', graph }, [graph.levels.buffer, graph.links.buffer]);
  } catch (e) {
    post({ type: 'error', message: e instanceof Error ? e.message : String(e) });
  }
}
