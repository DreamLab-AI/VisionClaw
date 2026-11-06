import { describe, it, expect } from 'vitest';
import { buildNodeColumns } from '../tableSchema';
import { toArrowTable } from '../nodeTable';
import { exploreNodes } from './fixtures';

describe('Arrow node table (ADR-2184 D1)', () => {
  it('keeps render-index rows, typed columns and nulls', async () => {
    const nodes = exploreNodes();
    // slot 12 is empty (a withheld node has no row content)
    const indexMap = new Map(nodes.map((n, i) => [String(n.id), i === 11 ? 12 : i]));
    const t = await toArrowTable(buildNodeColumns(nodes, { indexMap }));
    expect(t.numRows).toBe(13);
    expect(t.getChild('present')!.get(11)).toBe(false);
    expect(t.getChild('present')!.get(12)).toBe(true);
    expect(t.getChild('id')!.get(0)).toBe('1');
    expect(t.getChild('quality_score')!.get(1)).toBeCloseTo(0.2);
    expect(t.getChild('quality_score')!.get(11)).toBeNull();
    expect(t.getChild('domain')!.get(4)).toBe('blockchain');
    expect(String(t.schema.fields.find((f) => f.name === 'domain')!.type)).toMatch(/Dictionary/);
    expect(t.getChild('last_commit')!.get(0)).toBe(Date.parse('2026-01-10T00:00:00Z'));
    expect(t.getChild('last_commit')!.get(8)).toBeNull();
  });
});
