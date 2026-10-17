/**
 * Client-side graph model: dense, index-addressed views of a GraphSnapshot so
 * the render loop never touches the JSON objects or allocates per node.
 */
import {
  kindName,
  relationName,
  tierIndex,
  type CodeEdge,
  type CodeNode,
  type GraphDelta,
  type GraphSnapshot,
  type Lang,
} from './protocol';

export interface GraphModel {
  snapshot: GraphSnapshot;
  n: number;
  /** Wire id per index. */
  ids: Uint32Array;
  /** Wire id → index. */
  index: Map<number, number>;
  tier: Uint8Array;
  /** Parent index per node, -1 when none or unknown. */
  parent: Int32Array;
  /** Snapshot-time community per index (the live value comes from frames). */
  community: Uint32Array;
  importance: Float32Array;
  /** 1 when the node looks like test code. */
  isTest: Uint8Array;
  /** Index of the L0 aggregate each node rolls up to (itself for L0), -1 when none. */
  region: Int32Array;
  /** Undirected CSR adjacency over every non-structural relation plus structural links. */
  adjStart: Uint32Array;
  adjList: Uint32Array;
  /** Directed call edges as index pairs. */
  calls: Uint32Array;
  /** File-level import edges as index pairs (symbol imports lifted to their files, deduplicated). */
  fileImports: Uint32Array;
  /** Lower-cased labels for search. */
  labelsLower: string[];
  langs: Lang[];
  communities: number[];
}

const TEST_RE = /(^|\/)(tests?|__tests__|spec|specs|testdata|fixtures)\/|[._-](test|spec)\.[a-z]+$|_test\.go$|^test_.*\.py$|\/test_[^/]*\.py$/i;

export function looksLikeTest(n: Pick<CodeNode, 'path' | 'label'>): boolean {
  if (n.path && TEST_RE.test(n.path)) return true;
  return /^test_|^tests?$|_tests?$/i.test(n.label);
}

export function buildModel(snapshot: GraphSnapshot): GraphModel {
  const nodes = snapshot.nodes;
  const n = nodes.length;
  const ids = new Uint32Array(n);
  const index = new Map<number, number>();
  const tier = new Uint8Array(n);
  const community = new Uint32Array(n);
  const importance = new Float32Array(n);
  const isTest = new Uint8Array(n);
  const labelsLower: string[] = new Array(n);
  const langSet = new Set<Lang>();
  const commSet = new Set<number>();
  for (let i = 0; i < n; i++) {
    const nd = nodes[i];
    ids[i] = nd.id;
    index.set(nd.id, i);
    tier[i] = tierIndex(nd.tier);
    community[i] = nd.community;
    importance[i] = nd.importance;
    isTest[i] = looksLikeTest(nd) ? 1 : 0;
    labelsLower[i] = nd.label.toLowerCase();
    if (nd.tier !== 'l0') langSet.add(nd.lang);
    if (nd.tier === 'l2' || nd.tier === 'l1') commSet.add(nd.community);
  }
  const parent = new Int32Array(n).fill(-1);
  for (let i = 0; i < n; i++) {
    const p = nodes[i].parent;
    if (p !== 0) parent[i] = index.get(p) ?? -1;
  }
  const region = new Int32Array(n).fill(-1);
  for (let i = 0; i < n; i++) {
    let j = i;
    for (let guard = 0; guard < 8 && j >= 0; guard++) {
      if (tier[j] === 0) {
        region[i] = j;
        break;
      }
      j = parent[j];
    }
  }

  // Adjacency (undirected) for k-hop focus, plus call and lifted file-import pairs.
  const degree = new Uint32Array(n);
  const calls: number[] = [];
  const importKeys = new Set<number>();
  const fileImports: number[] = [];
  const fileOf = (i: number): number => {
    let j = i;
    for (let g = 0; g < 8 && j >= 0; g++) {
      if (tier[j] === 1) return j;
      j = parent[j];
    }
    return -1;
  };
  const resolved: [number, number][] = [];
  for (const e of snapshot.edges) {
    const s = index.get(e.source);
    const t = index.get(e.target);
    if (s === undefined || t === undefined || s === t) continue;
    resolved.push([s, t]);
    degree[s]++;
    degree[t]++;
    const rel = relationName(e.relation);
    if (rel === 'calls') {
      calls.push(s, t);
    } else if (rel === 'imports' || rel === 'imports_from') {
      const fs = fileOf(s);
      const ft = fileOf(t);
      if (fs >= 0 && ft >= 0 && fs !== ft) {
        const key = fs * 0x100000 + ft; // n < 2^20 per side keeps this exact below 2^53
        if (!importKeys.has(key)) {
          importKeys.add(key);
          fileImports.push(fs, ft);
        }
      }
    }
  }
  const adjStart = new Uint32Array(n + 1);
  for (let i = 0; i < n; i++) adjStart[i + 1] = adjStart[i] + degree[i];
  const fill = adjStart.slice(0, n);
  const adjList = new Uint32Array(adjStart[n]);
  for (const [s, t] of resolved) {
    adjList[fill[s]++] = t;
    adjList[fill[t]++] = s;
  }

  return {
    snapshot,
    n,
    ids,
    index,
    tier,
    parent,
    community,
    importance,
    isTest,
    region,
    adjStart,
    adjList,
    calls: Uint32Array.from(calls),
    fileImports: Uint32Array.from(fileImports),
    labelsLower,
    langs: [...langSet].sort(),
    communities: [...commSet].sort((a, b) => a - b),
  };
}

/** Breadth-first k-hop neighbourhood of `seeds` (indices), capped at `cap` nodes. Returns index → hop. */
export function kHop(model: GraphModel, seeds: readonly number[], k: number, cap = 50_000): Map<number, number> {
  const hop = new Map<number, number>();
  let frontier: number[] = [];
  for (const s of seeds) {
    if (s >= 0 && s < model.n && !hop.has(s)) {
      hop.set(s, 0);
      frontier.push(s);
    }
  }
  for (let d = 1; d <= k && frontier.length && hop.size < cap; d++) {
    const next: number[] = [];
    for (const u of frontier) {
      for (let a = model.adjStart[u]; a < model.adjStart[u + 1]; a++) {
        const v = model.adjList[a];
        if (!hop.has(v)) {
          hop.set(v, d);
          next.push(v);
          if (hop.size >= cap) return hop;
        }
      }
    }
    frontier = next;
  }
  return hop;
}

const edgeKey = (e: CodeEdge) => `${e.source}>${e.target}>${relationName(e.relation)}`;

/** Apply a server delta to a snapshot (pure; mirrors `GraphDelta::apply`). */
export function applyDelta(s: GraphSnapshot, d: GraphDelta): GraphSnapshot {
  const removed = new Set(d.removed_nodes);
  const changed = new Map(d.changed_nodes.map((nd) => [nd.id, nd] as const));
  const nodes: CodeNode[] = [];
  const seen = new Set<number>();
  for (const nd of s.nodes) {
    if (removed.has(nd.id)) continue;
    nodes.push(changed.get(nd.id) ?? nd);
    seen.add(nd.id);
  }
  for (const nd of d.added_nodes) {
    if (!seen.has(nd.id)) {
      nodes.push(nd);
      seen.add(nd.id);
    }
  }
  const drop = new Set(d.removed_edges.map(edgeKey));
  const edges: CodeEdge[] = [];
  const have = new Set<string>();
  for (const e of s.edges) {
    const k = edgeKey(e);
    if (drop.has(k) || removed.has(e.source) || removed.has(e.target)) continue;
    edges.push(e);
    have.add(k);
  }
  for (const e of d.added_edges) {
    const k = edgeKey(e);
    if (!have.has(k) && seen.has(e.source) && seen.has(e.target)) {
      edges.push(e);
      have.add(k);
    }
  }
  return { ...s, revision: d.to_revision, generated_at_ms: d.generated_at_ms, nodes, edges };
}

/** Community hue in degrees: the estate's `(id * 83) % 360`. */
export function communityHue(c: number): number {
  return (c * 83) % 360;
}

/** HSL (h in degrees, s/l in 0..1) to linear-ish sRGB triple 0..1. */
export function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0), f(8), f(4)];
}

export function communityRgb(c: number): [number, number, number] {
  return hslToRgb(communityHue(c), 0.68, 0.56);
}

export function communityCss(c: number): string {
  return `hsl(${communityHue(c)} 68% 56%)`;
}

export function displayKind(n: CodeNode): string {
  return kindName(n.kind);
}
