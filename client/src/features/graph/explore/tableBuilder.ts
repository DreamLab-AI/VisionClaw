/**
 * Builds the node table in a worker (ADR-2184 D1), falling back to the main
 * thread when a worker cannot start (no Worker, a construction error, a load
 * error) or reports a failure. Once the worker fails it is not retried.
 */
import { buildTable, leanNode, type BuiltTable, type TableInput } from './buildTable';
import type { TableRequest, TableResponse } from './nodeTable.worker';

export type WorkerFactory = () => Worker;

export const defaultWorkerFactory: WorkerFactory = () =>
  new Worker(new URL('./nodeTable.worker.ts', import.meta.url), { type: 'module', name: 'node-table' });

export interface TableBuilder {
  build(input: TableInput): Promise<BuiltTable & { offThread: boolean }>;
  dispose(): void;
}

export function createTableBuilder(factory: WorkerFactory | null = defaultWorkerFactory): TableBuilder {
  let worker: Worker | null = null;
  // No Worker global (jsdom, old runtimes): the default factory throws and ensure() falls back.
  let failed = factory === null;
  let nextId = 1;
  const pending = new Map<number, { resolve: (b: BuiltTable & { offThread: boolean }) => void; input: TableInput }>();

  const fallBackAll = () => {
    failed = true;
    worker?.terminate();
    worker = null;
    for (const [id, p] of pending) { pending.delete(id); p.resolve({ ...buildTable(p.input), offThread: false }); }
  };

  const ensure = (): Worker | null => {
    if (failed) return null;
    if (worker) return worker;
    try {
      worker = factory!();
    } catch {
      failed = true;
      return null;
    }
    worker.onmessage = (e: MessageEvent<TableResponse>) => {
      const p = pending.get(e.data.id);
      if (!p) return;
      pending.delete(e.data.id);
      if (e.data.ok) {
        p.resolve({ ...e.data.built, offThread: true });
      } else {
        fallBackAll();
        p.resolve({ ...buildTable(p.input), offThread: false });
      }
    };
    worker.onerror = (e) => { e.preventDefault?.(); fallBackAll(); };
    return worker;
  };

  return {
    build(input) {
      const w = ensure();
      if (!w) return Promise.resolve({ ...buildTable(input), offThread: false });
      const id = nextId++;
      return new Promise((resolve) => {
        pending.set(id, { resolve, input });
        const req: TableRequest = {
          id,
          input: {
            nodes: input.nodes.map(leanNode),
            indexMap: input.indexMap,
            analytics: input.analytics ?? null,
            nodeFilter: input.nodeFilter ? { includeLinkedPages: input.nodeFilter.includeLinkedPages } : null,
          },
        };
        try {
          w.postMessage(req);
        } catch {
          fallBackAll();
        }
      });
    },
    dispose() { worker?.terminate(); worker = null; },
  };
}
