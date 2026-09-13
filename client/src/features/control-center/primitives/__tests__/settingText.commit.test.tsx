/**
 * SettingText commit-path regression (DEFECT C).
 *
 * Typing into a text field (perplexity.model / pocketTts.defaultVoice) must land in
 * the settings store on BOTH blur and Enter — exactly like the sliders in the
 * same group, which write through the same useSettingField setter. The network
 * side (autoSaveManager) is mocked so this asserts only the in-memory store
 * receives the value.
 */

import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { SettingRow } from '../SettingRow';
import type { RegistryField } from '../../registry/types';
import { useSettingsStore } from '@/store/settingsStore';

vi.mock('@/store/autoSaveManager', () => ({
  autoSaveManager: {
    queueChange: vi.fn(),
    queueChanges: vi.fn(),
    initialize: vi.fn(),
    isInitialized: false,
  },
}));

import { autoSaveManager } from '@/store/autoSaveManager';

function readPath(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>(
    (o, k) => (o == null ? undefined : (o as Record<string, unknown>)[k]),
    obj
  );
}

const modelField: RegistryField = {
  key: 'perplexityModel',
  label: 'Perplexity Model',
  type: 'text',
  path: 'perplexity.model',
};

const voiceField: RegistryField = {
  key: 'pocketTtsVoice',
  label: 'Default Voice',
  type: 'text',
  path: 'pocketTts.defaultVoice',
};

describe('SettingText commit path (DEFECT C)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      settings: {},
      partialSettings: {},
      loadedPaths: new Set(),
    } as never);
  });

  it('commits typed text to the store on blur', () => {
    render(<SettingRow field={modelField} groupId="ai" />);
    const input = screen.getByTestId('setting-perplexity.model') as HTMLInputElement;

    fireEvent.change(input, { target: { value: 'sonar-pro' } });
    fireEvent.blur(input);

    expect(readPath(useSettingsStore.getState().settings, 'perplexity.model')).toBe('sonar-pro');
    expect(autoSaveManager.queueChange).toHaveBeenCalledWith('perplexity.model', 'sonar-pro');
  });

  it('commits typed text to the store on Enter', () => {
    render(<SettingRow field={voiceField} groupId="ai" />);
    const input = screen.getByTestId('setting-pocketTts.defaultVoice') as HTMLInputElement;

    fireEvent.change(input, { target: { value: 'af_bella' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(readPath(useSettingsStore.getState().settings, 'pocketTts.defaultVoice')).toBe('af_bella');
    expect(autoSaveManager.queueChange).toHaveBeenCalledWith('pocketTts.defaultVoice', 'af_bella');
  });
});
