import { describe, it, expect } from 'vitest';
import { graphView as GRAPH_VIEW_GROUP } from '../groups/graphView';
import { normaliseEdgeCapacity } from '../../../graph/state/overviewEdgeCapacity';

// ADR-2181 D4: the Edge Budget select is the desktop twin of the headset's Edges limit.
// A select writes strings, so every option must round-trip through the normaliser.
describe('Edge Budget control', () => {
  const field = GRAPH_VIEW_GROUP.fields.find((f) => f.key === 'overviewEdgeBudget');

  it('exists as a select on overviewEdges.capacity', () => {
    expect(field?.type).toBe('select');
    expect(field?.path).toBe('visualisation.graphView.overviewEdges.capacity');
  });

  it('every option normalises to auto or its number', () => {
    for (const opt of field?.options ?? []) {
      const want = opt === 'auto' ? 'auto' : Number(opt);
      expect(normaliseEdgeCapacity(opt)).toBe(want);
    }
  });

  it('offers the headset budget of 20,000', () => {
    expect(field?.options).toContain('20000');
  });
});
