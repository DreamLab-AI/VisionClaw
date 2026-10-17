/**
 * Owns the node buffers and the decoder. With cross-origin isolation the
 * buffers are SharedArrayBuffers written by `positions.worker.ts`; without it
 * (plain http dev) the same decoder runs on the main thread into ordinary
 * buffers, so rendering behaves identically.
 */
import type { GraphModel } from '../graph';
import {
  applyPositionFrame,
  canShareMemory,
  createNodeBuffers,
  type NodeBuffers,
  type WorkerInbound,
  type WorkerOutbound,
} from '../protocol';

export interface Applied {
  seq: number;
  count: number;
  applied: number;
  unknown: number;
}

function nextCapacity(n: number): number {
  let c = 1024;
  while (c < n) c *= 2;
  return c;
}

export class FramePipeline {
  buffers: NodeBuffers;
  readonly shared: boolean;
  private worker: Worker | null = null;
  private index = new Map<number, number>();
  private pending = 0;

  constructor(private readonly onApplied: (r: Applied) => void, private readonly onRejected?: (reason: string, firstByte: number) => void) {
    this.shared = canShareMemory();
    this.buffers = createNodeBuffers(1024, this.shared);
    if (this.shared) {
      this.worker = new Worker(new URL('../workers/positions.worker.ts', import.meta.url), { type: 'module', name: 'codescape-positions' });
      this.worker.onmessage = (ev: MessageEvent<WorkerOutbound>) => {
        const m = ev.data;
        this.pending = Math.max(0, this.pending - 1);
        if (m.type === 'applied') this.onApplied(m);
        else this.onRejected?.(m.reason, m.firstByte);
      };
    }
  }

  /** Frames handed to the decoder and not yet applied. */
  get inFlight(): number {
    return this.pending;
  }

  /**
   * Adopt a (new) snapshot. Values already known for surviving ids are carried
   * over; new nodes start at their parent's position plus jitter so they spring
   * out of it rather than popping in at the origin.
   */
  setModel(model: GraphModel, radius = 100): void {
    const old = this.buffers;
    const oldIndex = this.index;
    const cap = model.n <= old.capacity && old.capacity <= nextCapacity(model.n) * 2 ? old.capacity : nextCapacity(model.n);
    const nb = createNodeBuffers(cap, this.shared);
    const fresh: number[] = [];
    for (let i = 0; i < model.n; i++) {
      const j = oldIndex.get(model.ids[i]);
      if (j !== undefined) {
        nb.positions[i * 3] = old.positions[j * 3];
        nb.positions[i * 3 + 1] = old.positions[j * 3 + 1];
        nb.positions[i * 3 + 2] = old.positions[j * 3 + 2];
        nb.heat[i] = old.heat[j];
        nb.community[i] = old.community[j];
        nb.cluster[i] = old.cluster[j];
        nb.centrality[i] = old.centrality[j];
        nb.sssp[i] = old.sssp[j];
        nb.ssspParent[i] = old.ssspParent[j];
      } else {
        nb.community[i] = model.community[i];
        nb.centrality[i] = model.importance[i];
        nb.sssp[i] = Number.POSITIVE_INFINITY;
        nb.ssspParent[i] = -1;
        fresh.push(i);
      }
    }
    // Seed fresh nodes: parents first (tier order), so children can copy them.
    fresh.sort((a, b) => model.tier[a] - model.tier[b]);
    let seed = 0x9e3779b9;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 0x100000000 - 0.5;
    };
    const known = new Uint8Array(model.n);
    for (let i = 0; i < model.n; i++) known[i] = oldIndex.has(model.ids[i]) ? 1 : 0;
    for (const i of fresh) {
      const p = model.parent[i];
      const spread = model.tier[i] === 0 ? radius : model.tier[i] === 1 ? radius * 0.12 : radius * 0.03;
      const base = p >= 0 && known[p] ? [nb.positions[p * 3], nb.positions[p * 3 + 1], nb.positions[p * 3 + 2]] : [0, 0, 0];
      nb.positions[i * 3] = base[0] + rnd() * spread;
      nb.positions[i * 3 + 1] = base[1] + rnd() * spread;
      nb.positions[i * 3 + 2] = base[2] + rnd() * spread;
      known[i] = 1;
    }
    nb.header[0] = (old.header[0] + 1) | 0;
    this.buffers = nb;
    this.index = new Map(model.index);
    if (this.worker) {
      const msg: WorkerInbound = { type: 'init', buffers: nb, ids: model.ids.slice() };
      this.worker.postMessage(msg);
    }
  }

  /** Hand one binary frame to the decoder (transferred to the worker when isolated). */
  push(frame: ArrayBuffer): void {
    if (this.worker) {
      this.pending++;
      const msg: WorkerInbound = { type: 'frame', data: frame };
      this.worker.postMessage(msg, [frame]);
      return;
    }
    const r = applyPositionFrame(frame, this.buffers, this.index);
    if (r) this.onApplied(r);
    else this.onRejected?.('not a position frame', frame.byteLength ? new Uint8Array(frame)[0] : 0);
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
  }
}
