import React from 'react';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { act, render, screen, fireEvent, waitFor } from '@testing-library/react';

const request = vi.fn();
const applyWrites = vi.fn();
vi.mock('../../../control-center/state/settingWrites', async (orig) => ({
  ...(await orig<typeof import('../../../control-center/state/settingWrites')>()),
  applySettingWrites: (...a: unknown[]) => applyWrites(...a),
}));
vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: { post: vi.fn(async () => ({ data: {} })), get: vi.fn(async () => ({ data: {} })), request: (...a: unknown[]) => request(...a) },
}));
vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: Object.assign(() => ({}), { getState: () => ({ get: () => undefined, set: vi.fn(), updateSettings: vi.fn() }) }),
}));
vi.mock('../../../control-center/registry/commandInputCompat', () => ({ UNIFIED_SETTINGS_CONFIG: {} }));
vi.mock('../../../control-center/registry/settingsRegistry', () => ({ ALL_PATHS: [], ALL_FIELDS: [] }));
const followUp = vi.fn(async () => 'CLIENT-SIDE ANSWER');
vi.mock('../insightRunner', () => ({ runInsightFollowUp: (...a: unknown[]) => followUp(...a) }));

import { CommandInput } from '../CommandInput';

function submit(text: string) {
  render(<CommandInput isCollapsed={true} />);
  const input = screen.getByPlaceholderText(/Configure view/i);
  fireEvent.change(input, { target: { value: text } });
  fireEvent.submit(input.closest('form')!);
}

describe('CommandInput insight intents', () => {
  beforeEach(() => { request.mockReset(); applyWrites.mockReset(); });

  it('"what are the main themes" runs community detection and colours by community, not the LLM/search path', async () => {
    submit('what are the main themes?');
    await waitFor(() => expect(request).toHaveBeenCalledWith('POST', '/analytics/community/detect', { algorithm: 'leiden', resolution: 1.0 }));
    const writes = applyWrites.mock.calls[0][0] as Array<{ path: string; value: unknown }>;
    expect(writes).toContainEqual({ path: 'visualisation.graphs.knowledge.nodes.colorScheme', value: 'community' });
    expect(await screen.findByText(/Leiden community detection/)).toBeTruthy();
  });

  it('a question containing "find" is not swallowed by the search rule', async () => {
    const seen: unknown[] = [];
    window.addEventListener('visionclaw:search', (e) => seen.push(e));
    submit('find the outliers');
    await waitFor(() => expect(request).toHaveBeenCalledWith('POST', '/analytics/anomaly/detect', expect.objectContaining({ method: 'lof' })));
    expect(seen).toHaveLength(0);
  });

  it('a failed analytics request does not block the client-side answer (unsigned viewer, 401)', async () => {
    request.mockRejectedValue(new Error('HTTP 401: Unauthorized'));
    submit('what connects blockchain and robotics?');
    expect(await screen.findByText(/CLIENT-SIDE ANSWER/)).toBeTruthy();
    expect(await screen.findByText(/401/)).toBeTruthy();
  });

  it('keeps an insight answer readable after the transient status lines have faded', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      request.mockResolvedValue({});
      submit('what connects blockchain and robotics?');
      expect(await screen.findByText(/CLIENT-SIDE ANSWER/)).toBeTruthy();
      // Status lines fade out about 4.5 s after they appear; an answer of several
      // sentences cannot be read in that time.
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
      expect(screen.queryByText(/^Processing:/)).toBeNull();
      expect(screen.getByText(/CLIENT-SIDE ANSWER/)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('view commands still work: show hulls', async () => {
    submit('show hulls');
    await waitFor(() => expect(applyWrites).toHaveBeenCalled());
    expect(applyWrites.mock.calls[0][0]).toEqual([{ path: 'visualisation.clusterHulls.enabled', value: true }]);
    expect(request).not.toHaveBeenCalled();
  });
});
