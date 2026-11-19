import { describe, it, expect, vi } from 'vitest';
import { createAgentLogger } from '../loggerConfig';

describe('agent telemetry persistence', () => {
  it('does not write localStorage on each logAgentAction call', () => {
    const setItem = vi.spyOn(window.localStorage, 'setItem');
    const getItem = vi.spyOn(window.localStorage, 'getItem');
    const log = createAgentLogger('telemetry-test');
    for (let i = 0; i < 50; i++) log.logAgentAction('a', 't', 'act');
    expect(setItem).not.toHaveBeenCalledWith('agent-telemetry', expect.anything());
    expect(getItem).not.toHaveBeenCalledWith('agent-telemetry');
    expect(log.getAgentTelemetry()).toHaveLength(50);
    setItem.mockRestore();
    getItem.mockRestore();
  });
});
