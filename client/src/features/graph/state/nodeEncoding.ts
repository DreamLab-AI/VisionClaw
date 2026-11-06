/**
 * Per-node visual encoding buffers (ADR-2184 D4).
 *
 * Colour (linear RGB, stride 3) and size scale (stride 1), indexed by the shared
 * node render index (see `nodeIndex.ts`), so sprites (ADR-2182) and labels
 * (ADR-2186) read slot `i` of these buffers and of the highlight mask for the same
 * node. `encodeAll` runs the active encoder over the node list and bumps `version`.
 *
 * The DEFAULT encoder reproduces today's knowledge-gem base colour — GemNodes'
 * `computeColor` (GemNodes.tsx:454-583) for the knowledge-graph mode — and its
 * scale (`computeNodeScale(...) * nodeSize`, GemNodes.tsx:863). It covers the
 * anomaly overlay, the community / cluster / centrality analytic schemes (each
 * falling through when the node has no value), and the type / domain / base
 * schemes with their authority + connection modulation. It leaves to the renderer
 * the overlays that are not node attributes: the `cycles` and `sssp` schemes, an
 * on-demand SSSP result, ontology / agent populations, timeline tint and
 * attention heat. Characterised in `__tests__/nodeEncoding.test.ts`.
 *
 * Pure TS (three's Color only), no React.
 */
import * as THREE from 'three';
import type { Node as GraphNode } from '../managers/graphDataManager';
import type { GraphVisualMode } from '../hooks/useGraphVisualState';
import type { GraphTypeVisualsSettings } from '../../settings/config/settings';
import { computeNodeScale } from '../utils/nodeScaling';
import { getTypeColor, getDomainColor } from '../hooks/useGraphNodeColors';
import {
  ANALYTICS_STRIDE,
  ANALYTICS_CLUSTER_OFFSET,
  ANALYTICS_ANOMALY_OFFSET,
  ANALYTICS_COMMUNITY_OFFSET,
  ANALYTICS_CENTRALITY_OFFSET,
} from '../../analytics/store/nodeAnalyticsStore';
import { type NodeIndexResolver, sharedNodeIndexResolver, slotCountFor } from './nodeIndex';

/** The `visualisation.graphs.knowledge.nodes.colorScheme` values. */
export type NodeColorScheme =
  | 'type' | 'domain' | 'base' | 'community' | 'cluster' | 'centrality' | 'sssp' | 'cycles';

/** Everything an encoder may read besides the node itself. */
export interface EncodingContext {
  connectionCountMap: ReadonlyMap<string, number>;
  /** GemNodes reads this setting with a `?? 'type'` fallback; so does the default encoder. */
  colorScheme?: NodeColorScheme | string;
  /** `visualisation.graphs.knowledge.nodes.baseColor`, used by the `base` scheme. */
  baseNodeColor?: string | null;
  /** `nodeAnalyticsStore.getIndexedBuffer(map)`: stride ANALYTICS_STRIDE, by render index. */
  analytics?: Float32Array | null;
  /** Live maximum centrality over `analytics` (GemNodes' maxCentralityRef). */
  maxCentrality?: number;
  /** `qualityGates.showAnomalies`: the red anomaly overlay wins over every scheme. */
  showAnomalies?: boolean;
  /** `visualisation.graphs.knowledge.nodes.nodeSize`; GemNodes falls back to 1.0. */
  nodeSize?: number;
  graphMode?: GraphVisualMode;
  hierarchyMap?: Map<string, unknown>;
  graphTypeVisuals?: GraphTypeVisualsSettings;
}

/**
 * An encoding. `color` writes linear RGB into `out` (a three Color); `size`
 * returns the size scale. `index` is the node's render index.
 */
export interface NodeEncoder {
  color(node: GraphNode, index: number, ctx: EncodingContext, out: THREE.Color): void;
  size(node: GraphNode, index: number, ctx: EncodingContext): number;
}

export type NodeEncodingListener = (version: number) => void;

// --- Default encoder (today's knowledge-gem look) ------------------------------

const _hsl = { h: 0, s: 0, l: 0 };

/** Same hash as GemNodes' `hashHue`: deterministic hue in [0, 1). */
export function hashHue(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return ((h >>> 0) % 360) / 360;
}

function defaultColor(node: GraphNode, index: number, ctx: EncodingContext, out: THREE.Color): void {
  const scheme = ctx.colorScheme ?? 'type';
  const a = ctx.analytics;
  if (a && index * ANALYTICS_STRIDE + ANALYTICS_STRIDE - 1 < a.length) {
    const b = index * ANALYTICS_STRIDE;
    const anomaly = a[b + ANALYTICS_ANOMALY_OFFSET];
    if (ctx.showAnomalies && anomaly > 0.01) {
      const t = Math.min(anomaly, 1.0);
      out.setRGB(0.9 * t + 0.1, 0.15 * (1 - t), 0.1);
      return;
    }
    const community = a[b + ANALYTICS_COMMUNITY_OFFSET];
    if (scheme === 'community' && community > 0) { out.setHSL(((community * 83) % 360) / 360, 0.65, 0.5); return; }
    const cluster = a[b + ANALYTICS_CLUSTER_OFFSET];
    if (scheme === 'cluster' && cluster > 0) { out.setHSL(((cluster * 137) % 360) / 360, 0.7, 0.55); return; }
    const centrality = a[b + ANALYTICS_CENTRALITY_OFFSET];
    if (scheme === 'centrality' && centrality > 0) {
      const t = Math.min(centrality / (ctx.maxCentrality || 1), 1.0);
      out.setHSL(0.62 - 0.62 * t, 0.85, 0.25 + 0.4 * t);
      return;
    }
  }

  // Metadata values may be strings; JS coerces them in the arithmetic, as in GemNodes.
  const md = node.metadata as Record<string, any> | undefined;
  const auth = md?.authority ?? md?.authorityScore ?? 0;
  const cc = ctx.connectionCountMap.get(String(node.id)) || 0;

  if (scheme === 'type' || scheme === 'domain') {
    if (scheme === 'type') out.copy(getTypeColor(md?.type ?? md?.nodeType));
    else out.set(getDomainColor(md?.domain ?? md?.source_domain));
    out.getHSL(_hsl);
    const sat = _hsl.s + auth * 0.15 + Math.min(cc / 30, 0.1);
    const lit = _hsl.l + auth * 0.12 + Math.min(cc / 40, 0.06);
    out.setHSL(_hsl.h, Math.min(sat, 0.95), Math.min(lit, 0.8));
    return;
  }

  if (ctx.baseNodeColor) {
    out.set(ctx.baseNodeColor).getHSL(_hsl);
    const jitter = (hashHue(node.label || String(node.id)) - 0.5) * 0.12;
    const sat = _hsl.s + auth * 0.25 + Math.min(cc / 25, 0.15);
    const lit = _hsl.l + auth * 0.15;
    out.setHSL((_hsl.h + jitter + 1) % 1, Math.min(sat, 0.95), Math.min(lit, 0.8));
    return;
  }
  const sat = 0.35 + auth * 0.35 + Math.min(cc / 20, 0.2);
  const lit = 0.45 + auth * 0.2;
  out.setHSL(hashHue(node.label || String(node.id)), Math.min(sat, 0.9), Math.min(lit, 0.75));
}

function defaultSize(node: GraphNode, _index: number, ctx: EncodingContext): number {
  return computeNodeScale(
    node,
    ctx.connectionCountMap as Map<string, number>,
    ctx.graphMode ?? 'knowledge_graph',
    ctx.hierarchyMap as Map<string, any> | undefined,
    ctx.graphTypeVisuals,
  ) * (ctx.nodeSize ?? 1.0);
}

export const DEFAULT_NODE_ENCODER: NodeEncoder = { color: defaultColor, size: defaultSize };

/** `visualisation.graphView.explore.colorBy` sentinel: today's look under `nodes.colorScheme`. */
export const COLOR_BY_SCHEME = 'scheme';

/** True when `colorBy` selects the default encoder rather than a node-table column. */
export function isSchemeColorBy(colorBy: string | null | undefined): boolean {
  return colorBy == null || colorBy === '' || colorBy === COLOR_BY_SCHEME;
}

// --- Buffers -----------------------------------------------------------------

export class NodeEncoding {
  private _colors = new Float32Array(0);
  private _sizes = new Float32Array(0);
  private _version = 0;
  private _encoderVersion = 0;
  private encoder: NodeEncoder = DEFAULT_NODE_ENCODER;
  private readonly listeners = new Set<NodeEncodingListener>();
  private resolver: NodeIndexResolver;
  private readonly tmp = new THREE.Color();

  constructor(resolver: NodeIndexResolver = sharedNodeIndexResolver) {
    this.resolver = resolver;
  }

  /** Linear RGB, stride 3, by render index. Replaced on resize: re-read after a version change. */
  get colors(): Float32Array { return this._colors; }
  /** Size scale, stride 1, by render index. */
  get sizes(): Float32Array { return this._sizes; }
  get version(): number { return this._version; }

  subscribe(listener: NodeEncodingListener): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }

  setIndexResolver(resolver: NodeIndexResolver): void { this.resolver = resolver; }

  /**
   * Replace the active encoder. Either half may be given; the other half stays the
   * default. Pass `null` to restore the default. Bumps `version` and notifies the
   * listeners, so the one encodeAll caller (the renderer's useNodeEncodingSync)
   * re-encodes at once; the buffers keep the previous encoding until it does.
   */
  setEncoder(encoder: Partial<NodeEncoder> | null): void {
    this.encoder = encoder
      ? { color: encoder.color ?? DEFAULT_NODE_ENCODER.color, size: encoder.size ?? DEFAULT_NODE_ENCODER.size }
      : DEFAULT_NODE_ENCODER;
    this._encoderVersion++;
    this._version++;
    for (const l of this.listeners) l(this._version);
  }

  /** Bumps on every setEncoder (not on encodeAll): "the encoder changed, re-encode". */
  get encoderVersion(): number { return this._encoderVersion; }

  getEncoder(): NodeEncoder { return this.encoder; }

  /**
   * Encode every node at its render index. Nodes without an index (withheld or not
   * yet mapped) are skipped. Slots with no node keep zero colour and zero size.
   */
  encodeAll(nodes: readonly GraphNode[], ctx: EncodingContext): void {
    const map = this.resolver();
    const n = slotCountFor(map);
    if (this._sizes.length !== n) {
      this._colors = new Float32Array(n * 3);
      this._sizes = new Float32Array(n);
    } else {
      this._colors.fill(0);
      this._sizes.fill(0);
    }
    const c = this.tmp;
    for (const node of nodes) {
      const idx = map.get(String(node.id));
      if (idx === undefined || idx >= n) continue;
      this.encoder.color(node, idx, ctx, c);
      this._colors[idx * 3] = c.r;
      this._colors[idx * 3 + 1] = c.g;
      this._colors[idx * 3 + 2] = c.b;
      this._sizes[idx] = this.encoder.size(node, idx, ctx);
    }
    this._version++;
    for (const l of this.listeners) l(this._version);
  }
}

/** The client's one encoding, indexed by the shared node render index. */
export const nodeEncoding = new NodeEncoding();
