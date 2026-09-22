/**
 * ADR-2041 / EXP-V06 — the knowledge-graph settings key is `knowledge`; a
 * persisted `visualisation.graphs.logseq` object migrates onto it without loss
 * and the legacy key is dropped so the next save emits only `knowledge`.
 *
 * The server alias was retired by ADR-2115 (superseding ADR-2041); this
 * receive-side migration stays so existing users' localStorage keeps loading.
 */
import { describe, it, expect } from 'vitest';
import { migrateGraphSettingsKey } from '../../../store/settings/settingsHelpers';
import { toVisualKey } from '../../../api/settings/schemaMappings';

type Persisted = Record<string, any>;

const legacyPersisted = (): Persisted => ({
  visualisation: {
    glow: { intensity: 3 },
    graphs: {
      logseq: {
        nodes: { baseColor: '#202724', opacity: 0.8 },
        edges: { color: '#445566' },
        labels: { enableLabels: true },
        physics: { springK: 0.2, repelK: 1.0 },
        tweening: { enabled: true, lerpBase: 0.05 },
      },
      visionclaw: { nodes: { baseColor: '#ff0000' } },
    },
  },
});

describe('ADR-2041 settings migration: graphs.logseq → graphs.knowledge', () => {
  it('moves the whole legacy object onto `knowledge` without loss', () => {
    const before = legacyPersisted();
    const after = migrateGraphSettingsKey(before);
    const graphs = after.visualisation.graphs;

    // byte-for-byte the same payload, under the new key
    expect(graphs.knowledge).toEqual(legacyPersisted().visualisation.graphs.logseq);
    // the colours EXP-V06 names specifically
    expect(graphs.knowledge.nodes.baseColor).toBe('#202724');
    expect(graphs.knowledge.edges.color).toBe('#445566');
  });

  it('drops the legacy key so the next save emits only `knowledge`', () => {
    const after = migrateGraphSettingsKey(legacyPersisted());
    expect('logseq' in after.visualisation.graphs).toBe(false);
    expect(JSON.stringify(after)).not.toContain('logseq');
  });

  it('leaves sibling graphs and unrelated sections untouched', () => {
    const after = migrateGraphSettingsKey(legacyPersisted());
    expect(after.visualisation.graphs.visionclaw.nodes.baseColor).toBe('#ff0000');
    expect(after.visualisation.glow.intensity).toBe(3);
  });

  it('prefers an existing `knowledge` object when both keys are present', () => {
    const both: Persisted = {
      visualisation: {
        graphs: {
          logseq: { nodes: { baseColor: '#000000' } },
          knowledge: { nodes: { baseColor: '#ffffff' } },
        },
      },
    };
    const after = migrateGraphSettingsKey(both);
    expect(after.visualisation.graphs.knowledge.nodes.baseColor).toBe('#ffffff');
    expect('logseq' in after.visualisation.graphs).toBe(false);
  });

  it('is a no-op for already-migrated and for empty/partial state', () => {
    const migrated: Persisted = {
      visualisation: { graphs: { knowledge: { nodes: { baseColor: '#202724' } } } },
    };
    expect(migrateGraphSettingsKey(migrated)).toBe(migrated);
    expect(migrateGraphSettingsKey({})).toEqual({});
    expect(migrateGraphSettingsKey({ visualisation: {} })).toEqual({ visualisation: {} });
    expect(migrateGraphSettingsKey(undefined as unknown as Persisted)).toBeUndefined();
  });

  it('is idempotent', () => {
    const once = migrateGraphSettingsKey(legacyPersisted());
    expect(migrateGraphSettingsKey(once)).toEqual(once);
  });
});

describe('receive-side mapping of persisted pre-ADR-2041 settings paths', () => {
  it('maps a legacy settings path to the same visual key as the new one', () => {
    expect(toVisualKey('visualisation.graphs.logseq.nodes.baseColor')).toBe('nodes.baseColor');
    expect(toVisualKey('visualisation.graphs.knowledge.nodes.baseColor')).toBe('nodes.baseColor');
    expect(toVisualKey('visualisation.graphs.logseq.edges.color')).toBe('edges.color');
    expect(toVisualKey('visualisation.graphs.logseq.labels.enableLabels')).toBe('labels.enableLabels');
  });
});
