/**
 * TimelineScrubber — bottom-docked bi-temporal scrubber for the ADR-049
 * provenance timeline (W-G phase-1, client-only, DEFAULT-OFF).
 *
 * Renders a range slider over a time domain (min = earliest `validFrom` seen or
 * now-30d, max = now). Dragging it calls `fetchStateAtSubjects(t)` (via the timeline
 * store) and publishes the valid runtime-assertion subject set that the graph
 * render consumes through `nodeTimelineStatus` to fade / highlight nodes.
 *
 * A diff-mode toggle exposes a second handle: it loads state-at at `t1` and `t2`
 * and marks subjects added (green) / retracted (red) between them. The diff is
 * computed entirely client-side from two state-at calls — there is no server
 * diff endpoint (see the pinned HTTP contract).
 *
 * This component only mounts when `provenance.enableTimeline` is on (gated by the
 * caller in the graph view). It is a plain HTML overlay, not an R3F node, so it
 * lives beside the <Canvas>, not inside it.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useSettingField } from '../../control-center/hooks/useSettingField';
import { useTimelineStore } from '../../../store/timelineStore';

/** Slider granularity — 1000 steps across the domain is smooth and cheap. */
const SLIDER_STEPS = 1000;
/** Debounce on scrub → fetch so dragging doesn't storm the endpoint (ms). */
const SCRUB_DEBOUNCE_MS = 180;

/** Map a [0..SLIDER_STEPS] slider position to an epoch-ms instant in the domain. */
function posToMs(pos: number, minMs: number, maxMs: number): number {
  const frac = pos / SLIDER_STEPS;
  return Math.round(minMs + frac * (maxMs - minMs));
}

/** Map an epoch-ms instant back to the nearest slider position. */
function msToPos(ms: number, minMs: number, maxMs: number): number {
  if (maxMs <= minMs) return SLIDER_STEPS;
  const frac = (ms - minMs) / (maxMs - minMs);
  return Math.round(Math.min(1, Math.max(0, frac)) * SLIDER_STEPS);
}

/** Compact UTC label for a domain instant. */
function fmt(ms: number): string {
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return '—';
  return d.toISOString().replace('T', ' ').slice(0, 16) + 'Z';
}

const panelStyle: React.CSSProperties = {
  position: 'absolute',
  // ADR-2189 §4: sit just above the dock (measured by GlassDock), never under it.
  bottom: 'var(--cc-dock-clearance, 9rem)',
  left: '50%',
  transform: 'translateX(-50%)',
  zIndex: 120,
  width: 'min(680px, 92vw)',
  boxSizing: 'border-box',
  backgroundColor: 'rgba(0, 0, 0, 0.62)',
  color: 'rgba(255,255,255,0.82)',
  padding: '10px 14px',
  borderRadius: '6px',
  fontFamily: 'monospace',
  fontSize: '11px',
  letterSpacing: '0.03em',
  border: '1px solid rgba(255,255,255,0.12)',
  backdropFilter: 'blur(4px)',
  userSelect: 'none',
};

const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: '10px',
};

const TimelineScrubber: React.FC = () => {
  // One control (ADR-2189): the Diff checkbox here and Explore › Timeline ›
  // Timeline diff in the settings panel are the same setting, written through
  // the one write path. The timeline store's diffMode follows it.
  const [settingDiffModeRaw, setSettingDiffMode] = useSettingField<boolean>('provenance.diffMode');
  const settingDiffMode = settingDiffModeRaw ?? false;

  const domainMinMs = useTimelineStore(s => s.domainMinMs);
  const domainMaxMs = useTimelineStore(s => s.domainMaxMs);
  const diffMode = useTimelineStore(s => s.diffMode);
  const loading = useTimelineStore(s => s.loading);
  const validCount = useTimelineStore(s => s.validSubjects.size);
  const addedCount = useTimelineStore(s => s.addedSubjects.size);
  const retractedCount = useTimelineStore(s => s.retractedSubjects.size);
  const loadStateAt = useTimelineStore(s => s.loadStateAt);
  const loadDiff = useTimelineStore(s => s.loadDiff);
  const setDiffMode = useTimelineStore(s => s.setDiffMode);
  const reset = useTimelineStore(s => s.reset);

  // Local slider positions (single `pos` in normal mode; `pos1`/`pos2` in diff).
  const [pos, setPos] = useState(SLIDER_STEPS);
  const [pos1, setPos1] = useState(Math.round(SLIDER_STEPS * 0.5));
  const [pos2, setPos2] = useState(SLIDER_STEPS);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // The store follows the setting, whichever surface flipped it. On a flip
  // (not on mount, where the initial load below runs) reload the right view.
  const diffRangeRef = useRef({ ms1: 0, ms2: 0, currentMs: 0 });
  const diffMountedRef = useRef(false);
  useEffect(() => {
    setDiffMode(settingDiffMode);
    const first = !diffMountedRef.current;
    diffMountedRef.current = true;
    const { ms1: a, ms2: b, currentMs: c } = diffRangeRef.current;
    if (settingDiffMode) {
      scheduleLoad(() => void loadDiff(new Date(a).toISOString(), new Date(b).toISOString()));
    } else if (!first) {
      scheduleLoad(() => void loadStateAt(new Date(c).toISOString()));
    }
    // scheduleLoad/loadDiff/loadStateAt are stable store actions + a ref-backed helper.
  }, [settingDiffMode, setDiffMode]);

  // Initial load: highlight state at "now" so the overlay isn't blank on mount.
  useEffect(() => {
    const t = new Date(posToMs(SLIDER_STEPS, domainMinMs, domainMaxMs)).toISOString();
    void loadStateAt(t);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      reset();
    };
    // Mount-once: domain is stable enough at first paint; later widening only
    // shifts labels, it must not re-fire the initial load.
    // Deps intentionally narrowed; react-hooks/exhaustive-deps is not enforced in this config.
  }, []);

  const currentMs = useMemo(() => posToMs(pos, domainMinMs, domainMaxMs), [pos, domainMinMs, domainMaxMs]);
  const ms1 = useMemo(() => posToMs(Math.min(pos1, pos2), domainMinMs, domainMaxMs), [pos1, pos2, domainMinMs, domainMaxMs]);
  const ms2 = useMemo(() => posToMs(Math.max(pos1, pos2), domainMinMs, domainMaxMs), [pos1, pos2, domainMinMs, domainMaxMs]);

  diffRangeRef.current = { ms1, ms2, currentMs };

  function scheduleLoad(fn: () => void) {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(fn, SCRUB_DEBOUNCE_MS);
  }

  const onScrub = (next: number) => {
    setPos(next);
    const t = new Date(posToMs(next, domainMinMs, domainMaxMs)).toISOString();
    scheduleLoad(() => void loadStateAt(t));
  };

  const onScrubDiff = (which: 1 | 2, next: number) => {
    const nextPos1 = which === 1 ? next : pos1;
    const nextPos2 = which === 2 ? next : pos2;
    if (which === 1) setPos1(next); else setPos2(next);
    const lo = Math.min(nextPos1, nextPos2);
    const hi = Math.max(nextPos1, nextPos2);
    const t1 = new Date(posToMs(lo, domainMinMs, domainMaxMs)).toISOString();
    const t2 = new Date(posToMs(hi, domainMinMs, domainMaxMs)).toISOString();
    scheduleLoad(() => void loadDiff(t1, t2));
  };

  // Writes the setting; the effect above moves the store and reloads.
  const onToggleDiff = () => setSettingDiffMode(!settingDiffMode);

  return (
    <div style={panelStyle} role="group" aria-label="Provenance timeline scrubber">
      <div style={{ ...rowStyle, justifyContent: 'space-between', marginBottom: '8px' }}>
        <span style={{ color: 'rgba(255,255,255,0.6)' }}>
          Timeline{loading ? ' · loading…' : ''}
        </span>
        <label style={{ ...rowStyle, gap: '6px', cursor: 'pointer' }}>
          <input
            type="checkbox"
            checked={settingDiffMode}
            onChange={onToggleDiff}
            aria-label="Timeline diff"
          />
          <span style={{ color: settingDiffMode ? '#34d399' : 'rgba(255,255,255,0.6)' }}>Diff</span>
        </label>
      </div>

      {!diffMode ? (
        <>
          <div style={rowStyle}>
            <span style={{ minWidth: '132px', color: 'rgba(255,255,255,0.5)' }}>{fmt(domainMinMs)}</span>
            <input
              type="range"
              min={0}
              max={SLIDER_STEPS}
              step={1}
              value={pos}
              onChange={e => onScrub(Number(e.target.value))}
              style={{ flex: 1 }}
              aria-label="Valid-time instant"
            />
            <span style={{ minWidth: '132px', textAlign: 'right', color: 'rgba(255,255,255,0.5)' }}>{fmt(domainMaxMs)}</span>
          </div>
          <div style={{ ...rowStyle, justifyContent: 'space-between', marginTop: '6px' }}>
            <span style={{ color: '#8ab4ff' }}>t = {fmt(currentMs)}</span>
            <span style={{ color: 'rgba(255,255,255,0.6)' }}>{validCount} valid</span>
          </div>
        </>
      ) : (
        <>
          <div style={rowStyle}>
            <span style={{ minWidth: '52px', color: 'rgba(255,255,255,0.5)' }}>t1</span>
            <input
              type="range"
              min={0}
              max={SLIDER_STEPS}
              step={1}
              value={pos1}
              onChange={e => onScrubDiff(1, Number(e.target.value))}
              style={{ flex: 1 }}
              aria-label="Diff start instant"
            />
          </div>
          <div style={{ ...rowStyle, marginTop: '4px' }}>
            <span style={{ minWidth: '52px', color: 'rgba(255,255,255,0.5)' }}>t2</span>
            <input
              type="range"
              min={0}
              max={SLIDER_STEPS}
              step={1}
              value={pos2}
              onChange={e => onScrubDiff(2, Number(e.target.value))}
              style={{ flex: 1 }}
              aria-label="Diff end instant"
            />
          </div>
          <div style={{ ...rowStyle, justifyContent: 'space-between', marginTop: '6px', flexWrap: 'wrap', gap: '6px' }}>
            <span style={{ color: '#8ab4ff' }}>{fmt(ms1)} → {fmt(ms2)}</span>
            <span>
              <span style={{ color: '#34d399' }}>+{addedCount} added</span>
              {'  '}
              <span style={{ color: '#f87171' }}>-{retractedCount} retracted</span>
            </span>
          </div>
        </>
      )}
    </div>
  );
};

export default TimelineScrubber;
