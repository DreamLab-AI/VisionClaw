/**
 * Flat, transferable encoding of an HNSW graph, so the worker can hand the
 * built graph back without structured-cloning ~150k small arrays.
 *
 * Layout of `links`: for each node in id order, for each layer 0..level,
 * the neighbour count followed by the neighbour ids.
 */
import type { HnswGraph, HnswParams } from './types';

export interface PackedGraph {
  ep: number;
  maxL: number;
  params: HnswParams;
  levels: Uint8Array;
  links: Int32Array;
}

export function packGraph(g: HnswGraph): PackedGraph {
  let size = 0;
  for (let i = 0; i < g.links.length; i++) {
    for (const layer of g.links[i]) size += 1 + layer.length;
  }
  const links = new Int32Array(size);
  let o = 0;
  for (let i = 0; i < g.links.length; i++) {
    for (const layer of g.links[i]) {
      links[o++] = layer.length;
      for (const n of layer) links[o++] = n;
    }
  }
  return { ep: g.ep, maxL: g.maxL, params: { ...g.params }, levels: g.levels, links };
}

/** Decodes a packed graph; throws when the payload is truncated or inconsistent. */
export function unpackGraph(p: PackedGraph): HnswGraph {
  const count = p.levels.length;
  const links: number[][][] = new Array(count);
  const src = p.links;
  let o = 0;
  for (let i = 0; i < count; i++) {
    const lv = p.levels[i];
    const node: number[][] = new Array(lv + 1);
    for (let l = 0; l <= lv; l++) {
      if (o >= src.length) throw new RangeError('packed HNSW graph is truncated');
      const len = src[o++];
      if (len < 0 || o + len > src.length) throw new RangeError('packed HNSW graph is truncated');
      const layer = new Array<number>(len);
      for (let k = 0; k < len; k++) {
        const n = src[o++];
        if (n < 0 || n >= count) throw new RangeError(`packed HNSW graph links to unknown node ${n}`);
        layer[k] = n;
      }
      node[l] = layer;
    }
    links[i] = node;
  }
  if (o !== src.length) throw new RangeError('packed HNSW graph has trailing data');
  if (count > 0 && (p.ep < 0 || p.ep >= count)) throw new RangeError(`packed HNSW graph has invalid entry point ${p.ep}`);
  return { ep: p.ep, maxL: p.maxL, levels: p.levels, links, params: { ...p.params } };
}
