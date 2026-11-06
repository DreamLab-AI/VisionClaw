/**
 * <ExplorePanel/> — linked timeline, histogram, category and search widgets
 * (ADR-2185).
 *
 * Every widget is a crossfilter clause and a highlight-mask filter source
 * (`filter:explore:<widget>`): brushing greys the rest of the graph and never
 * removes nodes. Each widget's bars count the rows passing every OTHER widget's
 * clause, so its own distribution stays whole while the others narrow.
 *
 * The timeline reads `last_commit`, else `last_modified`; until the server lane
 * puts either on the node metadata it shows an explicit "no time data" state.
 *
 * The panel is a scene occluder (ADR-2180 fit keeps the graph out from under it);
 * the menus lane positions it through `className` / `style`.
 */
import React, { useEffect, useId, useMemo, useRef, useState } from 'react';
import { GlassPanel } from '../../control-center/primitives/GlassPanel';
import { useExploreState, useExploreTableSource, useActiveFilterCount } from './useExplore';
import { clearAllFilters, type ExploreStore } from './exploreStore';
import type { Crossfilter, Clause } from './crossfilter';
import { NODE_TABLE_SCHEMA } from './tableSchema';
import { columnLabel, formatValue } from './GraphLegend';

export const WIDGET = Object.freeze({
  TIMELINE: 'timeline',
  HISTOGRAM: 'histogram',
  CATEGORIES: 'categories',
  SEARCH: 'search',
});

export const TIMELINE_COLUMNS = ['last_commit', 'last_modified'] as const;
export const TIMELINE_BINS = 36;
export const HISTOGRAM_BINS = 20;
export const MAX_CATEGORY_BARS = 12;
const SEARCH_DEBOUNCE_MS = 200;

const NUMERIC_COLUMNS = NODE_TABLE_SCHEMA.filter((c) => c.type === 'numeric').map((c) => c.name);
const CATEGORY_COLUMNS = NODE_TABLE_SCHEMA.filter((c) => c.type === 'categorical' || c.type === 'boolean').map((c) => c.name);

const nf = new Intl.NumberFormat('en-GB');

const css = {
  panel: { padding: '10px 12px', width: 320, fontSize: 12, display: 'flex', flexDirection: 'column', gap: 12 } as React.CSSProperties,
  header: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 } as React.CSSProperties,
  title: { fontWeight: 600, fontSize: 13 } as React.CSSProperties,
  muted: { opacity: 0.7 } as React.CSSProperties,
  widgetHead: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 6, marginBottom: 4 } as React.CSSProperties,
  bars: { display: 'flex', alignItems: 'flex-end', gap: 1, height: 48, touchAction: 'none' } as React.CSSProperties,
  bar: (selected: boolean, dim: boolean): React.CSSProperties => ({
    flex: 1, height: '100%', padding: 0, border: 0, background: 'transparent', position: 'relative', cursor: 'pointer',
    outlineOffset: 1, opacity: dim ? 0.45 : 1,
    boxShadow: selected ? 'inset 0 -2px 0 #4FC3F7' : undefined,
  }),
  fill: (frac: number, color: string, alpha: number): React.CSSProperties => ({
    position: 'absolute', left: 0, right: 0, bottom: 0, height: `${Math.max(frac > 0 ? 2 : 0, frac * 100)}%`,
    background: color, opacity: alpha, borderRadius: '2px 2px 0 0',
  }),
  ends: { display: 'flex', justifyContent: 'space-between', opacity: 0.75, marginTop: 2, fontVariantNumeric: 'tabular-nums' } as React.CSSProperties,
  button: { background: 'rgba(255,255,255,0.08)', color: 'inherit', border: '1px solid rgba(255,255,255,0.18)', borderRadius: 4, padding: '2px 8px', cursor: 'pointer', fontSize: 11 } as React.CSSProperties,
  select: { background: 'rgba(0,0,0,0.3)', color: 'inherit', border: '1px solid rgba(255,255,255,0.18)', borderRadius: 4, fontSize: 11, padding: '1px 4px' } as React.CSSProperties,
  input: { width: '100%', boxSizing: 'border-box', background: 'rgba(0,0,0,0.3)', color: 'inherit', border: '1px solid rgba(255,255,255,0.18)', borderRadius: 4, padding: '4px 6px', fontSize: 12 } as React.CSSProperties,
  catRow: (active: boolean): React.CSSProperties => ({
    display: 'grid', gridTemplateColumns: '1fr 90px 44px', alignItems: 'center', gap: 6, width: '100%', textAlign: 'left',
    background: active ? 'rgba(79,195,247,0.15)' : 'transparent', border: 0, color: 'inherit', padding: '1px 2px', cursor: 'pointer', borderRadius: 3,
  }),
  catTrack: { position: 'relative', height: 8, background: 'rgba(255,255,255,0.06)', borderRadius: 2 } as React.CSSProperties,
};

const ACCENT = '#4FC3F7';

// --- brushable bars ------------------------------------------------------------------

interface BinBarsProps {
  label: string;
  counts: ArrayLike<number>;
  totals: ArrayLike<number>;
  /** Inclusive selected bin range, or null. */
  selected: [number, number] | null;
  onSelect: (range: [number, number] | null) => void;
  binLabel: (i: number) => string;
}

/**
 * Bars that brush a contiguous bin range: click selects a bin, shift-click or a
 * drag extends it, a click on the sole selected bin or Escape clears it. Every
 * bar is a focusable button (arrow keys move, Enter / Space select).
 */
export const BinBars: React.FC<BinBarsProps> = ({ label, counts, totals, selected, onSelect, binLabel }) => {
  const anchor = useRef<number | null>(null);
  const dragging = useRef(false);
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  let max = 0;
  for (let i = 0; i < totals.length; i++) max = Math.max(max, totals[i]);
  const inSel = (i: number) => !!selected && i >= selected[0] && i <= selected[1];

  const choose = (i: number, extend: boolean) => {
    if (extend && anchor.current !== null) {
      onSelect([Math.min(anchor.current, i), Math.max(anchor.current, i)]);
      return;
    }
    if (selected && selected[0] === i && selected[1] === i) { anchor.current = null; onSelect(null); return; }
    anchor.current = i;
    onSelect([i, i]);
  };

  useEffect(() => {
    const up = () => { dragging.current = false; };
    window.addEventListener('pointerup', up);
    return () => window.removeEventListener('pointerup', up);
  }, []);

  return (
    <div role="group" aria-label={label} style={css.bars}
      onKeyDown={(e) => { if (e.key === 'Escape' && selected) { e.stopPropagation(); anchor.current = null; onSelect(null); } }}>
      {Array.from({ length: totals.length }, (_, i) => (
        <button
          key={i}
          ref={(el) => { refs.current[i] = el; }}
          type="button"
          aria-pressed={inSel(i)}
          aria-label={`${binLabel(i)}: ${nf.format(counts[i])} of ${nf.format(totals[i])} nodes`}
          title={`${binLabel(i)}: ${nf.format(counts[i])} / ${nf.format(totals[i])}`}
          style={css.bar(inSel(i), !!selected && !inSel(i))}
          onPointerDown={(e) => { dragging.current = true; choose(i, e.shiftKey); }}
          onPointerEnter={() => { if (dragging.current) choose(i, true); }}
          onClick={(e) => { if (e.detail === 0) choose(i, e.shiftKey); /* keyboard activation */ }}
          onKeyDown={(e) => {
            const next = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : -1;
            if (next >= 0 && next < totals.length) {
              e.preventDefault();
              refs.current[next]?.focus();
              if (e.shiftKey) choose(next, true);
            }
          }}
        >
          <span style={css.fill(max ? totals[i] / max : 0, '#90A4AE', 0.35)} aria-hidden="true" />
          <span style={css.fill(max ? counts[i] / max : 0, ACCENT, 0.9)} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
};

// --- helpers ---------------------------------------------------------------------------

/** The inclusive bin range covered by a range clause (for re-highlighting after a rebuild). */
function binsOfRange(clause: Clause | undefined, edges: Float64Array): [number, number] | null {
  if (!clause || clause.kind !== 'range' || edges.length === 0) return null;
  let a = -1; let b = -1;
  for (let i = 0; i < edges.length; i++) {
    const lo = edges[i];
    const hi = i + 1 < edges.length ? edges[i + 1] : Infinity;
    if (lo >= clause.lo - 1e-9 && hi <= clause.hi + 1e-9) { if (a < 0) a = i; b = i; }
  }
  return a < 0 ? null : [a, b];
}

/** A range clause over bins [a, b], capturing the edges it was drawn on. */
function rangeClause(column: string, edges: Float64Array, [a, b]: [number, number]): Clause {
  return {
    kind: 'range', column, edges: Array.from(edges),
    lo: a === 0 ? -Infinity : edges[a], hi: b + 1 < edges.length ? edges[b + 1] : Infinity,
  };
}

/** The bin edges a widget's active brush was drawn on (stable under a live domain shift). */
function brushEdges(clause: Clause | undefined, column: string): readonly number[] | undefined {
  return clause?.kind === 'range' && clause.column === column ? clause.edges : undefined;
}

// --- widgets ---------------------------------------------------------------------------

interface WidgetProps { xf: Crossfilter; store: ExploreStore }

export const TimelineWidget: React.FC<WidgetProps> = ({ xf, store }) => {
  const column = TIMELINE_COLUMNS.find((c) => xf.hasValues(c));
  const headingId = useId();
  if (!column) {
    return (
      <section aria-labelledby={headingId} data-testid="explore-timeline">
        <div id={headingId} style={css.widgetHead}><strong>Timeline</strong></div>
        <div role="status" data-testid="explore-timeline-empty" style={{ ...css.muted, padding: '10px 0', textAlign: 'center', border: '1px dashed rgba(255,255,255,0.2)', borderRadius: 4 }}>
          No time data: the graph load carries no last commit or last modified dates yet.
        </div>
      </section>
    );
  }
  const clause = store.getClause(WIDGET.TIMELINE);
  const h = xf.histogram(column, TIMELINE_BINS, WIDGET.TIMELINE, brushEdges(clause, column));
  return (
    <section aria-labelledby={headingId} data-testid="explore-timeline">
      <div style={css.widgetHead}>
        <strong id={headingId}>Timeline · {columnLabel(column)}</strong>
        {clause && <button type="button" style={css.button} onClick={() => store.setClause(WIDGET.TIMELINE, null)}>Clear</button>}
      </div>
      <BinBars
        label={`${columnLabel(column)} timeline`}
        counts={h.counts} totals={h.totals}
        selected={binsOfRange(clause, h.edges)}
        onSelect={(r) => store.setClause(WIDGET.TIMELINE, r ? rangeClause(column, h.edges, r) : null)}
        binLabel={(i) => `from ${formatValue(h.edges[i], true)}`}
      />
      <div style={css.ends}><span>{formatValue(h.min, true)}</span><span>{formatValue(h.max, true)}</span></div>
      {h.nulls > 0 && <div style={css.muted}>{nf.format(h.nulls)} nodes without a date</div>}
    </section>
  );
};

export const HistogramWidget: React.FC<WidgetProps> = ({ xf, store }) => {
  const available = NUMERIC_COLUMNS.filter((c) => xf.hasValues(c));
  const [chosen, setChosen] = useState<string | null>(null);
  const column = chosen && available.includes(chosen) ? chosen : available[0];
  const selectId = useId();
  if (!column) return null;
  const clause = store.getClause(WIDGET.HISTOGRAM);
  const h = xf.histogram(column, HISTOGRAM_BINS, WIDGET.HISTOGRAM, brushEdges(clause, column));
  return (
    <section aria-label="Numeric histogram" data-testid="explore-histogram">
      <div style={css.widgetHead}>
        <label htmlFor={selectId}><strong>Histogram</strong></label>
        <span style={{ display: 'flex', gap: 6 }}>
          {clause && <button type="button" style={css.button} onClick={() => store.setClause(WIDGET.HISTOGRAM, null)}>Clear</button>}
          <select id={selectId} style={css.select} value={column}
            onChange={(e) => { store.setClause(WIDGET.HISTOGRAM, null); setChosen(e.target.value); }}>
            {available.map((c) => <option key={c} value={c}>{columnLabel(c)}</option>)}
          </select>
        </span>
      </div>
      <BinBars
        label={`${columnLabel(column)} histogram`}
        counts={h.counts} totals={h.totals}
        selected={binsOfRange(clause, h.edges)}
        onSelect={(r) => store.setClause(WIDGET.HISTOGRAM, r ? rangeClause(column, h.edges, r) : null)}
        binLabel={(i) => (i + 1 < h.edges.length ? `${formatValue(h.edges[i], false)} to ${formatValue(h.edges[i + 1], false)}` : `${formatValue(h.edges[i], false)} and above`)}
      />
      <div style={css.ends}><span>{formatValue(h.min, false)}</span><span>{formatValue(h.max, false)}</span></div>
    </section>
  );
};

export const CategoryWidget: React.FC<WidgetProps> = ({ xf, store }) => {
  const available = CATEGORY_COLUMNS.filter((c) => xf.hasValues(c));
  const [chosen, setChosen] = useState<string | null>(null);
  const column = chosen && available.includes(chosen) ? chosen : available.includes('domain') ? 'domain' : available[0];
  const selectId = useId();
  if (!column) return null;
  const cats = xf.categories(column, WIDGET.CATEGORIES);
  const clause = store.getClause(WIDGET.CATEGORIES);
  const picked = new Set(clause?.kind === 'values' ? clause.values : []);
  const max = Math.max(1, ...cats.totals);
  const toggle = (v: string) => {
    const next = new Set(picked);
    if (next.has(v)) next.delete(v); else next.add(v);
    store.setClause(WIDGET.CATEGORIES, next.size ? { kind: 'values', column, values: [...next] } : null);
  };
  const shown = cats.values.slice(0, MAX_CATEGORY_BARS);
  return (
    <section aria-label="Category bars" data-testid="explore-categories">
      <div style={css.widgetHead}>
        <label htmlFor={selectId}><strong>Categories</strong></label>
        <span style={{ display: 'flex', gap: 6 }}>
          {clause && <button type="button" style={css.button} onClick={() => store.setClause(WIDGET.CATEGORIES, null)}>Clear</button>}
          <select id={selectId} style={css.select} value={column}
            onChange={(e) => { store.setClause(WIDGET.CATEGORIES, null); setChosen(e.target.value); }}>
            {available.map((c) => <option key={c} value={c}>{columnLabel(c)}</option>)}
          </select>
        </span>
      </div>
      <div role="group" aria-label={`${columnLabel(column)} categories`}>
        {shown.map((v, k) => (
          <button key={v} type="button" aria-pressed={picked.has(v)} style={css.catRow(picked.has(v))}
            aria-label={`${v}: ${nf.format(cats.counts[k])} of ${nf.format(cats.totals[k])} nodes`} onClick={() => toggle(v)}>
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{v}</span>
            <span style={css.catTrack} aria-hidden="true">
              <span style={{ ...css.fill(cats.totals[k] / max, '#90A4AE', 0.35), borderRadius: 2, right: 'auto', width: `${(cats.totals[k] / max) * 100}%`, height: '100%' }} />
              <span style={{ ...css.fill(cats.counts[k] / max, ACCENT, 0.9), borderRadius: 2, right: 'auto', width: `${(cats.counts[k] / max) * 100}%`, height: '100%' }} />
            </span>
            <span style={{ textAlign: 'right', fontVariantNumeric: 'tabular-nums' }}>{nf.format(cats.counts[k])}</span>
          </button>
        ))}
        {cats.values.length > shown.length && <div style={css.muted}>+{cats.values.length - shown.length} more</div>}
      </div>
    </section>
  );
};

export const SearchWidget: React.FC<WidgetProps> = ({ xf, store }) => {
  const clause = store.getClause(WIDGET.SEARCH);
  const [text, setText] = useState(clause?.kind === 'search' ? clause.query : '');
  const inputId = useId();
  useEffect(() => {
    const t = setTimeout(() => {
      const current = store.getClause(WIDGET.SEARCH);
      const q = text.trim();
      if ((current?.kind === 'search' ? current.query : '') === q) return;
      store.setClause(WIDGET.SEARCH, q ? { kind: 'search', query: q } : null);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [text, store]);
  const matches = clause ? store.litCount() : null;
  return (
    <section data-testid="explore-search">
      <label htmlFor={inputId} style={{ display: 'block', marginBottom: 4 }}><strong>Search</strong></label>
      <input id={inputId} type="search" style={css.input} value={text} placeholder="Label, tags, definition…"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => { if (e.key === 'Escape') setText(''); }} />
      {matches !== null && <div role="status" style={{ ...css.muted, marginTop: 2 }}>{nf.format(matches)} nodes match</div>}
    </section>
  );
};

// --- panel -------------------------------------------------------------------------------

export const ExplorePanel: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ style, ...rest }) => {
  useExploreTableSource();
  const { crossfilter: xf, store } = useExploreState();
  const active = useActiveFilterCount();
  const summary = useMemo(() => (xf ? { lit: store.litCount(), total: xf.eligibleCount() } : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [xf, store.version]);
  const selected = store.graphSelectionSize();

  return (
    <GlassPanel elevation="overlay" role="region" aria-label="Explore graph" data-testid="explore-panel"
      data-scene-occluder="" style={{ ...css.panel, ...style }} {...rest}>
      <div style={css.header}>
        <span style={css.title}>Explore</span>
        <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {summary && <span style={css.muted} aria-live="polite">{nf.format(summary.lit)} of {nf.format(summary.total)} nodes</span>}
          <button type="button" style={css.button} disabled={active === 0} onClick={clearAllFilters}>
            Clear filters{active > 0 ? ` (${active})` : ''}
          </button>
        </span>
      </div>
      <div style={css.muted}>
        Counts are of nodes lit in the graph (explore and node-filter settings applied); nodes inside collapsed hierarchy groups are still counted.
      </div>
      {selected > 0 && (
        <div role="status" data-testid="explore-selection" style={{ ...css.header, background: 'rgba(79,195,247,0.12)', borderRadius: 4, padding: '4px 6px' }}>
          <span>Widgets show the graph selection ({nf.format(selected)} nodes: the selected node and its neighbours)</span>
          <button type="button" style={css.button}
            onClick={() => window.dispatchEvent(new CustomEvent('visionclaw:node-deselect'))}>Clear selection</button>
        </div>
      )}
      {!xf ? (
        <div role="status" style={css.muted}>Waiting for the graph to load…</div>
      ) : (
        <>
          <SearchWidget xf={xf} store={store} />
          <TimelineWidget xf={xf} store={store} />
          <HistogramWidget xf={xf} store={store} />
          <CategoryWidget xf={xf} store={store} />
        </>
      )}
    </GlassPanel>
  );
};

export default ExplorePanel;
