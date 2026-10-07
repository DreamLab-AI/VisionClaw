/**
 * In-browser HNSW over the snapshot's vectors, with a step-by-step search
 * trace so the scene can draw the route a query takes through memory.
 *
 * Adapted from the RuVector Explorer (https://github.com/ruvnet/RuVector,
 * docs/explorer, MIT licence): `ins`, `searchLayer`, `select`, `buildGraph`,
 * `graphSearch` and `brute`. Changes from the Explorer: cosine distance on
 * normalised rows (`1 - dot`), typed-array heaps and visited marks instead of
 * spliced arrays and sets, a cached distance per stored link, and a builder
 * that can run one insertion at a time so a worker or the main thread can
 * slice the work.
 */
import type { HnswGraph, HnswParams, SearchResult, TraceEvent, VectorSet } from './types';
import { assertVectorSet, dotRow, dotRows, seededRandom } from './vec';

/** Highest layer a node may be assigned (the level array is a Uint8Array). */
export const MAX_LEVEL = 16;

// ---------------------------------------------------------------------------
// small typed-array structures

/** Visited marks reset in O(1) by bumping an epoch. */
class Visited {
  private marks: Uint32Array;
  private epoch = 0;
  constructor(size: number) {
    this.marks = new Uint32Array(Math.max(1, size));
  }
  reset(): void {
    this.epoch++;
    if (this.epoch === 0xffffffff) {
      this.marks.fill(0);
      this.epoch = 1;
    }
  }
  /** Marks `i`; returns true when it was already marked this epoch. */
  testAndSet(i: number): boolean {
    if (this.marks[i] === this.epoch) return true;
    this.marks[i] = this.epoch;
    return false;
  }
}

/** Binary min-heap of (distance, id). */
class MinHeap {
  d: Float64Array;
  n: Int32Array;
  size = 0;
  constructor(cap = 64) {
    this.d = new Float64Array(cap);
    this.n = new Int32Array(cap);
  }
  clear(): void {
    this.size = 0;
  }
  push(d: number, n: number): void {
    if (this.size === this.d.length) {
      const d2 = new Float64Array(this.d.length * 2);
      const n2 = new Int32Array(this.n.length * 2);
      d2.set(this.d);
      n2.set(this.n);
      this.d = d2;
      this.n = n2;
    }
    let i = this.size++;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (this.d[p] <= d) break;
      this.d[i] = this.d[p];
      this.n[i] = this.n[p];
      i = p;
    }
    this.d[i] = d;
    this.n[i] = n;
  }
  /** Removes the minimum; read it from `topD` / `topN` before calling. */
  pop(): void {
    const last = --this.size;
    if (last <= 0) return;
    const d = this.d[last];
    const n = this.n[last];
    let i = 0;
    for (;;) {
      let c = 2 * i + 1;
      if (c >= last) break;
      if (c + 1 < last && this.d[c + 1] < this.d[c]) c++;
      if (this.d[c] >= d) break;
      this.d[i] = this.d[c];
      this.n[i] = this.n[c];
      i = c;
    }
    this.d[i] = d;
    this.n[i] = n;
  }
}

/** Ascending, bounded list of (distance, id); a new entry goes before equal ones, as in the Explorer's `ins`. */
class SortedBeam {
  d: Float64Array;
  n: Int32Array;
  len = 0;
  constructor(public cap: number) {
    this.d = new Float64Array(cap + 1);
    this.n = new Int32Array(cap + 1);
  }
  reset(cap: number): void {
    if (cap + 1 > this.d.length) {
      this.d = new Float64Array(cap + 1);
      this.n = new Int32Array(cap + 1);
    }
    this.cap = cap;
    this.len = 0;
  }
  worst(): number {
    return this.d[this.len - 1];
  }
  insert(d: number, n: number): void {
    let lo = 0;
    let hi = this.len;
    while (lo < hi) {
      const m = (lo + hi) >> 1;
      if (this.d[m] < d) lo = m + 1;
      else hi = m;
    }
    this.d.copyWithin(lo + 1, lo, this.len);
    this.n.copyWithin(lo + 1, lo, this.len);
    this.d[lo] = d;
    this.n[lo] = n;
    if (this.len < this.cap) this.len++;
  }
}

// ---------------------------------------------------------------------------
// layer search shared by build and query

interface LayerScratch {
  visited: Visited;
  cand: MinHeap;
  res: SortedBeam;
}

function makeScratch(count: number): LayerScratch {
  return { visited: new Visited(count), cand: new MinHeap(), res: new SortedBeam(64) };
}

/**
 * Best-first search of one layer, as the Explorer's `searchLayer`: stop when
 * the nearest open candidate is farther than the worst of a full beam. The
 * result is left in `s.res` (ascending).
 */
function searchLayer(
  dist: (i: number) => number,
  eps: readonly number[],
  epsDist: readonly number[] | null,
  ef: number,
  layer: number,
  links: number[][][],
  s: LayerScratch,
  trace: TraceEvent[] | null,
): void {
  const { visited, cand, res } = s;
  visited.reset();
  cand.clear();
  res.reset(ef);
  for (let i = 0; i < eps.length; i++) {
    const e = eps[i];
    if (visited.testAndSet(e)) continue;
    const d = epsDist ? epsDist[i] : dist(e);
    cand.push(d, e);
    res.insert(d, e);
  }
  while (cand.size > 0) {
    const dc = cand.d[0];
    const c = cand.n[0];
    cand.pop();
    if (res.len >= ef && dc > res.worst()) break;
    if (trace) trace.push({ t: 'pop', n: c, l: layer, d: dc });
    const nb = links[c][layer];
    for (let j = 0; j < nb.length; j++) {
      const e = nb[j];
      if (visited.testAndSet(e)) continue;
      const de = dist(e);
      const ok = res.len < ef || de < res.worst();
      if (trace) trace.push({ t: 'eval', from: c, n: e, l: layer, d: de, ok });
      if (ok) {
        cand.push(de, e);
        res.insert(de, e);
      }
    }
  }
}

// ---------------------------------------------------------------------------
// build

/** Incremental HNSW builder; insert rows 0..count-1 in order, then `finish()`. */
export class HnswBuilder {
  private readonly vs: VectorSet;
  private readonly params: HnswParams;
  private readonly links: number[][][] = [];
  /** cached `1 - dot` distance of each stored link, parallel to `links` */
  private readonly linkDist: number[][][] = [];
  /** per (node, layer) cache of pairwise distances between list members, indexed by list position */
  private readonly pairCache: (Float32Array | null)[][] = [];
  private readonly orderScratch: number[] = [];
  private readonly sortedDs: number[] = [];
  private readonly chosenScratch: number[] = [];
  private readonly levels: Uint8Array;
  private readonly rand: () => number;
  private readonly mL: number;
  private readonly scratch: LayerScratch;
  private ep = -1;
  private maxL = -1;
  private next = 0;

  constructor(vs: VectorSet, params: HnswParams) {
    assertVectorSet(vs);
    if (!Number.isInteger(params.M) || params.M < 2) throw new RangeError(`HNSW M must be an integer >= 2, got ${params.M}`);
    if (!Number.isInteger(params.efConstruction) || params.efConstruction < 1) {
      throw new RangeError(`HNSW efConstruction must be a positive integer, got ${params.efConstruction}`);
    }
    this.vs = vs;
    this.params = { M: params.M, efConstruction: params.efConstruction, seed: params.seed };
    this.levels = new Uint8Array(vs.count);
    this.rand = seededRandom((params.seed + 77 + params.M) | 0);
    this.mL = 1 / Math.log(params.M);
    this.scratch = makeScratch(vs.count);
  }

  get inserted(): number {
    return this.next;
  }

  get total(): number {
    return this.vs.count;
  }

  /** Inserts the next row; returns false once every row is in. */
  step(): boolean {
    if (this.next >= this.vs.count) return false;
    this.insert(this.next++);
    return true;
  }

  private distRows(a: number, b: number): number {
    return 1 - dotRows(this.vs.data, a, b, this.vs.dim);
  }

  /**
   * The Explorer's `select` heuristic over an ascending candidate list: keep
   * a candidate only if it is nearer the base than to every one already
   * kept, then fill up to `M` with the nearest of the rest. `pair(a, b)` is
   * the distance between candidates at positions `a` and `b`; the chosen
   * positions are written to `out`.
   */
  private select(ds: ArrayLike<number>, len: number, M: number, pair: (a: number, b: number) => number, out: number[]): void {
    out.length = 0;
    for (let i = 0; i < len && out.length < M; i++) {
      const d = ds[i];
      let good = true;
      for (let r = 0; r < out.length; r++) {
        if (pair(i, out[r]) < d) {
          good = false;
          break;
        }
      }
      if (good) out.push(i);
    }
    if (out.length < M) {
      const taken = new Uint8Array(len);
      for (const o of out) taken[o] = 1;
      for (let i = 0; i < len && out.length < M; i++) if (!taken[i]) out.push(i);
    }
  }

  /**
   * Appends `id` to node `n`'s list on `layer` and re-prunes the list with
   * the heuristic when it overflows. Pairwise distances between list members
   * are cached per (node, layer) and carried across prunes, which removes
   * most of the repeated dot products without changing the result.
   */
  private link(n: number, layer: number, id: number, d: number, Mm: number): void {
    const L = this.links[n][layer];
    const LD = this.linkDist[n][layer];
    const cap = Mm + 1;
    let pd = this.pairCache[n][layer];
    const pos = L.length;
    L.push(id);
    LD.push(d);
    if (pd) {
      for (let k = 0; k < cap; k++) {
        pd[pos * cap + k] = NaN;
        pd[k * cap + pos] = NaN;
      }
    }
    if (L.length <= Mm) return;
    if (!pd) {
      pd = new Float32Array(cap * cap).fill(NaN);
      this.pairCache[n][layer] = pd;
    }
    const len = L.length;
    const order = this.orderScratch;
    order.length = len;
    for (let k = 0; k < len; k++) order[k] = k;
    order.sort((a, b) => LD[a] - LD[b] || a - b);
    const sDs = this.sortedDs;
    sDs.length = len;
    for (let k = 0; k < len; k++) sDs[k] = LD[order[k]];
    const cache = pd;
    const pair = (a: number, b: number) => {
      const ia = order[a];
      const ib = order[b];
      let v = cache[ia * cap + ib];
      if (v !== v) {
        v = this.distRows(L[ia], L[ib]);
        cache[ia * cap + ib] = v;
        cache[ib * cap + ia] = v;
      }
      return v;
    };
    const chosen = this.chosenScratch;
    this.select(sDs, len, Mm, pair, chosen);
    const idx = chosen.map((c) => order[c]);
    const next = new Float32Array(cap * cap).fill(NaN);
    for (let x = 0; x < idx.length; x++) {
      for (let y = 0; y < idx.length; y++) next[x * cap + y] = cache[idx[x] * cap + idx[y]];
    }
    this.pairCache[n][layer] = next;
    this.links[n][layer] = idx.map((k) => L[k]);
    this.linkDist[n][layer] = idx.map((k) => LD[k]);
  }

  private insert(i: number): void {
    const { M, efConstruction } = this.params;
    const lv = Math.min(MAX_LEVEL, Math.floor(-Math.log(1 - this.rand()) * this.mL));
    this.levels[i] = lv;
    this.links.push(Array.from({ length: lv + 1 }, () => []));
    this.linkDist.push(Array.from({ length: lv + 1 }, () => []));
    this.pairCache.push(new Array<Float32Array | null>(lv + 1).fill(null));
    if (this.ep < 0) {
      this.ep = i;
      this.maxL = lv;
      return;
    }
    const { data, dim } = this.vs;
    const q = data.subarray(i * dim, (i + 1) * dim);
    const dist = (j: number) => 1 - dotRow(q, data, j, dim);
    const s = this.scratch;
    let c: number[] = [this.ep];
    let cd: number[] | null = null;
    for (let l = this.maxL; l > lv; l--) {
      searchLayer(dist, c, cd, 1, l, this.links, s, null);
      c = [s.res.n[0]];
      cd = [s.res.d[0]];
    }
    const chosen: number[] = [];
    for (let l = Math.min(lv, this.maxL); l >= 0; l--) {
      searchLayer(dist, c, cd, efConstruction, l, this.links, s, null);
      const len = s.res.len;
      const wIds = Array.from(s.res.n.subarray(0, len));
      const wDs = Array.from(s.res.d.subarray(0, len));
      const Mm = l ? M : 2 * M;
      this.select(wDs, len, M, (a, b) => this.distRows(wIds[a], wIds[b]), chosen);
      this.links[i][l] = chosen.map((k) => wIds[k]);
      this.linkDist[i][l] = chosen.map((k) => wDs[k]);
      for (const k of chosen) this.link(wIds[k], l, i, wDs[k], Mm);
      c = wIds;
      cd = wDs;
    }
    if (lv > this.maxL) {
      this.ep = i;
      this.maxL = lv;
    }
  }

  finish(): HnswGraph {
    if (this.next < this.vs.count) throw new Error(`HNSW build incomplete: ${this.next} of ${this.vs.count} rows inserted`);
    this.pairCache.length = 0;
    return {
      ep: this.ep,
      maxL: Math.max(0, this.maxL),
      levels: this.levels,
      links: this.links,
      params: { ...this.params },
    };
  }
}

export interface BuildOptions {
  onProgress?(done: number, total: number): void;
  signal?: AbortSignal;
  /** yield after this many insertions; by default the build yields every ~12 ms */
  yieldEvery?: number;
}

const SLICE_MS = 12;

function abortError(): Error {
  if (typeof DOMException === 'function') return new DOMException('HNSW build aborted', 'AbortError');
  const e = new Error('HNSW build aborted');
  e.name = 'AbortError';
  return e;
}

const yieldTask = () => new Promise<void>((r) => setTimeout(r, 0));
const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());

/**
 * Builds the graph with seeded levels, yielding to the event loop between
 * slices. Rejects on invalid input or when `signal` aborts.
 */
export async function buildHnsw(vs: VectorSet, params: HnswParams, opts: BuildOptions = {}): Promise<HnswGraph> {
  const b = new HnswBuilder(vs, params);
  const { signal, onProgress } = opts;
  const every = opts.yieldEvery && opts.yieldEvery > 0 ? Math.floor(opts.yieldEvery) : 0;
  if (signal?.aborted) throw abortError();
  let sliceStart = now();
  let lastReported = -1;
  while (b.step()) {
    const done = b.inserted;
    const due = every ? done % every === 0 : (done & 15) === 0 && now() - sliceStart > SLICE_MS;
    if (due && done < b.total) {
      onProgress?.(done, b.total);
      lastReported = done;
      await yieldTask();
      if (signal?.aborted) throw abortError();
      sliceStart = now();
    }
  }
  if (lastReported !== b.total) onProgress?.(b.total, b.total);
  return b.finish();
}

// ---------------------------------------------------------------------------
// query

export interface SearchOptions {
  trace?: boolean;
  /** node ids from the learning memory, injected as extra layer-0 entry points */
  hints?: number[];
}

/**
 * HNSW search, as the Explorer's `graphSearch`: greedy descent on the upper
 * layers, learned hints added to the layer-0 entry set, then a beam of
 * `max(ef, k)` on layer 0.
 */
export function searchHnsw(g: HnswGraph, vs: VectorSet, q: Float32Array, ef: number, k: number, opts: SearchOptions = {}): SearchResult {
  const trace: TraceEvent[] = [];
  if (g.ep < 0 || vs.count === 0 || k <= 0) return { top: [], beam: [], hops: [], distanceEvals: 0, trace };
  if (q.length !== vs.dim) throw new RangeError(`query dimension ${q.length} does not match ${vs.dim}`);
  const tr = opts.trace ? trace : null;
  const { data, dim } = vs;
  let evals = 0;
  const dist = (i: number) => {
    evals++;
    return 1 - dotRow(q, data, i, dim);
  };
  const s = makeScratch(vs.count);
  let c: number[] = [g.ep];
  const hops: number[] = [];
  for (let l = g.maxL; l > 0; l--) {
    searchLayer(dist, c, null, 1, l, g.links, s, tr);
    c = [s.res.n[0]];
    hops.push(c[0]);
  }
  if (opts.hints) {
    for (const h of opts.hints) {
      if (!Number.isInteger(h) || h < 0 || h >= vs.count || c.includes(h)) continue;
      const d = dist(h);
      if (tr) tr.push({ t: 'eval', from: g.ep, n: h, l: 0, d, ok: true, learned: true });
      c.push(h);
    }
  }
  const beamSize = Math.max(Math.floor(ef), k);
  searchLayer(dist, c, null, beamSize, 0, g.links, s, tr);
  const beam = Array.from(s.res.n.subarray(0, s.res.len));
  return { top: beam.slice(0, k), beam, hops, distanceEvals: evals, trace };
}

/** Exact top-k by a bounded insertion scan (the Explorer's `brute`), nearest first, ties by id. */
export function exactTopK(vs: VectorSet, q: Float32Array, k: number): number[] {
  const K = Math.min(Math.floor(k), vs.count);
  if (K <= 0) return [];
  if (q.length !== vs.dim) throw new RangeError(`query dimension ${q.length} does not match ${vs.dim}`);
  const kd = new Float64Array(K);
  const ki = new Int32Array(K);
  let cnt = 0;
  let worst = Infinity;
  const { data, dim, count } = vs;
  for (let i = 0; i < count; i++) {
    const s = 1 - dotRow(q, data, i, dim);
    if (cnt < K || s < worst) {
      let j = cnt < K ? cnt++ : K - 1;
      while (j > 0 && kd[j - 1] > s) {
        kd[j] = kd[j - 1];
        ki[j] = ki[j - 1];
        j--;
      }
      kd[j] = s;
      ki[j] = i;
      worst = kd[cnt - 1];
    }
  }
  return Array.from(ki.subarray(0, cnt));
}

/**
 * |approx[:k] ∩ exact[:k]| divided by the attainable count `min(k, |exact|)`.
 * Duplicates count once; with nothing attainable the recall is vacuously 1.
 */
export function recallAtK(approx: number[], exact: number[], k: number): number {
  const denom = Math.min(Math.floor(k), exact.length);
  if (denom <= 0) return 1;
  const truth = new Set(exact.slice(0, denom));
  const found = new Set<number>();
  for (const a of approx.slice(0, Math.floor(k))) if (truth.has(a)) found.add(a);
  return found.size / denom;
}
