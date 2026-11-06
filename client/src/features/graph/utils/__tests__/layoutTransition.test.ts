import { describe, it, expect } from 'vitest';
import { lerpLayoutTransition, transitionMassFactor } from '../layoutTransition';

// VC-31 debt: the mass lookup used the render index as the key of a map keyed
// by node id, so it found the right degree only where an id equals its slot.
describe('layout transition mass (by node id, not render index)', () => {
  // Two nodes whose ids are not their render slots: id "a" (hub, 100 edges) in
  // slot 1, id "b" (leaf, 0 edges) in slot 0.
  const nodeIdToIndexMap = new Map([['a', 1], ['b', 0]]);
  const nodes = [{ id: 'a' }, { id: 'b' }];
  const connectionCountMap = new Map([['a', 100], ['b', 0]]);
  const start = new Float32Array([0, 0, 0, 0, 0, 0]);
  const target = new Float32Array([10, 0, 0, 10, 0, 0]);

  it('the hub runs ahead and the leaf moves at the eased pace', () => {
    const drawn = new Float32Array(6);
    lerpLayoutTransition(drawn, start, target, 0.5, 2, connectionCountMap, nodes, nodeIdToIndexMap);
    // slot 0 is the leaf "b": progress 0.5 / factor 1 → halfway
    expect(drawn[0]).toBeCloseTo(5, 5);
    // slot 1 is the hub "a": factor 1/(1+10·0.3) = 0.25 → min(0.5/0.25, 1) = 1
    expect(transitionMassFactor(100)).toBeCloseTo(0.25, 9);
    expect(drawn[3]).toBeCloseTo(10, 5);
  });

  it('a slot with no node keeps unit mass', () => {
    const drawn = new Float32Array(9);
    lerpLayoutTransition(drawn, new Float32Array(9), new Float32Array([0, 0, 0, 0, 0, 0, 4, 0, 0]), 0.5, 3, connectionCountMap, nodes, nodeIdToIndexMap);
    expect(drawn[6]).toBeCloseTo(2, 5);
  });
});
