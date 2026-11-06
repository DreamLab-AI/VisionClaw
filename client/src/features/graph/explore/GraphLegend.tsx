/**
 * <GraphLegend/> — the legend for the active colour and size encodings (ADR-2184 D4).
 *
 * Mounting it also installs the column encoder (`useExploreEncoding`) and keeps
 * the node table fed, so colour-by / size-by take effect wherever the legend is
 * mounted, even with `explore.showLegend` off (it then renders nothing). The
 * menus lane positions it through `className` / `style`.
 *
 * Categorical: one swatch per value with its node count, "other" and "no value"
 * rows. Numeric / temporal: the sequential ramp with min and max. Size: a
 * small-to-large dot pair with min and max.
 */
import React from 'react';
import { useSettingsStore } from '../../../store/settingsStore';
import { GlassPanel } from '../../control-center/primitives/GlassPanel';
import { useExploreEncoding, useExploreTableSource } from './useExplore';
import { NULL_COLOR, OTHER_COLOR, rampCss, type LegendEntry, type LegendModel } from './columnEncoder';

const nf = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 2 });
const df = new Intl.DateTimeFormat('en-GB', { year: 'numeric', month: 'short', day: 'numeric' });

export function formatValue(v: number, temporal: boolean): string {
  if (!Number.isFinite(v)) return '—';
  return temporal ? df.format(new Date(v)) : nf.format(v);
}

/** Human column name: `quality_score` → "Quality score". */
export function columnLabel(name: string): string {
  const s = name.replace(/_/g, ' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
}

const css = {
  panel: { padding: '10px 12px', minWidth: 180, maxWidth: 260, fontSize: 12, lineHeight: 1.35 } as React.CSSProperties,
  title: { fontWeight: 600, marginBottom: 4 } as React.CSSProperties,
  list: { listStyle: 'none', margin: 0, padding: 0 } as React.CSSProperties,
  row: { display: 'flex', alignItems: 'center', gap: 6, padding: '1px 0' } as React.CSSProperties,
  swatch: (color: string): React.CSSProperties => ({
    width: 10, height: 10, borderRadius: '50%', background: color, flex: '0 0 auto',
    boxShadow: '0 0 0 1px rgba(255,255,255,0.25)',
  }),
  value: { flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } as React.CSSProperties,
  count: { opacity: 0.7, fontVariantNumeric: 'tabular-nums' } as React.CSSProperties,
  ramp: { height: 8, borderRadius: 4, background: rampCss(), margin: '4px 0 2px' } as React.CSSProperties,
  ends: { display: 'flex', justifyContent: 'space-between', opacity: 0.8, fontVariantNumeric: 'tabular-nums' } as React.CSSProperties,
  section: { marginTop: 8 } as React.CSSProperties,
};

const Section: React.FC<{ entry: LegendEntry }> = ({ entry }) => {
  const name = columnLabel(entry.column);
  const titleId = `graph-legend-${entry.channel}-${entry.column}`;
  if (entry.kind === 'categorical') {
    return (
      <section style={css.section} aria-labelledby={titleId} data-testid={`legend-${entry.channel}`}>
        <div id={titleId} style={css.title}>Colour: {name}</div>
        <ul style={css.list}>
          {entry.swatches.map((s) => (
            <li key={s.value} style={css.row}>
              <span style={css.swatch(s.color)} aria-hidden="true" />
              <span style={css.value}>{s.value}</span>
              <span style={css.count}>{nf.format(s.count)}</span>
            </li>
          ))}
          {entry.other > 0 && (
            <li style={css.row}>
              <span style={css.swatch(OTHER_COLOR)} aria-hidden="true" />
              <span style={css.value}>Other</span>
              <span style={css.count}>{nf.format(entry.other)}</span>
            </li>
          )}
          {entry.nulls > 0 && (
            <li style={css.row}>
              <span style={css.swatch(NULL_COLOR)} aria-hidden="true" />
              <span style={css.value}>No value</span>
              <span style={css.count}>{nf.format(entry.nulls)}</span>
            </li>
          )}
        </ul>
      </section>
    );
  }
  const min = formatValue(entry.min, entry.temporal);
  const max = formatValue(entry.max, entry.temporal);
  const noData = !Number.isFinite(entry.min);
  return (
    <section style={css.section} aria-labelledby={titleId} data-testid={`legend-${entry.channel}`}>
      <div id={titleId} style={css.title}>{entry.channel === 'color' ? 'Colour' : 'Size'}: {name}</div>
      {noData ? (
        <div style={css.count}>No values on the graph load</div>
      ) : entry.channel === 'color' ? (
        <div role="img" aria-label={`${name} from ${min} to ${max}`}>
          <div style={css.ramp} />
          <div style={css.ends}><span>{min}</span><span>{max}</span></div>
        </div>
      ) : (
        <div role="img" aria-label={`${name}: small ${min}, large ${max}`} style={{ ...css.ends, alignItems: 'center' }}>
          <span style={css.row}><span style={{ ...css.swatch('#cfd8dc'), width: 5, height: 5 }} />{min}</span>
          <span style={css.row}>{max}<span style={{ ...css.swatch('#cfd8dc'), width: 14, height: 14 }} /></span>
        </div>
      )}
      {entry.nulls > 0 && !noData && <div style={css.count}>{nf.format(entry.nulls)} without a value</div>}
    </section>
  );
};

/** Presentational legend for a model (exported for tests and custom hosts). */
export const LegendView: React.FC<{ legend: LegendModel } & React.HTMLAttributes<HTMLDivElement>> = ({ legend, style, ...rest }) => {
  if (legend.entries.length === 0) return null;
  return (
    <GlassPanel elevation="overlay" role="group" aria-label="Graph legend" data-testid="graph-legend"
      data-scene-occluder="" style={{ ...css.panel, ...style }} {...rest}>
      {legend.entries.map((e) => <Section key={`${e.channel}:${e.column}`} entry={e} />)}
    </GlassPanel>
  );
};

/**
 * The live legend. Renders nothing when `explore.showLegend` is off or both
 * encodings are today's defaults (colorBy 'scheme', sizeBy 'none').
 */
export const GraphLegend: React.FC<React.HTMLAttributes<HTMLDivElement>> = (props) => {
  useExploreTableSource();
  const legend = useExploreEncoding();
  const showLegend = useSettingsStore((s) => s.settings?.visualisation?.graphView?.explore?.showLegend ?? true);
  if (!showLegend) return null;
  return <LegendView legend={legend} {...props} />;
};

export default GraphLegend;
