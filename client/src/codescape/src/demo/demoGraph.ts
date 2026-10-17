/**
 * `?demo=1`: a synthetic repository (~5k nodes, communities, calls, imports)
 * plus a feed that drives the REAL client paths — V5 frames encoded with the
 * wire encoder and pushed through the FramePipeline, ServerEvents through the
 * store — so rendering can be verified without codescape-server.
 */
import { kHop, type GraphModel } from '../graph';
import {
  encodeV5Frame,
  type CodeEdge,
  type CodeNode,
  type GraphSnapshot,
  type Lang,
  type NodeKindName,
  type PresenceKind,
  type QueryResult,
  type ServerEvent,
  type WireRecord,
} from '../protocol';

/** Deterministic PRNG (mulberry32). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const DIRS: [string, Lang, string][] = [
  ['crates', 'rust', 'rs'],
  ['src', 'rust', 'rs'],
  ['client', 'type_script', 'tsx'],
  ['xr-client', 'gd_script', 'gd'],
  ['scripts', 'shell', 'sh'],
  ['gpu', 'cuda', 'cu'],
  ['services', 'python', 'py'],
  ['tools', 'go', 'go'],
  ['nix', 'nix', 'nix'],
  ['tests', 'rust', 'rs'],
  ['bench', 'cpp', 'cpp'],
  ['web', 'java_script', 'js'],
];
const STEMS = ['graph', 'layout', 'wire', 'socket', 'store', 'ingest', 'parser', 'render', 'camera', 'query', 'actor', 'voice', 'floor', 'timeline', 'delta', 'index', 'search', 'physics', 'shader', 'atlas', 'label', 'hull', 'picker', 'presence', 'config', 'auth', 'cache', 'codec', 'frame', 'tier'];
const VERBS = ['build', 'apply', 'decode', 'encode', 'load', 'resolve', 'select', 'update', 'compute', 'render', 'handle', 'spawn', 'merge', 'flush', 'parse', 'emit', 'step', 'project', 'rank', 'track'];
const SYMBOL_KINDS: NodeKindName[] = ['function', 'function', 'function', 'method', 'method', 'struct', 'enum', 'interface', 'constant', 'type_alias'];

export interface DemoWorld {
  snapshot: GraphSnapshot;
  /** Seed position per node index (hierarchical shell layout). */
  base: Float32Array;
  phase: Float32Array;
  radius: number;
}

export function buildDemoWorld(targetNodes = 5000, seed = 7): DemoWorld {
  const r = rng(seed);
  const nodes: CodeNode[] = [];
  const edges: CodeEdge[] = [];
  let nextId = 1;
  const COMMS = 18;
  const dirIds: number[] = [];
  for (const [name] of DIRS) {
    const id = nextId++;
    dirIds.push(id);
    nodes.push({ id, key: `\u0001dir:${name}`, kind: 'directory', lang: 'other', label: name, path: name, span: null, parent: 0, community: 0, cluster: dirIds.length - 1, importance: 0.6 + r() * 0.4, tier: 'l0', size: 0 });
  }
  const files: CodeNode[] = [];
  const fileCount = Math.max(DIRS.length, Math.round(targetNodes / 12));
  for (let f = 0; f < fileCount; f++) {
    const d = f < DIRS.length ? f : Math.floor(Math.pow(r(), 1.4) * DIRS.length);
    const [dir, lang, ext] = DIRS[d];
    const stem = STEMS[Math.floor(r() * STEMS.length)];
    const sub = r() < 0.5 ? `${STEMS[Math.floor(r() * STEMS.length)]}/` : '';
    const test = dir === 'tests' || r() < 0.06;
    const fname = test ? `${stem}_test.${ext}` : `${stem}${f}.${ext}`;
    const path = `${dir}/${sub}${fname}`;
    const homeComm = (d * 3 + Math.floor(r() * 2)) % COMMS;
    const comm = r() < 0.18 ? Math.floor(r() * COMMS) : homeComm;
    const id = nextId++;
    const node: CodeNode = { id, key: path, kind: 'file', lang, label: fname, path, span: null, parent: dirIds[d], community: comm, cluster: d, importance: Math.pow(r(), 2.2), tier: 'l1', size: 0 };
    nodes.push(node);
    files.push(node);
    edges.push({ source: dirIds[d], target: id, relation: 'contains', weight: 1, confidence: 'extracted' });
  }
  const symbols: CodeNode[] = [];
  let fi = 0;
  while (nodes.length < targetNodes) {
    const file = files[fi++ % files.length];
    const kind = SYMBOL_KINDS[Math.floor(r() * SYMBOL_KINDS.length)];
    const name =
      kind === 'struct' || kind === 'enum' || kind === 'interface' || kind === 'type_alias'
        ? `${STEMS[Math.floor(r() * STEMS.length)].replace(/^./, (c) => c.toUpperCase())}${['State', 'Config', 'Frame', 'Event', 'Engine', 'Kind'][Math.floor(r() * 6)]}`
        : kind === 'constant'
          ? `${STEMS[Math.floor(r() * STEMS.length)].toUpperCase()}_MAX`
          : `${VERBS[Math.floor(r() * VERBS.length)]}_${STEMS[Math.floor(r() * STEMS.length)]}`;
    const id = nextId++;
    const start = 1 + Math.floor(r() * 800);
    const node: CodeNode = {
      id,
      key: `${file.path}::${name}#${id}`,
      kind,
      lang: file.lang,
      label: name,
      path: file.path,
      span: { start_line: start, end_line: start + 3 + Math.floor(r() * 60) },
      parent: file.id,
      community: r() < 0.12 ? Math.floor(r() * COMMS) : file.community,
      cluster: file.cluster,
      importance: Math.pow(r(), 3),
      tier: 'l2',
      size: 0,
    };
    nodes.push(node);
    symbols.push(node);
    edges.push({ source: file.id, target: id, relation: kind === 'method' ? 'method' : 'defines', weight: 1, confidence: 'extracted' });
  }
  // Calls: mostly within a community.
  const byComm = new Map<number, CodeNode[]>();
  for (const s of symbols) {
    let l = byComm.get(s.community);
    if (!l) byComm.set(s.community, (l = []));
    l.push(s);
  }
  for (const s of symbols) {
    const calls = Math.floor(r() * 3.2);
    for (let c = 0; c < calls; c++) {
      const pool = r() < 0.78 ? byComm.get(s.community)! : symbols;
      const t = pool[Math.floor(r() * pool.length)];
      if (t.id !== s.id) edges.push({ source: s.id, target: t.id, relation: 'calls', weight: 1, confidence: r() < 0.85 ? 'extracted' : 'inferred' });
    }
  }
  // File imports.
  for (const f of files) {
    const k = Math.floor(r() * 3);
    for (let c = 0; c < k; c++) {
      const t = files[Math.floor(r() * files.length)];
      if (t.id !== f.id) edges.push({ source: f.id, target: t.id, relation: 'imports', weight: 1, confidence: 'extracted' });
    }
  }
  // Aggregate sizes, community of aggregates = most common member community.
  const idx = new Map(nodes.map((n, i) => [n.id, i] as const));
  for (const s of symbols) nodes[idx.get(s.parent)!].size++;
  for (const f of files) nodes[idx.get(f.parent)!].size += 1 + f.size;
  for (let d = 0; d < dirIds.length; d++) {
    const counts = new Map<number, number>();
    for (const f of files) if (f.parent === dirIds[d]) counts.set(f.community, (counts.get(f.community) ?? 0) + 1 + f.size);
    let best = 0;
    let bestN = -1;
    counts.forEach((v, k) => {
      if (v > bestN) {
        bestN = v;
        best = k;
      }
    });
    nodes[d].community = best;
  }

  // Hierarchical seed positions.
  const radius = 420;
  const base = new Float32Array(nodes.length * 3);
  const phase = new Float32Array(nodes.length);
  const golden = Math.PI * (3 - Math.sqrt(5));
  for (let d = 0; d < dirIds.length; d++) {
    const y = 1 - (2 * (d + 0.5)) / dirIds.length;
    const rr = Math.sqrt(1 - y * y);
    const th = golden * d;
    base[d * 3] = Math.cos(th) * rr * radius;
    base[d * 3 + 1] = y * radius;
    base[d * 3 + 2] = Math.sin(th) * rr * radius;
  }
  const scatter = (i: number, p: number, rad: number) => {
    const u = r() * 2 - 1;
    const th = r() * Math.PI * 2;
    const s = Math.sqrt(1 - u * u);
    const k = rad * Math.cbrt(0.25 + 0.75 * r());
    base[i * 3] = base[p * 3] + Math.cos(th) * s * k;
    base[i * 3 + 1] = base[p * 3 + 1] + u * k;
    base[i * 3 + 2] = base[p * 3 + 2] + Math.sin(th) * s * k;
  };
  for (let i = dirIds.length; i < nodes.length; i++) {
    const n = nodes[i];
    const p = idx.get(n.parent)!;
    scatter(i, p, n.tier === 'l1' ? 120 : 22);
    phase[i] = r() * Math.PI * 2;
  }
  const snapshot: GraphSnapshot = { repo: 'demo', epoch: 1, revision: 1, generated_at_ms: Date.now(), nodes, edges };
  return { snapshot, base, phase, radius };
}

const ACTORS = [
  { id: 0x3ff_0001, label: 'claude:tab0', kind: 'agent' },
  { id: 0x3ff_0002, label: 'codex:tab2', kind: 'agent' },
  { id: 0x3ff_0003, label: 'john', kind: 'human' },
];

/** Drives the demo: frames at `hz`, presence every ~1.4 s, captions now and then. */
export class DemoFeed {
  private seq = 1;
  private t0 = performance.now();
  private timers: ReturnType<typeof setInterval>[] = [];
  private heat: Float32Array;
  private walkers: number[];
  private readonly r = rng(99);
  private lastHeat = performance.now();

  constructor(
    private readonly world: DemoWorld,
    private readonly model: () => GraphModel | null,
    private readonly pushFrame: (frame: ArrayBuffer) => void,
    private readonly emit: (e: ServerEvent) => void,
    private readonly hz = 10,
  ) {
    this.heat = new Float32Array(world.snapshot.nodes.length);
    this.walkers = ACTORS.map(() => world.snapshot.nodes.length - 1 - Math.floor(this.r() * 2000));
  }

  start(): void {
    const s = this.world.snapshot;
    this.emit({ type: 'hello', repo: s.repo, epoch: s.epoch, revision: s.revision, nodes: s.nodes.length, edges: s.edges.length, engine: 'demo', version: 'demo' });
    this.timers.push(setInterval(() => this.frame(), 1000 / this.hz));
    this.timers.push(setInterval(() => this.presence(), 1400));
    this.timers.push(setTimeout(() => this.emit({ type: 'settled', revision: s.revision }), 12_000) as unknown as ReturnType<typeof setInterval>);
  }

  stop(): void {
    this.timers.forEach((t) => clearInterval(t));
    this.timers = [];
  }

  private frame(): void {
    const { snapshot, base, phase } = this.world;
    const t = (performance.now() - this.t0) / 1000;
    const settle = Math.exp(-t / 10);
    const now = performance.now();
    const decay = Math.pow(0.5, (now - this.lastHeat) / 60_000);
    this.lastHeat = now;
    const recs: WireRecord[] = new Array(snapshot.nodes.length);
    for (let i = 0; i < snapshot.nodes.length; i++) {
      const n = snapshot.nodes[i];
      this.heat[i] *= decay;
      const amp = (n.tier === 'l2' ? 14 : n.tier === 'l1' ? 30 : 0) * settle + (n.tier === 'l2' ? 0.6 : 0.3);
      const ph = phase[i];
      recs[i] = {
        id: n.id,
        position: [base[i * 3] + Math.sin(t * 0.9 + ph) * amp, base[i * 3 + 1] + Math.cos(t * 0.7 + ph * 1.3) * amp, base[i * 3 + 2] + Math.sin(t * 0.5 + ph * 0.7) * amp],
        cluster: n.cluster,
        community: n.community,
        centrality: n.importance,
        anomaly: Math.min(1, this.heat[i]),
      };
    }
    this.pushFrame(encodeV5Frame(this.seq++, recs));
  }

  private presence(): void {
    const m = this.model();
    if (!m) return;
    const k = Math.floor(this.r() * ACTORS.length);
    const actor = ACTORS[k];
    let cur = this.walkers[k];
    if (cur >= m.n) cur = m.n - 1;
    const deg = m.adjStart[cur + 1] - m.adjStart[cur];
    const next = deg > 0 && this.r() < 0.85 ? m.adjList[m.adjStart[cur] + Math.floor(this.r() * deg)] : Math.floor(this.r() * m.n);
    this.walkers[k] = next;
    const node = m.snapshot.nodes[next];
    const kinds: PresenceKind[] = ['query', 'query', 'update', 'focus'];
    const kind = actor.kind === 'human' ? 'focus' : kinds[Math.floor(this.r() * kinds.length)];
    const touched = [node.id];
    if (node.parent && m.index.has(node.parent)) touched.unshift(node.parent);
    for (const id of touched) {
      const i = m.index.get(id)!;
      this.heat[i] = Math.min(1, this.heat[i] + (kind === 'update' ? 0.9 : 0.45));
    }
    const ts = Date.now();
    this.emit({ type: 'presence', actor: actor.id, actor_label: actor.label, actor_kind: actor.kind, kind, node_ids: touched, paths: node.path ? [node.path] : [], text: kind === 'update' ? 'Edit' : kind === 'query' ? 'Read' : null, ts_ms: ts });
    if (this.r() < 0.18) {
      const text = actor.kind === 'human' ? `what calls ${node.label}?` : `I'm ${kind === 'update' ? 'editing' : 'reading'} ${node.label} in ${node.path ?? 'the repo'}`;
      const words = text.split(' ');
      words.forEach((_, w) =>
        setTimeout(() => this.emit({ type: 'caption', actor: actor.id, text: words.slice(0, w + 1).join(' '), partial: w < words.length - 1, ts_ms: Date.now() }), w * 160),
      );
    }
  }
}

function rankLabels(m: GraphModel, q: string, limit: number): number[] {
  const needle = q.toLowerCase().replace(/\s+/g, '_');
  const scored: [number, number][] = [];
  for (let i = 0; i < m.n; i++) {
    const l = m.labelsLower[i];
    if (l === needle) scored.push([i, 3 + m.importance[i]]);
    else if (l.startsWith(needle)) scored.push([i, 2 + m.importance[i]]);
    else if (l.includes(needle)) scored.push([i, 1 + m.importance[i]]);
  }
  scored.sort((a, b) => b[1] - a[1]);
  return scored.slice(0, limit).map(([i]) => i);
}

/** Local query engine for the demo (focus/find/callers/callees/affected/explain/path). */
export function demoQuery(m: GraphModel, text: string): QueryResult {
  const t = text.trim().toLowerCase();
  const res: QueryResult = { command: t, node_ids: [], edge_pairs: [], path: [], candidates: [], answer: '' };
  const pathM = /^path (?:from )?(.+?) to (.+)$/.exec(t);
  if (pathM) {
    const a = rankLabels(m, pathM[1], 1)[0];
    const b = rankLabels(m, pathM[2], 1)[0];
    if (a === undefined || b === undefined) return { ...res, answer: 'No match for one of the endpoints.' };
    const prev = new Int32Array(m.n).fill(-1);
    prev[a] = a;
    const q = [a];
    for (let h = 0; h < q.length && prev[b] < 0; h++) {
      const u = q[h];
      for (let e = m.adjStart[u]; e < m.adjStart[u + 1]; e++) {
        const v = m.adjList[e];
        if (prev[v] < 0) {
          prev[v] = u;
          q.push(v);
        }
      }
    }
    if (prev[b] < 0) return { ...res, answer: 'No path.' };
    const p: number[] = [];
    for (let v = b; v !== a; v = prev[v]) p.push(v);
    p.push(a);
    p.reverse();
    const ids = p.map((i) => m.ids[i]);
    return { ...res, path: ids, node_ids: ids, edge_pairs: ids.slice(1).map((id, k) => [ids[k], id] as [number, number]), answer: `${ids.length - 1} hops.` };
  }
  const verbM = /^(focus|find|show|go to|select|callers(?: of)?|callees(?: of)?|affected(?: by)?|explain)\s+(.+)$/.exec(t);
  if (!verbM) return { ...res, answer: 'Demo understands: focus/find X, callers X, callees X, affected X, explain X, path A to B.' };
  const verb = verbM[1].split(' ')[0];
  const hits = rankLabels(m, verbM[2], 8);
  if (!hits.length) return { ...res, answer: `Nothing called “${verbM[2]}”.` };
  const exact = hits.filter((i) => m.labelsLower[i] === verbM[2].replace(/\s+/g, '_'));
  if (hits.length > 1 && exact.length !== 1 && (verb === 'focus' || verb === 'find' || verb === 'show' || verb === 'go' || verb === 'select')) {
    return { ...res, candidates: hits.map((i) => [`${m.snapshot.nodes[i].label} — ${m.snapshot.nodes[i].path ?? ''}`, m.ids[i]] as [string, number]), answer: `${hits.length} matches; say “number N”.` };
  }
  const target = exact[0] ?? hits[0];
  const id = m.ids[target];
  if (verb === 'callers' || verb === 'callees') {
    const out: number[] = [];
    const pairs: [number, number][] = [];
    for (let k = 0; k < m.calls.length; k += 2) {
      const s = m.calls[k];
      const d = m.calls[k + 1];
      if (verb === 'callers' && d === target) {
        out.push(m.ids[s]);
        pairs.push([m.ids[s], id]);
      }
      if (verb === 'callees' && s === target) {
        out.push(m.ids[d]);
        pairs.push([id, m.ids[d]]);
      }
    }
    return { ...res, node_ids: [id, ...out], edge_pairs: pairs, answer: `${out.length} ${verb} of ${m.snapshot.nodes[target].label}.` };
  }
  if (verb === 'affected') {
    const hop = kHop(m, [target], 2, 2000);
    return { ...res, node_ids: [...hop.keys()].map((i) => m.ids[i]), answer: `${hop.size - 1} nodes within 2 hops.` };
  }
  const n = m.snapshot.nodes[target];
  const deg = m.adjStart[target + 1] - m.adjStart[target];
  if (verb === 'explain') {
    return { ...res, node_ids: [id], answer: `${n.label} is a ${typeof n.kind === 'string' ? n.kind : n.kind.other} in ${n.path ?? 'the repo'} (lines ${n.span?.start_line ?? '?'}–${n.span?.end_line ?? '?'}), community ${n.community}, ${deg} links.` };
  }
  return { ...res, node_ids: [id], answer: `${n.label} — ${n.path ?? ''}` };
}
