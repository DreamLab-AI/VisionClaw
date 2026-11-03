/**
 * timelineStore — bi-temporal scrub state for the ADR-049 provenance timeline.
 *
 * The TimelineScrubber writes here (current instant `t`, or the `t1`/`t2` pair
 * in diff mode) by calling `fetchStateAtSubjects` and reducing the returned
 * assertion-version set into subject sets. The graph render consumes this store
 * via `nodeTimelineStatus(nodeId)` to fade / highlight nodes:
 *
 *   - normal mode : subjects valid at `t` read 'valid'; runtime subjects seen at
 *     any point but NOT valid at `t` read 'faded'; everything else (the atemporal
 *     corpus backdrop) reads 'neutral' and is untouched.
 *   - diff mode   : subjects added between `t1` and `t2` read 'added' (green),
 *     retracted subjects read 'retracted' (red), still-valid subjects read
 *     'valid', other known runtime subjects 'faded', the rest 'neutral'.
 *
 * Governed writes and, since ADR-2022, full corpus syncs (public class typing
 * and class relations) carry assertion-version entities, so state-at covers the
 * ontology classes as well as runtime decisions. It still never describes every
 * node: KG pages and literals are not versioned, and read 'neutral'. Subjects
 * are IRIs, so a node matches by its `owlClassIri` as well as by its
 * `String()`-coerced id (Rust returns numeric node ids).
 */

import { create } from 'zustand';
import { fetchStateAtSubjects, type StateAtSubjects } from '../features/graph/managers/dataManager/restClient';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('TimelineStore');

/** Per-node overlay classification the graph render maps to colour/opacity. */
export type TimelineNodeStatus = 'valid' | 'faded' | 'added' | 'retracted' | 'neutral';

/** Default look-back when no earlier `validFrom` has been observed yet (30 days). */
export const DEFAULT_DOMAIN_LOOKBACK_MS = 30 * 24 * 60 * 60 * 1000;

interface TimelineState {
  /** Whether the overlay is active (scrubber mounted + a state-at load has run). */
  active: boolean;
  /** Diff mode compares two instants instead of highlighting one. */
  diffMode: boolean;
  /** Current single-instant scrub position (RFC3339) in normal mode. */
  t: string | null;
  /** Diff endpoints (RFC3339). */
  t1: string | null;
  t2: string | null;
  /** Domain bounds for the slider, epoch-ms. min = earliest validFrom seen or now-30d. */
  domainMinMs: number;
  domainMaxMs: number;
  /** Subjects (String-coerced) valid at the current instant (`t`, or `t2` in diff mode). */
  validSubjects: Set<string>;
  /** Diff: subjects present at t2 but not t1. */
  addedSubjects: Set<string>;
  /** Diff: subjects present at t1 but not t2. */
  retractedSubjects: Set<string>;
  /** Union of every runtime subject ever observed — the set eligible for fading. */
  knownSubjects: Set<string>;
  loading: boolean;
  error: string | null;
  /** Bumped whenever the derived sets change, so renderers can cache on it. */
  version: number;

  /** Load state-at a single instant and populate `validSubjects` (normal mode). */
  loadStateAt: (t: string, recordedAsOf?: string) => Promise<void>;
  /** Load state-at two instants and populate added/retracted (diff mode). */
  loadDiff: (t1: string, t2: string, recordedAsOf?: string) => Promise<void>;
  /** Toggle diff mode; clears the derived sets so the render doesn't show stale colours. */
  setDiffMode: (on: boolean) => void;
  /** Deactivate and clear all derived state (scrubber unmount / flag off). */
  reset: () => void;
}

/** Widen the observed domain to include the projection's earliest `validFrom`. */
function earliestValidFromMs(projection: StateAtSubjects, floor: number): number {
  const ms = projection.earliestValidFrom ? Date.parse(projection.earliestValidFrom) : NaN;
  return Number.isFinite(ms) && ms < floor ? ms : floor;
}

/** The String-coerced subject set of a projection. */
function subjectSet(projection: StateAtSubjects): Set<string> {
  const set = new Set<string>();
  for (const s of projection.subjects) {
    if (s) set.add(String(s));
  }
  return set;
}

export const useTimelineStore = create<TimelineState>((set, get) => {
  const now = Date.now();
  return {
    active: false,
    diffMode: false,
    t: null,
    t1: null,
    t2: null,
    domainMinMs: now - DEFAULT_DOMAIN_LOOKBACK_MS,
    domainMaxMs: now,
    validSubjects: new Set<string>(),
    addedSubjects: new Set<string>(),
    retractedSubjects: new Set<string>(),
    knownSubjects: new Set<string>(),
    loading: false,
    error: null,
    version: 0,

    loadStateAt: async (t, recordedAsOf) => {
      set({ loading: true, error: null });
      try {
        const assertions = await fetchStateAtSubjects(t, recordedAsOf);
        const valid = subjectSet(assertions);
        set(state => {
          const known = new Set(state.knownSubjects);
          for (const s of valid) known.add(s);
          const earliest = earliestValidFromMs(assertions, state.domainMinMs);
          return {
            active: true,
            t,
            validSubjects: valid,
            addedSubjects: new Set<string>(),
            retractedSubjects: new Set<string>(),
            knownSubjects: known,
            domainMinMs: Math.min(state.domainMinMs, earliest),
            loading: false,
            version: state.version + 1,
          };
        });
      } catch (err) {
        // fetchStateAtSubjects already fails open; this only guards the reducer.
        logger.warn('loadStateAt reducer error:', err);
        set({ loading: false, error: 'timeline load failed' });
      }
    },

    loadDiff: async (t1, t2, recordedAsOf) => {
      set({ loading: true, error: null });
      try {
        const [a1, a2] = await Promise.all([
          fetchStateAtSubjects(t1, recordedAsOf),
          fetchStateAtSubjects(t2, recordedAsOf),
        ]);
        const s1 = subjectSet(a1);
        const s2 = subjectSet(a2);
        const added = new Set<string>();
        const retracted = new Set<string>();
        for (const s of s2) if (!s1.has(s)) added.add(s);
        for (const s of s1) if (!s2.has(s)) retracted.add(s);
        set(state => {
          const known = new Set(state.knownSubjects);
          for (const s of s1) known.add(s);
          for (const s of s2) known.add(s);
          const earliest = Math.min(
            earliestValidFromMs(a1, state.domainMinMs),
            earliestValidFromMs(a2, state.domainMinMs),
          );
          return {
            active: true,
            diffMode: true,
            t1,
            t2,
            // In diff mode, "valid now" is the later snapshot's subject set.
            validSubjects: s2,
            addedSubjects: added,
            retractedSubjects: retracted,
            knownSubjects: known,
            domainMinMs: Math.min(state.domainMinMs, earliest),
            loading: false,
            version: state.version + 1,
          };
        });
      } catch (err) {
        logger.warn('loadDiff reducer error:', err);
        set({ loading: false, error: 'timeline diff failed' });
      }
    },

    setDiffMode: (on) => {
      set(state => ({
        diffMode: on,
        addedSubjects: new Set<string>(),
        retractedSubjects: new Set<string>(),
        // leave validSubjects intact so normal-mode highlight persists on toggle-off
        version: state.version + 1,
      }));
      if (!on) {
        // Re-run the single-instant load so validSubjects reflects `t`, not `t2`.
        const t = get().t;
        if (t) void get().loadStateAt(t);
      }
    },

    reset: () => {
      const n = Date.now();
      set({
        active: false,
        diffMode: false,
        t: null,
        t1: null,
        t2: null,
        domainMinMs: n - DEFAULT_DOMAIN_LOOKBACK_MS,
        domainMaxMs: n,
        validSubjects: new Set<string>(),
        addedSubjects: new Set<string>(),
        retractedSubjects: new Set<string>(),
        knownSubjects: new Set<string>(),
        loading: false,
        error: null,
        version: get().version + 1,
      });
    },
  };
});

/**
 * Classify a graph node for the timeline overlay. Non-reactive read (safe to
 * call inside a render loop); the graph render maps the result to colour
 * (fade non-valid, green=added, red=retracted). A node matches a subject by its
 * class IRI (`iri`, the form corpus and governed versions use) or by its
 * `String()`-coerced id.
 */
export function nodeTimelineStatus(nodeId: string | number, iri?: string | null): TimelineNodeStatus {
  const s = useTimelineStore.getState();
  if (!s.active) return 'neutral';
  const id = String(nodeId);
  const has = (set: Set<string>) => set.has(id) || (!!iri && set.has(iri));

  if (s.diffMode) {
    if (has(s.addedSubjects)) return 'added';
    if (has(s.retractedSubjects)) return 'retracted';
    if (has(s.validSubjects)) return 'valid';
    return has(s.knownSubjects) ? 'faded' : 'neutral';
  }

  if (has(s.validSubjects)) return 'valid';
  return has(s.knownSubjects) ? 'faded' : 'neutral';
}

/**
 * React hook alias — components that only need to observe timeline state (rather
 * than the whole store surface) subscribe through this. Accepts an optional
 * selector so callers slice to just the fields they render on.
 */
export function useTimelineState<T>(selector: (state: TimelineState) => T): T {
  return useTimelineStore(selector);
}
