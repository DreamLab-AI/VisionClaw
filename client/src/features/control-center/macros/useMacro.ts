/**
 * useMacro — forward-write + derive-read binding for one L1 macro dial.
 * design-spec.md §1.1, §3.1, §4.4.
 *
 * - READ: `value` derives the dial's 0..1 position from the current settings via
 *   `macro.derive`, subscribed fine-grained (the selector returns a number, so
 *   the dial re-renders only when its derived position actually changes).
 * - WRITE: `onChange(t)` runs `macro.apply(t)` and pushes every resulting path
 *   through the SAME write path as useSettingField — one immer batch into the
 *   settings store, then an explicit per-path `autoSaveManager.queueChange`
 *   (mirroring useSettingField's deliberate redundancy). No new persistence.
 * - COMMIT: `onCommit(t)` re-applies and fires a single echo pulse (gated by the
 *   echoPulseEnabled UI flag), never per drag tick.
 */

import { useCallback, useEffect, useRef } from 'react';
import type { MacroDef } from '../registry/types';
import { useSettingsStore } from '../../../store/settingsStore';
import { applySettingWrites } from '../state/settingWrites';
import { emitEchoPulse } from '../echo/echoPulseBus';
import { useControlCenterUI } from '../state/useControlCenterUI';

type MacroWrites = Array<{ path: string; value: number | boolean }>;

function getByPath(root: unknown, path: string): unknown {
  if (!path) return undefined;
  let cur: unknown = root;
  for (const key of path.split('.')) {
    if (cur == null || typeof cur !== 'object') return undefined;
    cur = (cur as Record<string, unknown>)[key];
  }
  return cur;
}

function applyMacroWrites(writes: MacroWrites): void {
  applySettingWrites(writes);
}

export interface MacroControl {
  value: number;
  /** plain-language readout of what the dial sets (aria-valuetext) */
  valueText: string;
  onChange: (t: number) => void;
  onCommit: (t: number) => void;
}

export function useMacro(macro: MacroDef): MacroControl {
  const value = useSettingsStore((s) => macro.derive((p) => getByPath(s.settings, p)));
  const valueText = useSettingsStore((s) => macro.describe((p) => getByPath(s.settings, p)));
  const echoEnabled = useControlCenterUI((s) => s.echoPulseEnabled);

  // rAF coalescing: a pointer drag fires onChange many times per frame (often
  // faster than the display refresh). Applying every tick means one immer batch
  // + full R3F fan-out + full label re-layout per event — the redraw thrash.
  // We instead keep only the LATEST t and flush a single write per animation
  // frame, so drag cost is bounded to one store update per rendered frame.
  const rafRef = useRef<number | null>(null);
  const pendingRef = useRef<number | null>(null);
  const hasRaf = typeof requestAnimationFrame === 'function';

  const flush = useCallback(() => {
    rafRef.current = null;
    const t = pendingRef.current;
    if (t == null) return;
    pendingRef.current = null;
    applyMacroWrites(macro.apply(t));
  }, [macro]);

  const cancelPending = useCallback(() => {
    if (rafRef.current != null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(rafRef.current);
    }
    rafRef.current = null;
    pendingRef.current = null;
  }, []);

  const onChange = useCallback(
    (t: number) => {
      if (!hasRaf) {
        // No rAF (SSR / test env): apply synchronously, matching legacy behaviour.
        applyMacroWrites(macro.apply(t));
        return;
      }
      pendingRef.current = t;
      if (rafRef.current == null) rafRef.current = requestAnimationFrame(flush);
    },
    [macro, flush, hasRaf],
  );

  const onCommit = useCallback(
    (t: number) => {
      // The commit value is authoritative — drop any frame still queued so the
      // final write is exactly `t`, never a stale coalesced tick landing after.
      cancelPending();
      applyMacroWrites(macro.apply(t));
      if (echoEnabled) emitEchoPulse({ origin: 'camera-center', strength: 0.6 });
    },
    [macro, echoEnabled, cancelPending],
  );

  // Flush-safe unmount: never invoke a store write after the dial is gone.
  useEffect(() => cancelPending, [cancelPending]);

  return { value, valueText, onChange, onCommit };
}
