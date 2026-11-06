/**
 * The explore panel's state: the node table, the crossfilter over it, and the
 * per-widget highlight-mask sources (ADR-2184 D1, ADR-2185 D2-D3).
 *
 * Each widget is a filter source `filter:explore:<widget>` whose include-set is
 * the ids passing that widget's clause; the mask greys a node excluded by any
 * source, so the graph shows the intersection. No widget removes nodes.
 *
 * Two context clauses never write the mask, because the graph already shows them:
 *  - `graph:selection` (ADR-2185 D3): the graph selection and its neighbours. The
 *    widgets count only that subset; the graph emphasises it (useGraphSelection).
 *  - `ext:<source>`: every non-explore filter source on the mask (the node-filter
 *    settings grey-out, `filter:settings`), mirrored so the widget counts and the
 *    "N of M" figure count only what is lit on screen.
 *
 * Pure TS (no React): the hooks are in `useExplore.ts`. One shared instance,
 * `exploreStore`, is fed from graphDataManager by `useExploreTableSource()`.
 */
import { highlightMask as sharedMask, type HighlightMask } from '../state/highlightMask';
import type { NodeColumns } from './tableSchema';
import { buildTable, type TableInput } from './buildTable';
import { Crossfilter, type Clause } from './crossfilter';
import {
  EXPLORE_FILTER_PREFIX, writeFilter, activeFilterSources, filterEntries, subscribeFilterRegistry,
} from './filterRegistry';

export type ExploreListener = () => void;

export type { TableInput } from './buildTable';

export const widgetSource = (widget: string) => `${EXPLORE_FILTER_PREFIX}${widget}`;

/** The graph-selection context clause (ADR-2185 D3). */
export const GRAPH_SELECTION_CLAUSE = 'graph:selection';
const EXT_PREFIX = 'ext:';
const isContextClause = (w: string) => w === GRAPH_SELECTION_CLAUSE || w.startsWith(EXT_PREFIX);

export class ExploreStore {
  private _table: NodeColumns | null = null;
  private _xf: Crossfilter | null = null;
  private _version = 0;
  private readonly clauses = new Map<string, Clause>();
  private readonly listeners = new Set<ExploreListener>();
  private readonly mask: HighlightMask;

  constructor(mask: HighlightMask = sharedMask) {
    this.mask = mask;
    subscribeFilterRegistry(() => this.syncExternalFilters(), mask);
    this.syncExternalFilters(false);
  }

  get version(): number { return this._version; }
  get table(): NodeColumns | null { return this._table; }
  get crossfilter(): Crossfilter | null { return this._xf; }

  subscribe(l: ExploreListener): () => void {
    this.listeners.add(l);
    return () => { this.listeners.delete(l); };
  }

  /** (Re)build the table on this thread (tests, and the fallback when the worker cannot start). */
  setTable(input: TableInput): void {
    const { table, eligible } = buildTable(input);
    this.setColumns(table, eligible);
  }

  /** Adopt a built table (from the worker or `buildTable`); active clauses are re-applied and re-written. */
  setColumns(table: NodeColumns, eligible: Uint8Array): void {
    this._table = table;
    this._xf = new Crossfilter(table, eligible);
    for (const [w, c] of this.clauses) this._xf.setClause(w, c);
    for (const w of this.clauses.keys()) if (!isContextClause(w)) this.writeMask(w);
    this.bump();
  }

  /**
   * The graph selection as a widget context (ADR-2185 D3): widgets count only
   * these ids. `null` or empty clears it. Never writes the mask.
   */
  setGraphSelection(ids: Iterable<string> | null): void {
    const set = ids === null ? null : new Set(ids);
    const prev = this.clauses.get(GRAPH_SELECTION_CLAUSE);
    if (!set || set.size === 0) {
      if (!prev) return;
      this.setContext(GRAPH_SELECTION_CLAUSE, null);
      return;
    }
    if (prev?.kind === 'ids' && prev.ids.size === set.size && [...set].every((id) => prev.ids.has(id))) return;
    this.setContext(GRAPH_SELECTION_CLAUSE, { kind: 'ids', ids: set, mode: 'include' });
  }

  /** Size of the graph-selection context (0 when none). */
  graphSelectionSize(): number {
    const c = this.clauses.get(GRAPH_SELECTION_CLAUSE);
    return c?.kind === 'ids' ? c.ids.size : 0;
  }

  /**
   * Rows lit on screen: passing the widget filters and the mirrored settings
   * grey-out. The graph selection only emphasises, so it is not counted here.
   */
  litCount(): number {
    return this._xf?.count(GRAPH_SELECTION_CLAUSE) ?? 0;
  }

  private setContext(name: string, clause: Clause | null): void {
    if (clause) this.clauses.set(name, clause); else this.clauses.delete(name);
    this._xf?.setClause(name, clause);
    this.bump();
  }

  /** Mirror non-explore filter sources (the settings grey-out) as `ext:` clauses. */
  private syncExternalFilters(notify = true): void {
    const wanted = new Map<string, Clause>();
    for (const [source, f] of filterEntries(this.mask)) {
      if (source.startsWith(EXPLORE_FILTER_PREFIX)) continue;
      wanted.set(EXT_PREFIX + source, { kind: 'ids', ids: f.ids, mode: f.mode });
    }
    let changed = false;
    for (const k of [...this.clauses.keys()]) {
      if (k.startsWith(EXT_PREFIX) && !wanted.has(k)) { this.clauses.delete(k); this._xf?.setClause(k, null); changed = true; }
    }
    for (const [k, c] of wanted) {
      const prev = this.clauses.get(k);
      if (prev?.kind === 'ids' && c.kind === 'ids' && prev.ids === c.ids && prev.mode === c.mode) continue;
      this.clauses.set(k, c);
      this._xf?.setClause(k, c);
      changed = true;
    }
    if (changed && notify) this.bump();
  }

  /** Set (or clear with `null`) a widget's clause; writes that widget's mask source. */
  setClause(widget: string, clause: Clause | null): void {
    if (clause && clause.kind === 'search' && !clause.query.trim()) clause = null;
    if (clause) this.clauses.set(widget, clause); else this.clauses.delete(widget);
    this._xf?.setClause(widget, clause);
    this.writeMask(widget);
    this.bump();
  }

  getClause(widget: string): Clause | undefined { return this.clauses.get(widget); }

  /** Active explore-widget filters (context clauses excluded). */
  activeCount(): number {
    let n = 0;
    for (const w of this.clauses.keys()) if (!isContextClause(w)) n++;
    return n;
  }

  /** Clear every explore widget clause and every `filter:explore:*` mask source. Selection, hover and settings filters are untouched. */
  clearAll(): void {
    for (const w of [...this.clauses.keys()]) {
      if (isContextClause(w)) continue;
      this._xf?.setClause(w, null);
      this.clauses.delete(w);
    }
    for (const s of activeFilterSources(this.mask)) {
      if (s.startsWith(EXPLORE_FILTER_PREFIX)) writeFilter(s, null, 'include', this.mask);
    }
    this.bump();
  }

  private writeMask(widget: string): void {
    const ids = this._xf?.idsPassingClause(widget) ?? null;
    // Before a table exists, a clause cannot be resolved to ids: leave the mask alone.
    if (ids === null && this.clauses.has(widget)) return;
    writeFilter(widgetSource(widget), ids, 'include', this.mask);
  }

  private bump(): void {
    this._version++;
    for (const l of this.listeners) l();
  }
}

/** The client's one explore store, writing the shared highlight mask. */
export const exploreStore = new ExploreStore();

/** Clear every explore-panel filter (menus lane: "Clear filters"). */
export function clearAllFilters(): void {
  exploreStore.clearAll();
}
