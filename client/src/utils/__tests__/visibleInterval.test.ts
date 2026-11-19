import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { visibleInterval } from '../visibleInterval';

function setHidden(hidden: boolean) {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => hidden });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('visibleInterval', () => {
  beforeEach(() => { vi.useFakeTimers(); setHidden(false); });
  afterEach(() => { vi.useRealTimers(); setHidden(false); });

  it('ticks on schedule while the tab is visible (3 s poll: 10 ticks in 30 s)', () => {
    const fn = vi.fn();
    const stop = visibleInterval(fn, 3000);
    vi.advanceTimersByTime(30_000);
    expect(fn).toHaveBeenCalledTimes(10);
    stop();
  });

  it('makes no calls over 30 s in a hidden tab, then one on return', () => {
    const fn = vi.fn();
    setHidden(true);
    const stop = visibleInterval(fn, 3000);
    vi.advanceTimersByTime(30_000);
    expect(fn).not.toHaveBeenCalled();
    setHidden(false);
    expect(fn).toHaveBeenCalledTimes(1);
    stop();
  });

  it('stop clears the timer and the listener', () => {
    const fn = vi.fn();
    const stop = visibleInterval(fn, 1000);
    stop();
    vi.advanceTimersByTime(5000);
    setHidden(true); setHidden(false);
    expect(fn).not.toHaveBeenCalled();
  });
});
