import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  agentOffsetsEnabled,
  setAgentOffsetsEnabled,
  subscribeAgentOffsets,
  __resetAgentOffsetPrefsForTest,
} from '../agentOffsetPrefs';

describe('agentOffsetPrefs', () => {
  beforeEach(() => {
    try { localStorage.clear(); } catch { /* storage unavailable */ }
    __resetAgentOffsetPrefsForTest();
  });

  it('defaults to enabled, matching the long-standing behaviour and the XR default', () => {
    expect(agentOffsetsEnabled()).toBe(true);
  });

  it('toggles, notifies subscribers once per real change, and stops after unsubscribe', () => {
    const cb = vi.fn();
    const off = subscribeAgentOffsets(cb);
    setAgentOffsetsEnabled(false);
    setAgentOffsetsEnabled(false); // no change: no notification
    expect(agentOffsetsEnabled()).toBe(false);
    expect(cb).toHaveBeenCalledTimes(1);
    off();
    setAgentOffsetsEnabled(true);
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it('persists across reloads', () => {
    setAgentOffsetsEnabled(false);
    __resetAgentOffsetPrefsForTest(); // simulate a fresh module load reading storage
    expect(agentOffsetsEnabled()).toBe(false);
  });

  it('still works when storage throws', () => {
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('denied'); });
    expect(() => setAgentOffsetsEnabled(false)).not.toThrow();
    expect(agentOffsetsEnabled()).toBe(false);
    spy.mockRestore();
  });
});
