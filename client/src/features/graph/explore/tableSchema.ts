/**
 * Node-table column schema (ADR-2184 D2) and a dependency-free column builder.
 *
 * Every column is typed by this declaration, never by guessing per value: the
 * server's `metadata` is a `HashMap<String, String>`, so numbers, booleans and
 * dates all arrive as strings and are parsed here. Keys not declared are text.
 *
 * Sources:
 *   - `node`: a field of the `/api/graph/data` node object (`id`, `label`);
 *   - `metadata`: a key of `node.metadata` (strings on the wire);
 *   - `wire`: an analytics field of the live 52-byte position record
 *     (crates/visionclaw-wire/src/lib.rs), read from the render-index-aligned
 *     analytics buffer (`nodeAnalyticsStore.getIndexedBuffer`, stride 5).
 *
 * Rows are aligned to the shared node render index (see `../state/nodeIndex.ts`):
 * row `i` is the node in slot `i` of the highlight mask and the encoding buffers,
 * so an encoder or widget never needs a second lookup. The explore lane turns
 * these arrays into an Apache Arrow table; this module stays Arrow-free.
 */
import type { Node as GraphNode } from '../managers/graphDataManager';
import {
  ANALYTICS_STRIDE,
  ANALYTICS_CLUSTER_OFFSET,
  ANALYTICS_ANOMALY_OFFSET,
  ANALYTICS_COMMUNITY_OFFSET,
  ANALYTICS_CENTRALITY_OFFSET,
} from '../../analytics/store/nodeAnalyticsStore';
import { slotCountFor } from '../state/nodeIndex';

export type ColumnType = 'categorical' | 'numeric' | 'temporal' | 'text' | 'boolean';

export type ColumnSource =
  | { from: 'node'; key: 'id' | 'label' }
  | { from: 'metadata'; key: string }
  | { from: 'wire'; offset: number };

export interface ColumnDef {
  name: string;
  type: ColumnType;
  nullable: boolean;
  source: ColumnSource;
  /** Categorical display order (e.g. maturity tiers); unordered when absent. */
  order?: readonly string[];
  /** Multi-valued text joined by this separator on the wire (`tags`: ", "). */
  separator?: string;
  /** Wire categoricals: this id means "no value" (cluster / community 0 = unassigned). */
  nullId?: number;
  description?: string;
}

const md = (key: string, type: ColumnType, extra: Partial<ColumnDef> = {}): ColumnDef =>
  ({ name: key, type, nullable: true, source: { from: 'metadata', key }, ...extra });

/** The OWL maturity tiers, lowest first (matches useGraphFiltering's MATURITY_RANK). */
export const MATURITY_ORDER = ['draft', 'developing', 'emerging', 'growing', 'established', 'mature'] as const;

export const NODE_TABLE_SCHEMA: readonly ColumnDef[] = Object.freeze([
  { name: 'id', type: 'text', nullable: false, source: { from: 'node', key: 'id' } },
  { name: 'label', type: 'text', nullable: false, source: { from: 'node', key: 'label' } },

  // metadata keys present on /api/graph/data today
  md('type', 'categorical', { description: 'Authoritative population origin (page, linked_page, owl_class, …); not the top-level scaffold `type`.' }),
  md('domain', 'categorical'),
  md('source_domain', 'categorical'),
  md('maturity', 'categorical', { order: MATURITY_ORDER }),
  md('ontology_type', 'categorical'),
  md('owl_type', 'categorical'),
  md('public', 'boolean', { description: '"true" / "false" on the wire.' }),
  md('quality_score', 'numeric'),
  md('wikilink_count', 'numeric'),
  md('file_size', 'numeric', { description: 'Bytes.' }),
  md('tags', 'text', { separator: ', ' }),
  md('definition', 'text'),
  md('class_iri', 'text'),
  md('page_iri', 'text'),
  md('slug', 'text'),
  md('source_file', 'text'),

  // temporal columns the server lane adds (RFC 3339 dates, integer-string count)
  md('last_modified', 'temporal', { description: 'RFC 3339; epoch ms in the column.' }),
  md('last_commit', 'temporal', { description: 'RFC 3339; epoch ms in the column.' }),
  md('change_count', 'numeric', { description: 'Integer string on the wire.' }),

  // live wire-record analytics (ADR-2018), joined by render index
  { name: 'cluster', type: 'categorical', nullable: true, source: { from: 'wire', offset: ANALYTICS_CLUSTER_OFFSET }, nullId: 0 },
  { name: 'community', type: 'categorical', nullable: true, source: { from: 'wire', offset: ANALYTICS_COMMUNITY_OFFSET }, nullId: 0 },
  { name: 'centrality', type: 'numeric', nullable: true, source: { from: 'wire', offset: ANALYTICS_CENTRALITY_OFFSET } },
  { name: 'anomaly', type: 'numeric', nullable: true, source: { from: 'wire', offset: ANALYTICS_ANOMALY_OFFSET } },
] as ColumnDef[]);

export const NODE_TABLE_COLUMN = Object.freeze(
  Object.fromEntries(NODE_TABLE_SCHEMA.map((c) => [c.name, c])) as Record<string, ColumnDef>,
);

// --- Column arrays -------------------------------------------------------------

/** numeric / temporal: Float64Array, NaN = null. temporal holds epoch ms. */
export interface NumberColumn { def: ColumnDef; type: 'numeric' | 'temporal'; values: Float64Array }
/** boolean: Int8Array, 1 / 0, -1 = null. */
export interface BooleanColumn { def: ColumnDef; type: 'boolean'; values: Int8Array }
/** categorical: dictionary-encoded (Arrow-ready). codes index `dictionary`; -1 = null. */
export interface CategoricalColumn { def: ColumnDef; type: 'categorical'; codes: Int32Array; dictionary: string[] }
/** text: one string per row, null = missing. */
export interface TextColumn { def: ColumnDef; type: 'text'; values: (string | null)[] }

export type Column = NumberColumn | BooleanColumn | CategoricalColumn | TextColumn;

export interface NodeColumns {
  /** Row count = render slot count. */
  rowCount: number;
  /** 1 where a delivered node occupies the slot. */
  present: Uint8Array;
  columns: Record<string, Column>;
}

export interface BuildColumnsOptions {
  /** id → render index; rows are aligned to it. */
  indexMap: ReadonlyMap<string, number>;
  /** Render-index-aligned analytics buffer (stride 5); wire columns are null without it. */
  analytics?: Float32Array | null;
  /** Defaults to NODE_TABLE_SCHEMA. */
  schema?: readonly ColumnDef[];
}

function rawValue(node: GraphNode, idx: number, def: ColumnDef, analytics: Float32Array | null | undefined): unknown {
  const s = def.source;
  if (s.from === 'node') return s.key === 'id' ? String(node.id) : node.label;
  if (s.from === 'metadata') return (node.metadata as Record<string, unknown> | undefined)?.[s.key];
  const at = idx * ANALYTICS_STRIDE + s.offset;
  if (!analytics || idx * ANALYTICS_STRIDE + ANALYTICS_STRIDE > analytics.length) return undefined;
  return analytics[at];
}

const isMissing = (v: unknown) => v === undefined || v === null || v === '';

/** Parse a wire value to a number; NaN when missing or unparseable. */
export function parseNumeric(v: unknown): number {
  if (isMissing(v)) return NaN;
  const n = typeof v === 'number' ? v : Number(String(v).trim());
  return Number.isFinite(n) ? n : NaN;
}

/** Parse an RFC 3339 / ISO date (or epoch ms number) to epoch ms; NaN when missing. */
export function parseTemporal(v: unknown): number {
  if (isMissing(v)) return NaN;
  if (typeof v === 'number') return Number.isFinite(v) ? v : NaN;
  const t = Date.parse(String(v));
  return Number.isFinite(t) ? t : NaN;
}

/** "true"/"false" (any case), "1"/"0", or a boolean; -1 when missing or unrecognised. */
export function parseBoolean(v: unknown): 1 | 0 | -1 {
  if (typeof v === 'boolean') return v ? 1 : 0;
  if (isMissing(v)) return -1;
  const s = String(v).trim().toLowerCase();
  if (s === 'true' || s === '1') return 1;
  if (s === 'false' || s === '0') return 0;
  return -1;
}

/**
 * Build one typed array per column from the node list, rows aligned to the
 * render index. Nodes absent from `indexMap` (withheld) get no row.
 */
export function buildNodeColumns(nodes: readonly GraphNode[], opts: BuildColumnsOptions): NodeColumns {
  const schema = opts.schema ?? NODE_TABLE_SCHEMA;
  const n = slotCountFor(opts.indexMap);
  const present = new Uint8Array(n);
  const slots: (GraphNode | undefined)[] = new Array(n);
  for (const node of nodes) {
    const idx = opts.indexMap.get(String(node.id));
    if (idx === undefined || idx >= n) continue;
    slots[idx] = node;
    present[idx] = 1;
  }

  const columns: Record<string, Column> = {};
  for (const def of schema) {
    switch (def.type) {
      case 'numeric':
      case 'temporal': {
        const values = new Float64Array(n).fill(NaN);
        const parse = def.type === 'numeric' ? parseNumeric : parseTemporal;
        for (let i = 0; i < n; i++) {
          const node = slots[i];
          if (node) values[i] = parse(rawValue(node, i, def, opts.analytics));
        }
        columns[def.name] = { def, type: def.type, values };
        break;
      }
      case 'boolean': {
        const values = new Int8Array(n).fill(-1);
        for (let i = 0; i < n; i++) {
          const node = slots[i];
          if (node) values[i] = parseBoolean(rawValue(node, i, def, opts.analytics));
        }
        columns[def.name] = { def, type: 'boolean', values };
        break;
      }
      case 'categorical': {
        const codes = new Int32Array(n).fill(-1);
        const dictionary: string[] = def.order ? [...def.order] : [];
        const lookup = new Map(dictionary.map((v, i) => [v, i]));
        for (let i = 0; i < n; i++) {
          const node = slots[i];
          if (!node) continue;
          const raw = rawValue(node, i, def, opts.analytics);
          if (isMissing(raw)) continue;
          if (typeof raw === 'number' && (!Number.isFinite(raw) || raw === def.nullId)) continue;
          const key = String(raw);
          let code = lookup.get(key);
          if (code === undefined) { code = dictionary.length; dictionary.push(key); lookup.set(key, code); }
          codes[i] = code;
        }
        columns[def.name] = { def, type: 'categorical', codes, dictionary };
        break;
      }
      case 'text': {
        const values: (string | null)[] = new Array(n).fill(null);
        for (let i = 0; i < n; i++) {
          const node = slots[i];
          if (!node) continue;
          const raw = rawValue(node, i, def, opts.analytics);
          values[i] = isMissing(raw) ? null : String(raw);
        }
        columns[def.name] = { def, type: 'text', values };
        break;
      }
    }
  }
  return { rowCount: n, present, columns };
}

/** Split a multi-valued text cell (`tags`) by its declared separator. */
export function splitMulti(def: ColumnDef, cell: string | null): string[] {
  if (!cell) return [];
  return def.separator ? cell.split(def.separator).map((s) => s.trim()).filter(Boolean) : [cell];
}
