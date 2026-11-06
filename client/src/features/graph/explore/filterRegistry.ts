/**
 * The explore lane's writes to the highlight mask (ADR-2183 D2).
 *
 * Every filter and emphasis writer on the desktop goes through here instead of
 * calling `highlightMask.setFilter` / `setEmphasis` directly, for three reasons:
 *
 *  1. **Naming.** `activeSources()` on the mask mixes filters and emphasis, so
 *     sources are namespaced: `filter:*` greys (FILTERED_OUT), `emph:*` emphasises.
 *     Explore-panel widgets live under `filter:explore:*`, the persisted node-filter
 *     settings under `filter:settings`.
 *  2. **De-duplication.** `graphData` churns with the position stream, and every
 *     mask write is an O(n) recomposition plus a version bump that re-renders every
 *     subscriber. A write whose id set equals the one already in force is skipped.
 *  3. **Hide filtered.** `visualisation.graphView.explore.hideFiltered` removes the
 *     greyed nodes from the render set. useGraphFiltering asks `isFilteredOut(id)`,
 *     which answers by id from this registry — not from the mask bytes, which are
 *     indexed by a render map that is one render stale on a new graph load.
 *
 * Pure TS, no React; the hooks live in `useExploreFilters.ts`.
 */
import {
  highlightMask as sharedMask,
  type HighlightMask,
  type EmphasisFlag,
  type FilterMode,
  HIGHLIGHT_HOVERED,
} from '../state/highlightMask';

export const FILTER_PREFIX = 'filter:';
export const EMPHASIS_PREFIX = 'emph:';
/** Explore-panel widgets: `filter:explore:<widget>`. */
export const EXPLORE_FILTER_PREFIX = 'filter:explore:';

/** Source names used by the explore lane. */
export const SOURCE = Object.freeze({
  /** Persisted node-filter settings (quality / authority, minConnections, minMaturity). */
  SETTINGS_FILTER: 'filter:settings',
  /** The explore panel's combined crossfilter selection. */
  EXPLORE: 'filter:explore:crossfilter',
  SELECTION: 'emph:selection',
  NEIGHBOURS: 'emph:neighbours',
  HOVER: 'emph:hover',
  SEARCH: 'emph:search',
});

interface FilterEntry { mode: FilterMode; ids: Set<string> }

/** Per-mask registry of what this module last wrote. */
class Registry {
  readonly filters = new Map<string, FilterEntry>();
  readonly emphasis = new Map<string, { flag: EmphasisFlag; ids: Set<string> }>();
  readonly listeners = new Set<() => void>();
  version = 0;
  bump(): void { this.version++; for (const l of this.listeners) l(); }
}

const registries = new WeakMap<HighlightMask, Registry>();
function reg(mask: HighlightMask): Registry {
  let r = registries.get(mask);
  if (!r) {
    const created = new Registry();
    registries.set(mask, created);
    // Someone else may clear a source (mask.clearAll / clearSource): drop our record
    // of it, so isFilteredOut and hideFiltered follow the mask. Bumps only on a prune.
    mask.subscribe(() => {
      const live = new Set(mask.activeSources());
      let pruned = false;
      for (const k of [...created.filters.keys()]) if (!live.has(k)) { created.filters.delete(k); pruned = true; }
      for (const k of [...created.emphasis.keys()]) if (!live.has(k)) created.emphasis.delete(k);
      if (pruned) created.bump();
    });
    r = created;
  }
  return r;
}

function sameSet(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a.size !== b.size) return false;
  for (const v of a) if (!b.has(v)) return false;
  return true;
}

/** Was `source` written by us and is it still on the mask (nobody cleared it)? */
function liveOnMask(mask: HighlightMask, source: string): boolean {
  return mask.activeSources().includes(source);
}

/**
 * Set (or clear with `null`) a filter source. Skips the write when the same mode
 * and id set are already in force. Returns true when the mask changed.
 */
export function writeFilter(
  source: string,
  ids: Iterable<string> | null,
  mode: FilterMode = 'include',
  mask: HighlightMask = sharedMask,
): boolean {
  if (!source.startsWith(FILTER_PREFIX)) throw new Error(`filter source must start with '${FILTER_PREFIX}': ${source}`);
  const r = reg(mask);
  if (ids === null) {
    const had = r.filters.delete(source);
    const live = liveOnMask(mask, source);
    if (live) mask.clearSource(source);
    if (had) r.bump();
    return live;
  }
  const next = new Set(ids);
  const prev = r.filters.get(source);
  if (prev && prev.mode === mode && sameSet(prev.ids, next) && liveOnMask(mask, source)) return false;
  r.filters.set(source, { mode, ids: next });
  mask.setFilter(source, next, mode);
  r.bump();
  return true;
}

/** Set (or clear with `null`) an emphasis source, skipping a no-op write. */
export function writeEmphasis(
  source: string,
  flag: EmphasisFlag,
  ids: Iterable<string> | null,
  mask: HighlightMask = sharedMask,
): boolean {
  if (!source.startsWith(EMPHASIS_PREFIX)) throw new Error(`emphasis source must start with '${EMPHASIS_PREFIX}': ${source}`);
  const r = reg(mask);
  const next = ids === null ? null : new Set(ids);
  if (next === null || next.size === 0) {
    r.emphasis.delete(source);
    if (!liveOnMask(mask, source)) return false;
    mask.clearSource(source);
    return true;
  }
  const prev = r.emphasis.get(source);
  if (prev && prev.flag === flag && sameSet(prev.ids, next) && liveOnMask(mask, source)) return false;
  r.emphasis.set(source, { flag, ids: next });
  mask.setEmphasis(source, flag, next);
  return true;
}

/**
 * True when `id` is greyed by any filter source this module wrote: excluded by an
 * `exclude` source or missing from an `include` source. Index-free.
 */
export function isFilteredOut(id: string, mask: HighlightMask = sharedMask): boolean {
  for (const f of reg(mask).filters.values()) {
    if (f.mode === 'exclude' ? f.ids.has(id) : !f.ids.has(id)) return true;
  }
  return false;
}

/** Bumps whenever a filter source changes (for `hideFiltered` consumers). */
export function filterRegistryVersion(mask: HighlightMask = sharedMask): number {
  return reg(mask).version;
}

export function subscribeFilterRegistry(listener: () => void, mask: HighlightMask = sharedMask): () => void {
  const r = reg(mask);
  r.listeners.add(listener);
  return () => { r.listeners.delete(listener); };
}

/** Active filter sources on the mask (emphasis excluded). */
export function activeFilterSources(mask: HighlightMask = sharedMask): string[] {
  return mask.activeSources().filter((s) => s.startsWith(FILTER_PREFIX));
}

/**
 * Hover emphasis for the render lane: GraphManager owns hover state and calls this
 * on hover change (`null` clears).
 */
export function setHoveredNodeEmphasis(id: string | null, mask: HighlightMask = sharedMask): void {
  writeEmphasis(SOURCE.HOVER, HIGHLIGHT_HOVERED, id === null ? null : [id], mask);
}
