/**
 * Characterisation of the default node encoder (ADR-2184 D4: "the defaults
 * reproduce today's look").
 *
 * GemNodes now calls the encoder for the knowledge base colour, so the
 * verbatim GemNodes oracle and its source guard are gone. The look is pinned
 * by GOLDEN snapshots: literal linear-RGB values for every scheme × base colour
 * × analytics combination, recorded while the oracle still ran in the same
 * suite and matched the encoder exactly (render lane, feat/gv-render). Never
 * update these snapshots to make a change pass: a diff here is a change of
 * today's look.
 */
import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { NodeEncoding, DEFAULT_NODE_ENCODER, COLOR_BY_SCHEME, isSchemeColorBy, type EncodingContext } from '../nodeEncoding';
import { DEFAULT_GRAPH_VIEW, DEFAULT_NODES_SETTINGS } from '../../../../api/settings/defaults';
import { getTypeColor, getDomainColor } from '../../hooks/useGraphNodeColors';
import { computeNodeScale } from '../../utils/nodeScaling';
import type { Node as GraphNode } from '../../managers/graphDataManager';



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

  it('GOLDEN: pinned linear RGB for every scheme × base colour × analytics combination', () => {
    for (const scheme of SCHEMES) {
      for (const baseNodeColor of [null, '#4a6fa5']) {
        for (const withAnalytics of [false, true]) {
          const enc = encode({
            connectionCountMap: CC, colorScheme: scheme, baseNodeColor,
            analytics: withAnalytics ? ANALYTICS : null, maxCentrality: 0.04, showAnomalies: true,
          });
          expect(Array.from(enc.colors, (v) => Number(v.toFixed(6))), `${scheme}/${baseNodeColor ?? 'none'}/${withAnalytics}`)
            .toMatchSnapshot();
        }
      }
    }
  });

  it('writes at the render index, not the loop index, and skips unmapped nodes', () => {
    const shuffled = new Map([['5', 0], ['1', 1], ['private', 2]]);
    const enc = new NodeEncoding(() => shuffled);
    enc.encodeAll(NODES, { connectionCountMap: CC, colorScheme: 'domain' });
    expect(enc.sizes.length).toBe(3);
    const ref = encode({ connectionCountMap: CC, colorScheme: 'domain' });
    // node '1' is at slot 1 here and slot 0 in the reference encoding
    expect([enc.colors[3], enc.colors[4], enc.colors[5]]).toEqual([ref.colors[0], ref.colors[1], ref.colors[2]]);
    expect(enc.sizes[2]).toBe(0); // slot with no delivered node
  });

  it('setEncoder swaps one half and keeps the default other half; version bumps per setEncoder and encodeAll', () => {
    const enc = new NodeEncoding(() => idx);
    const versions: number[] = [];
    enc.subscribe((v) => versions.push(v));
    enc.setEncoder({ color: (_n, _i, _c, out) => { out.setRGB(1, 0, 0); } });
    enc.encodeAll(NODES, { connectionCountMap: CC, nodeSize: 1 });
    expect(Array.from(enc.colors.slice(0, 3))).toEqual([1, 0, 0]);
    expect(enc.sizes[0]).toBe(Math.fround(DEFAULT_NODE_ENCODER.size(NODES[0], 0, { connectionCountMap: CC, nodeSize: 1 })));
    enc.setEncoder(null);
    enc.encodeAll(NODES, { connectionCountMap: CC });
    // setEncoder bumps and notifies too (so the renderer re-encodes at once)
    expect(versions).toEqual([1, 2, 3, 4]);
    expect(enc.encoderVersion).toBe(2);
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
    // The shipped defaults are a pinned GOLDEN combination ('community/#4a6fa5/true').
    expect([scheme, base]).toEqual(['community', '#4a6fa5']);
    expect(SCHEMES).toContain(scheme);
    expect(enc.colors.length).toBe(NODES.length * 3);
  });
});
