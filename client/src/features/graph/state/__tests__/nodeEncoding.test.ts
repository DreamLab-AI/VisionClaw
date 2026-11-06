// Delete the GemNodes SOURCE GUARD and ORACLE below once GemNodes calls DEFAULT_NODE_ENCODER; never bend the encoder to pass them.
/**
 * Characterisation of the default node encoder against GemNodes (ADR-2184 D4:
 * "the defaults reproduce today's look").
 *
 * GemNodes' `computeColor` is a closure inside the component, so it cannot be
 * imported. The test pins it three ways:
 *   1. ORACLE: the knowledge-mode body of GemNodes.tsx `computeColor`, copied
 *      verbatim below, run against the same inputs as the encoder;
 *   2. SOURCE GUARD: the exact GemNodes expressions the oracle copies must still
 *      be present in GemNodes.tsx, so a change there fails this test rather than
 *      silently diverging from the encoder;
 *   3. GOLDEN: literal linear-RGB values for sample nodes (domain, type, base).
 */
import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { NodeEncoding, DEFAULT_NODE_ENCODER, COLOR_BY_SCHEME, isSchemeColorBy, type EncodingContext } from '../nodeEncoding';
import { DEFAULT_GRAPH_VIEW, DEFAULT_NODES_SETTINGS } from '../../../../api/settings/defaults';
import { getTypeColor, getDomainColor } from '../../hooks/useGraphNodeColors';
import { computeNodeScale } from '../../utils/nodeScaling';
import type { Node as GraphNode } from '../../managers/graphDataManager';

// vite-node here cannot resolve static `fs`/`path` imports; take them from the runtime
// (same pattern as features/graph/__tests__/singleDrawnSource.test.ts).
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule;
const { readFileSync } = getBuiltin('fs') as typeof import('fs');
const { resolve } = getBuiltin('path') as typeof import('path');

// ---- 1. Oracle: verbatim from GemNodes.tsx computeColor (knowledge mode) ----
const _col = new THREE.Color();
const _baseHSL = { h: 0, s: 0, l: 0 };
const hashHue = (s: string): number => {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return ((h >>> 0) % 360) / 360;
};
function gemNodesColour(node: any, nodeIndex: number, colorScheme: string, connectionCountMap: Map<string, number>,
  baseNodeColor: string | null, analytics: Float32Array | null, maxCentrality: number, showAnomalies: boolean): THREE.Color {
  const qualityGates = { showAnomalies };
  const maxCentralityRef = { current: maxCentrality };
  if (analytics && nodeIndex !== undefined && nodeIndex * 5 + 4 < analytics.length) {
    const a = nodeIndex * 5;
    const clusterId = analytics[a + 0];
    const anomalyScore = analytics[a + 1];
    const communityId = analytics[a + 2];
    const centrality = analytics[a + 3];
    if (qualityGates?.showAnomalies && anomalyScore > 0.01) {
      const intensity = Math.min(anomalyScore, 1.0);
      return _col.setRGB(0.9 * intensity + 0.1, 0.15 * (1 - intensity), 0.1);
    }
    if (colorScheme === 'community' && communityId > 0) {
      const hue = ((communityId * 83) % 360) / 360;
      return _col.setHSL(hue, 0.65, 0.5);
    }
    if (colorScheme === 'cluster' && clusterId > 0) {
      const hue = ((clusterId * 137) % 360) / 360;
      return _col.setHSL(hue, 0.7, 0.55);
    }
    if (colorScheme === 'centrality' && centrality > 0) {
      const max = maxCentralityRef.current || 1;
      const t = Math.min(centrality / max, 1.0);
      return _col.setHSL(0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t);
    }
  }
  const auth = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;
  const cc = connectionCountMap.get(String(node.id)) || 0;
  if (colorScheme === 'type') {
    const nodeType = node.metadata?.type ?? node.metadata?.nodeType;
    _col.copy(getTypeColor(nodeType)).getHSL(_baseHSL);
    const sat = _baseHSL.s + auth * 0.15 + Math.min(cc / 30, 0.1);
    const lit = _baseHSL.l + auth * 0.12 + Math.min(cc / 40, 0.06);
    _col.setHSL(_baseHSL.h, Math.min(sat, 0.95), Math.min(lit, 0.8));
    return _col;
  }
  if (colorScheme === 'domain') {
    const domain = node.metadata?.domain ?? node.metadata?.source_domain;
    _col.set(getDomainColor(domain)).getHSL(_baseHSL);
    const sat = _baseHSL.s + auth * 0.15 + Math.min(cc / 30, 0.1);
    const lit = _baseHSL.l + auth * 0.12 + Math.min(cc / 40, 0.06);
    _col.setHSL(_baseHSL.h, Math.min(sat, 0.95), Math.min(lit, 0.8));
    return _col;
  }
  if (baseNodeColor) {
    _col.set(baseNodeColor).getHSL(_baseHSL);
    const jitter = (hashHue(node.label || String(node.id)) - 0.5) * 0.12;
    const hue = (_baseHSL.h + jitter + 1) % 1;
    const sat = _baseHSL.s + auth * 0.25 + Math.min(cc / 25, 0.15);
    const lit = _baseHSL.l + auth * 0.15;
    _col.setHSL(hue, Math.min(sat, 0.95), Math.min(lit, 0.8));
    return _col;
  }
  const hue = hashHue(node.label || String(node.id));
  const sat = 0.35 + auth * 0.35 + Math.min(cc / 20, 0.2);
  const lit = 0.45 + auth * 0.2;
  _col.setHSL(hue, Math.min(sat, 0.9), Math.min(lit, 0.75));
  return _col;
}

// Both GemNodes (instanceColor) and the encoder store Float32; compare at that precision.
const f32 = (c: THREE.Color) => [Math.fround(c.r), Math.fround(c.g), Math.fround(c.b)];

// ---- Fixtures ----
const node = (id: string, label: string, metadata: Record<string, unknown>): GraphNode =>
  ({ id, label, position: { x: 0, y: 0, z: 0 }, metadata } as unknown as GraphNode);

const NODES: GraphNode[] = [
  node('1', 'Neural Networks', { domain: 'artificial-intelligence', type: 'page' }),
  node('2', 'Smart Contracts', { source_domain: 'BC', type: 'page', authority: 0.8 }),
  node('3', 'Unknown Thing', { domain: 'not-a-domain', type: 'linked_page' }),
  node('4', 'Hub Page', { domain: 'robotics', type: 'page', authority: '0.5' }), // string authority, coerced
  node('5', 'Bare', {}),
];
const CC = new Map<string, number>([['1', 3], ['2', 0], ['3', 12], ['4', 60], ['5', 1]]);
const idx = new Map(NODES.map((n, i) => [String(n.id), i]));

// analytics stride 5: [cluster, anomaly, community, centrality, sssp]
const ANALYTICS = new Float32Array([
  2, 0, 7, 0.02, 0,     // node 1: cluster 2, community 7, centrality 0.02
  0, 0.5, 3, 0, 0,      // node 2: anomaly 0.5
  0, 0, 0, 0, 0,        // node 3: no analytics → falls through
  5, 0, 11, 0.04, 0,    // node 4
  0, 0, 0, 0, 0,        // node 5
]);

const SCHEMES = ['domain', 'type', 'base', 'community', 'cluster', 'centrality'] as const;

function encode(ctx: EncodingContext) {
  const enc = new NodeEncoding(() => idx);
  enc.encodeAll(NODES, ctx);
  return enc;
}

describe('default node encoder (ADR-2184 D4) reproduces GemNodes', () => {
  for (const scheme of SCHEMES) {
    for (const baseNodeColor of [null, '#4a6fa5']) {
      for (const withAnalytics of [false, true]) {
        it(`colour: scheme=${scheme} base=${baseNodeColor ?? 'none'} analytics=${withAnalytics}`, () => {
          const analytics = withAnalytics ? ANALYTICS : null;
          const enc = encode({
            connectionCountMap: CC, colorScheme: scheme, baseNodeColor, analytics,
            maxCentrality: 0.04, showAnomalies: true,
          });
          NODES.forEach((n, i) => {
            const want = gemNodesColour(n, i, scheme, CC, baseNodeColor, analytics, 0.04, true);
            expect([enc.colors[i * 3], enc.colors[i * 3 + 1], enc.colors[i * 3 + 2]], `node ${n.id}`)
              .toEqual(f32(want));
          });
        });
      }
    }
  }

  it('size: computeNodeScale(...) * nodeSize, as GemNodes.tsx:863', () => {
    const enc = encode({ connectionCountMap: CC, nodeSize: 0.4 });
    NODES.forEach((n, i) => {
      expect(enc.sizes[i]).toBe(Math.fround(computeNodeScale(n, CC, 'knowledge_graph', undefined, undefined) * 0.4));
    });
    // GemNodes falls back to a 1.0 gain when nodeSize is unset.
    const enc1 = encode({ connectionCountMap: CC });
    expect(enc1.sizes[0]).toBe(Math.fround(computeNodeScale(NODES[0], CC, 'knowledge_graph') * 1.0));
  });

  it('GOLDEN: pinned linear RGB for the domain, type and base schemes', () => {
    const golden = (scheme: string, base: string | null) => {
      const e = encode({ connectionCountMap: CC, colorScheme: scheme, baseNodeColor: base });
      return Array.from(e.colors, (v) => Number(v.toFixed(5)));
    };
    expect(golden('domain', null)).toMatchSnapshot();
    expect(golden('type', null)).toMatchSnapshot();
    expect(golden('base', '#4a6fa5')).toMatchSnapshot();
  });

  it('writes at the render index, not the loop index, and skips unmapped nodes', () => {
    const shuffled = new Map([['5', 0], ['1', 1], ['private', 2]]);
    const enc = new NodeEncoding(() => shuffled);
    enc.encodeAll(NODES, { connectionCountMap: CC, colorScheme: 'domain' });
    expect(enc.sizes.length).toBe(3);
    const want = gemNodesColour(NODES[0], 1, 'domain', CC, null, null, 0, false);
    expect([enc.colors[3], enc.colors[4], enc.colors[5]]).toEqual(f32(want));
    expect(enc.sizes[2]).toBe(0); // slot with no delivered node
  });

  it('setEncoder swaps one half and keeps the default other half; version bumps per encodeAll', () => {
    const enc = new NodeEncoding(() => idx);
    const versions: number[] = [];
    enc.subscribe((v) => versions.push(v));
    enc.setEncoder({ color: (_n, _i, _c, out) => { out.setRGB(1, 0, 0); } });
    enc.encodeAll(NODES, { connectionCountMap: CC, nodeSize: 1 });
    expect(Array.from(enc.colors.slice(0, 3))).toEqual([1, 0, 0]);
    expect(enc.sizes[0]).toBe(Math.fround(DEFAULT_NODE_ENCODER.size(NODES[0], 0, { connectionCountMap: CC, nodeSize: 1 })));
    enc.setEncoder(null);
    enc.encodeAll(NODES, { connectionCountMap: CC });
    expect(versions).toEqual([1, 2]);
  });

  it('SOURCE GUARD: GemNodes still contains the expressions the oracle copies', () => {
    const src = readFileSync(resolve(__dirname, '../../components/GemNodes.tsx'), 'utf8');
    const expressions = [
      "const auth = node.metadata?.authority ?? node.metadata?.authorityScore ?? 0;",
      "const cc = connectionCountMap.get(String(node.id)) || 0;",
      "const domain = node.metadata?.domain ?? node.metadata?.source_domain;",
      "_col.set(getDomainColor(domain)).getHSL(_baseHSL);",
      "const nodeType = node.metadata?.type ?? node.metadata?.nodeType;",
      "_col.copy(getTypeColor(nodeType)).getHSL(_baseHSL);",
      "const sat = _baseHSL.s + auth * 0.15 + Math.min(cc / 30, 0.1);",
      "const lit = _baseHSL.l + auth * 0.12 + Math.min(cc / 40, 0.06);",
      "_col.setHSL(_baseHSL.h, Math.min(sat, 0.95), Math.min(lit, 0.8));",
      "const jitter = (hashHue(node.label || String(node.id)) - 0.5) * 0.12;",
      "const sat = _baseHSL.s + auth * 0.25 + Math.min(cc / 25, 0.15);",
      "const lit = _baseHSL.l + auth * 0.15;",
      "const sat = 0.35 + auth * 0.35 + Math.min(cc / 20, 0.2);",
      "const lit = 0.45 + auth * 0.2;",
      "_col.setHSL(hue, Math.min(sat, 0.9), Math.min(lit, 0.75));",
      "return _col.setRGB(0.9 * intensity + 0.1, 0.15 * (1 - intensity), 0.1);",
      "const hue = ((communityId * 83) % 360) / 360;",
      "return _col.setHSL(hue, 0.65, 0.5);",
      "const hue = ((clusterId * 137) % 360) / 360;",
      "return _col.setHSL(hue, 0.7, 0.55);",
      "return _col.setHSL(0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t);",
      "scaleCache[i] = computeNodeScale(node, connectionCountMap, mode, hierarchyMap, graphTypeVisuals) * baseScale;",
      "const baseScale = (nodeSettings?.nodeSize as number | undefined) ?? 1.0;",
      "?? 'type';",
    ];
    for (const e of expressions) expect(src.includes(e), e).toBe(true);
  });
});

describe('explore.colorBy default (ADR-2184 D4)', () => {
  it("defaults to the 'scheme' sentinel, which selects the default encoder", () => {
    expect(DEFAULT_GRAPH_VIEW.explore.colorBy).toBe(COLOR_BY_SCHEME);
    expect(isSchemeColorBy(DEFAULT_GRAPH_VIEW.explore.colorBy)).toBe(true);
    expect(isSchemeColorBy('domain')).toBe(false);
    expect(new NodeEncoding().getEncoder()).toBe(DEFAULT_NODE_ENCODER);
  });

  it('under the shipped settings defaults, reproduces GemNodes (today\'s look)', () => {
    const scheme = DEFAULT_NODES_SETTINGS.colorScheme;
    const base = DEFAULT_NODES_SETTINGS.baseColor;
    const enc = encode({ connectionCountMap: CC, colorScheme: scheme, baseNodeColor: base, analytics: ANALYTICS, maxCentrality: 0.04, showAnomalies: true });
    NODES.forEach((n, i) => {
      const want = gemNodesColour(n, i, scheme, CC, base, ANALYTICS, 0.04, true);
      expect([enc.colors[i * 3], enc.colors[i * 3 + 1], enc.colors[i * 3 + 2]], `node ${n.id}`).toEqual(f32(want));
    });
  });
});
