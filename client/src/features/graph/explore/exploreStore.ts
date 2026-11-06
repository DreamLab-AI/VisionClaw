/**
 * The explore panel's state: the node table, the crossfilter over it, and the
 * per-widget highlight-mask sources (ADR-2184 D1, ADR-2185 D2-D3).
 *
 * Each widget is a filter source `filter:explore:<widget>` whose include-set is
 * the ids passing that widget's clause; the mask greys a node excluded by any
 * source, so the graph shows the intersection. No widget removes nodes.
 *
 * Pure TS (no React): the hooks are in `useExplore.ts`. One shared instance,
 * `exploreStore`, is fed from graphDataManager by `connectGraphSource()`.
 */
import { highlightMask as sharedMask, type HighlightMask } from '../state/highlightMask';
import type { Node as GraphNode } from '../managers/graphDataManager';
import { buildNodeColumns, type NodeColumns } from './tableSchema';
import { Crossfilter, type Clause } from './crossfilter';
import { EXPLORE_FILTER_PREFIX, writeFilter, activeFilterSources } from './filterRegistry';
import { isRemovedByScope, resolveNodeFilter, type NodeFilterInput } from './nodeFilter';

export type ExploreListener = () => void;

export interface TableInput {
  nodes: readonly GraphNode[];
  /** id → render index (defaults to the order of `nodes`, which is how the shared index is built). */
  indexMap?: ReadonlyMap<string, number>;
  /** Render-index-aligned analytics buffer (stride 5). */
  analytics?: Float32Array | null;
  /** Node-filter settings: their population scope decides which rows count. */
  nodeFilter?: NodeFilterInput | null;
}

export const widgetSource = (widget: string) => `${EXPLORE_FILTER_PREFIX}${widget}`;

export class ExploreStore {
  private _table: NodeColumns | null = null;
  private _xf: Crossfilter | null = null;
  private _version = 0;
  private readonly clauses = new Map<string, Clause>();
  private readonly listeners = new Set<ExploreListener>();
  private readonly mask: HighlightMask;

  constructor(mask: HighlightMask = sharedMask) {
    this.mask = mask;
  }

  get version(): number { return this._version; }
  get table(): NodeColumns | null { return this._table; }
  get crossfilter(): Crossfilter | null { return this._xf; }

  subscribe(l: ExploreListener): () => void {
    this.listeners.add(l);
    return () => { this.listeners.delete(l); };
  }

  /** (Re)build the table and crossfilter; active clauses are re-applied and re-written. */
  setTable(input: TableInput): void {
    const indexMap = input.indexMap ?? new Map(input.nodes.map((n, i) => [String(n.id), i]));
    const table = buildNodeColumns(input.nodes, { indexMap, analytics: input.analytics ?? null });
    const eligible = new Uint8Array(table.rowCount);
    const scope = resolveNodeFilter(input.nodeFilter);
    for (const node of input.nodes) {
      const i = indexMap.get(String(node.id));
      if (i !== undefined && i < eligible.length && !isRemovedByScope(node, scope)) eligible[i] = 1;
    }
    this._table = table;
    this._xf = new Crossfilter(table, eligible);
    for (const [w, c] of this.clauses) this._xf.setClause(w, c);
    for (const w of this.clauses.keys()) this.writeMask(w);
    this.bump();
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

  /** Active widget clauses. */
  activeCount(): number { return this.clauses.size; }

  /** Clear every explore widget clause and every `filter:explore:*` mask source. Selection, hover and settings filters are untouched. */
  clearAll(): void {
    for (const w of [...this.clauses.keys()]) this._xf?.setClause(w, null);
    this.clauses.clear();
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
