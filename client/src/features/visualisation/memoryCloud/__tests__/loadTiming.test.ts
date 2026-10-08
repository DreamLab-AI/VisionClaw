import { describe, it, expect } from 'vitest';
import { createLoadTimer, formatLoadTiming } from '../loadTiming';

function clock(...ticks: number[]) {
  let i = 0;
  return () => ticks[Math.min(i++, ticks.length - 1)];
}

describe('loadTiming', () => {
  it('records the first occurrence of each step relative to the start', () => {
    // clock reads: start, snapshot, vectorsStart, ready, finish (the retry start is counted, not re-timed)
    const t = createLoadTimer(clock(1000, 1120, 1130, 9000, 9001));
    t.mark('snapshot');
    t.mark('vectorsStart');
    t.mark('vectorsStart');
    t.setSnapshotId('abc');
    t.mark('ready');
    const r = t.finish('ready');
    expect(r).toEqual({
      snapshotId: 'abc',
      outcome: 'ready',
      marks: { snapshot: 120, vectorsStart: 130, ready: 8000 },
      vectorsRequests: 2,
      vectorsReused: false,
      totalMs: 8001,
    });
  });

  it('freezes on finish: later marks and a second finish change nothing', () => {
    const t = createLoadTimer(clock(0, 5, 10, 20));
    t.mark('snapshot');
    const first = t.finish('aborted');
    t.mark('ready');
    expect(t.finish('ready')).toBe(first);
    expect(first.marks).toEqual({ snapshot: 5 });
  });

  it('formats one line in step order', () => {
    const line = formatLoadTiming({
      snapshotId: 'abc', outcome: 'ready', vectorsRequests: 1, vectorsReused: false, totalMs: 900,
      marks: { ready: 900, snapshot: 100, vectorsBody: 700 },
    });
    expect(line).toBe('[memoryCloud] load abc ready in 900 ms; vectors 1 request(s); ms: snapshot=100 vectorsBody=700 ready=900');
    expect(line).not.toContain('\n');
  });
});
