/// <reference lib="webworker" />
/**
 * Position-frame decoder. Receives raw V5 frames (transferred, zero-copy),
 * writes them into the SharedArrayBuffer-backed node arrays through the
 * id→index map, and posts back only "frame applied, seq N". The main thread
 * owns the socket and sends the 0x34 ack.
 */
import { applyPositionFrame, type NodeBuffers, type WorkerInbound, type WorkerOutbound } from '../protocol';

let buffers: NodeBuffers | null = null;
let index = new Map<number, number>();

const post = (m: WorkerOutbound) => (self as unknown as DedicatedWorkerGlobalScope).postMessage(m);

self.onmessage = (ev: MessageEvent<WorkerInbound>) => {
  const msg = ev.data;
  if (msg.type === 'init') {
    buffers = msg.buffers;
    index = new Map();
    const ids = msg.ids;
    for (let i = 0; i < ids.length; i++) index.set(ids[i], i);
    return;
  }
  if (msg.type === 'frame') {
    if (!buffers) {
      post({ type: 'rejected', reason: 'no index yet', firstByte: new Uint8Array(msg.data, 0, 1)[0] ?? 0 });
      return;
    }
    const r = applyPositionFrame(msg.data, buffers, index);
    if (!r) {
      post({ type: 'rejected', reason: 'not a position frame', firstByte: msg.data.byteLength ? new Uint8Array(msg.data, 0, 1)[0] : 0 });
      return;
    }
    post({ type: 'applied', seq: r.seq, count: r.count, applied: r.applied, unknown: r.unknown });
  }
};
