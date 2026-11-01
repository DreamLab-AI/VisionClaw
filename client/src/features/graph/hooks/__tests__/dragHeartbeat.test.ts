import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DRAG_HEARTBEAT_MS, startDragHeartbeat } from '../dragHeartbeat';

// The server drops a hold after 2000 ms without an update (ADR-2169).
const SERVER_DRAG_TIMEOUT_MS = 2000;

describe('startDragHeartbeat', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('re-sends well inside the server timeout while the pointer is still', () => {
    const send = vi.fn();
    const stop = startDragHeartbeat(send);
    vi.advanceTimersByTime(SERVER_DRAG_TIMEOUT_MS);
    // at least three sends inside any one timeout window
    expect(send.mock.calls.length).toBeGreaterThanOrEqual(3);
    expect(DRAG_HEARTBEAT_MS).toBeLessThan(SERVER_DRAG_TIMEOUT_MS / 2);
    stop();
  });

  it('stops sending once stopped', () => {
    const send = vi.fn();
    const stop = startDragHeartbeat(send);
    vi.advanceTimersByTime(DRAG_HEARTBEAT_MS);
    stop();
    const n = send.mock.calls.length;
    vi.advanceTimersByTime(SERVER_DRAG_TIMEOUT_MS * 3);
    expect(send.mock.calls.length).toBe(n);
  });
});
