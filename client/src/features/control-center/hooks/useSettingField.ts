/**
 * useSettingField — fine-grained per-path selector + legacy-faithful setter.
 *
 * Write semantics replicate UnifiedSettingsTabContent.updateSettingByPath
 * exactly: (a) mutate the settings store via the same `updateSettings` immer
 * draft path the legacy panel used, (b) explicitly call
 * `autoSaveManager.queueChange(path, value)` on top (the legacy panel does
 * both — `updateSettings` already queues internally via `queueChanges`, and
 * then queues again explicitly; that redundancy is preserved rather than
 * "fixed" here, per the no-new-persistence-flow constraint), and (c) trigger
 * the live layout API call for the two paths that need it. No new
 * persistence code is introduced.
 *
 * See design-spec.md §4.1 and §0.4.
 */

import { useCallback, useState } from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { applySettingWrites } from '../state/settingWrites';
import type { Settings } from '../../../features/settings/config/settings';


function getValueFromPath(settings: unknown, path: string): unknown {
  if (!path) return undefined;
  const keys = path.split('.');
  let value: unknown = settings;
  for (const key of keys) {
    if (value === undefined || value === null) return undefined;
    value = (value as Record<string, unknown>)[key];
  }
  return value;
}

export type SettingFieldSetter<T> = (value: T) => void;

/**
 * Fine-grained zustand selector: only the row reading this exact path
 * re-renders when it changes, never the whole panel.
 */
export function useSettingField<T = unknown>(path: string): [T | undefined, SettingFieldSetter<T>] {
  const value = useSettingsStore((state) => getValueFromPath(state.settings, path) as T | undefined);

  const set = useCallback<SettingFieldSetter<T>>(
    (next) => {
      if (!path) return;
      // The one write path (ADR-2189): store batch + autosave queue + layout side effect.
      applySettingWrites([{ path, value: next }]);
    },
    [path]
  );

  return [value, set];
}

/** Read-only variant for fields that never write (e.g. renderer capabilities). */
export function useSettingFieldValue<T = unknown>(path: string): T | undefined {
  return useSettingsStore((state) => getValueFromPath(state.settings, path) as T | undefined);
}

export interface LocalFieldMap<T extends Record<string, unknown>> {
  values: T;
  setValue: <K extends keyof T>(key: K, value: T[K]) => void;
}

/**
 * Plain useState map for transient localKey fields (e.g. the Analytics
 * "Run Grouping" method/params) — these are one-shot inputs to an action
 * endpoint, not settings, and never touch the settings store.
 */
export function useLocalFieldMap<T extends Record<string, unknown>>(initial: T): LocalFieldMap<T> {
  const [values, setValues] = useState<T>(initial);

  const setValue = useCallback(<K extends keyof T>(key: K, value: T[K]) => {
    setValues((prev) => ({ ...prev, [key]: value }));
  }, []);

  return { values, setValue };
}

export type { Settings };
