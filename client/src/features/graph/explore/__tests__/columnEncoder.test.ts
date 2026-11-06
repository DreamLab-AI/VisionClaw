import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { buildNodeColumns } from '../tableSchema';
import {
  createColumnEncoding, createSchemeLegend, rampColor, CATEGORICAL_PALETTE, NULL_COLOR, SEQUENTIAL_RAMP,
  SIZE_FACTOR_MIN, SIZE_FACTOR_MAX,
} from '../columnEncoder';
import { DEFAULT_NODE_ENCODER, type EncodingContext } from '../../state/nodeEncoding';
import { getDomainColor } from '../../utils/domainColors';
import { ANALYTICS_STRIDE, ANALYTICS_CENTRALITY_OFFSET } from '../../../analytics/analyticsLayout';
import { exploreNodes, node } from './fixtures';

const nodes = exploreNodes();
const indexMap = new Map(nodes.map((n, i) => [String(n.id), i]));
const table = buildNodeColumns(nodes, { indexMap });
const ctx: EncodingContext = { connectionCountMap: new Map() };
const hex = (c: THREE.Color) => `#${c.getHexString()}`;
const colorOf = (enc: ReturnType<typeof createColumnEncoding>, n = nodes[0], i = 0, c: EncodingContext = ctx) => {
  const out = new THREE.Color();
  enc.encoder!.color!(n, i, c, out);
  return out;
};

describe('column encoder (ADR-2184 D4)', () => {
  it("'scheme' and 'none' keep the default encoder and show no legend", () => {
    const e = createColumnEncoding(table, 'scheme', 'none');
    expect(e.encoder).toBeNull();
    expect(e.legend.entries).toEqual([]);
  });

  it('colour by domain uses getDomainColor, so it matches today\'s domain look', () => {
    const e = createColumnEncoding(table, 'domain', 'none');
    expect(e.encoder!.size).toBeUndefined(); // the size half stays default
    expect(hex(colorOf(e, nodes[4], 4))).toBe(new THREE.Color(getDomainColor('blockchain')).getHexString().replace(/^/, '#'));
    const legend = e.legend.entries[0];
    expect(legend.kind).toBe('categorical');
    if (legend.kind !== 'categorical') return;
    // counts over delivered rows (the stub is delivered and robotics)
    expect(legend.swatches.map((s) => [s.value, s.count])).toEqual([
      ['robotics', 5], ['artificial-intelligence', 4], ['blockchain', 3],
    ]);
    expect(legend.swatches[0].color).toBe(getDomainColor('robotics'));
  });

  it('other categoricals take the palette in frequency order; no value is the neutral slate', () => {
    const e = createColumnEncoding(table, 'type', 'none');
    if (e.legend.entries[0].kind !== 'categorical') throw new Error('expected categorical');
    expect(e.legend.entries[0].swatches.map((s) => [s.value, s.color])).toEqual([
      ['page', CATEGORICAL_PALETTE[0]], ['linked_page', CATEGORICAL_PALETTE[1]],
    ]);
    const bare = node(99, 'bare', {});
    expect(hex(colorOf(e, bare, 0))).toBe(new THREE.Color(NULL_COLOR).getHexString().replace(/^/, '#'));
  });

  it('ordered categoricals (maturity) read along the sequential ramp in declared order', () => {
    const e = createColumnEncoding(table, 'maturity', 'none');
    const entry = e.legend.entries[0];
    if (entry.kind !== 'categorical') throw new Error('expected categorical');
    expect(entry.swatches.map((s) => s.value)).toEqual(['draft', 'growing', 'mature']);
    expect(entry.swatches[0].color).toBe(SEQUENTIAL_RAMP[0]);
    expect(entry.swatches[2].color).toBe(SEQUENTIAL_RAMP[SEQUENTIAL_RAMP.length - 1]);
  });

  it('numeric colour maps the robust domain onto the ramp, with min/max in the legend', () => {
    const e = createColumnEncoding(table, 'quality_score', 'none');
    const lo = colorOf(e, nodes[0], 0);
    const hi = colorOf(e, nodes[10], 10);
    expect(hex(lo)).toBe(hex(rampColor(0, new THREE.Color())));
    expect(hex(hi)).toBe(hex(rampColor(1, new THREE.Color())));
    const entry = e.legend.entries[0];
    expect(entry).toMatchObject({ kind: 'sequential', channel: 'color', column: 'quality_score', temporal: false });
    if (entry.kind === 'sequential') { expect(entry.min).toBeCloseTo(0.1); expect(entry.max).toBeCloseTo(0.95); expect(entry.nulls).toBe(1); }
  });

  it('temporal colour is sequential and flagged temporal', () => {
    const e = createColumnEncoding(table, 'last_commit', 'none');
    expect(e.legend.entries[0]).toMatchObject({ kind: 'sequential', temporal: true, nulls: 6 });
  });

  it('wire centrality is read live from the analytics buffer, normalised by the live maximum', () => {
    const e = createColumnEncoding(table, 'centrality', 'none');
    const a = new Float32Array(nodes.length * ANALYTICS_STRIDE);
    a[2 * ANALYTICS_STRIDE + ANALYTICS_CENTRALITY_OFFSET] = 4;
    const live: EncodingContext = { ...ctx, analytics: a, maxCentrality: 4 };
    expect(hex(colorOf(e, nodes[2], 2, live))).toBe(hex(rampColor(1, new THREE.Color())));
    // without frames: no value
    expect(hex(colorOf(e, nodes[2], 2))).toBe(new THREE.Color(NULL_COLOR).getHexString().replace(/^/, '#'));
  });

  it('size by a numeric column scales today\'s size within the clamp', () => {
    const e = createColumnEncoding(table, 'scheme', 'quality_score');
    expect(e.encoder!.color).toBeUndefined();
    const base = DEFAULT_NODE_ENCODER.size(nodes[0], 0, ctx);
    expect(e.encoder!.size!(nodes[0], 0, ctx)).toBeCloseTo(base * SIZE_FACTOR_MIN);
    expect(e.encoder!.size!(nodes[10], 10, ctx)).toBeCloseTo(base * SIZE_FACTOR_MAX);
    expect(e.legend.entries[0]).toMatchObject({ channel: 'size', column: 'quality_score' });
  });

  it('an unknown or text column falls back to the default', () => {
    expect(createColumnEncoding(table, 'definition', 'domain').encoder).toBeNull();
    expect(createColumnEncoding(table, 'no_such_column', 'none').encoder).toBeNull();
  });
});

describe("legend for colorBy 'scheme' (today's look)", () => {
  it('domain: getDomainColor swatches, source_domain as fallback, with the modulation note', () => {
    const t = buildNodeColumns([...nodes, node(50, 'sd', { source_domain: 'blockchain' })],
      { indexMap: new Map([...indexMap, ['50', 12]]) });
    const [e] = createSchemeLegend(t, 'domain');
    if (e.kind !== 'categorical') throw new Error('categorical');
    expect(e.swatches.map((s) => [s.value, s.count, s.color])).toEqual([
      ['robotics', 5, getDomainColor('robotics')],
      ['artificial-intelligence', 4, getDomainColor('artificial-intelligence')],
      ['blockchain', 4, getDomainColor('blockchain')],
    ]);
    expect(e.note).toMatch(/authority/);
  });

  it('community: the default scheme hue per community, unassigned rows labelled', () => {
    const a = new Float32Array(nodes.length * ANALYTICS_STRIDE);
    a[0 * ANALYTICS_STRIDE + 2] = 3; a[1 * ANALYTICS_STRIDE + 2] = 3; a[2 * ANALYTICS_STRIDE + 2] = 7;
    const t = buildNodeColumns(nodes, { indexMap, analytics: a });
    const [e] = createSchemeLegend(t, 'community');
    if (e.kind !== 'categorical') throw new Error('categorical');
    expect(e.swatches.map((s) => [s.value, s.count])).toEqual([['3', 2], ['7', 1]]);
    expect(e.swatches[0].color).toBe(`#${new THREE.Color().setHSL(((3 * 83) % 360) / 360, 0.65, 0.5).getHexString()}`);
    expect(e.nullLabel).toBe('Unassigned');
    expect(e.nulls).toBe(9);
  });

  it('type and centrality have entries; base, sssp and cycles have none', () => {
    expect(createSchemeLegend(table, 'type')[0]).toMatchObject({ kind: 'categorical', column: 'type' });
    expect(createSchemeLegend(table, undefined)[0]).toMatchObject({ column: 'type' });
    expect(createSchemeLegend(table, 'centrality')[0]).toMatchObject({ kind: 'sequential', gradient: expect.stringContaining('linear-gradient') });
    for (const s of ['base', 'sssp', 'cycles']) expect(createSchemeLegend(table, s)).toEqual([]);
  });
});
