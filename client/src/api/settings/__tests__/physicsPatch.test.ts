import { describe, it, expect } from 'vitest';
import { physicsPatchEntry } from '../physicsPatch';

describe('physicsPatchEntry (registry path -> physics PUT patch)', () => {
  it('keeps a top-level physics field flat', () => {
    expect(physicsPatchEntry('visualisation.graphs.knowledge.physics.springK', 12)).toEqual({ springK: 12 });
  });

  it('nests a semantic path by its segments below ".physics."', () => {
    // Previously the key was the LAST segment only, so this slider would have written a
    // top-level "levelAttraction" that no server field has: the control would do nothing.
    expect(physicsPatchEntry('visualisation.graphs.knowledge.physics.semantic.dag.levelAttraction', 0.1)).toEqual({
      semantic: { dag: { levelAttraction: 0.1 } },
    });
  });

  it('works for any graph name and for booleans', () => {
    expect(physicsPatchEntry('visualisation.graphs.logseq.physics.semantic.typeCluster.enabled', false)).toEqual({
      semantic: { typeCluster: { enabled: false } },
    });
  });

  it('returns null for a path that is not under .physics.', () => {
    expect(physicsPatchEntry('visualisation.rendering.ambientLightIntensity', 1)).toBeNull();
  });
});

import { semanticDefaults } from '../defaults';

describe('semanticDefaults (client reset values come from the shared Rust-emitted document)', () => {
  it('has the eight forces, each with an enabled flag, and the calibrated values', () => {
    const d = semanticDefaults();
    expect(Object.keys(d).sort()).toEqual([
      'attributeSpring', 'collision', 'crossDomain', 'dag', 'maturityLayout', 'physicalityCluster', 'roleCluster', 'typeCluster',
    ]);
    for (const g of Object.values(d)) expect(typeof g.enabled).toBe('boolean');
    // the two forces that impose planes ship off; DAG strength is the calibrated 0.03, not 0.5
    expect(d.dag.enabled).toBe(false);
    expect(d.maturityLayout.enabled).toBe(false);
    expect(d.dag.levelAttraction).toBe(0.03);
    expect(d.collision.collisionStrength).toBe(0.2);
  });
});
