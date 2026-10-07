import { describe, it, expect } from 'vitest';
import {
  buildIndexMaps,
  buildCloudColours,
  resolveFlashTargets,
  applyFocusDim,
  ageRanks,
  CLOUD_PALETTE,
} from '../cloudData';
import type { MemoryCloudMeta } from '../types';

const meta = (key: string, namespace: string, sourceType: string, updatedAt: number): MemoryCloudMeta => ({
  id: key, key, namespace, sourceType, updatedAt,
});

const rows: MemoryCloudMeta[] = [
  meta('a', 'patterns', 'memory', 300),
  meta('b', 'project-state', 'hook', 100),
  meta('a', 'patterns', 'memory', 200),
  meta('c', 'patterns', 'doc', 400),
];

describe('buildIndexMaps', () => {
  it('indexes rows by key and by namespace, keeping duplicates', () => {
    const m = buildIndexMaps(rows);
    expect(m.byKey.get('a')).toEqual([0, 2]);
    expect(m.byNamespace.get('patterns')).toEqual([0, 2, 3]);
    expect(m.byNamespace.get('project-state')).toEqual([1]);
  });
  it('also indexes namespace-qualified keys', () => {
    const m = buildIndexMaps(rows);
    expect(m.byKey.get('patterns:c')).toEqual([3]);
  });
});

describe('resolveFlashTargets', () => {
  const maps = buildIndexMaps(rows);
  it('targets every row with the exact key', () => {
    expect(resolveFlashTargets({ key: 'a', namespace: 'patterns' }, maps)).toEqual({ indices: [0, 2], match: 'key' });
  });
  it('narrows a duplicated key to the event namespace when that disambiguates', () => {
    const m = buildIndexMaps([meta('k', 'x', 't', 1), meta('k', 'y', 't', 1)]);
    expect(resolveFlashTargets({ key: 'k', namespace: 'y' }, m)).toEqual({ indices: [1], match: 'key' });
  });
  it('falls back to up to three rows of the namespace', () => {
    const r = resolveFlashTargets({ key: 'zzz', namespace: 'patterns' }, maps, () => 0);
    expect(r.match).toBe('namespace');
    expect(r.indices.length).toBeGreaterThan(0);
    expect(r.indices.length).toBeLessThanOrEqual(3);
    r.indices.forEach((i) => expect([0, 2, 3]).toContain(i));
  });
  it('never invents a random point: unmatched flashes report none', () => {
    expect(resolveFlashTargets({ key: 'zzz', namespace: 'unknown' }, maps)).toEqual({ indices: [], match: 'none' });
    expect(resolveFlashTargets({ key: 'zzz' }, maps)).toEqual({ indices: [], match: 'none' });
  });
});

describe('ageRanks', () => {
  it('maps the oldest row to 0 and the newest to 1 by rank', () => {
    expect(ageRanks(rows)).toEqual([2 / 3, 0, 1 / 3, 1]);
  });
  it('handles a single row', () => {
    expect(ageRanks([rows[0]])).toEqual([1]);
  });
});

describe('buildCloudColours', () => {
  const snap = { count: 4, metadata: rows, namespaces: ['patterns', 'project-state'], sourceTypes: ['doc', 'hook', 'memory'] };
  const rgb = (hex: number) => [((hex >> 16) & 255) / 255, ((hex >> 8) & 255) / 255, (hex & 255) / 255];

  it('colours by namespace category', () => {
    const c = buildCloudColours(snap, 'namespace');
    expect(c.length).toBe(12);
    expect(Array.from(c.slice(0, 3))).toEqual(rgb(CLOUD_PALETTE[0]).map((v) => Math.fround(v)));
    expect(Array.from(c.slice(3, 6))).toEqual(rgb(CLOUD_PALETTE[1]).map((v) => Math.fround(v)));
  });
  it('colours by source type category', () => {
    const c = buildCloudColours(snap, 'sourceType');
    expect(Array.from(c.slice(9, 12))).toEqual(rgb(CLOUD_PALETTE[0]).map((v) => Math.fround(v)));
  });
  it('colours by age: newest brightest mint, oldest dim blue', () => {
    const c = buildCloudColours(snap, 'age');
    const lum = (i: number) => c[i * 3] + c[i * 3 + 1] + c[i * 3 + 2];
    expect(lum(3)).toBeGreaterThan(lum(0));
    expect(lum(0)).toBeGreaterThan(lum(2));
    expect(lum(2)).toBeGreaterThan(lum(1));
  });
});

describe('applyFocusDim', () => {
  const base = new Float32Array([1, 1, 1, 0.5, 0.5, 0.5]);
  it('copies the base when there is no focus', () => {
    const out = new Float32Array(6);
    applyFocusDim(base, out, null, 0.8);
    expect(Array.from(out)).toEqual(Array.from(base));
  });
  it('dims rows off the route by the amount and leaves route rows untouched', () => {
    const out = new Float32Array(6);
    applyFocusDim(base, out, new Set([1]), 0.75);
    expect(Array.from(out.slice(0, 3)).map((v) => +v.toFixed(4))).toEqual([0.25, 0.25, 0.25]);
    expect(Array.from(out.slice(3))).toEqual([0.5, 0.5, 0.5]);
  });
});
