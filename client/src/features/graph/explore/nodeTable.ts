/**
 * The node table as Apache Arrow (ADR-2184 D1).
 *
 * Built from `buildNodeColumns` (tableSchema.ts) — the data the client already
 * received, so the table exposes nothing beyond the graph load and the live
 * analytics. Rows stay aligned to the shared render index: row `i` is slot `i`
 * of the highlight mask and the encoding buffers; slots with no delivered node
 * are all-null with `present = false`.
 *
 * Column types: numeric → Float64, temporal → TimestampMillisecond, boolean →
 * Bool, categorical → Dictionary<Utf8, Int32>, text → Utf8. NaN / -1 / null in
 * the typed arrays become Arrow nulls.
 *
 * apache-arrow (Apache-2.0) is imported dynamically, so it lands in its own
 * chunk and is fetched only when a consumer asks for the Arrow form. Encoding and
 * the widgets read the typed arrays directly and never need it.
 */
import type { Table, Vector } from 'apache-arrow';
import type { NodeColumns } from './tableSchema';

export async function toArrowTable(cols: NodeColumns): Promise<Table> {
  const arrow = await import('apache-arrow');
  const { vectorFromArray, Float64, Bool, Utf8, Int32, Dictionary, TimestampMillisecond } = arrow;
  const n = cols.rowCount;
  const vectors: Record<string, Vector> = {};

  vectors.present = vectorFromArray(Array.from(cols.present, (p) => p === 1), new Bool());
  for (const [name, c] of Object.entries(cols.columns)) {
    switch (c.type) {
      case 'numeric':
        vectors[name] = vectorFromArray(Array.from(c.values, (v) => (Number.isNaN(v) ? null : v)), new Float64());
        break;
      case 'temporal':
        vectors[name] = vectorFromArray(Array.from(c.values, (v) => (Number.isNaN(v) ? null : v)), new TimestampMillisecond());
        break;
      case 'boolean':
        vectors[name] = vectorFromArray(Array.from(c.values, (v) => (v < 0 ? null : v === 1)), new Bool());
        break;
      case 'categorical': {
        const vals: (string | null)[] = new Array(n);
        for (let i = 0; i < n; i++) vals[i] = c.codes[i] < 0 ? null : c.dictionary[c.codes[i]];
        vectors[name] = vectorFromArray(vals, new Dictionary(new Utf8(), new Int32()));
        break;
      }
      case 'text':
        vectors[name] = vectorFromArray(c.values, new Utf8());
        break;
    }
  }
  return new arrow.Table(vectors);
}
