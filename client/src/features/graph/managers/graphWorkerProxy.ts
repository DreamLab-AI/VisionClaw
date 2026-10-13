/**
 * graphWorkerProxy.ts — Phase 4 D7 surface
 *
 * Per ADR-03 D7, the main-thread proxy exposes EXACTLY four methods plus
 * `WORKER_USES_SAB` capability detection:
 *
 *   - processBinaryFrame(frame): single-flight binary parse + position write
 *   - getPositions(): zero-copy SAB view or transferred Float32Array
 *   - setGraphTopology(graph): topology delivery to the worker
 *   - dispose(): tear down the worker
 *
 * All other historical methods (`tick`, `getGraphData`, `getAnalyticsBuffer`,
 * `reheatSimulation`, `updateForcePhysicsSettings`, etc.) are removed.
 * Deprecation shims throw a helpful error if any legacy callsite was missed
 * during the migration sweep.
 *
 * Capability detection (T3):
 *   const WORKER_USES_SAB = (typeof SharedArrayBuffer !== 'undefined')
 *     && self.crossOriginIsolated === true;
 *
 *   - SAB mode: positions are written by the worker into a SharedArrayBuffer.
 *     `getPositions()` returns a Float32Array view over the SAB — main thread
 *     reads it directly each frame, no IPC.
 *   - Comlink mode: positions are transferred back from the worker (zero-copy
 *     by neutering the ArrayBuffer). `getPositions()` returns the latest view.
 *
 * Ordered single-flight discipline (T2, amended by ADR-2141):
 *   - At most one Comlink call is in flight.
 *   - Frames that arrive meanwhile wait in a FIFO queue and are processed in order. NONE is
 *     collapsed: a position frame is one chunk of the client's acknowledged stream (only the
 *     nodes that changed since its last acknowledgement), so a newer chunk does not supersede an
 *     older one. Each call's promise resolves when THAT frame has been applied, which is when the
 *     caller may acknowledge it.
 */

import { wrap, transfer, Remote } from 'comlink';
import type { GraphWorkerType, NodeMetadata } from '../workers/graph.worker';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GraphWorkerProxy');

/** Backstop on the frame queue; see processBinaryFrame. */
const MAX_QUEUED_FRAMES = 256;

// ── Re-exported types (kept for backwards-compatible imports) ────────────
export type { NodeMetadata } from '../workers/graph.worker';

export interface Node {
  id: string;
  label: string;
  position: { x: number; y: number; z: number };
  metadata?: NodeMetadata;
}

export interface Edge {
  id: string;
  source: string;
  target: string;
  label?: string;
  weight?: number;
  edgeType?: string;
  metadata?: Record<string, any>;
}

export interface GraphData {
  nodes: Node[];
  edges: Edge[];
}

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

// ── Capability detection (ADR-03 D3, fires once at module load) ──────────
const FORCE_COMLINK =
  typeof import.meta !== 'undefined' &&
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (import.meta as any).env?.VITE_FORCE_COMLINK === '1';

const SAB_CAPABLE =
  typeof SharedArrayBuffer !== 'undefined' &&
  // `self.crossOriginIsolated` is defined in browser globals; cast for TS in Node.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (typeof self !== 'undefined' && (self as any).crossOriginIsolated === true);

export const WORKER_USES_SAB: boolean = SAB_CAPABLE && !FORCE_COMLINK;

if (!WORKER_USES_SAB) {
  if (FORCE_COMLINK) {
    logger.info('VITE_FORCE_COMLINK=1 — forcing Comlink transfer path');
  } else if (!SAB_CAPABLE) {
    logger.warn(
      'SharedArrayBuffer unavailable or cross-origin isolation not active; ' +
        'falling back to Comlink transfer path.',
    );
  }
}

// ── Internal state ───────────────────────────────────────────────────────

// SAB capacity. The worker writes currentPositions at stride 3 (x,y,z) and the
// renderer reads at stride 3, so usable node slots = (MAX_NODES * POSITION_FLOATS_PER_NODE) / 3.
// The full KG∪Agent graph after a GitHub sync exceeds 14.5k nodes; at 10k the tail
// overflowed the SAB and rendered as NaN/garbage (the 3D "blob"). Sized to cover
// large graphs with generous headroom (~66k node slots, 800KB SAB).
const MAX_NODES = 50_000;
const POSITION_FLOATS_PER_NODE = 4; // x,y,z + padding (matches worker layout)
const POSITION_BUFFER_FLOATS = MAX_NODES * POSITION_FLOATS_PER_NODE;
const POSITION_BUFFER_BYTES = POSITION_BUFFER_FLOATS * 4;

class GraphWorkerProxy {
  private static instance: GraphWorkerProxy;

  private worker: Worker | null = null;
  private workerApi: Remote<GraphWorkerType> | null = null;
  private isInitialized = false;
  private initPromise: Promise<void> | null = null;

  // SAB mode: a single SAB-backed Float32Array, shared with the worker.
  private sharedBuffer: SharedArrayBuffer | null = null;
  private sharedPositionView: Float32Array | null = null;

  // Comlink mode: latest transferred ArrayBuffer wrapped as Float32Array.
  private lastTransferredView: Float32Array | null = null;

  // Ordered single-flight binary-frame discipline (ADR-03 D2, ADR-2141).
  private _frameQueue: Array<{ frame: Uint8Array; resolve: () => void; reject: (e: unknown) => void }> = [];
  private _draining = false;
  private _framesProcessed = 0;
  private _framesDropped = 0;

  private constructor() {}

  public static getInstance(): GraphWorkerProxy {
    if (!GraphWorkerProxy.instance) {
      GraphWorkerProxy.instance = new GraphWorkerProxy();
    }
    return GraphWorkerProxy.instance;
  }

  // ── Initialisation ─────────────────────────────────────────────────────

  public async initialize(): Promise<void> {
    if (this.isInitialized) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = this._doInitialize();
    return this.initPromise;
  }

  private async _doInitialize(): Promise<void> {
    logger.info(
      `Starting worker initialization (SAB mode=${WORKER_USES_SAB})`,
    );

    this.worker = new Worker(
      new URL('../workers/graph.worker.ts', import.meta.url),
      { type: 'module' },
    );

    this.worker.onerror = (error) => {
      logger.error('Worker error:', error);
    };

    this.workerApi = wrap<GraphWorkerType>(this.worker);

    try {
      await this.workerApi.initialize();
    } catch (err) {
      logger.error('Worker handshake failed:', err);
      throw err;
    }

    if (WORKER_USES_SAB) {
      try {
        this.sharedBuffer = new SharedArrayBuffer(POSITION_BUFFER_BYTES);
        this.sharedPositionView = new Float32Array(this.sharedBuffer);
        await this.workerApi.setupSharedPositions(this.sharedBuffer);
        logger.info(
          `SAB attached: ${POSITION_BUFFER_BYTES} bytes for ${MAX_NODES} nodes`,
        );
      } catch (err) {
        logger.warn(
          'SAB attach failed — degrading to Comlink transfer path:',
          err,
        );
        this.sharedBuffer = null;
        this.sharedPositionView = null;
      }
    }

    this.isInitialized = true;
    logger.info('Worker proxy initialized');
  }

  public isReady(): boolean {
    return this.isInitialized && this.workerApi !== null;
  }

  // ── D7 method 1: processBinaryFrame (ordered single-flight) ───────────

  /**
   * Process a binary position frame. At most one frame is in flight; the rest queue in arrival
   * order and none is dropped (see the module comment). The returned promise resolves once THIS
   * frame has been applied by the worker.
   *
   * The caller transfers ownership of `frame.buffer` — the underlying ArrayBuffer is neutered
   * after the frame is dispatched (zero-copy).
   */
  public processBinaryFrame(frame: Uint8Array): Promise<void> {
    if (!this.workerApi) {
      return Promise.reject(new Error('Worker not initialized; call initialize() first'));
    }
    return new Promise<void>((resolve, reject) => {
      if (this._frameQueue.length >= MAX_QUEUED_FRAMES) {
        // Backstop against a misbehaving sender; the acknowledged stream keeps the real depth tiny.
        const dropped = this._frameQueue.shift()!;
        this._framesDropped++;
        dropped.reject(new Error('binary frame queue overflow'));
      }
      this._frameQueue.push({ frame, resolve, reject });
      void this._drain();
    });
  }

  private async _drain(): Promise<void> {
    if (this._draining) return;
    this._draining = true;
    try {
      while (this._frameQueue.length > 0) {
        const job = this._frameQueue.shift()!;
        try {
          await this._dispatchFrame(job.frame);
          this._framesProcessed++;
          job.resolve();
        } catch (err) {
          logger.error('processBinaryFrame failed:', err);
          job.reject(err);
        }
        // Yield between frames so the render loop gets a turn (D2).
        await new Promise<void>((r) => queueMicrotask(r));
      }
    } finally {
      this._draining = false;
    }
  }

  private async _dispatchFrame(frame: Uint8Array): Promise<void> {
    if (!this.workerApi) return;

    // Comlink.transfer marks the ArrayBuffer as transferable. After the call,
    // `frame.buffer` is neutered — the WebSocket's internal copy is gone.
    const transferable = transfer(frame.buffer, [frame.buffer]);

    if (WORKER_USES_SAB && this.sharedBuffer) {
      // SAB mode: worker writes into the SAB view we shared at startup.
      // Return value is `void` — the renderer reads SAB directly.
      await this.workerApi.processBinaryFrame(
        transferable as unknown as ArrayBuffer,
      );
    } else {
      // Comlink mode: worker returns a transferred ArrayBuffer with the
      // parsed positions; we re-wrap it as Float32Array.
      const returned = (await this.workerApi.processBinaryFrame(
        transferable as unknown as ArrayBuffer,
      )) as ArrayBuffer | void;
      if (returned instanceof ArrayBuffer) {
        this.lastTransferredView = new Float32Array(returned);
      }
    }
  }

  // ── D7 method 2: getPositions (zero-copy) ─────────────────────────────

  /**
   * Returns the current positions buffer.
   * - SAB mode: returns the Float32Array view over the SharedArrayBuffer
   *   (writes by the worker are visible immediately on the main thread).
   * - Comlink mode: returns the most-recently-transferred Float32Array.
   *
   * The async signature matches the D7 contract; the body is synchronous
   * (the buffer is already in main-thread-visible memory).
   */
  public async getPositions(): Promise<Float32Array> {
    if (WORKER_USES_SAB && this.sharedPositionView) {
      return this.sharedPositionView;
    }
    if (this.lastTransferredView) {
      return this.lastTransferredView;
    }
    return new Float32Array(0);
  }

  /**
   * Synchronous accessor used by per-frame render code (`useFrame`).
   * Mirrors `getPositions()` but avoids the Promise allocation. In SAB mode
   * this is identical to the async path — both return the same SAB view.
   */
  public getPositionsSync(): Float32Array | null {
    if (WORKER_USES_SAB && this.sharedPositionView) {
      return this.sharedPositionView;
    }
    return this.lastTransferredView;
  }

  // ── D7 method 3: setGraphTopology ─────────────────────────────────────

  /**
   * Hand topology to the worker. The worker uses it for binary-frame node-id
   * resolution and edge-length computation. Positions in the topology payload
   * are advisory — the SAB is authoritative once binary frames flow.
   */
  public async setGraphTopology(graph: GraphData): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized; call initialize() first');
    }
    await this.workerApi.setGraphData(graph);
  }

  // ── D7 method 4: dispose ──────────────────────────────────────────────

  public async dispose(): Promise<void> {
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }
    this.workerApi = null;
    this.sharedBuffer = null;
    this.sharedPositionView = null;
    this.lastTransferredView = null;
    // Frames still queued will never be applied: fail them rather than leave callers hanging.
    for (const job of this._frameQueue.splice(0)) {
      job.reject(new Error('worker disposed'));
    }
    this.isInitialized = false;
    this.initPromise = null;
  }

  // ── Diagnostics (read-only, for dev overlay) ──────────────────────────

  public getStats(): { framesProcessed: number; framesDropped: number } {
    return {
      framesProcessed: this._framesProcessed,
      framesDropped: this._framesDropped,
    };
  }

  // ── Deprecation shims for removed surface (T1) ────────────────────────
  //
  // Any of these throws if a legacy callsite was missed during the migration
  // sweep. The error message points at the new surface so the fix is obvious.

  private static _deprecated(method: string, replacement: string): never {
    throw new Error(
      `graphWorkerProxy.${method}() was removed in Phase 4 (ADR-03 D7). ` +
        `Use ${replacement} instead.`,
    );
  }

  public setGraphType(_type: 'knowledge' | 'visionclaw'): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'setGraphType',
      'topology fields on setGraphTopology(...)',
    );
  }
  public getGraphType(): never {
    return GraphWorkerProxy._deprecated('getGraphType', 'main-thread state');
  }
  public setGraphData(_data: GraphData): Promise<void> {
    return GraphWorkerProxy._deprecated('setGraphData', 'setGraphTopology(...)');
  }
  public processBinaryData(_data: ArrayBuffer): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'processBinaryData',
      'processBinaryFrame(Uint8Array)',
    );
  }
  public getGraphData(): Promise<GraphData> {
    return GraphWorkerProxy._deprecated(
      'getGraphData',
      'graphDataManager.getLastGraphData() (cached on main thread)',
    );
  }
  public hasUnknownNodes(): Promise<boolean> {
    return GraphWorkerProxy._deprecated(
      'hasUnknownNodes',
      'topology re-fetch on user action',
    );
  }
  public updateNode(_node: Node): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'updateNode',
      'setGraphTopology(updated)',
    );
  }
  public removeNode(_nodeId: string): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'removeNode',
      'setGraphTopology(updated)',
    );
  }
  public updateSettings(_settings: unknown): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'updateSettings',
      'main-thread settings store (worker has no settings)',
    );
  }
  public pinNode(_nodeId: number): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'pinNode',
      'server-side pin RPC (worker no longer owns physics)',
    );
  }
  public unpinNode(_nodeId: number): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'unpinNode',
      'server-side unpin RPC (worker no longer owns physics)',
    );
  }
  public updateUserDrivenNodePosition(
    _nodeId: number,
    _position: Vec3,
  ): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'updateUserDrivenNodePosition',
      'WebSocket binary position upload (worker no longer owns positions)',
    );
  }
  public tick(_dt: number): Promise<Float32Array> {
    return GraphWorkerProxy._deprecated(
      'tick',
      'GPU physics on the server (client physics is removed)',
    );
  }
  public requestTick(_dt: number): void {
    GraphWorkerProxy._deprecated(
      'requestTick',
      'GPU physics on the server (client physics is removed)',
    );
  }
  public getConsecutiveErrors(): number {
    return GraphWorkerProxy._deprecated(
      'getConsecutiveErrors',
      'getStats().framesDropped',
    );
  }
  public getAnalyticsBuffer(): Promise<Float32Array> {
    return GraphWorkerProxy._deprecated(
      'getAnalyticsBuffer',
      'main-thread analytics store (worker no longer owns analytics)',
    );
  }
  public recomputeAnalytics(): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'recomputeAnalytics',
      'main-thread analytics store',
    );
  }
  public reheatSimulation(_alpha?: number): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'reheatSimulation',
      'server-side reheat RPC',
    );
  }
  public updateForcePhysicsSettings(_s: unknown): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'updateForcePhysicsSettings',
      'server-side physics settings RPC',
    );
  }
  public getForcePhysicsSettings(): Promise<unknown> {
    return GraphWorkerProxy._deprecated(
      'getForcePhysicsSettings',
      'server-side physics settings RPC',
    );
  }
  public setTweeningSettings(_s: unknown): Promise<void> {
    return GraphWorkerProxy._deprecated(
      'setTweeningSettings',
      'main-thread interpolation (worker tweening is removed)',
    );
  }
  public getSharedPositionBuffer(): Float32Array | null {
    return this.sharedPositionView;
  }
  public onGraphDataChange(_listener: (data: GraphData) => void): () => void {
    GraphWorkerProxy._deprecated(
      'onGraphDataChange',
      'graphDataManager.subscribe(...)',
    );
  }
  public onPositionUpdate(
    _listener: (positions: Float32Array) => void,
  ): () => void {
    GraphWorkerProxy._deprecated(
      'onPositionUpdate',
      'useFrame() polling getPositionsSync() (SAB mode is push-free)',
    );
  }
}

// Singleton instance
export const graphWorkerProxy = GraphWorkerProxy.getInstance();

// Re-export ForcePhysicsSettings for callers that still type against it.
export type { ForcePhysicsSettings } from '../workers/graph.worker';
