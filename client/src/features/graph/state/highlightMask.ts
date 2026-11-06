/**
 * Per-node highlight mask (ADR-2183).
 *
 * One `Uint8Array` of flag bits per node render index (see `nodeIndex.ts`). It is
 * display-only: it never reaches the GPU layout or the wire, and no writer removes
 * a node from the render set. Sprites, overview edges and labels read it.
 *
 * Independent writers compose through named SOURCES instead of writing bits
 * directly, so selection, search and every crossfilter widget can update without
 * clobbering one another:
 *   - a filter source owns an include- or exclude-set of node ids. FILTERED_OUT is
 *     set on a node excluded by ANY active filter source (the intersection of the
 *     active predicates is what stays lit);
 *   - an emphasis source owns one flag (SELECTED, HOVERED, SEARCH_HIT,
 *     FOCUS_NEIGHBOUR) over a set of ids; several sources may share a flag (OR).
 *
 * Sources are stored by node id, not by index, so a new graph load (a new index
 * map) recomposes the mask without the writers doing anything.
 *
 * Pure TS, no React: importable from a worker. The React hook lives in
 * `useHighlightMaskVersion.ts`.
 */
import { type NodeIndexResolver, sharedNodeIndexResolver, slotCountFor } from './nodeIndex';

// --- Flag bits ---------------------------------------------------------------

export const HIGHLIGHT_SELECTED = 1 << 0;
export const HIGHLIGHT_HOVERED = 1 << 1;
export const HIGHLIGHT_SEARCH_HIT = 1 << 2;
/** Greyed out: excluded by at least one active filter source. */
export const HIGHLIGHT_FILTERED_OUT = 1 << 3;
export const HIGHLIGHT_FOCUS_NEIGHBOUR = 1 << 4;

/** The flags an emphasis source may own. FILTERED_OUT is owned by filter sources only. */
export type EmphasisFlag =
  | typeof HIGHLIGHT_SELECTED
  | typeof HIGHLIGHT_HOVERED
  | typeof HIGHLIGHT_SEARCH_HIT
  | typeof HIGHLIGHT_FOCUS_NEIGHBOUR;

const EMPHASIS_MASK = HIGHLIGHT_SELECTED | HIGHLIGHT_HOVERED | HIGHLIGHT_SEARCH_HIT | HIGHLIGHT_FOCUS_NEIGHBOUR;

/** ADR-2183 D1's three display levels. */
export const HIGHLIGHT_LEVEL_GREYED = 0;
export const HIGHLIGHT_LEVEL_NORMAL = 1;
export const HIGHLIGHT_LEVEL_EMPHASISED = 2;
export type HighlightLevel = 0 | 1 | 2;

/**
 * Collapse a node's bits to ADR-2183's 0 / 1 / 2. Selection and hover always
 * emphasise, even on a greyed node (the user pointed at it); otherwise a greyed
 * node stays greyed, and a search hit or focus neighbour emphasises.
 */
export function highlightLevel(bits: number): HighlightLevel {
  if (bits & (HIGHLIGHT_SELECTED | HIGHLIGHT_HOVERED)) return HIGHLIGHT_LEVEL_EMPHASISED;
  if (bits & HIGHLIGHT_FILTERED_OUT) return HIGHLIGHT_LEVEL_GREYED;
  if (bits & EMPHASIS_MASK) return HIGHLIGHT_LEVEL_EMPHASISED;
  return HIGHLIGHT_LEVEL_NORMAL;
}

// --- Sources -----------------------------------------------------------------

/** `include`: only these ids pass. `exclude`: these ids are greyed. */
export type FilterMode = 'include' | 'exclude';

interface FilterSource { kind: 'filter'; mode: FilterMode; ids: Set<string> }
interface EmphasisSource { kind: 'emphasis'; flag: EmphasisFlag; ids: Set<string> }
type Source = FilterSource | EmphasisSource;

export type HighlightListener = (version: number) => void;

export class HighlightMask {
  private bits = new Uint8Array(0);
  private _version = 0;
  private _presentFlags = 0;
  private readonly sources = new Map<string, Source>();
  private readonly listeners = new Set<HighlightListener>();
  private resolver: NodeIndexResolver;
  private indexMap: ReadonlyMap<string, number> | null = null;
  private batchDepth = 0;
  private dirty = false;

  constructor(resolver: NodeIndexResolver = sharedNodeIndexResolver) {
    this.resolver = resolver;
  }

  /** Monotonically increasing; bumps once per recomposition (once per batch). */
  get version(): number { return this._version; }

  /** OR of every bit set on any node: lets a renderer skip "dim the rest" when nothing is emphasised. */
  get presentFlags(): number { return this._presentFlags; }

  /** Slot count of the mask. */
  get size(): number { return this.bits.length; }

  /**
   * The mask, one byte per render index. Re-reads the index map first, so a new
   * graph load is picked up on the next read. Do not hold the array across a
   * version change: a resize replaces it.
   */
  getMask(): Uint8Array {
    this.syncIndex();
    return this.bits;
  }

  /** Bits of one node by id (0 for an id with no slot). */
  bitsFor(id: string): number {
    this.syncIndex();
    const idx = this.indexMap?.get(id);
    return idx === undefined || idx >= this.bits.length ? 0 : this.bits[idx];
  }

  subscribe(listener: HighlightListener): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }

  /** Replace the id → index mapping hook (tests, workers). Recomposes. */
  setIndexResolver(resolver: NodeIndexResolver): void {
    this.resolver = resolver;
    this.indexMap = null;
    this.syncIndex();
  }

  /** Force the slot count (e.g. a renderer that pre-sizes to capacity). Recomposes. */
  resize(n: number): void {
    if (n === this.bits.length) return;
    this.bits = new Uint8Array(Math.max(0, n | 0));
    this.markDirty();
  }

  /** Set a filter source, or clear it with `null` (an inactive widget greys nothing). */
  setFilter(source: string, ids: Iterable<string> | null, mode: FilterMode = 'include'): void {
    if (ids === null) { this.clearSource(source); return; }
    this.sources.set(source, { kind: 'filter', mode, ids: new Set(ids) });
    this.markDirty();
  }

  /** Set an emphasis source's ids for one flag, or clear it with `null`. */
  setEmphasis(source: string, flag: EmphasisFlag, ids: Iterable<string> | null): void {
    if (ids === null) { this.clearSource(source); return; }
    this.sources.set(source, { kind: 'emphasis', flag, ids: new Set(ids) });
    this.markDirty();
  }

  clearSource(source: string): void {
    if (this.sources.delete(source)) this.markDirty();
  }

  /** Remove every source (e.g. "clear all filters and selection"). */
  clearAll(): void {
    if (this.sources.size === 0) return;
    this.sources.clear();
    this.markDirty();
  }

  /** Names of the active sources (for a "N filters active" chip). */
  activeSources(): string[] { return [...this.sources.keys()]; }

  /** Group several writes into one recomposition and one version bump. */
  batch(fn: () => void): void {
    this.batchDepth++;
    try { fn(); } finally {
      this.batchDepth--;
      if (this.batchDepth === 0 && this.dirty) this.recompose();
    }
  }

  // --- internals -------------------------------------------------------------

  private markDirty(): void {
    this.dirty = true;
    if (this.batchDepth === 0) this.recompose();
  }

  /** Recompose when the resolver hands back a new map (a new graph load). */
  private syncIndex(): void {
    if (this.resolver() === this.indexMap) return;
    this.markDirty();
  }

  /** Adopt the resolver's current map, sizing the mask to it when it is new. */
  private adoptIndex(): ReadonlyMap<string, number> {
    const map = this.resolver();
    if (map !== this.indexMap) {
      this.indexMap = map;
      const n = slotCountFor(map);
      if (n !== this.bits.length) this.bits = new Uint8Array(n);
    }
    return map;
  }

  private recompose(): void {
    this.dirty = false;
    const map = this.adoptIndex();
    const bits = this.bits;
    bits.fill(0);
    let present = 0;
    const n = bits.length;

    for (const src of this.sources.values()) {
      if (src.kind === 'emphasis') {
        for (const id of src.ids) {
          const idx = map.get(id);
          if (idx !== undefined && idx < n) bits[idx] |= src.flag;
        }
      } else if (src.mode === 'exclude') {
        for (const id of src.ids) {
          const idx = map.get(id);
          if (idx !== undefined && idx < n) bits[idx] |= HIGHLIGHT_FILTERED_OUT;
        }
      } else {
        // include: every mapped node NOT in the set is greyed.
        for (const [id, idx] of map) {
          if (idx < n && !src.ids.has(id)) bits[idx] |= HIGHLIGHT_FILTERED_OUT;
        }
      }
    }
    for (let i = 0; i < n; i++) present |= bits[i];
    this._presentFlags = present;
    this._version++;
    for (const l of this.listeners) l(this._version);
  }
}

/** The client's one highlight mask, indexed by the shared node render index. */
export const highlightMask = new HighlightMask();
