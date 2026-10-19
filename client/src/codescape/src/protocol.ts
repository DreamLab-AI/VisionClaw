/**
 * Wire mirror of `codescape/crates/codescape-model/src/{graph,delta,events,wire}.rs`.
 *
 * JSON enums are serde `tag = "type"`, snake_case. Newtype variants
 * (`Delta(GraphDelta)`, `Presence(PresenceEvent)`, …) flatten their payload's
 * fields next to `type`; `FloorState` is itself tagged with `state`.
 *
 * Binary position frames are the frozen 52-byte V3 record inside the V5
 * envelope `[0x05][u64 seq LE][records]`. The decoder here writes straight into
 * preallocated typed arrays (optionally SharedArrayBuffer-backed) indexed by
 * snapshot position, so a 100k-node frame allocates nothing per node.
 */

// ---------------------------------------------------------------- graph model

export type NodeKindName =
  | 'directory'
  | 'file'
  | 'module'
  | 'class'
  | 'struct'
  | 'enum'
  | 'interface'
  | 'function'
  | 'method'
  | 'variable'
  | 'constant'
  | 'type_alias'
  | 'document'
  | 'external';
/** `NodeKind::Other(s)` serialises externally tagged as `{ "other": s }`. */
export type NodeKind = NodeKindName | { other: string };

export type Lang =
  | 'rust'
  | 'type_script'
  | 'java_script'
  | 'python'
  | 'go'
  | 'gd_script'
  | 'cpp'
  | 'cuda'
  | 'shell'
  | 'nix'
  | 'markdown'
  | 'other';

export type Tier = 'l0' | 'l1' | 'l2';
export const TIERS: readonly Tier[] = ['l0', 'l1', 'l2'];
export function tierIndex(t: Tier): 0 | 1 | 2 {
  return t === 'l0' ? 0 : t === 'l1' ? 1 : 2;
}

export type RelationName =
  | 'contains'
  | 'method'
  | 'calls'
  | 'imports'
  | 'imports_from'
  | 'inherits'
  | 'implements'
  | 'references'
  | 'defines'
  | 'extends'
  | 'case_of';
export type Relation = RelationName | { other: string };

export type Confidence = 'extracted' | 'inferred' | 'ambiguous';

export interface Span {
  start_line: number;
  end_line: number;
}

export interface CodeNode {
  id: number;
  key: string;
  kind: NodeKind;
  lang: Lang;
  label: string;
  path: string | null;
  span: Span | null;
  /** Parent id (file for a symbol, directory aggregate for a file), 0 for none. */
  parent: number;
  community: number;
  cluster: number;
  importance: number;
  tier: Tier;
  size: number;
}

export interface CodeEdge {
  source: number;
  target: number;
  relation: Relation;
  weight: number;
  confidence: Confidence;
}

export interface GraphSnapshot {
  repo: string;
  epoch: number;
  revision: number;
  generated_at_ms: number;
  nodes: CodeNode[];
  edges: CodeEdge[];
}

export interface GraphDelta {
  from_revision: number;
  to_revision: number;
  generated_at_ms: number;
  changed_paths: string[];
  added_nodes: CodeNode[];
  changed_nodes: CodeNode[];
  removed_nodes: number[];
  added_edges: CodeEdge[];
  removed_edges: CodeEdge[];
}

export function kindName(k: NodeKind): string {
  return typeof k === 'string' ? k : k.other;
}
export function relationName(r: Relation): string {
  return typeof r === 'string' ? r : r.other;
}
export function isStructural(r: Relation): boolean {
  return r === 'contains' || r === 'method' || r === 'case_of' || r === 'defines';
}

// ---------------------------------------------------------------- events

export type PresenceKind = 'query' | 'update' | 'create' | 'delete' | 'say' | 'focus';

export interface PresenceEvent {
  actor: number;
  actor_label: string;
  actor_kind: string;
  kind: PresenceKind;
  node_ids: number[];
  paths: string[];
  text: string | null;
  ts_ms: number;
}

export interface CaptionEvent {
  actor: number;
  text: string;
  partial: boolean;
  ts_ms: number;
}

export type FloorState =
  | { state: 'free' }
  | { state: 'held'; holder: number; queue: number[] }
  | { state: 'busy' };

export interface QueryResult {
  command: string;
  node_ids: number[];
  edge_pairs: [number, number][];
  path: number[];
  candidates: [string, number][];
  answer: string;
}

export interface HelloEvent {
  type: 'hello';
  repo: string;
  epoch: number;
  revision: number;
  nodes: number;
  edges: number;
  engine: string;
  version: string;
}

export type ServerEvent =
  | HelloEvent
  | ({ type: 'delta' } & GraphDelta)
  | { type: 'settled'; revision: number }
  | ({ type: 'presence' } & PresenceEvent)
  | ({ type: 'caption' } & CaptionEvent)
  | ({ type: 'floor' } & FloorState)
  | ({ type: 'query' } & QueryResult)
  | { type: 'lag'; behind: number }
  | { type: 'progress'; phase: string; done: number; total: number | null }
  | { type: 'error'; message: string };

export type ServerEventType = ServerEvent['type'];

export type ClientEvent =
  | { type: 'hello'; name: string; kind: 'human' | 'agent' }
  | { type: 'command'; text: string }
  | { type: 'focus'; node_id: number }
  | { type: 'floor'; request: boolean }
  | { type: 'view'; tier: Tier; focus_files: number[] };

const SERVER_TYPES: ReadonlySet<string> = new Set([
  'hello',
  'delta',
  'settled',
  'presence',
  'caption',
  'floor',
  'query',
  'lag',
  'progress',
  'error',
]);

/** Parse one `/ws/events` text frame; `null` for anything that is not a known event. */
export function parseServerEvent(text: string): ServerEvent | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (!v || typeof v !== 'object') return null;
  const t = (v as { type?: unknown }).type;
  if (typeof t !== 'string' || !SERVER_TYPES.has(t)) return null;
  return v as ServerEvent;
}

/** Strip the `type` tag from a flattened newtype event. */
export function presenceOf(e: { type: 'presence' } & PresenceEvent): PresenceEvent {
  const { type: _t, ...rest } = e;
  return rest;
}
export function deltaOf(e: { type: 'delta' } & GraphDelta): GraphDelta {
  const { type: _t, ...rest } = e;
  return rest;
}
export function queryOf(e: { type: 'query' } & QueryResult): QueryResult {
  const { type: _t, ...rest } = e;
  return rest;
}
export function floorOf(e: { type: 'floor' } & FloorState): FloorState {
  const { type: _t, ...rest } = e;
  return rest as FloorState;
}
export function captionOf(e: { type: 'caption' } & CaptionEvent): CaptionEvent {
  const { type: _t, ...rest } = e;
  return rest;
}

// ---------------------------------------------------------------- binary wire

export const PROTOCOL_V3 = 0x03;
export const PROTOCOL_V5 = 0x05;
export const OPCODE_AGENT_ACTION = 0x23;
export const OPCODE_BROADCAST_ACK = 0x34;
export const NODE_RECORD_BYTES = 52;
export const V5_HEADER_BYTES = 9;
export const BROADCAST_ACK_BYTES = 21;
export const NODE_ID_MASK = 0x03ff_ffff;
export const AGENT_FLAG = 0x8000_0000;

/** Byte offsets inside one 52-byte record (ADR-2018 / ADR-031 D2). */
export const OFF = {
  id: 0,
  position: 4,
  velocity: 16,
  ssspDistance: 28,
  ssspParent: 32,
  cluster: 36,
  anomaly: 40,
  community: 44,
  centrality: 48,
} as const;

/** Per-node live attributes, indexed by snapshot position. */
export interface NodeBuffers {
  capacity: number;
  shared: boolean;
  /** xyz per node. */
  positions: Float32Array;
  /** Activity heat 0..1 (wire `anomaly`). */
  heat: Float32Array;
  community: Uint32Array;
  cluster: Uint32Array;
  centrality: Float32Array;
  sssp: Float32Array;
  ssspParent: Int32Array;
  /** `[0]` frame version (incremented per applied frame), `[1]` records applied in the last frame. */
  header: Int32Array;
}

function backing(bytes: number, shared: boolean): ArrayBuffer | SharedArrayBuffer {
  return shared ? new SharedArrayBuffer(bytes) : new ArrayBuffer(bytes);
}

/** True when the page may allocate SharedArrayBuffers (COOP/COEP isolation). */
export function canShareMemory(): boolean {
  return typeof SharedArrayBuffer !== 'undefined' && (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true;
}

export function createNodeBuffers(capacity: number, shared: boolean): NodeBuffers {
  const cap = Math.max(1, capacity);
  return {
    capacity: cap,
    shared,
    positions: new Float32Array(backing(cap * 12, shared)),
    heat: new Float32Array(backing(cap * 4, shared)),
    community: new Uint32Array(backing(cap * 4, shared)),
    cluster: new Uint32Array(backing(cap * 4, shared)),
    centrality: new Float32Array(backing(cap * 4, shared)),
    sssp: new Float32Array(backing(cap * 4, shared)),
    ssspParent: new Int32Array(backing(cap * 4, shared)),
    header: new Int32Array(backing(16, shared)),
  };
}

export interface FrameResult {
  /** V5 broadcast sequence (0 for a V3 frame). */
  seq: number;
  /** Records in the frame. */
  count: number;
  /** Records applied to known nodes. */
  applied: number;
  /** Records carrying the agent flag (actors, not code nodes). */
  actors: number;
  /** Records whose id is not in the current index (snapshot is stale). */
  unknown: number;
}

/**
 * Decode a V5 (or bare V3) position frame into `buf` through `index` (wire id → snapshot position).
 * Returns `null` for anything that is not a position frame (sibling opcodes, truncated frames).
 */
export function applyPositionFrame(frame: ArrayBuffer, buf: NodeBuffers, index: Map<number, number>): FrameResult | null {
  if (frame.byteLength < 1) return null;
  const view = new DataView(frame);
  const version = view.getUint8(0);
  let offset: number;
  let seq = 0;
  if (version === PROTOCOL_V5) {
    if (frame.byteLength < V5_HEADER_BYTES) return null;
    seq = Number(view.getBigUint64(1, true));
    offset = V5_HEADER_BYTES;
  } else if (version === PROTOCOL_V3) {
    offset = 1;
  } else {
    return null;
  }
  const body = frame.byteLength - offset;
  if (body % NODE_RECORD_BYTES !== 0) return null;
  const count = body / NODE_RECORD_BYTES;
  let applied = 0;
  let actors = 0;
  let unknown = 0;
  const { positions, heat, community, cluster, centrality, sssp, ssspParent } = buf;
  for (let r = 0; r < count; r++) {
    const o = offset + r * NODE_RECORD_BYTES;
    const raw = view.getUint32(o + OFF.id, true);
    if ((raw & AGENT_FLAG) !== 0) {
      actors++;
      continue;
    }
    const i = index.get(raw & NODE_ID_MASK);
    if (i === undefined || i >= buf.capacity) {
      unknown++;
      continue;
    }
    const p = i * 3;
    positions[p] = view.getFloat32(o + OFF.position, true);
    positions[p + 1] = view.getFloat32(o + OFF.position + 4, true);
    positions[p + 2] = view.getFloat32(o + OFF.position + 8, true);
    sssp[i] = view.getFloat32(o + OFF.ssspDistance, true);
    ssspParent[i] = view.getInt32(o + OFF.ssspParent, true);
    cluster[i] = view.getUint32(o + OFF.cluster, true);
    heat[i] = view.getFloat32(o + OFF.anomaly, true);
    community[i] = view.getUint32(o + OFF.community, true);
    centrality[i] = view.getFloat32(o + OFF.centrality, true);
    applied++;
  }
  if (buf.shared) {
    Atomics.store(buf.header, 1, applied);
    Atomics.add(buf.header, 0, 1);
  } else {
    buf.header[1] = applied;
    buf.header[0] = buf.header[0] + 1;
  }
  return { seq, count, applied, actors, unknown };
}

/** Current frame version counter (cheap per-frame poll from the render loop). */
export function frameVersion(buf: NodeBuffers): number {
  return buf.shared ? Atomics.load(buf.header, 0) : buf.header[0];
}

/** The 21-byte `0x34` acknowledgement: opcode, u64 seq, u32 node count, u64 client ms, all LE. */
export function encodeAck(seq: number, nodes: number, timestampMs: number): Uint8Array {
  const out = new Uint8Array(BROADCAST_ACK_BYTES);
  const v = new DataView(out.buffer);
  v.setUint8(0, OPCODE_BROADCAST_ACK);
  v.setBigUint64(1, BigInt(Math.max(0, Math.floor(seq))), true);
  v.setUint32(9, nodes >>> 0, true);
  v.setBigUint64(13, BigInt(Math.max(0, Math.floor(timestampMs))), true);
  return out;
}

export function decodeAck(bytes: Uint8Array): { seq: number; nodes: number; timestampMs: number } | null {
  if (bytes.length !== BROADCAST_ACK_BYTES || bytes[0] !== OPCODE_BROADCAST_ACK) return null;
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return {
    seq: Number(v.getBigUint64(1, true)),
    nodes: v.getUint32(9, true),
    timestampMs: Number(v.getBigUint64(13, true)),
  };
}

/** One record as the server encodes it (`wire::record_for`). */
export interface WireRecord {
  id: number;
  position: [number, number, number];
  velocity?: [number, number, number];
  ssspDistance?: number;
  ssspParent?: number;
  cluster?: number;
  anomaly?: number;
  community?: number;
  centrality?: number;
}

/** Encode records into a V5 frame (the demo feed and tests use the real wire path). */
export function encodeV5Frame(seq: number, records: readonly WireRecord[]): ArrayBuffer {
  const out = new ArrayBuffer(V5_HEADER_BYTES + records.length * NODE_RECORD_BYTES);
  const v = new DataView(out);
  v.setUint8(0, PROTOCOL_V5);
  v.setBigUint64(1, BigInt(seq), true);
  records.forEach((r, k) => {
    const o = V5_HEADER_BYTES + k * NODE_RECORD_BYTES;
    v.setUint32(o + OFF.id, r.id >>> 0, true);
    v.setFloat32(o + OFF.position, r.position[0], true);
    v.setFloat32(o + OFF.position + 4, r.position[1], true);
    v.setFloat32(o + OFF.position + 8, r.position[2], true);
    const vel = r.velocity ?? [0, 0, 0];
    v.setFloat32(o + OFF.velocity, vel[0], true);
    v.setFloat32(o + OFF.velocity + 4, vel[1], true);
    v.setFloat32(o + OFF.velocity + 8, vel[2], true);
    v.setFloat32(o + OFF.ssspDistance, r.ssspDistance ?? Number.POSITIVE_INFINITY, true);
    v.setInt32(o + OFF.ssspParent, r.ssspParent ?? -1, true);
    v.setUint32(o + OFF.cluster, (r.cluster ?? 0) >>> 0, true);
    v.setFloat32(o + OFF.anomaly, r.anomaly ?? 0, true);
    v.setUint32(o + OFF.community, (r.community ?? 0) >>> 0, true);
    v.setFloat32(o + OFF.centrality, r.centrality ?? 0, true);
  });
  return out;
}

/** Decode one `0x23` agent-action batch: `[0x23][u16 count]([u16 len][u32 agent][u32 node][u8 type][u32 ts][u16 dur])*`. */
export function decodeAgentActions(frame: ArrayBuffer): { agent: number; node: number; action: number; ts: number; durationMs: number }[] {
  const v = new DataView(frame);
  if (frame.byteLength < 3 || v.getUint8(0) !== OPCODE_AGENT_ACTION) return [];
  const n = v.getUint16(1, true);
  const out: { agent: number; node: number; action: number; ts: number; durationMs: number }[] = [];
  let o = 3;
  for (let k = 0; k < n && o + 2 <= frame.byteLength; k++) {
    const len = v.getUint16(o, true);
    o += 2;
    if (len < 15 || o + len > frame.byteLength) break;
    out.push({
      agent: v.getUint32(o, true) & NODE_ID_MASK,
      node: v.getUint32(o + 4, true),
      action: v.getUint8(o + 8),
      ts: v.getUint32(o + 9, true),
      durationMs: v.getUint16(o + 13, true),
    });
    o += len;
  }
  return out;
}

// ---------------------------------------------------------------- worker messages

export type WorkerInbound =
  | { type: 'init'; buffers: NodeBuffers; ids: Uint32Array }
  | { type: 'frame'; data: ArrayBuffer };

export type WorkerOutbound =
  | { type: 'applied'; seq: number; count: number; applied: number; unknown: number }
  | { type: 'rejected'; reason: string; firstByte: number };
