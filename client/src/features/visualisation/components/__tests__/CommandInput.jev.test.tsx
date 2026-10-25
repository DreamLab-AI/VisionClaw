import React from 'react';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';

const post = vi.fn();
const get = vi.fn();
const request = vi.fn();
const storeSet = vi.fn();

vi.mock('../../../../services/api/UnifiedApiClient', () => ({
  unifiedApiClient: {
    post: (...a: unknown[]) => post(...a),
    get: (...a: unknown[]) => get(...a),
    request: (...a: unknown[]) => request(...a),
  },
}));

vi.mock('../../../../store/settingsStore', () => ({
  useSettingsStore: Object.assign(() => ({}), {
    getState: () => ({
      get: (p: string) => (p === 'physics.repelK' ? 100 : undefined),
      set: (...a: unknown[]) => storeSet(...a),
      updateSettings: vi.fn(),
    }),
  }),
}));

vi.mock('../../../control-center/registry/commandInputCompat', () => ({ UNIFIED_SETTINGS_CONFIG: {} }));
vi.mock('../../../control-center/registry/settingsRegistry', () => ({
  ALL_PATHS: ['physics.repelK'],
  ALL_FIELDS: [{ key: 'repel', path: 'physics.repelK', label: 'Repulsion', type: 'slider', min: 0, max: 1000, step: 10 }],
}));

import { CommandInput } from '../CommandInput';
import { resetJevStatusCache } from '../../../../services/jev/jevClient';

const SETTINGS_ASSISTANT = '/bots/settings-command';

async function submit(text: string) {
  render(<CommandInput isCollapsed={true} />);
  const input = screen.getByPlaceholderText(/Configure view/i);
  fireEvent.change(input, { target: { value: text } });
  fireEvent.submit(input.closest('form')!);
}

describe('CommandInput Jev option', () => {
  beforeEach(() => {
    resetJevStatusCache();
    post.mockReset();
    get.mockReset();
    request.mockReset();
    storeSet.mockReset();
    post.mockImplementation(async (url: string) => {
      if (url === SETTINGS_ASSISTANT) return { data: { taskId: 'abcdef012345' } };
      return { data: { used: false } };
    });
  });

  it('flag off: an unmatched command goes to the settings assistant exactly as before and never to /jev/command', async () => {
    get.mockResolvedValue({ data: { enabled: {}, available: false } });
    await submit('zorp the flibbertigibbet');
    await waitFor(() => expect(post).toHaveBeenCalledWith(SETTINGS_ASSISTANT, expect.objectContaining({ command: 'zorp the flibbertigibbet' })));
    expect(post.mock.calls.some(([u]) => u === '/jev/command')).toBe(false);
    expect(screen.queryByRole('group', { name: /Jev suggestion/i })).toBeNull();
  });

  it('Jev error: falls back to the settings assistant call', async () => {
    get.mockResolvedValue({ data: { enabled: { 'settings-patch': true }, available: true } });
    post.mockImplementation(async (url: string) => {
      if (url === '/jev/command') throw new Error('http 500');
      return { data: { taskId: 'abcdef012345' } };
    });
    await submit('zorp the flibbertigibbet');
    await waitFor(() => expect(post).toHaveBeenCalledWith(SETTINGS_ASSISTANT, expect.anything()));
  });

  it('deterministic keyword commands never consult Jev', async () => {
    get.mockResolvedValue({ data: { enabled: { 'settings-patch': true }, available: true } });
    await submit('increase repulsion');
    await waitFor(() => expect(request).toHaveBeenCalled());
    expect(get).not.toHaveBeenCalled();
    expect(post.mock.calls.some(([u]) => u === '/jev/command')).toBe(false);
  });

  it('accept: shows a reviewed patch, does NOT call the assistant, and Apply uses the settings store', async () => {
    get.mockResolvedValue({ data: { enabled: { 'settings-patch': true, 'action-class': true }, available: true } });
    post.mockImplementation(async (url: string) => {
      if (url === '/jev/command') {
        return { data: { used: true, actionClass: { id: 'persistent_setting', confidence: 0.9 }, setting: { id: 'physics.repelK', confidence: 0.9 } } };
      }
      return { data: { taskId: 'x' } };
    });
    await submit('set the node push to 300');
    const panel = await screen.findByRole('group', { name: /Jev suggestion/i });
    expect(panel.textContent).toContain('Repulsion');
    expect(panel.textContent).toContain('300');
    expect(post.mock.calls.some(([u]) => u === SETTINGS_ASSISTANT)).toBe(false);
    expect(storeSet).not.toHaveBeenCalled(); // proposal only until the user clicks
    fireEvent.click(screen.getByText('Apply'));
    expect(storeSet).toHaveBeenCalledWith('physics.repelK', 300);
  });

  it('review can fall through to the assistant on request', async () => {
    get.mockResolvedValue({ data: { enabled: { 'action-class': true }, available: true } });
    post.mockImplementation(async (url: string) =>
      url === '/jev/command'
        ? { data: { used: true, actionClass: { id: 'temporary_physics_action', confidence: 0.9 } } }
        : { data: { taskId: 'x' } });
    await submit('pause everything');
    await screen.findByRole('group', { name: /Jev suggestion/i });
    expect(post.mock.calls.some(([u]) => u === SETTINGS_ASSISTANT)).toBe(false);
    fireEvent.click(screen.getByText(/Ask the settings assistant instead/i));
    await waitFor(() => expect(post).toHaveBeenCalledWith(SETTINGS_ASSISTANT, expect.objectContaining({ command: 'pause everything' })));
  });
});
