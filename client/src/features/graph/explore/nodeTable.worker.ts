/**
 * Node-table worker (ADR-2184 D1): builds the columns off the main thread and
 * transfers the typed arrays back. Same-origin module worker, so it satisfies
 * COEP require-corp without extra headers.
 */
import { buildTable, transferablesOf, type TableInput } from './buildTable';

export interface TableRequest { id: number; input: TableInput }
export type TableResponse =
  | { id: number; ok: true; built: ReturnType<typeof buildTable> }
  | { id: number; ok: false; error: string };

const scope = self as unknown as {
  onmessage: ((e: MessageEvent<TableRequest>) => void) | null;
  postMessage: (msg: TableResponse, transfer?: Transferable[]) => void;
};

scope.onmessage = (e) => {
  const { id, input } = e.data;
  try {
    const built = buildTable(input);
    scope.postMessage({ id, ok: true, built }, transferablesOf(built));
  } catch (err) {
    scope.postMessage({ id, ok: false, error: err instanceof Error ? err.message : String(err) });
  }
};
