/**
 * Column-driven node encoding and its legend model (ADR-2184 D4).
 *
 * `createColumnEncoding(table, colorBy, sizeBy)` returns a `Partial<NodeEncoder>`
 * for `nodeEncoding.setEncoder` (the omitted half stays today's default) and a
 * `LegendModel` for `<GraphLegend/>`.
 *
 * Palettes follow the repo's conventions:
 *  - `domain` / `source_domain` use `getDomainColor`, so colour-by-domain matches
 *    today's knowledge look and the hulls;
 *  - wire `community` / `cluster` use the default encoder's golden-angle hue
 *    formulas (nodeEncoding.ts), so the analytics schemes and the column agree;
 *  - other categoricals take the DOMAIN_COLORS Material-300 family in frequency
 *    order, with a neutral "other" past the palette;
 *  - ordered categoricals (maturity), numeric and temporal columns take a
 *    sequential viridis ramp (trimmed of its darkest stops so it reads on the
 *    dark scene background).
 *
 * A node with no value is drawn in DEFAULT_DOMAIN_COLOR, the colour unknown
 * domains already get; it is a mid-light slate, distinct from a greyed-out node
 * (desaturated toward the background).
 *
 * Values are read from the node (metadata) or the live analytics buffer at
 * encode time, never from a row index of the table snapshot, so wire columns
 * (centrality, community) recolour as frames arrive and a re-ordered render
 * index cannot misalign them. The table supplies only the domain and categories.
 */
import * as THREE from 'three';
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { NodeEncoder, EncodingContext } from '../state/nodeEncoding';
import { DEFAULT_NODE_ENCODER, isSchemeColorBy } from '../state/nodeEncoding';
import { ANALYTICS_STRIDE } from './analyticsLayout';
import { DEFAULT_DOMAIN_COLOR, getDomainColor } from '../utils/domainColors';
import { getTypeColor } from '../hooks/useGraphNodeColors';
import {
  NODE_TABLE_COLUMN, parseNumeric, parseTemporal, parseBoolean,
  type NodeColumns, type ColumnDef,
} from './tableSchema';

/** Categorical palette: the DOMAIN_COLORS Material-300 family, then more 300 tones. */
export const CATEGORICAL_PALETTE: readonly string[] = Object.freeze([
  '#4FC3F7', '#81C784', '#FFB74D', '#CE93D8', '#4DB6AC', '#FFD54F',
  '#F06292', '#7986CB', '#E57373', '#AED581', '#A1887F', '#64B5F6',
]);
/** Categories past the palette share this colour in the legend's "other" row. */
export const OTHER_COLOR = '#B0BEC5';
/** No value. */
export const NULL_COLOR = DEFAULT_DOMAIN_COLOR;

/** Sequential ramp (viridis, from 0.25 to 1.0). */
export const SEQUENTIAL_RAMP: readonly string[] = Object.freeze([
  '#3b528b', '#2c728e', '#21918c', '#28ae80', '#5ec962', '#addc30', '#fde725',
]);

/** Size-by maps t ∈ [0, 1] onto this factor of today's size (the clamp). */
export const SIZE_FACTOR_MIN = 0.5;
export const SIZE_FACTOR_MAX = 2.5;
/** Most legend swatches before an "other" row. */
export const LEGEND_MAX_SWATCHES = CATEGORICAL_PALETTE.length;

const rampColors = SEQUENTIAL_RAMP.map((h) => new THREE.Color(h));

/** Sequential ramp colour at t ∈ [0, 1] (linear RGB into `out`). */
export function rampColor(t: number, out: THREE.Color): THREE.Color {
  const x = Math.min(1, Math.max(0, Number.isFinite(t) ? t : 0)) * (rampColors.length - 1);
  const i = Math.min(rampColors.length - 2, Math.floor(x));
  return out.copy(rampColors[i]).lerp(rampColors[i + 1], x - i);
}

/** The same ramp as a CSS gradient for the legend. */
export function rampCss(): string {
  return `linear-gradient(to right, ${SEQUENTIAL_RAMP.join(', ')})`;
}

const hslHex = (h: number, s: number, l: number) => `#${new THREE.Color().setHSL(h, s, l).getHexString()}`;

// --- Legend model ----------------------------------------------------------------

export interface LegendSwatch { value: string; color: string; count: number }

interface LegendCommon {
  /** Title override (default: the column name). */
  title?: string;
  /** A short qualifier shown under the entry. */
  note?: string;
}

export type LegendEntry =
  | (LegendCommon & { channel: 'color'; kind: 'categorical'; column: string; swatches: LegendSwatch[]; other: number; nulls: number;
      /** Label of the no-value row (default "No value"). */ nullLabel?: string })
  | (LegendCommon & { channel: 'color' | 'size'; kind: 'sequential'; column: string; temporal: boolean; min: number; max: number; nulls: number;
      /** CSS gradient of the ramp (default: the sequential ramp). */ gradient?: string });

export interface LegendModel { entries: LegendEntry[] }

export interface ColumnEncoding {
  /** null = the default encoder (colorBy 'scheme' and sizeBy 'none'). */
  encoder: Partial<NodeEncoder> | null;
  legend: LegendModel;
}

// --- Value accessors -------------------------------------------------------------

type Accessor = (node: GraphNode, index: number, ctx: EncodingContext) => unknown;

function accessor(def: ColumnDef): Accessor {
  const s = def.source;
  if (s.from === 'node') return (node) => (s.key === 'id' ? String(node.id) : node.label);
  if (s.from === 'metadata') return (node) => (node.metadata as Record<string, unknown> | undefined)?.[s.key];
  return (_node, index, ctx) => {
    const a = ctx.analytics;
    if (!a || index * ANALYTICS_STRIDE + ANALYTICS_STRIDE > a.length) return undefined;
    return a[index * ANALYTICS_STRIDE + s.offset];
  };
}

function numericAccessor(def: ColumnDef): (node: GraphNode, index: number, ctx: EncodingContext) => number {
  const get = accessor(def);
  const parse = def.type === 'temporal' ? parseTemporal : parseNumeric;
  return (n, i, ctx) => parse(get(n, i, ctx));
}

/** Robust numeric domain: [p2, p98] of the table column (whole range when that collapses). */
function numericDomain(table: NodeColumns, name: string): { lo: number; hi: number; min: number; max: number; nulls: number } {
  const c = table.columns[name];
  const vals: number[] = [];
  let nulls = 0;
  if (c && (c.type === 'numeric' || c.type === 'temporal')) {
    for (let i = 0; i < table.rowCount; i++) {
      if (!table.present[i]) continue;
      const v = c.values[i];
      if (Number.isNaN(v)) nulls++; else vals.push(v);
    }
  }
  if (vals.length === 0) return { lo: NaN, hi: NaN, min: NaN, max: NaN, nulls };
  vals.sort((a, b) => a - b);
  const q = (p: number) => vals[Math.min(vals.length - 1, Math.max(0, Math.round(p * (vals.length - 1))))];
  const min = vals[0]; const max = vals[vals.length - 1];
  let lo = q(0.02); let hi = q(0.98);
  if (!(hi > lo)) { lo = min; hi = max; }
  return { lo, hi, min, max, nulls };
}

const normalise = (v: number, lo: number, hi: number) => (hi > lo ? (v - lo) / (hi - lo) : 0.5);

// --- Colour ----------------------------------------------------------------------

interface ColorPart { color: NodeEncoder['color']; legend: LegendEntry }

function categoricalColor(table: NodeColumns, def: ColumnDef): ColorPart {
  const name = def.name;
  const get = accessor(def);
  const c = table.columns[name];
  // counts per value over delivered rows
  const counts = new Map<string, number>();
  let nulls = 0;
  if (c && c.type === 'categorical') {
    for (let i = 0; i < table.rowCount; i++) {
      if (!table.present[i]) continue;
      const k = c.codes[i];
      if (k < 0) { nulls++; continue; }
      const v = c.dictionary[k];
      counts.set(v, (counts.get(v) ?? 0) + 1);
    }
  } else if (c && c.type === 'boolean') {
    for (let i = 0; i < table.rowCount; i++) {
      if (!table.present[i]) continue;
      const v = c.values[i];
      if (v < 0) nulls++; else counts.set(v ? 'true' : 'false', (counts.get(v ? 'true' : 'false') ?? 0) + 1);
    }
  }

  // Ordered categoricals (maturity) read as a ramp, in declared order.
  if (def.order) {
    const tiers = [...def.order];
    const pos = new Map(tiers.map((v, i) => [v, tiers.length > 1 ? i / (tiers.length - 1) : 0.5]));
    const swatches = tiers.filter((v) => counts.has(v))
      .map((v) => ({ value: v, color: `#${rampColor(pos.get(v)!, new THREE.Color()).getHexString()}`, count: counts.get(v)! }));
    const extra = [...counts.entries()].filter(([v]) => !pos.has(v)).reduce((s, [, n]) => s + n, 0);
    return {
      color: (node, idx, ctx, out) => {
        const raw = get(node, idx, ctx);
        const t = raw == null || raw === '' ? undefined : pos.get(String(raw));
        if (t === undefined) out.set(raw == null || raw === '' ? NULL_COLOR : OTHER_COLOR);
        else rampColor(t, out);
      },
      legend: { channel: 'color', kind: 'categorical', column: name, swatches, other: extra, nulls },
    };
  }

  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  let colorOf: (v: string) => string;
  if (name === 'domain' || name === 'source_domain') {
    colorOf = (v) => getDomainColor(v);
  } else if (def.source.from === 'wire' && name === 'community') {
    colorOf = (v) => hslHex(((Number(v) * 83) % 360) / 360, 0.65, 0.5);
  } else if (def.source.from === 'wire' && name === 'cluster') {
    colorOf = (v) => hslHex(((Number(v) * 137) % 360) / 360, 0.7, 0.55);
  } else {
    const assigned = new Map(ranked.slice(0, CATEGORICAL_PALETTE.length).map(([v], i) => [v, CATEGORICAL_PALETTE[i]]));
    colorOf = (v) => assigned.get(v) ?? OTHER_COLOR;
  }
  const shown = ranked.slice(0, LEGEND_MAX_SWATCHES);
  const other = ranked.slice(LEGEND_MAX_SWATCHES).reduce((s, [, n]) => s + n, 0);
  const cache = new Map<string, THREE.Color>();
  const isWire = def.source.from === 'wire';
  return {
    color: (node, idx, ctx, out) => {
      let raw = get(node, idx, ctx);
      if (def.type === 'boolean') { const b = parseBoolean(raw); raw = b < 0 ? undefined : b ? 'true' : 'false'; }
      if (raw == null || raw === '' || (isWire && (!Number.isFinite(raw as number) || raw === def.nullId))) { out.set(NULL_COLOR); return; }
      const key = String(raw);
      let col = cache.get(key);
      if (!col) { col = new THREE.Color(colorOf(key)); cache.set(key, col); }
      out.copy(col);
    },
    legend: {
      channel: 'color', kind: 'categorical', column: name,
      swatches: shown.map(([value, count]) => ({ value, color: colorOf(value), count })),
      other, nulls,
    },
  };
}

function sequentialColor(table: NodeColumns, def: ColumnDef): ColorPart {
  const read = numericAccessor(def);
  const d = numericDomain(table, def.name);
  const liveCentrality = def.source.from === 'wire' && def.name === 'centrality';
  return {
    color: (node, idx, ctx, out) => {
      const v = read(node, idx, ctx);
      if (Number.isNaN(v)) { out.set(NULL_COLOR); return; }
      // centrality: the live maximum, as the default 'centrality' scheme does
      const hi = liveCentrality && ctx.maxCentrality ? ctx.maxCentrality : d.hi;
      const lo = liveCentrality ? 0 : d.lo;
      rampColor(normalise(v, lo, hi), out);
    },
    legend: { channel: 'color', kind: 'sequential', column: def.name, temporal: def.type === 'temporal', min: d.min, max: d.max, nulls: d.nulls },
  };
}

// --- Size ------------------------------------------------------------------------

interface SizePart { size: NodeEncoder['size']; legend: LegendEntry }

function sequentialSize(table: NodeColumns, def: ColumnDef): SizePart {
  const read = numericAccessor(def);
  const d = numericDomain(table, def.name);
  return {
    size: (node, idx, ctx) => {
      const base = DEFAULT_NODE_ENCODER.size(node, idx, ctx);
      const v = read(node, idx, ctx);
      const t = Number.isNaN(v) ? 0 : Math.min(1, Math.max(0, normalise(v, d.lo, d.hi)));
      return base * (SIZE_FACTOR_MIN + (SIZE_FACTOR_MAX - SIZE_FACTOR_MIN) * t);
    },
    legend: { channel: 'size', kind: 'sequential', column: def.name, temporal: def.type === 'temporal', min: d.min, max: d.max, nulls: d.nulls },
  };
}

/** Columns a colour encoding accepts. */
export function isColorableColumn(name: string): boolean {
  const def = NODE_TABLE_COLUMN[name];
  return !!def && def.type !== 'text';
}

/** Columns a size encoding accepts. */
export function isSizeableColumn(name: string): boolean {
  const def = NODE_TABLE_COLUMN[name];
  return !!def && (def.type === 'numeric' || def.type === 'temporal');
}

/**
 * Build the encoder and legend for `colorBy` / `sizeBy`. An unknown or
 * unsuitable column falls back to the default half (and no legend entry).
 */
export function createColumnEncoding(
  table: NodeColumns | null,
  colorBy: string | null | undefined,
  sizeBy: string | null | undefined,
): ColumnEncoding {
  const entries: LegendEntry[] = [];
  const encoder: Partial<NodeEncoder> = {};
  if (table && !isSchemeColorBy(colorBy) && isColorableColumn(colorBy!)) {
    const def = NODE_TABLE_COLUMN[colorBy!];
    const part = def.type === 'categorical' || def.type === 'boolean' ? categoricalColor(table, def) : sequentialColor(table, def);
    encoder.color = part.color;
    entries.push(part.legend);
  }
  if (table && sizeBy && sizeBy !== 'none' && isSizeableColumn(sizeBy)) {
    const part = sequentialSize(table, NODE_TABLE_COLUMN[sizeBy]);
    encoder.size = part.size;
    entries.push(part.legend);
  }
  return { encoder: encoder.color || encoder.size ? encoder : null, legend: { entries } };
}

// --- Legend for the default 'scheme' encoding ---------------------------------------

/** Value counts of a categorical column over delivered rows, falling back to `fallback` per row. */
function categoryCounts(table: NodeColumns, name: string, fallback?: string): { counts: Map<string, number>; nulls: number } {
  const counts = new Map<string, number>();
  let nulls = 0;
  const c = table.columns[name];
  const f = fallback ? table.columns[fallback] : undefined;
  for (let i = 0; i < table.rowCount; i++) {
    if (!table.present[i]) continue;
    let v: string | undefined;
    if (c?.type === 'categorical' && c.codes[i] >= 0) v = c.dictionary[c.codes[i]];
    else if (f?.type === 'categorical' && f.codes[i] >= 0) v = f.dictionary[f.codes[i]];
    if (v === undefined) nulls++; else counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return { counts, nulls };
}

function schemeCategorical(
  table: NodeColumns, column: string, title: string, colorOf: (v: string) => string,
  opts: { fallback?: string; note?: string; nullLabel?: string } = {},
): LegendEntry {
  const { counts, nulls } = categoryCounts(table, column, opts.fallback);
  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  return {
    channel: 'color', kind: 'categorical', column, title, note: opts.note, nullLabel: opts.nullLabel,
    swatches: ranked.slice(0, LEGEND_MAX_SWATCHES).map(([value, count]) => ({ value, color: colorOf(value), count })),
    other: ranked.slice(LEGEND_MAX_SWATCHES).reduce((s, [, n]) => s + n, 0),
    nulls,
  };
}

/** The default 'centrality' scheme's ramp (nodeEncoding.ts): hue 0.62 → 0, lightness 0.25 → 0.65. */
export function centralitySchemeCss(): string {
  const stops = [0, 0.25, 0.5, 0.75, 1].map((t) => hslHex(0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t));
  return `linear-gradient(to right, ${stops.join(', ')})`;
}

const MODULATION_NOTE = 'Base hues; saturation and lightness rise with authority and connections.';
const FALLTHROUGH_NOTE = 'Nodes without a value keep a per-node hue.';

/**
 * The legend for colorBy 'scheme' (today's look, DEFAULT_NODE_ENCODER) under
 * `visualisation.graphs.knowledge.nodes.colorScheme`. Schemes that are renderer
 * overlays (sssp, cycles) or a single base colour (base) have no legend.
 */
export function createSchemeLegend(table: NodeColumns | null, colorScheme: string | null | undefined): LegendEntry[] {
  if (!table) return [];
  switch (colorScheme ?? 'type') {
    case 'domain':
      return [schemeCategorical(table, 'domain', 'Colour: Domain', (v) => getDomainColor(v),
        { fallback: 'source_domain', note: MODULATION_NOTE })];
    case 'type':
      return [schemeCategorical(table, 'type', 'Colour: Type', (v) => `#${getTypeColor(v).getHexString()}`,
        { note: MODULATION_NOTE })];
    case 'community':
      return [schemeCategorical(table, 'community', 'Colour: Community',
        (v) => hslHex(((Number(v) * 83) % 360) / 360, 0.65, 0.5), { note: FALLTHROUGH_NOTE, nullLabel: 'Unassigned' })];
    case 'cluster':
      return [schemeCategorical(table, 'cluster', 'Colour: Cluster',
        (v) => hslHex(((Number(v) * 137) % 360) / 360, 0.7, 0.55), { note: FALLTHROUGH_NOTE, nullLabel: 'Unassigned' })];
    case 'centrality': {
      const d = numericDomain(table, 'centrality');
      return [{
        channel: 'color', kind: 'sequential', column: 'centrality', title: 'Colour: Centrality', temporal: false,
        min: Number.isFinite(d.min) ? 0 : NaN, max: d.max, nulls: d.nulls, gradient: centralitySchemeCss(), note: FALLTHROUGH_NOTE,
      }];
    }
    default:
      return [];
  }
}
