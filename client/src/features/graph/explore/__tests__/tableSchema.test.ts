import { describe, it, expect } from 'vitest';
import {
  NODE_TABLE_SCHEMA, NODE_TABLE_COLUMN, MATURITY_ORDER, buildNodeColumns, splitMulti,
  parseNumeric, parseTemporal, parseBoolean,
  type NumberColumn, type BooleanColumn, type CategoricalColumn, type TextColumn,
} from '../tableSchema';
import type { Node as GraphNode } from '../../managers/graphDataManager';

const node = (id: string, label: string, metadata: Record<string, string>): GraphNode =>
  ({ id, label, position: { x: 0, y: 0, z: 0 }, metadata } as unknown as GraphNode);

const NODES = [
  node('10', 'Alpha', {
    type: 'page', domain: 'robotics', maturity: 'growing', public: 'true', quality_score: '0.92',
    wikilink_count: '14', file_size: '2048', tags: 'ai, robots', last_modified: '2026-10-01T12:00:00Z',
    last_commit: '2026-10-02T08:30:00+01:00', change_count: '7', class_iri: 'urn:x:Alpha',
  }),
  node('20', 'Beta', { type: 'linked_page', public: 'false', quality_score: 'n/a', maturity: 'draft' }),
  node('30', 'Private', { type: 'page' }), // not in the index map: withheld
];
// render index deliberately not the list order
const INDEX = new Map([['20', 0], ['10', 2]]);
const ANALYTICS = new Float32Array([
  0, 0.0, 4, 0.01, 0,   // slot 0 (Beta): community 4, cluster 0 = unassigned
  0, 0, 0, 0, 0,        // slot 1: empty
  3, 0.5, 9, 0.2, 0,    // slot 2 (Alpha)
]);

describe('node table schema (ADR-2184 D2)', () => {
  it('declares every required column with the right type and nullability', () => {
    const want: Record<string, string> = {
      wikilink_count: 'numeric', definition: 'text', public: 'boolean', maturity: 'categorical',
      class_iri: 'text', type: 'categorical', source_domain: 'categorical', domain: 'categorical',
      source_file: 'text', ontology_type: 'categorical', page_iri: 'text', owl_type: 'categorical',
      slug: 'text', file_size: 'numeric', quality_score: 'numeric', tags: 'text',
      cluster: 'categorical', community: 'categorical', centrality: 'numeric', anomaly: 'numeric',
      last_modified: 'temporal', last_commit: 'temporal', change_count: 'numeric',
    };
    for (const [name, type] of Object.entries(want)) {
      expect(NODE_TABLE_COLUMN[name]?.type, name).toBe(type);
    }
    for (const t of ['last_modified', 'last_commit', 'change_count']) expect(NODE_TABLE_COLUMN[t].nullable).toBe(true);
    expect(NODE_TABLE_COLUMN.type.source).toEqual({ from: 'metadata', key: 'type' });
    expect(new Set(NODE_TABLE_SCHEMA.map((c) => c.name)).size).toBe(NODE_TABLE_SCHEMA.length);
  });

  it('builds render-index-aligned typed columns; withheld nodes have no row', () => {
    const t = buildNodeColumns(NODES, { indexMap: INDEX, analytics: ANALYTICS });
    expect(t.rowCount).toBe(3);
    expect(Array.from(t.present)).toEqual([1, 0, 1]);

    const id = t.columns.id as TextColumn;
    expect(id.values).toEqual(['20', null, '10']);

    const q = t.columns.quality_score as NumberColumn;
    expect(q.values[2]).toBe(0.92);
    expect(q.values[0]).toBeNaN(); // 'n/a' → null, not a guess

    const pub = t.columns.public as BooleanColumn;
    expect(Array.from(pub.values)).toEqual([0, -1, 1]);

    const lm = t.columns.last_modified as NumberColumn;
    expect(lm.type).toBe('temporal');
    expect(lm.values[2]).toBe(Date.UTC(2026, 9, 1, 12));
    expect(lm.values[0]).toBeNaN();
    expect((t.columns.last_commit as NumberColumn).values[2]).toBe(Date.UTC(2026, 9, 2, 7, 30));
    expect((t.columns.change_count as NumberColumn).values[2]).toBe(7);

    const mat = t.columns.maturity as CategoricalColumn;
    expect(mat.dictionary.slice(0, MATURITY_ORDER.length)).toEqual([...MATURITY_ORDER]);
    expect(mat.dictionary[mat.codes[0]]).toBe('draft');
    expect(mat.dictionary[mat.codes[2]]).toBe('growing');
    expect(mat.codes[1]).toBe(-1);

    const cluster = t.columns.cluster as CategoricalColumn;
    expect(cluster.codes[0]).toBe(-1); // id 0 = unassigned
    expect(cluster.dictionary[cluster.codes[2]]).toBe('3');
    const comm = t.columns.community as CategoricalColumn;
    expect(comm.dictionary[comm.codes[0]]).toBe('4');
    expect((t.columns.centrality as NumberColumn).values[2]).toBeCloseTo(0.2, 6);

    expect(splitMulti(NODE_TABLE_COLUMN.tags, (t.columns.tags as TextColumn).values[2])).toEqual(['ai', 'robots']);
  });

  it('wire columns are null without an analytics buffer', () => {
    const t = buildNodeColumns(NODES, { indexMap: INDEX });
    expect(Array.from((t.columns.community as CategoricalColumn).codes)).toEqual([-1, -1, -1]);
    expect((t.columns.anomaly as NumberColumn).values[2]).toBeNaN();
  });

  it('parsers', () => {
    expect(parseNumeric(' 42 ')).toBe(42);
    expect(parseNumeric('')).toBeNaN();
    expect(parseTemporal('not a date')).toBeNaN();
    expect(parseBoolean('TRUE')).toBe(1);
    expect(parseBoolean('yes')).toBe(-1);
  });
});
