import { describe, it, expect } from 'vitest';
import { createTableBuilder } from '../tableBuilder';
import { buildTable, transferablesOf, type TableInput } from '../buildTable';
import type { TableRequest, TableResponse } from '../nodeTable.worker';
import { exploreNodes } from './fixtures';

/** A stand-in Worker that runs the worker's job asynchronously, as the real one does. */
class FakeWorker {
  onmessage: ((e: MessageEvent<TableResponse>) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  requests: TableRequest[] = [];
  terminated = false;
  constructor(private readonly mode: 'ok' | 'fail' | 'crash' = 'ok') {}
  postMessage(req: TableRequest) {
    this.requests.push(req);
    setTimeout(() => {
      if (this.mode === 'crash') { this.onerror?.({ preventDefault() {} } as ErrorEvent); return; }
      const res: TableResponse = this.mode === 'ok'
        ? { id: req.id, ok: true, built: buildTable(req.input) }
        : { id: req.id, ok: false, error: 'boom' };
      this.onmessage?.({ data: res } as MessageEvent<TableResponse>);
    }, 0);
  }
  terminate() { this.terminated = true; }
}

const input = (): TableInput => ({ nodes: exploreNodes(), nodeFilter: { includeLinkedPages: false } });

describe('node-table builder (ADR-2184 D1: off the main thread, main-thread fallback)', () => {
  it('builds in the worker and sends only the fields the table reads', async () => {
    const w = new FakeWorker();
    const b = createTableBuilder(() => w as unknown as Worker);
    const built = await b.build(input());
    expect(built.offThread).toBe(true);
    expect(built.table.rowCount).toBe(12);
    expect(built.eligible[11]).toBe(0); // linked_page stub: population scope
    expect(Object.keys(w.requests[0].input.nodes[0]).sort()).toEqual(['id', 'label', 'metadata']);
    expect(w.requests[0].input.nodeFilter).toEqual({ includeLinkedPages: false });
  });

  it('falls back to the main thread when the worker cannot be constructed', async () => {
    const b = createTableBuilder(() => { throw new Error('blocked by policy'); });
    const built = await b.build(input());
    expect(built.offThread).toBe(false);
    expect(built.table.rowCount).toBe(12);
  });

  it('falls back (and stays on the main thread) when the worker fails to load', async () => {
    const w = new FakeWorker('crash');
    let made = 0;
    const b = createTableBuilder(() => { made++; return w as unknown as Worker; });
    expect((await b.build(input())).offThread).toBe(false);
    expect(w.terminated).toBe(true);
    expect((await b.build(input())).offThread).toBe(false);
    expect(made).toBe(1);
  });

  it('falls back when the worker reports a build error', async () => {
    const b = createTableBuilder(() => new FakeWorker('fail') as unknown as Worker);
    const built = await b.build(input());
    expect(built.offThread).toBe(false);
    expect(built.table.columns.domain).toBeDefined();
  });

  it('transfers every typed-array buffer of the table', () => {
    const built = buildTable(input());
    const bufs = transferablesOf(built);
    expect(bufs).toContain(built.eligible.buffer);
    expect(bufs).toContain(built.table.present.buffer);
    const q = built.table.columns.quality_score;
    if (q.type !== 'numeric') throw new Error('numeric');
    expect(bufs).toContain(q.values.buffer);
  });
});
