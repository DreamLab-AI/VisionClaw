import { describe, it, expect } from 'vitest';
import { buildNodeColumns, type NumberColumn, type CategoricalColumn, type TextColumn } from '../tableSchema';
import { exploreNodes } from './fixtures';

// ADR-2184 D1 (amended 2026-11-06): the node table is typed arrays. These are
// the facts the deleted Arrow conversion was tested for, on the typed arrays
// themselves: render-index rows, empty slots, typed columns and nulls.
describe('node table, typed arrays (ADR-2184 D1)', () => {
  it('keeps render-index rows, typed columns and nulls', () => {
    const nodes = exploreNodes();
    // slot 12 is empty (a withheld node has no row content)
    const indexMap = new Map(nodes.map((n, i) => [String(n.id), i === 11 ? 12 : i]));
    const t = buildNodeColumns(nodes, { indexMap });
    expect(t.rowCount).toBe(13);
    expect(t.present[11]).toBe(0);
    expect(t.present[12]).toBe(1);
    expect((t.columns.id as TextColumn).values[0]).toBe('1');
    const q = t.columns.quality_score as NumberColumn;
    expect(q.values[1]).toBeCloseTo(0.2);
    expect(q.values[11]).toBeNaN();
    const domain = t.columns.domain as CategoricalColumn;
    expect(domain.type).toBe('categorical');
    expect(domain.dictionary[domain.codes[4]]).toBe('blockchain');
    const lc = t.columns.last_commit as NumberColumn;
    expect(lc.type).toBe('temporal');
    expect(lc.values[0]).toBe(Date.parse('2026-01-10T00:00:00Z'));
    expect(lc.values[8]).toBeNaN();
  });
});
