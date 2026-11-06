/**
 * Typed-array crossfilter over the node table (ADR-2185 D2, D5).
 *
 * Each widget owns at most one clause. A row's `failCount` is the number of
 * clauses it fails, so "passes every clause except widget W's" is
 * `failCount - failsW === 0`: O(1) per row, and a widget's counts exclude its
 * own clause (the standard crossfilter rule), so its own bars keep the full
 * distribution while the others narrow.
 *
 * Why not DuckDB-WASM (ADR-2184 D5): at ~9.7k rows a full re-aggregation of
 * every widget is a handful of passes over Float64/Int32 arrays (well under a
 * millisecond each), which ADR-2185 D5 sanctions as the fallback. DuckDB would
 * add a multi-megabyte WASM download and a worker for no measurable gain.
 *
 * Only `eligible` rows count (delivered and in the render set: linked_page stubs
 * removed by population scope are not counted, so counts match the screen).
 * Clauses are value-level (ranges, category values, a query), so they survive a
 * table rebuild.
 */
import type { NodeColumns, Column, CategoricalColumn, NumberColumn } from './tableSchema';
import { splitMulti } from './tableSchema';

export type Clause =
  /**
   * numeric / temporal: lo <= v < hi (hi = Infinity for an open top). `edges`
   * are the bin lower edges the brush was drawn on: the widget keeps binning by
   * them while the clause is active, so a live domain shift (wire columns) does
   * not move the highlighted bars.
   */
  | { kind: 'range'; column: string; lo: number; hi: number; edges?: readonly number[] }
  /** categorical / boolean: the value is one of `values` (boolean: 'true' / 'false'). */
  | { kind: 'values'; column: string; values: readonly string[] }
  /** case-insensitive substring over label and the text columns. */
  | { kind: 'search'; query: string }
  /** a node-id set: `include` passes only these, `exclude` fails these (graph selection, settings grey-out). */
  | { kind: 'ids'; ids: ReadonlySet<string>; mode: 'include' | 'exclude' };

/** Text columns a search clause matches besides the label. */
export const SEARCH_COLUMNS: readonly string[] = ['label', 'tags', 'definition', 'slug'];

export interface Histogram {
  column: string;
  /** Bin lower edges; bin i covers [edges[i], edges[i+1]), the last bin is open-topped. */
  edges: Float64Array;
  /** Counts over rows passing every OTHER widget's clause. */
  counts: Uint32Array;
  /** Counts over every eligible row (the unfiltered distribution). */
  totals: Uint32Array;
  min: number;
  max: number;
  /** Eligible rows with no value. */
  nulls: number;
}

export interface CategoryCounts {
  column: string;
  /** Category values in display order (declared order, else descending total). */
  values: string[];
  counts: number[];
  totals: number[];
  nulls: number;
}

interface Domain { min: number; max: number; top: number }

export class Crossfilter {
  readonly rowCount: number;
  private readonly cols: NodeColumns;
  private readonly eligible: Uint8Array;
  private readonly clauses = new Map<string, Clause>();
  private readonly fails = new Map<string, Uint8Array>();
  private readonly failCount: Uint8Array;
  private readonly domains = new Map<string, Domain>();
  private haystack: (string | null)[] | null = null;

  constructor(cols: NodeColumns, eligible?: Uint8Array) {
    this.cols = cols;
    this.rowCount = cols.rowCount;
    this.eligible = eligible ?? cols.present;
    this.failCount = new Uint8Array(cols.rowCount);
  }

  get columns(): NodeColumns { return this.cols; }
  isEligible(row: number): boolean { return this.eligible[row] === 1; }

  /** Rows that are eligible. */
  eligibleCount(): number {
    let n = 0;
    for (let i = 0; i < this.rowCount; i++) n += this.eligible[i];
    return n;
  }

  getClause(widget: string): Clause | undefined { return this.clauses.get(widget); }
  clauseEntries(): [string, Clause][] { return [...this.clauses]; }

  /** Set or clear (`null`) one widget's clause. */
  setClause(widget: string, clause: Clause | null): void {
    const old = this.fails.get(widget);
    if (old) for (let i = 0; i < this.rowCount; i++) this.failCount[i] -= old[i];
    this.fails.delete(widget);
    this.clauses.delete(widget);
    if (!clause) return;
    const f = this.evaluate(clause);
    for (let i = 0; i < this.rowCount; i++) this.failCount[i] += f[i];
    this.fails.set(widget, f);
    this.clauses.set(widget, clause);
  }

  /** Does `row` pass every clause, optionally ignoring `exceptWidget`'s? */
  passes(row: number, exceptWidget?: string): boolean {
    if (!this.eligible[row]) return false;
    const own = exceptWidget ? this.fails.get(exceptWidget) : undefined;
    return this.failCount[row] - (own ? own[row] : 0) === 0;
  }

  /** Eligible rows passing every clause except `exceptWidget`'s. */
  count(exceptWidget?: string): number {
    let n = 0;
    for (let i = 0; i < this.rowCount; i++) if (this.passes(i, exceptWidget)) n++;
    return n;
  }

  /** Node ids passing ONE widget's clause (the widget's mask filter source); null without a clause. */
  idsPassingClause(widget: string): string[] | null {
    const f = this.fails.get(widget);
    if (!f) return null;
    const ids = this.idColumn();
    const out: string[] = [];
    for (let i = 0; i < this.rowCount; i++) if (this.cols.present[i] && !f[i] && ids[i] !== null) out.push(ids[i]!);
    return out;
  }

  /** Node ids passing every clause; null when no clause is active. */
  selectedIds(): string[] | null {
    if (this.clauses.size === 0) return null;
    const ids = this.idColumn();
    const out: string[] = [];
    for (let i = 0; i < this.rowCount; i++) if (this.passes(i) && ids[i] !== null) out.push(ids[i]!);
    return out;
  }

  /** Does this numeric / temporal column have any value on an eligible row? */
  hasValues(column: string): boolean {
    const c = this.cols.columns[column];
    if (!c) return false;
    if (c.type === 'numeric' || c.type === 'temporal') return Number.isFinite(this.domain(c).min);
    if (c.type === 'categorical') { for (let i = 0; i < this.rowCount; i++) if (this.eligible[i] && c.codes[i] >= 0) return true; return false; }
    if (c.type === 'boolean') { for (let i = 0; i < this.rowCount; i++) if (this.eligible[i] && c.values[i] >= 0) return true; return false; }
    for (let i = 0; i < this.rowCount; i++) if (this.eligible[i] && c.values[i] !== null) return true;
    return false;
  }

  /**
   * Binned counts of a numeric / temporal column. Bins span [min, p99] so one
   * outlier cannot squash the distribution into a single bar; the last bin is
   * open-topped and holds the tail.
   */
  histogram(column: string, bins: number, exceptWidget: string = column, fixedEdges?: readonly number[]): Histogram {
    const c = this.numberColumn(column);
    const d = this.domain(c);
    if (fixedEdges && fixedEdges.length > 0) bins = fixedEdges.length;
    const edges = new Float64Array(bins);
    const counts = new Uint32Array(bins);
    const totals = new Uint32Array(bins);
    let nulls = 0;
    if (!Number.isFinite(d.min)) return { column, edges, counts, totals, min: NaN, max: NaN, nulls: this.eligibleCount() };
    let binOf: (v: number) => number;
    if (fixedEdges && fixedEdges.length > 0) {
      // Fixed (brush-time) edges: first bin open below, last open above.
      edges.set(fixedEdges);
      binOf = (v) => { let b = 0; while (b + 1 < bins && v >= edges[b + 1]) b++; return b; };
    } else {
      const w = (d.top - d.min || 1) / bins;
      for (let b = 0; b < bins; b++) edges[b] = d.min + b * w;
      binOf = (v) => Math.min(bins - 1, Math.max(0, Math.floor((v - d.min) / w)));
    }
    for (let i = 0; i < this.rowCount; i++) {
      if (!this.eligible[i]) continue;
      const v = c.values[i];
      if (Number.isNaN(v)) { nulls++; continue; }
      const b = binOf(v);
      totals[b]++;
      if (this.passes(i, exceptWidget)) counts[b]++;
    }
    return { column, edges, counts, totals, min: d.min, max: d.max, nulls };
  }

  /** Per-category counts of a categorical or boolean column. */
  categories(column: string, exceptWidget: string = column): CategoryCounts {
    const c = this.cols.columns[column];
    if (!c || (c.type !== 'categorical' && c.type !== 'boolean')) throw new Error(`not a categorical column: ${column}`);
    const dict = c.type === 'categorical' ? c.dictionary : ['false', 'true'];
    const codes = c.type === 'categorical' ? c.codes : c.values;
    const counts = new Array<number>(dict.length).fill(0);
    const totals = new Array<number>(dict.length).fill(0);
    let nulls = 0;
    for (let i = 0; i < this.rowCount; i++) {
      if (!this.eligible[i]) continue;
      const k = codes[i];
      if (k < 0) { nulls++; continue; }
      totals[k]++;
      if (this.passes(i, exceptWidget)) counts[k]++;
    }
    let order = dict.map((_, k) => k).filter((k) => totals[k] > 0);
    if (!(c.type === 'categorical' && c.def.order)) order = order.sort((a, b) => totals[b] - totals[a] || dict[a].localeCompare(dict[b]));
    return {
      column,
      values: order.map((k) => dict[k]),
      counts: order.map((k) => counts[k]),
      totals: order.map((k) => totals[k]),
      nulls,
    };
  }

  // --- internals -------------------------------------------------------------

  private idColumn(): (string | null)[] {
    const c = this.cols.columns.id;
    if (!c || c.type !== 'text') throw new Error('node table has no id column');
    return c.values;
  }

  private numberColumn(column: string): NumberColumn {
    const c = this.cols.columns[column];
    if (!c || (c.type !== 'numeric' && c.type !== 'temporal')) throw new Error(`not a numeric column: ${column}`);
    return c;
  }

  private domain(c: NumberColumn): Domain {
    const cached = this.domains.get(c.def.name);
    if (cached) return cached;
    const vals: number[] = [];
    for (let i = 0; i < this.rowCount; i++) {
      const v = c.values[i];
      if (this.eligible[i] && !Number.isNaN(v)) vals.push(v);
    }
    let d: Domain = { min: NaN, max: NaN, top: NaN };
    if (vals.length > 0) {
      vals.sort((a, b) => a - b);
      const min = vals[0];
      const max = vals[vals.length - 1];
      const p99 = vals[Math.min(vals.length - 1, Math.floor(vals.length * 0.99))];
      d = { min, max, top: p99 > min ? p99 : max };
    }
    this.domains.set(c.def.name, d);
    return d;
  }

  private searchHaystack(): (string | null)[] {
    if (this.haystack) return this.haystack;
    const parts = SEARCH_COLUMNS.map((n) => this.cols.columns[n]).filter((c): c is Column => !!c && c.type === 'text');
    const out: (string | null)[] = new Array(this.rowCount).fill(null);
    for (let i = 0; i < this.rowCount; i++) {
      if (!this.cols.present[i]) continue;
      let s = '';
      for (const c of parts) {
        const v = (c as { values: (string | null)[] }).values[i];
        if (v) s += (s ? '\n' : '') + (c.def.separator ? splitMulti(c.def, v).join(' ') : v);
      }
      out[i] = s.toLowerCase();
    }
    this.haystack = out;
    return out;
  }

  /** 1 where the row FAILS the clause. */
  private evaluate(clause: Clause): Uint8Array {
    const n = this.rowCount;
    const f = new Uint8Array(n);
    if (clause.kind === 'ids') {
      const ids = this.idColumn();
      const inc = clause.mode === 'include';
      for (let i = 0; i < n; i++) { const has = ids[i] !== null && clause.ids.has(ids[i]!); f[i] = has === inc ? 0 : 1; }
      return f;
    }
    if (clause.kind === 'search') {
      const q = clause.query.trim().toLowerCase();
      if (!q) return f;
      const hay = this.searchHaystack();
      for (let i = 0; i < n; i++) f[i] = hay[i] !== null && hay[i]!.includes(q) ? 0 : 1;
      return f;
    }
    const c = this.cols.columns[clause.column];
    if (!c) { f.fill(1); return f; }
    if (clause.kind === 'range') {
      if (c.type !== 'numeric' && c.type !== 'temporal') throw new Error(`range clause on non-numeric column ${clause.column}`);
      for (let i = 0; i < n; i++) {
        const v = c.values[i];
        f[i] = !Number.isNaN(v) && v >= clause.lo && v < clause.hi ? 0 : 1;
      }
      return f;
    }
    const wanted = new Set(clause.values);
    if (c.type === 'categorical') {
      const ok = (c as CategoricalColumn).dictionary.map((v) => wanted.has(v));
      for (let i = 0; i < n; i++) f[i] = c.codes[i] >= 0 && ok[c.codes[i]] ? 0 : 1;
    } else if (c.type === 'boolean') {
      const t = wanted.has('true'); const fl = wanted.has('false');
      for (let i = 0; i < n; i++) f[i] = (c.values[i] === 1 && t) || (c.values[i] === 0 && fl) ? 0 : 1;
    } else {
      throw new Error(`values clause on non-categorical column ${clause.column}`);
    }
    return f;
  }
}
