#!/usr/bin/env node
// Acceptance test for live interactive dragging (ADR "server-side holds").
//
// Two WebSocket connections to one backend. Connection A grabs a node (or two, with
// --second-node) and drags it; connection B only watches. The script measures, from B's and A's
// position streams:
//
//   * how long until the held node's neighbours move (the whole graph answering the hand);
//   * how much later B sees the same motion than A does (observer latency);
//   * the frame rate of the position stream before, during and after the hold;
//   * how long after nodeDragEnd until the graph is at rest, and that the released node rejoined
//     the simulation instead of staying pinned.
//
// It MOVES NODES on the server it is pointed at. Do not run it against a backend someone is
// using without their consent.
//
//   node scripts/drag-physics-acceptance.mjs --url ws://HOST:PORT/wss [options]
//
// Options (all optional except --url):
//   --token T --pubkey P     authenticate with a session token (default: dev-mode authenticate)
//   --node ID                node to hold (default: the node nearest the origin)
//   --second-node ID         hold a second node from the same connection (the two-controller case)
//   --neighbours a,b,c       nodes to treat as neighbours (default: the 12 nearest by position)
//   --distance U             drag distance in world units along +x (default 150)
//   --drag-ms MS             how long the drag lasts (default 1500)
//   --rate HZ                drag update rate (default 60)
//   --move-eps U             a node "moved" past this many units (default 0.05)
//   --rest-eps U             a frame counts as motion if any node moved more than this (default 0.05)
//   --quiet-ms MS            no motion frames for this long = at rest (default 500)
//   --max-settle-ms MS       settle budget for the PASS verdict (default 2000)
//   --max-neighbour-ms MS    neighbour-response budget for the PASS verdict (default 500)
//   --observe-ms MS          how long to watch after release (default 6000)
//   --ws-module PATH         path to the `ws` package (default: client/node_modules/ws)
//
// The stream is paced per client (ADR-2141): the server sends a V5 chunk
// `[0x05][u64 seq](52-byte record)*` and sends the next only after the client acknowledges it with
// `[0x34][u64 seq][u32 nodes][u64 timestamp_ms]` (21 bytes), or after the ack timeout (500 ms). A
// chunk carries only nodes that moved past the stream threshold (default 0.05) since the client
// last held them, so a graph at rest sends NOTHING. This script therefore acknowledges every V5
// chunk, reads "rest" as a quiet stream (`--quiet-ms`, default 500) and treats any record for a
// node as that node having moved. The record starts `[u32 wire_id][f32 x][f32 y][f32 z]`; the node
// id is `wire_id & 0x03FFFFFF` (crates/visionclaw-wire).
//
// Exit code 0 = every check passed, 1 = a check failed, 2 = could not run.

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) continue;
    out[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[++i] : 'true';
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
if (!args.url) {
  console.error('usage: drag-physics-acceptance.mjs --url ws://HOST:PORT/wss [options]  (see the header)');
  process.exit(2);
}
const opt = {
  distance: Number(args.distance ?? 150),
  dragMs: Number(args['drag-ms'] ?? 1500),
  rate: Number(args.rate ?? 60),
  moveEps: Number(args['move-eps'] ?? 0.05),
  restEps: Number(args['rest-eps'] ?? 0.05),
  quietMs: Number(args['quiet-ms'] ?? 500),
  maxSettleMs: Number(args['max-settle-ms'] ?? 2000),
  maxNeighbourMs: Number(args['max-neighbour-ms'] ?? 500),
  observeMs: Number(args['observe-ms'] ?? 6000),
};

const require = createRequire(import.meta.url);
let WebSocket;
try {
  WebSocket = require(args['ws-module'] ?? path.resolve(here, '../client/node_modules/ws'));
} catch (e) {
  console.error(`cannot load the ws package (${e.message}); pass --ws-module PATH`);
  process.exit(2);
}

const NODE_ID_MASK = 0x03ffffff;
const RECORD = 52;
const now = () => performance.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Decode a binary position frame into {seq, nodes:[{id,x,y,z}]} (seq null for V3), or null. */
function decodeFrame(buf) {
  if (!buf || buf.length < 1) return null;
  let off;
  let seq = null;
  if (buf[0] === 0x03) off = 1;
  else if (buf[0] === 0x05 && buf.length >= 9) {
    off = 9;
    seq = buf.readBigUInt64LE(1);
  } else return null;
  const body = buf.length - off;
  if (body <= 0 || body % RECORD !== 0) return null;
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const nodes = [];
  for (let p = off; p < buf.length; p += RECORD) {
    nodes.push({
      id: dv.getUint32(p, true) & NODE_ID_MASK,
      x: dv.getFloat32(p + 4, true),
      y: dv.getFloat32(p + 8, true),
      z: dv.getFloat32(p + 12, true),
    });
  }
  return { seq, nodes };
}

/** One connection: latest positions, a timestamped frame log, JSON control messages. */
class Client {
  constructor(name) {
    this.name = name;
    this.pos = new Map(); // id -> [x,y,z]
    this.log = []; // {t, moved: Map(id -> distance from previous), count}
    this.frames = 0;
    this.authed = false;
    this.acks = [];
  }

  async open() {
    this.ws = new WebSocket(args.url, { perMessageDeflate: true });
    this.ws.binaryType = 'nodebuffer';
    this.ws.on('message', (data, isBinary) => (isBinary ? this.onBinary(data) : this.onText(data.toString())));
    await new Promise((res, rej) => {
      this.ws.once('open', res);
      this.ws.once('error', rej);
    });
    const auth = { type: 'authenticate' };
    if (args.token) auth.token = args.token;
    if (args.pubkey) auth.pubkey = args.pubkey;
    this.send(auth);
    const deadline = now() + 8000;
    while (!this.authed && now() < deadline) await sleep(25);
    if (!this.authed) throw new Error(`${this.name}: no authenticate_success within 8 s`);
    this.send({ type: 'requestInitialData' });
  }

  send(obj) {
    this.ws.send(JSON.stringify(obj));
  }

  /** Acknowledge a V5 chunk so the server sends the next one without waiting out its timeout. */
  ack(seq, nodes) {
    const f = Buffer.alloc(21);
    f[0] = 0x34;
    f.writeBigUInt64LE(seq, 1);
    f.writeUInt32LE(nodes, 9);
    f.writeBigUInt64LE(BigInt(Date.now()), 13);
    this.ws.send(f);
  }

  onText(s) {
    let m;
    try {
      m = JSON.parse(s);
    } catch {
      return;
    }
    if (m.type === 'authenticate_success') this.authed = true;
    if (m.type === 'authenticate_error') console.error(`${this.name}: ${m.error}`);
    if (typeof m.type === 'string' && m.type.startsWith('node')) this.acks.push({ t: now(), type: m.type });
  }

  onBinary(buf) {
    const frame = decodeFrame(buf);
    if (!frame) return;
    const { seq, nodes } = frame;
    const t = now();
    if (seq !== null) this.ack(seq, nodes.length);
    const moved = new Map();
    for (const n of nodes) {
      const prev = this.pos.get(n.id);
      if (prev) moved.set(n.id, Math.hypot(n.x - prev[0], n.y - prev[1], n.z - prev[2]));
      this.pos.set(n.id, [n.x, n.y, n.z]);
    }
    this.frames++;
    this.log.push({ t, moved, count: nodes.length });
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* already closed */
    }
  }
}

/** Frames per second over [t0, t1). */
function fps(client, t0, t1) {
  const n = client.log.filter((f) => f.t >= t0 && f.t < t1).length;
  return t1 > t0 ? (n * 1000) / (t1 - t0) : 0;
}

/** First time at or after t0 that any of `ids` has travelled more than eps. */
function firstMove(client, ids, t0, eps) {
  // Path length from the frame log: each frame adds how far the node moved since the last one.
  const disp = new Map(ids.map((i) => [i, 0]));
  for (const f of client.log) {
    if (f.t < t0) continue;
    for (const i of ids) {
      disp.set(i, disp.get(i) + (f.moved.get(i) ?? 0));
      if (disp.get(i) > eps) return f.t;
    }
  }
  return null;
}

/**
 * Time of the last frame at or after `t0` that carried motion (a node past `eps`), provided the
 * stream has then been quiet for `quietMs` up to `tEnd`; null when it was still moving at the end
 * or never moved. A graph at rest sends no frames, so rest is silence, not small frames.
 */
function lastMotion(client, t0, tEnd, eps, quietMs) {
  let last = null;
  for (const f of client.log) {
    if (f.t < t0) continue;
    let max = 0;
    for (const d of f.moved.values()) if (d > max) max = d;
    if (max > eps) last = f.t;
  }
  if (last === null) return t0; // nothing moved after t0: already at rest
  return tEnd - last >= quietMs ? last : null;
}

const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

const A = new Client('dragger');
const B = new Client('observer');
let exit = 0;

try {
  await A.open();
  await B.open();
  console.log('both connections authenticated; waiting for position frames');
  const warm = now() + 20000;
  while ((A.frames < 1 || B.frames < 1) && now() < warm) await sleep(50);
  if (A.frames < 1 || B.frames < 1) throw new Error('no position frames received in 20 s');
  // The first load is chunked and acknowledged chunk by chunk; wait for it to go quiet.
  let lastCount = -1;
  for (const quietSince = { t: now() }; now() < warm; await sleep(100)) {
    const c = A.frames + B.frames;
    if (c !== lastCount) {
      lastCount = c;
      quietSince.t = now();
    } else if (now() - quietSince.t > 1500) break;
  }

  // Choose the nodes and their neighbours.
  const ids = [...A.pos.keys()];
  const nearest = (from, k, exclude) =>
    ids
      .filter((i) => !exclude.includes(i))
      .map((i) => {
        const p = A.pos.get(i);
        return [i, Math.hypot(p[0] - from[0], p[1] - from[1], p[2] - from[2])];
      })
      .sort((a, b) => a[1] - b[1])
      .slice(0, k)
      .map((e) => e[0]);
  const nodeId = args.node
    ? Number(args.node)
    : [...ids].sort(
        (a, b) => Math.hypot(...A.pos.get(a)) - Math.hypot(...A.pos.get(b)),
      )[0];
  const held = [nodeId];
  if (args['second-node']) held.push(Number(args['second-node']));
  for (const h of held) if (!A.pos.has(h)) throw new Error(`node ${h} is not in the position stream`);
  const neighbours = args.neighbours
    ? args.neighbours.split(',').map(Number)
    : nearest(A.pos.get(nodeId), 12, held);
  console.log(`holding ${held.join(', ')}; watching neighbours ${neighbours.join(', ')}`);

  // Baseline frame rate with nothing held.
  const tBase0 = now();
  await sleep(2000);
  const baseFpsA = fps(A, tBase0, now());
  const baseFpsB = fps(B, tBase0, now());

  // Grab.
  const start = new Map(held.map((h) => [h, [...A.pos.get(h)]]));
  const tGrab = now();
  for (const h of held) {
    const p = start.get(h);
    A.send({ type: 'nodeDragStart', data: { nodeId: h, position: { x: p[0], y: p[1], z: p[2] } } });
  }

  // Drag along +x for dragMs at `rate`.
  const period = 1000 / opt.rate;
  const steps = Math.max(1, Math.round(opt.dragMs / period));
  const tFirstUpdate = now();
  for (let k = 1; k <= steps; k++) {
    const f = k / steps;
    for (const h of held) {
      const p = start.get(h);
      A.send({
        type: 'nodeDragUpdate',
        data: {
          nodeId: h,
          position: { x: p[0] + opt.distance * f, y: p[1], z: p[2] },
          timestamp: Date.now(),
        },
      });
    }
    await sleep(period);
  }
  const tRelease = now();
  const heldNow = new Map(held.map((h) => [h, [...A.pos.get(h)]]));
  for (const h of held) A.send({ type: 'nodeDragEnd', data: { nodeId: h } });

  // Watch the aftermath.
  await sleep(opt.observeMs);
  const tEnd = now();

  // --- Report ---------------------------------------------------------------------------
  const nA = firstMove(A, neighbours, tFirstUpdate, opt.moveEps);
  const nB = firstMove(B, neighbours, tFirstUpdate, opt.moveEps);
  const hA = firstMove(A, held, tFirstUpdate, opt.moveEps);
  const hB = firstMove(B, held, tFirstUpdate, opt.moveEps);
  const ms = (t, t0) => (t == null ? null : Math.round(t - t0));

  const report = {
    heldNodes: held,
    neighbours,
    streamFps: {
      baselineDragger: +baseFpsA.toFixed(1),
      baselineObserver: +baseFpsB.toFixed(1),
      duringHoldDragger: +fps(A, tFirstUpdate, tRelease).toFixed(1),
      duringHoldObserver: +fps(B, tFirstUpdate, tRelease).toFixed(1),
      afterReleaseObserver: +fps(B, tRelease, tEnd).toFixed(1),
    },
    heldNodeVisibleMs: { dragger: ms(hA, tFirstUpdate), observer: ms(hB, tFirstUpdate) },
    neighbourMovedMs: { dragger: ms(nA, tFirstUpdate), observer: ms(nB, tFirstUpdate) },
    observerLatencyMs: hA != null && hB != null ? Math.round(hB - hA) : null,
    grabAckMs: (() => {
      const a = A.acks.find((x) => x.type === 'nodeDragStartAck');
      return a ? Math.round(a.t - tGrab) : null;
    })(),
  };

  const restB = lastMotion(B, tRelease, tEnd, opt.restEps, opt.quietMs);
  report.settleMs = restB == null ? null : Math.round(restB - tRelease);
  report.framesAfterRelease = B.log.filter((f) => f.t >= tRelease).length;

  // Did the released node rejoin the simulation? A pinned node never moves after release; a free
  // one is pulled by its springs, so it travels from the drop point.
  const afterB = new Map(held.map((h) => [h, B.pos.get(h)]));
  report.releasedNodeTravel = Object.fromEntries(
    held.map((h) => {
      const a = heldNow.get(h);
      const b = afterB.get(h);
      return [h, +Math.hypot(b[0] - a[0], b[1] - a[1], b[2] - a[2]).toFixed(2)];
    }),
  );

  console.log(JSON.stringify(report, null, 2));

  check('observer saw the held node move', hB != null, `${report.heldNodeVisibleMs.observer} ms after the first update`);
  check(
    'neighbours moved while held (dragger)',
    nA != null && nA - tFirstUpdate <= opt.maxNeighbourMs,
    `${report.neighbourMovedMs.dragger} ms, budget ${opt.maxNeighbourMs}`,
  );
  check(
    'neighbours moved while held (observer)',
    nB != null && nB - tFirstUpdate <= opt.maxNeighbourMs,
    `${report.neighbourMovedMs.observer} ms, budget ${opt.maxNeighbourMs}`,
  );
  check(
    'graph at rest after release',
    report.settleMs != null && report.settleMs <= opt.maxSettleMs,
    `${report.settleMs} ms, budget ${opt.maxSettleMs}`,
  );
  check(
    'released node rejoined the physics (not left pinned)',
    Object.values(report.releasedNodeTravel).every((d) => d > opt.moveEps),
    JSON.stringify(report.releasedNodeTravel),
  );
  if (results.some((r) => !r.ok)) exit = 1;
} catch (e) {
  console.error(`could not run: ${e.message}`);
  exit = 2;
} finally {
  A.close();
  B.close();
}
process.exit(exit);
