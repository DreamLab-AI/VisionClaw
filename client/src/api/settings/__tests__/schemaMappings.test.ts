import { describe, it, expect } from 'vitest';
import { getNestedValue, setNestedFromDotPath, isVisualSettingsPath, toVisualKey, deepMergeVisual } from '../schemaMappings';

describe('getNestedValue', () => {
  const obj = { a: { b: { c: 42 } }, x: 'hello' } as any;

  it('retrieves deeply nested value', () => {
    expect(getNestedValue(obj, 'a.b.c')).toBe(42);
  });

  it('retrieves top-level value', () => {
    expect(getNestedValue(obj, 'x')).toBe('hello');
  });

  it('returns undefined for missing path', () => {
    expect(getNestedValue(obj, 'a.b.z')).toBeUndefined();
  });

  it('returns undefined when intermediate key is missing', () => {
    expect(getNestedValue(obj, 'a.z.c')).toBeUndefined();
  });

  it('returns undefined on null intermediate', () => {
    expect(getNestedValue({ a: null } as any, 'a.b')).toBeUndefined();
  });
});

describe('setNestedFromDotPath', () => {
  it('sets a deeply nested value', () => {
    const obj: Record<string, unknown> = {};
    setNestedFromDotPath(obj, 'a.b.c', 99);
    expect((obj as any).a.b.c).toBe(99);
  });

  it('overwrites an existing value', () => {
    const obj = { a: { b: 1 } } as any;
    setNestedFromDotPath(obj, 'a.b', 2);
    expect(obj.a.b).toBe(2);
  });

  it('creates intermediate objects as needed', () => {
    const obj: Record<string, unknown> = {};
    setNestedFromDotPath(obj, 'x.y.z', 'leaf');
    expect((obj as any).x.y.z).toBe('leaf');
  });

  it('handles single-segment path', () => {
    const obj: Record<string, unknown> = {};
    setNestedFromDotPath(obj, 'key', 'val');
    expect(obj.key).toBe('val');
  });
});

describe('isVisualSettingsPath', () => {
  it('returns true for visual paths not handled by other endpoints', () => {
    expect(isVisualSettingsPath('visualisation.glow.intensity')).toBe(true);
  });

  it('returns false for rendering sub-paths', () => {
    expect(isVisualSettingsPath('visualisation.rendering.fxaa')).toBe(false);
  });

  it('returns false for physics inside graphs', () => {
    expect(isVisualSettingsPath('visualisation.graphs.knowledge.physics.damping')).toBe(false);
  });

  it('returns false for non-visualisation paths', () => {
    expect(isVisualSettingsPath('system.debug.enabled')).toBe(false);
  });
});

describe('toVisualKey', () => {
  it('maps nodes sub-path', () => {
    expect(toVisualKey('visualisation.graphs.knowledge.nodes.size')).toBe('nodes.size');
  });

  it('maps edges sub-path', () => {
    expect(toVisualKey('visualisation.graphs.knowledge.edges.width')).toBe('edges.width');
  });

  it('maps labels sub-path', () => {
    expect(toVisualKey('visualisation.graphs.knowledge.labels.fontSize')).toBe('labels.fontSize');
  });

  it('maps generic visualisation path', () => {
    expect(toVisualKey('visualisation.glow.intensity')).toBe('glow.intensity');
  });
});

describe('deepMergeVisual', () => {
  it('overrides top-level scalar', () => {
    const result = deepMergeVisual({ a: 1 }, { a: 2 });
    expect(result.a).toBe(2);
  });

  it('preserves defaults not present in stored', () => {
    const result = deepMergeVisual({ a: 1, b: 2 }, { a: 3 });
    expect(result.b).toBe(2);
  });

  it('deep-merges nested objects', () => {
    const result = deepMergeVisual({ x: { y: 1, z: 2 } }, { x: { y: 99 } });
    expect((result.x as any).y).toBe(99);
    expect((result.x as any).z).toBe(2);
  });

  it('does not deep-merge arrays — stored value wins', () => {
    const result = deepMergeVisual({ arr: [1, 2] }, { arr: [3] });
    expect(result.arr).toEqual([3]);
  });
});

describe('retired system.websocket settings', () => {
  // The update-rate, motion, chunk, compression and reconnect knobs did nothing:
  // the position stream rate is visualisation.graphs.knowledge.physics.broadcastFps,
  // and the server heartbeat is file config (settings.yaml), not a client setting.
  it('the client settings model carries no system.websocket block', async () => {
    const { transformApiToClientSettings } = await import('../schemaMappings');
    const s = transformApiToClientSettings({} as any) as any;
    expect(s.system).toBeDefined();
    expect('websocket' in s.system).toBe(false);
  });

  it('no system.websocket path is loaded at startup', async () => {
    const { ESSENTIAL_PATHS } = await import('../../../store/settings/settingsHelpers');
    expect(ESSENTIAL_PATHS.filter((p: string) => p.startsWith('system.websocket'))).toEqual([]);
  });
});
