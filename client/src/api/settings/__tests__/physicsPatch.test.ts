import { describe, it, expect } from 'vitest';
import { physicsPatchEntry } from '../physicsPatch';
import { transformApiToClientSettings, emptyAllSettings } from '../schemaMappings';

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

describe('physics.layoutMode round trip (ADR-2140 item 1)', () => {
  const PATH = 'visualisation.graphs.knowledge.physics.layoutMode';

  it('writes: the registry path is a flat physics patch, like any other scalar', () => {
    expect(physicsPatchEntry(PATH, 'dagLeftRight')).toEqual({ layoutMode: 'dagLeftRight' });
    expect(physicsPatchEntry('visualisation.graphs.knowledge.physics.ssspAlpha', 1.5)).toEqual({ ssspAlpha: 1.5 });
  });

  it('reads: the server physics document reaches the same registry path unchanged', () => {
    const api = { ...emptyAllSettings(), physics: { ssspAlpha: 1.5, layoutMode: 'typeClustering' } } as never;
    const client = transformApiToClientSettings(api) as unknown as {
      visualisation: { graphs: { knowledge: { physics: Record<string, unknown> } } };
    };
    const physics = client.visualisation.graphs.knowledge.physics;
    expect(physics.layoutMode).toBe('typeClustering');
    expect(physics.ssspAlpha).toBe(1.5);
  });
});
