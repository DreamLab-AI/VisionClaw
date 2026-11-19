import { describe, it, expect, vi } from 'vitest';
import { installMeasureGuard } from '../devMeasureGuard';

function cloneError() {
  const e = new Error('Data cannot be cloned, out of memory');
  e.name = 'DataCloneError';
  return e;
}

describe('installMeasureGuard', () => {
  it('retries without detail when the first call throws, and returns the retry result', () => {
    const entry = { name: 'x' };
    const measure = vi.fn((_n: unknown, o?: { detail?: unknown }) => {
      if (o && 'detail' in o) throw cloneError();
      return entry;
    });
    const host = { measure } as { measure: (...a: unknown[]) => unknown };
    expect(installMeasureGuard(host)).toBe(true);

    const out = host.measure('c', { start: 1, end: 2, detail: { devtools: { properties: [] } } });
    expect(out).toBe(entry);
    expect(measure).toHaveBeenCalledTimes(2);
    expect(measure.mock.calls[1][1]).toEqual({ start: 1, end: 2 });
  });

  it('never propagates, even when the retry also throws', () => {
    const host = { measure: vi.fn(() => { throw cloneError(); }) } as { measure: (...a: unknown[]) => unknown };
    installMeasureGuard(host);
    expect(() => host.measure('c', { start: 1, detail: {} })).not.toThrow();
    expect(() => host.measure('c')).not.toThrow();
  });

  it('passes a healthy call straight through once', () => {
    const measure = vi.fn(() => 'ok');
    const host = { measure } as { measure: (...a: unknown[]) => unknown };
    installMeasureGuard(host);
    expect(host.measure('c', { detail: { a: 1 } })).toBe('ok');
    expect(measure).toHaveBeenCalledTimes(1);
  });

  it('is idempotent', () => {
    const host = { measure: vi.fn() } as { measure: (...a: unknown[]) => unknown };
    expect(installMeasureGuard(host)).toBe(true);
    const wrapped = host.measure;
    expect(installMeasureGuard(host)).toBe(false);
    expect(host.measure).toBe(wrapped);
  });
});
