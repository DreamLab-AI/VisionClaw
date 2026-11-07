import { describe, it, expect } from 'vitest';
import { spritePopulation, SPRITE_POPULATION_MIN } from '../gemLod';

// Regression for the live fault of 2026-11-07. ADR-2182's note hard-wired the
// overview sprites and the gem LOD to the knowledge population ("only the
// knowledge population becomes sprites… revisit if either grows to
// knowledge-graph size"). The live scene is ontology 9,595 / knowledge 84, so
// the 9,595 ontology gems never handed off to sprites and the overview was a
// solid disc of lit gems. The policy now follows size, not the name.
describe('spritePopulation', () => {
  it('picks the ontology population when it is the large one (the live scene)', () => {
    expect(spritePopulation([
      { mode: 'knowledge_graph', count: 84 },
      { mode: 'ontology', count: 9595 },
      { mode: 'agent', count: 0 },
    ])).toBe('ontology');
  });

  it('keeps the knowledge population when it is the large one (the ADR-2182 scene)', () => {
    expect(spritePopulation([
      { mode: 'knowledge_graph', count: 9604 },
      { mode: 'ontology', count: 84 },
      { mode: 'agent', count: 4 },
    ])).toBe('knowledge_graph');
  });

  it('draws every gem when no population reaches knowledge-graph size', () => {
    expect(spritePopulation([
      { mode: 'knowledge_graph', count: 840 },
      { mode: 'ontology', count: 468 },
      { mode: 'agent', count: 0 },
    ])).toBeNull();
  });

  it('is exact at the threshold and prefers the first listed on a tie', () => {
    expect(spritePopulation([{ mode: 'ontology', count: SPRITE_POPULATION_MIN }])).toBe('ontology');
    expect(spritePopulation([{ mode: 'ontology', count: SPRITE_POPULATION_MIN - 1 }])).toBeNull();
    expect(spritePopulation([
      { mode: 'knowledge_graph', count: 5000 },
      { mode: 'ontology', count: 5000 },
    ])).toBe('knowledge_graph');
  });
});
