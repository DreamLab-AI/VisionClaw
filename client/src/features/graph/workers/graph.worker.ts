

import { expose } from 'comlink';
import {
  BinaryNodeData,
  Vec3,
  decodePositionFrame,
  createBinaryNodeData,
} from '../../../types/binaryProtocol';
import { stringToU32 } from '../../../types/idMapping';

const MAX_HASH_PROBES = 1000;

function findFreeMappedId(nodeId: string, reverseNodeIdMap: Map<number, string>): number {
  let h = stringToU32(nodeId);
  let probe = 0;
  while (reverseNodeIdMap.has(h) && reverseNodeIdMap.get(h) !== nodeId && probe < MAX_HASH_PROBES) {
    probe += 1;
    h = (h + probe * probe) >>> 0;
  }
  if (reverseNodeIdMap.has(h) && reverseNodeIdMap.get(h) !== nodeId) {
    throw new Error(`Hash collision limit exceeded for node '${nodeId}'`);
  }
  return h;
}

// Worker-safe logger (createLogger depends on localStorage/window which are unavailable in Workers)
// Only warn/error by default; set self.__WORKER_DEBUG = true in devtools to enable info/debug
const workerSelf = self as unknown as Record<string, unknown>;
const workerLogger = {
  info: (...args: unknown[]) => { if (workerSelf.__WORKER_DEBUG) console.log('[GraphWorker]', ...args); },
  warn: (...args: unknown[]) => console.warn('[GraphWorker]', ...args),
  error: (...args: unknown[]) => console.error('[GraphWorker]', ...args),
  debug: (...args: unknown[]) => { if (workerSelf.__WORKER_DEBUG) console.debug('[GraphWorker]', ...args); },
};

/**
 * Typed metadata for graph nodes.
 *
 * The index signature `[key: string]: any` preserves full backward compatibility —
 * existing code can access arbitrary fields from the backend without casts.
 * The named optional fields provide autocomplete, documentation, and a canonical
 * reference of known metadata shapes used across the codebase.
 *
 * Known node archetypes (discriminated by `type`):
 *  - `'agent'`    — agentType, health, status, workload, tokenRate, currentTask, etc.
 *  - `'knowledge'`— quality, authority, source_domain, page_url, file_path, etc.
 *  - `'ontology'` — hierarchyDepth, classIri, violations, constraintValid, etc.
 */
export interface NodeMetadata {
  // --- Discriminator ---
  type?: string;
  nodeType?: string;

  // --- Common ---
  // Note: quality, quality_score, qualityScore, authority, authority_score,
  // authorityScore, and instanceCount are intentionally omitted from named
  // fields because they arrive from the backend as mixed string/number/unknown
  // types and are consumed inconsistently (parseInt, parseFloat, ?? 0, etc.).
  // They remain fully accessible via the [key: string]: any index signature.
  size?: number;
  depth?: number;
  lastModified?: string | number;
  last_modified?: string | number;
  updated_at?: string | number;
  updatedAt?: string | number;
  color?: string;
  name?: string;
  velocity?: { x: number; y: number; z: number };

  // --- Domain / clustering ---
  source_domain?: string;
  domain?: string;
  cluster?: string;

  // --- Ontology / hierarchy ---
  classIri?: string;
  hierarchyDepth?: number;
  violations?: number;
  constraintValid?: boolean;

  // --- Agent ---
  agentType?: string;
  agent_type?: string;
  health?: number;
  status?: string;
  workload?: number;
  tokenRate?: number;
  currentTask?: string;
  tasksActive?: number;
  tasks?: number;

  // --- Resource metrics ---
  cpu_usage?: string;
  memory_usage?: string;
  tokens?: string;
  created_at?: string;
  age?: string;
  swarm_id?: string;
  parent_queen_id?: string;
  capabilities?: string;

  // --- Navigation ---
  page_url?: string;
  pageUrl?: string;
  url?: string;
  file_path?: string;
  filePath?: string;
  path?: string;

  // --- Content metrics ---
  fileSize?: string;
  role?: string;

  // Any additional untyped fields from the backend
  [key: string]: any;
}

export interface Node {
  id: string;
  label: string;
  position: {
    x: number;
    y: number;
    z: number;
  };
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


async function decompressZlib(compressedData: ArrayBuffer): Promise<ArrayBuffer> {
  if (typeof DecompressionStream !== 'undefined') {
    try {
      const cs = new DecompressionStream('deflate-raw');
      const writer = cs.writable.getWriter();
      writer.write(new Uint8Array(compressedData.slice(2))); 
      writer.close();

      const output = [];
      const reader = cs.readable.getReader();

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        output.push(value);
      }

      const totalLength = output.reduce((acc, arr) => acc + arr.length, 0);
      const result = new Uint8Array(totalLength);
      let offset = 0;

      for (const arr of output) {
        result.set(arr, offset);
        offset += arr.length;
      }

      return result.buffer;
    } catch (error) {
      workerLogger.error('Decompression failed:', error);
      throw error;
    }
  }
  throw new Error('DecompressionStream not available');
}


function isZlibCompressed(data: ArrayBuffer): boolean {
  if (data.byteLength < 2) return false;
  const view = new Uint8Array(data);
  return view[0] === 0x78 && [0x01, 0x5E, 0x9C, 0xDA].includes(view[1]);
}


// Force-directed physics settings — retained for API compatibility.
// Client-side force simulation is REMOVED: the server (Rust/CUDA GPU physics)
// is the single source of truth for all graph types. The client only performs
// optimistic interpolation/tweening toward server-provided target positions.
export interface ForcePhysicsSettings {
  repulsionStrength: number;
  attractionStrength: number;
  centerGravity: number;
  damping: number;
  maxVelocity: number;
  idealEdgeLength: number;
  theta: number;
  enabled: boolean;
  alpha: number;
  alphaDecay: number;
  alphaMin: number;
  clusterStrength: number;
  enableClustering: boolean;
}

class GraphWorker {
  private graphData: GraphData = { nodes: [], edges: [] };
  private nodeIdMap: Map<string, number> = new Map();
  private reverseNodeIdMap: Map<number, string> = new Map();
  private graphType: 'logseq' | 'visionflow' = 'logseq';


  private nodeIndexMap: Map<string, number> = new Map();

  private currentPositions: Float32Array | null = null;
  private targetPositions: Float32Array | null = null;
  private velocities: Float32Array | null = null;
  private pinnedNodeIds: Set<number> = new Set();
  private physicsSettings = {
    springStrength: 0.001,
    damping: 0.98,
    maxVelocity: 0.5,
    updateThreshold: 0.05,
  };


  // Server physics is ALWAYS authoritative — all graph types use server positions.
  // This flag is kept for API compatibility but always returns true.
  private useServerPhysics: boolean = true;

  // Client-side tweening: interpolates toward server-computed target positions.
  // lerpFactor = 1 - lerpBase^dt. At 60fps (dt≈0.016):
  //   lerpBase=0.15 → factor≈0.028 (sluggish, takes ~2s to converge)
  //   lerpBase=0.001 → factor≈0.10 (snappy, converges in ~0.3s)
  //   lerpBase=0.0001 → factor≈0.14 (near-instant)
  private tweenSettings = {
    enabled: true,
    lerpBase: 0.003,      // ~200ms smooth settle — clients interpolate to server targets
    snapThreshold: 0.05,  // Snap when within 0.05 units (sub-pixel)
    maxDivergence: 50.0,  // Force snap on large jumps (topology change)
  };
  private positionBuffer: SharedArrayBuffer | null = null;
  private positionView: Float32Array | null = null;


  private frameCount: number = 0;
  private binaryUpdateCount: number = 0;
  private lastBinaryUpdate: number = 0;

  // Retained for API compatibility — client-side force simulation is removed.
  // Physics settings are now sent to the server via REST API.
  private forcePhysics: ForcePhysicsSettings = {
    repulsionStrength: 500,
    attractionStrength: 0.05,
    centerGravity: 0.01,
    damping: 0.85,
    maxVelocity: 5.0,
    idealEdgeLength: 30,
    theta: 0.8,
    enabled: true,
    alpha: 1.0,
    alphaDecay: 0.0228,
    alphaMin: 0.001,
    clusterStrength: 0.3,
    enableClustering: true,
  };

  // Idempotency guard: skip updateSettings when physics values haven't changed
  private _lastPhysicsKey: string = '';

  // Edge lookup for O(1) neighbor access (kept for graph structure queries)
  private edgeSourceMap: Map<string, string[]> = new Map();
  private edgeTargetMap: Map<string, string[]> = new Map();

  // Pre-allocated buffer for binary position output (reused every processBinaryData call)
  private binaryOutputBuffer: Float32Array | null = null;
  private binaryOutputBufferSize: number = 0;

  // ADR-061: per-node analytics (cluster_id, community_id, anomaly_score) no
  // longer ride the per-frame binary stream. They arrive on the
  // `analytics_update` text-message channel and are merged into the main-thread
  // `useAnalyticsStore` (Zustand). The worker keeps a degree-based fallback
  // anomaly + Louvain community computation for graphs where the server has
  // not yet emitted analytics — the result is still surfaced via
  // getAnalyticsBuffer() as a Float32Array of [clusterId, anomalyScore,
  // communityId] per node.
  private analyticsBuffer: Float32Array | null = null;

  // FIX 4: JSON/binary race guard — binary frames arriving before setGraphData()
  // are queued here and replayed once graph data is loaded. Without this, binary
  // position updates for unknown node IDs are silently dropped.
  private graphDataLoaded: boolean = false;
  private pendingBinaryFrames: ArrayBuffer[] = [];

  // FIX 2: Track unknown node IDs from binary stream. If binary positions arrive
  // for nodes not in reverseNodeIdMap, it means the server added nodes (via graph
  // mutation) that the client doesn't know about. The main thread can poll this
  // to trigger a REST re-fetch of /api/graph/data.
  private unknownNodeIds: Set<number> = new Set();
  private lastUnknownNodeAlert: number = 0;

  
  async initialize(): Promise<void> {
    workerLogger.info('Initialize method called');
    return Promise.resolve();
  }
  
  
  async setGraphType(type: 'logseq' | 'visionflow'): Promise<void> {
    this.graphType = type;
    // All graph types use server-authoritative physics.
    // The server (Rust/CUDA GPU) is the single source of truth for positions.
    // Client only performs optimistic tweening toward server targets.
    this.useServerPhysics = true;
    workerLogger.info(`Graph type set to ${type} - using SERVER-AUTHORITATIVE physics (single source of truth)`);
  }


  async setGraphData(data: GraphData): Promise<void> {
    this.graphData = {
      nodes: data.nodes.map(node => this.ensureNodeHasValidPosition(node)),
      edges: data.edges
    };

    // Capture old state BEFORE clearing maps — needed for position preservation.
    // The nodeIndexMap maps nodeId → array index in the OLD position buffers.
    const nodeCount = data.nodes.length;
    const oldCurrentPos = this.currentPositions;
    const oldTargetPos = this.targetPositions;
    const oldNodeIndexMap = new Map(this.nodeIndexMap);

    this.nodeIdMap.clear();
    this.reverseNodeIdMap.clear();
    this.nodeIndexMap.clear();
    this.graphData.nodes.forEach((node, index) => {
        // Normalize node ID to string for consistent Map lookups.
        // Edge source/target are always strings — keys must match.
        const nodeId = String(node.id);
        node.id = nodeId;

        // Server sends compact IDs (0..N-1) as the node.id directly.
        // No wireId indirection needed — node.id IS the compact wire ID.
        const numericId = parseInt(nodeId, 10);

        if (!isNaN(numericId) && numericId >= 0 && numericId <= 0xFFFFFFFF) {
            this.nodeIdMap.set(nodeId, numericId);
            this.reverseNodeIdMap.set(numericId, nodeId);
        } else {
            const mappedId = findFreeMappedId(nodeId, this.reverseNodeIdMap);
            this.nodeIdMap.set(nodeId, mappedId);
            this.reverseNodeIdMap.set(mappedId, nodeId);
        }
        this.nodeIndexMap.set(nodeId, index);
    });

    // Build edge adjacency maps for O(1) neighbor lookup
    this.edgeSourceMap.clear();
    this.edgeTargetMap.clear();
    for (const edge of data.edges) {
      // Source -> targets
      if (!this.edgeSourceMap.has(edge.source)) {
        this.edgeSourceMap.set(edge.source, []);
      }
      this.edgeSourceMap.get(edge.source)!.push(edge.target);

      // Target -> sources (for bidirectional edge springs)
      if (!this.edgeTargetMap.has(edge.target)) {
        this.edgeTargetMap.set(edge.target, []);
      }
      this.edgeTargetMap.get(edge.target)!.push(edge.source);
    }

    // Preserve positions for nodes that already exist (prevents reset on
    // initialGraphLoad / filter-update / reconnect). Only allocate fresh
    // positions for genuinely new nodes.

    const newCurrentPositions = new Float32Array(nodeCount * 3);
    const newTargetPositions = new Float32Array(nodeCount * 3);
    const newVelocities = new Float32Array(nodeCount * 3);

    let preservedCount = 0;
    this.graphData.nodes.forEach((node, index) => {
      const i3 = index * 3;
      const oldIndex = oldNodeIndexMap.get(String(node.id));

      if (oldIndex !== undefined && oldCurrentPos && oldCurrentPos.length > oldIndex * 3 + 2) {
        // Existing node — keep its interpolated position
        const oi3 = oldIndex * 3;
        newCurrentPositions[i3] = oldCurrentPos[oi3];
        newCurrentPositions[i3 + 1] = oldCurrentPos[oi3 + 1];
        newCurrentPositions[i3 + 2] = oldCurrentPos[oi3 + 2];

        if (oldTargetPos && oldTargetPos.length > oi3 + 2) {
          newTargetPositions[i3] = oldTargetPos[oi3];
          newTargetPositions[i3 + 1] = oldTargetPos[oi3 + 1];
          newTargetPositions[i3 + 2] = oldTargetPos[oi3 + 2];
        } else {
          newTargetPositions[i3] = newCurrentPositions[i3];
          newTargetPositions[i3 + 1] = newCurrentPositions[i3 + 1];
          newTargetPositions[i3 + 2] = newCurrentPositions[i3 + 2];
        }
        preservedCount++;
      } else {
        // New node — use data position (defensive: handle missing position)
        const pos = node.position || node as unknown as { x?: number; y?: number; z?: number };
        let px = Number(pos.x) || 0;
        let py = Number(pos.y) || 0;
        let pz = Number(pos.z) || 0;

        // Generate deterministic Fibonacci sphere position for nodes at origin
        // (prevents all nodes piling up at (0,0,0) when server hasn't run physics yet)
        if (px === 0 && py === 0 && pz === 0) {
          const goldenAngle = Math.PI * (3 - Math.sqrt(5));
          const theta = index * goldenAngle;
          const t = 1 - (index / Math.max(nodeCount, 1)) * 2; // -1 to 1
          const r = Math.sqrt(1 - t * t);
          const spread = 15;
          px = Math.cos(theta) * r * spread;
          py = t * spread;
          pz = Math.sin(theta) * r * spread;
        }

        newCurrentPositions[i3] = px;
        newCurrentPositions[i3 + 1] = py;
        newCurrentPositions[i3 + 2] = pz;
        newTargetPositions[i3] = px;
        newTargetPositions[i3 + 1] = py;
        newTargetPositions[i3 + 2] = pz;
      }
    });

    this.currentPositions = newCurrentPositions;
    this.targetPositions = newTargetPositions;
    this.velocities = newVelocities;

    // Allocate per-node analytics buffer (3 floats per node: clusterId, anomalyScore, communityId)
    this.analyticsBuffer = new Float32Array(nodeCount * 3);

    // Defer client-side analytics to avoid blocking setGraphData response.
    // Louvain + anomaly scoring on 13K nodes takes 100-500ms; running it
    // synchronously here starves the Comlink message queue and delays the
    // first render frame.
    setTimeout(() => this.recomputeAnalytics(), 0);

    // Write preserved positions back into graphData so that:
    // 1) Any consumer reading node.position gets current values, not stale DB positions
    // 2) Future setGraphData() calls have up-to-date fallback data
    for (let i = 0; i < nodeCount; i++) {
      const i3 = i * 3;
      this.graphData.nodes[i].position = {
        x: newCurrentPositions[i3],
        y: newCurrentPositions[i3 + 1],
        z: newCurrentPositions[i3 + 2],
      };
    }

    workerLogger.info(`Initialized ${this.graphType} graph with ${nodeCount} nodes, ${data.edges.length} edges (${preservedCount} positions preserved, server-authoritative physics)`);

    // Sync initial positions to SharedArrayBuffer so main thread
    // has real positions before the first tick() completes.
    this.syncToSharedBuffer();

    // FIX 4: Mark graph data as loaded and replay any binary frames that
    // arrived before setGraphData(). This prevents position data loss during
    // the race between JSON graph load and binary position stream.
    this.graphDataLoaded = true;
    if (this.pendingBinaryFrames.length > 0) {
      workerLogger.info(`Replaying ${this.pendingBinaryFrames.length} queued binary frames`);
      const queued = this.pendingBinaryFrames;
      this.pendingBinaryFrames = [];
      for (const frame of queued) {
        await this.processBinaryData(frame);
      }
    }
  }

  
  async setupSharedPositions(buffer: SharedArrayBuffer): Promise<void> {
    this.positionBuffer = buffer;
    this.positionView = new Float32Array(buffer);
    workerLogger.info(`SharedArrayBuffer set up with ${buffer.byteLength} bytes`);
  }

  /** Copy currentPositions into the SharedArrayBuffer so the main thread can read synchronously. */
  private syncToSharedBuffer(): void {
    if (this.positionView && this.currentPositions) {
      const len = Math.min(this.currentPositions.length, this.positionView.length);
      this.positionView.set(this.currentPositions.subarray(0, len));
    }
  }

  
  async updateSettings(settings: Record<string, unknown>): Promise<void> {
    // Extract only physics-relevant settings via nested Record traversal
    const vis = settings?.visualisation as Record<string, unknown> | undefined;
    const graphs = vis?.graphs as Record<string, Record<string, unknown>> | undefined;
    const graphSettings = graphs?.[this.graphType]?.physics as Record<string, unknown> | undefined ??
                         vis?.physics as Record<string, unknown> | undefined;
    const vfPhysics = (this.graphType === 'visionflow')
      ? (graphs?.visionflow?.physics as Record<string, unknown> | undefined ?? {})
      : null;

    // Idempotency: bail if physics values haven't changed (prevents unnecessary
    // parameter resets that can disrupt a running force-directed simulation)
    const physicsKey = JSON.stringify({ gs: graphSettings, vf: vfPhysics });
    if (physicsKey === this._lastPhysicsKey) return;
    this._lastPhysicsKey = physicsKey;

    this.physicsSettings = {
      springStrength: (graphSettings?.springStrength as number | undefined) ?? 0.001,
      damping: (graphSettings?.damping as number | undefined) ?? 0.98,
      maxVelocity: (graphSettings?.maxVelocity as number | undefined) ?? 0.5,
      updateThreshold: (graphSettings?.updateThreshold as number | undefined) ?? 0.05
    };

    // Also extract per-graph tweening settings if present in the settings payload
    const tweening = graphs?.[this.graphType]?.tweening as Record<string, unknown> | undefined;
    if (tweening) {
      this.setTweeningSettings({
        enabled: tweening.enabled as boolean | undefined,
        lerpBase: tweening.lerpBase as number | undefined,
        snapThreshold: tweening.snapThreshold as number | undefined,
        maxDivergence: tweening.maxDivergence as number | undefined,
      });
    }

    // Physics settings for visionflow are now routed to the server via REST API.
    // The client stores them for reference but does not run local force simulation.
  }

  
  async processBinaryData(data: ArrayBuffer): Promise<Float32Array> {
    // FIX 4: If graph data hasn't been loaded yet, queue binary frames for
    // later replay. Without this guard, binary position updates arrive before
    // reverseNodeIdMap is populated, causing all updates to be silently dropped.
    if (!this.graphDataLoaded) {
      this.pendingBinaryFrames.push(data.slice(0)); // defensive copy
      workerLogger.info(`Binary frame queued (graphData not yet loaded), queue size: ${this.pendingBinaryFrames.length}`);
      return new Float32Array(0);
    }

    // All graph types process binary position updates from the server.
    // Server is the single source of truth for positions.

    this.binaryUpdateCount = (this.binaryUpdateCount || 0) + 1;
    this.lastBinaryUpdate = Date.now();

    if (isZlibCompressed(data)) {
      data = await decompressZlib(data);
    }

    // ADR-061: single 28 B/node decoder. Frame := [u8 0x42][u64 seq][N × 28 B node].
    // No flag bits, no version dispatch, no per-frame analytics.
    const frame = decodePositionFrame(data);
    if (!frame) {
      return new Float32Array(0);
    }

    const nodeCount = frame.nodes.size;

    // Reuse binary output buffer, only reallocate if size changed
    const requiredBinarySize = nodeCount * 4;
    if (!this.binaryOutputBuffer || this.binaryOutputBufferSize !== requiredBinarySize) {
      this.binaryOutputBuffer = new Float32Array(requiredBinarySize);
      this.binaryOutputBufferSize = requiredBinarySize;
    }
    const positionArray = this.binaryOutputBuffer;

    let unknownCount = 0;
    let outIdx = 0;
    for (const update of frame.nodes.values()) {
      const nodeId = update.nodeId;
      const stringNodeId = this.reverseNodeIdMap.get(nodeId);
      if (!stringNodeId) {
        // FIX 2: Track unknown node IDs from binary stream for REST re-fetch
        this.unknownNodeIds.add(nodeId);
        unknownCount++;
      } else {
        const nodeIndex = this.nodeIndexMap.get(stringNodeId);
        if (nodeIndex !== undefined && !this.pinnedNodeIds.has(nodeId)) {
          const i3 = nodeIndex * 3;
          // Server is authoritative; SET both target and current so SAB
          // immediately reflects server state. Tweening in tick() smooths
          // subsequent micro-adjustments.
          this.targetPositions![i3] = update.position.x;
          this.targetPositions![i3 + 1] = update.position.y;
          this.targetPositions![i3 + 2] = update.position.z;
          this.currentPositions![i3] = update.position.x;
          this.currentPositions![i3 + 1] = update.position.y;
          this.currentPositions![i3 + 2] = update.position.z;
        }
      }

      const arrayOffset = outIdx * 4;
      positionArray[arrayOffset] = nodeId;
      positionArray[arrayOffset + 1] = update.position.x;
      positionArray[arrayOffset + 2] = update.position.y;
      positionArray[arrayOffset + 3] = update.position.z;
      outIdx++;
    }

    // FIX 2: Log unknown nodes and signal need for REST re-fetch (throttled to once per 5s)
    if (unknownCount > 0) {
      const now = Date.now();
      if (now - this.lastUnknownNodeAlert > 5000) {
        this.lastUnknownNodeAlert = now;
        workerLogger.warn(
          `Binary stream contains ${unknownCount} unknown node IDs (total tracked: ${this.unknownNodeIds.size}). ` +
          `Graph mutation likely occurred — client should re-fetch /api/graph/data.`
        );
      }
    }

    // Push target positions to SAB immediately so main thread sees fresh data
    // even if tick() is delayed or coalesced by the concurrency guard.
    this.syncToSharedBuffer();

    return positionArray;
  }

  /**
   * Return current node positions as Float32Array [x,y,z,x,y,z,...].
   * Used as non-SAB fallback — called after processBinaryData to give
   * the main thread a synchronous-accessible position snapshot.
   */
  async getCurrentPositions(): Promise<Float32Array> {
    return this.currentPositions ? new Float32Array(this.currentPositions) : new Float32Array(0);
  }

  /**
   * FIX 2: Returns true if the binary stream has received positions for node IDs
   * not present in the current graph data. The main thread should poll this
   * (e.g. every few seconds) and trigger a REST re-fetch when true.
   * Clears the unknown set after reading to avoid redundant re-fetches.
   */
  async hasUnknownNodes(): Promise<boolean> {
    const has = this.unknownNodeIds.size > 0;
    if (has) {
      workerLogger.info(`Clearing ${this.unknownNodeIds.size} unknown node IDs after check`);
      this.unknownNodeIds.clear();
    }
    return has;
  }


  async getGraphData(): Promise<GraphData> {
    if (this.currentPositions) {
      this.graphData.nodes.forEach((node, i) => {
        const i3 = i * 3;
        if (i3 + 2 < this.currentPositions!.length) {
          node.position = {
            x: this.currentPositions![i3],
            y: this.currentPositions![i3 + 1],
            z: this.currentPositions![i3 + 2]
          };
        }
      });
    }
    return this.graphData;
  }


  async updateNode(node: Node): Promise<void> {
    const existingIndex = this.nodeIndexMap.get(node.id);

    if (existingIndex !== undefined) {
      this.graphData.nodes[existingIndex] = { ...this.graphData.nodes[existingIndex], ...node };
    } else {
      const newIndex = this.graphData.nodes.length;
      this.graphData.nodes.push(this.ensureNodeHasValidPosition(node));


      // Node IDs are compact (0..N-1) from the server — parse directly
      const numericId = parseInt(node.id, 10);
      if (!isNaN(numericId)) {
        this.nodeIdMap.set(node.id, numericId);
        this.reverseNodeIdMap.set(numericId, node.id);
      } else {
        const mappedId = findFreeMappedId(node.id, this.reverseNodeIdMap);
        this.nodeIdMap.set(node.id, mappedId);
        this.reverseNodeIdMap.set(mappedId, node.id);
      }
      this.nodeIndexMap.set(node.id, newIndex);
    }
  }


  async removeNode(nodeId: string): Promise<void> {
    const numericId = this.nodeIdMap.get(nodeId);

    this.graphData.nodes = this.graphData.nodes.filter(node => node.id !== nodeId);
    this.graphData.edges = this.graphData.edges.filter(
      edge => edge.source !== nodeId && edge.target !== nodeId
    );

    if (numericId !== undefined) {
      this.nodeIdMap.delete(nodeId);
      this.reverseNodeIdMap.delete(numericId);
    }

    this.reallocateNodeArraysAfterRemoval();
  }

  private reallocateNodeArraysAfterRemoval(): void {
    const nodeCount = this.graphData.nodes.length;
    const oldPos = this.currentPositions;
    const oldTarget = this.targetPositions;
    const oldVel = this.velocities;
    const oldIndexMap = new Map(this.nodeIndexMap);

    const newCurrent = new Float32Array(nodeCount * 3);
    const newTarget = new Float32Array(nodeCount * 3);
    const newVel = new Float32Array(nodeCount * 3);

    this.nodeIndexMap.clear();
    this.graphData.nodes.forEach((node, newIndex) => {
      this.nodeIndexMap.set(node.id, newIndex);
      const oldIndex = oldIndexMap.get(node.id);
      if (oldIndex !== undefined && oldPos && oldTarget && oldVel) {
        for (let k = 0; k < 3; ++k) {
          newCurrent[newIndex * 3 + k] = oldPos[oldIndex * 3 + k];
          newTarget[newIndex * 3 + k] = oldTarget[oldIndex * 3 + k];
          newVel[newIndex * 3 + k] = oldVel[oldIndex * 3 + k];
        }
      }
    });

    this.currentPositions = newCurrent;
    this.targetPositions = newTarget;
    this.velocities = newVel;
  }

  
  async createBinaryData(nodes: BinaryNodeData[]): Promise<ArrayBuffer> {
    return createBinaryNodeData(nodes);
  }

  private ensureNodeHasValidPosition(node: Node): Node {
    if (!node.position) {
      return { ...node, position: { x: 0, y: 0, z: 0 } };
    }

    return {
      ...node,
      position: {
        x: typeof node.position.x === 'number' ? node.position.x : 0,
        y: typeof node.position.y === 'number' ? node.position.y : 0,
        z: typeof node.position.z === 'number' ? node.position.z : 0
      }
    };
  }

  // Client-side force computation (computeForces, applyForces) REMOVED.
  // The server (Rust/CUDA GPU) handles all force-directed layout.
  // Client only performs optimistic interpolation toward server targets.

  
  async pinNode(nodeId: number): Promise<void> { this.pinnedNodeIds.add(nodeId); }
  async unpinNode(nodeId: number): Promise<void> { this.pinnedNodeIds.delete(nodeId); }

  
  async setUseServerPhysics(useServer: boolean): Promise<void> {
    // Server physics is always authoritative. This method is kept for API
    // compatibility but always enforces server mode.
    this.useServerPhysics = true;
    if (!useServer) {
      workerLogger.warn('Client-side physics requested but server is authoritative — ignoring');
    }
    workerLogger.info('Physics mode: server-authoritative (single source of truth)');
  }


  async getPhysicsMode(): Promise<boolean> {
    return this.useServerPhysics;
  }

  /** Update client-side tweening configuration (does NOT affect server physics). */
  async setTweeningSettings(settings: Partial<{
    enabled: boolean;
    lerpBase: number;
    snapThreshold: number;
    maxDivergence: number;
  }>): Promise<void> {
    if (settings.enabled !== undefined) this.tweenSettings.enabled = settings.enabled;
    if (settings.lerpBase !== undefined) this.tweenSettings.lerpBase = Math.max(0.0001, Math.min(0.5, settings.lerpBase));
    if (settings.snapThreshold !== undefined) this.tweenSettings.snapThreshold = Math.max(0.01, settings.snapThreshold);
    if (settings.maxDivergence !== undefined) this.tweenSettings.maxDivergence = Math.max(1, settings.maxDivergence);
    workerLogger.info(`Tweening settings updated: lerpBase=${this.tweenSettings.lerpBase}, snap=${this.tweenSettings.snapThreshold}`);
  }

  /**
   * Compute anomaly scores using degree-based z-score outlier detection.
   * Populates analyticsBuffer[i*3 + 1] for each node.
   */
  private computeAnomalyScores(): void {
    if (!this.analyticsBuffer || this.graphData.nodes.length === 0) return;

    // Build degree map
    const degreeMap = new Map<string, number>();
    for (const node of this.graphData.nodes) {
      degreeMap.set(node.id, 0);
    }
    for (const edge of this.graphData.edges) {
      degreeMap.set(edge.source, (degreeMap.get(edge.source) ?? 0) + 1);
      degreeMap.set(edge.target, (degreeMap.get(edge.target) ?? 0) + 1);
    }

    // Compute mean and stddev
    const degrees = Array.from(degreeMap.values());
    const mean = degrees.reduce((a, b) => a + b, 0) / (degrees.length || 1);
    const variance = degrees.reduce((a, d) => a + (d - mean) ** 2, 0) / (degrees.length || 1);
    const stddev = Math.sqrt(variance) || 1; // avoid division by zero

    // Assign z-score normalized to 0-1
    for (let i = 0; i < this.graphData.nodes.length; i++) {
      const degree = degreeMap.get(this.graphData.nodes[i].id) ?? 0;
      const zScore = Math.abs(degree - mean) / stddev;
      // Normalize: z-score of 3+ maps to 1.0
      this.analyticsBuffer[i * 3 + 1] = Math.min(zScore / 3, 1.0);
    }
  }

  /**
   * Compute communities using simplified Louvain modularity optimization.
   * Populates analyticsBuffer[i*3 + 2] for each node.
   */
  private computeCommunities(): void {
    if (!this.analyticsBuffer || this.graphData.nodes.length === 0) return;

    const n = this.graphData.nodes.length;
    const nodeIndex = new Map<string, number>();
    for (let i = 0; i < n; i++) {
      nodeIndex.set(this.graphData.nodes[i].id, i);
    }

    // Build adjacency (all weight=1 for unweighted)
    const adj: number[][] = Array.from({ length: n }, () => []);
    let totalEdges = 0;
    for (const edge of this.graphData.edges) {
      const si = nodeIndex.get(edge.source);
      const ti = nodeIndex.get(edge.target);
      if (si !== undefined && ti !== undefined && si !== ti) {
        adj[si].push(ti);
        adj[ti].push(si);
        totalEdges++;
      }
    }

    if (totalEdges === 0) {
      // No edges -- each node is its own community
      for (let i = 0; i < n; i++) {
        this.analyticsBuffer[i * 3 + 2] = i + 1;
      }
      return;
    }

    const m2 = totalEdges * 2; // 2 * number of edges (each counted once above)
    // Degree of each node
    const degree = adj.map(neighbors => neighbors.length);

    // Initialize: each node in its own community
    const community = new Array<number>(n);
    for (let i = 0; i < n; i++) community[i] = i;

    // Community total degree (sum of degrees of all nodes in community)
    const communityTotalDegree = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      communityTotalDegree[i] = degree[i];
    }

    // Iterative optimization (max 10 passes)
    const MAX_PASSES = 10;
    for (let pass = 0; pass < MAX_PASSES; pass++) {
      let moved = false;

      for (let i = 0; i < n; i++) {
        const currentComm = community[i];
        const ki = degree[i];
        if (ki === 0) continue; // isolated node

        // Count edges to each neighboring community
        const neighborComms = new Map<number, number>();
        let edgesToCurrent = 0;
        for (const j of adj[i]) {
          const cj = community[j];
          neighborComms.set(cj, (neighborComms.get(cj) ?? 0) + 1);
          if (cj === currentComm) edgesToCurrent++;
        }

        // Try moving to best neighbor community
        let bestComm = currentComm;
        let bestGain = 0;

        // Modularity gain of removing i from current community
        const removeLoss = edgesToCurrent - (ki * (communityTotalDegree[currentComm] - ki)) / m2;

        for (const [comm, edgesToComm] of neighborComms) {
          if (comm === currentComm) continue;
          // Modularity gain of adding i to comm
          const addGain = edgesToComm - (ki * communityTotalDegree[comm]) / m2;
          const totalGain = addGain - removeLoss;
          if (totalGain > bestGain) {
            bestGain = totalGain;
            bestComm = comm;
          }
        }

        if (bestComm !== currentComm && bestGain > 1e-10) {
          // Move node
          communityTotalDegree[currentComm] -= ki;
          communityTotalDegree[bestComm] += ki;
          community[i] = bestComm;
          moved = true;
        }
      }

      if (!moved) break;
    }

    // Renumber communities to 1-based contiguous IDs
    const commMap = new Map<number, number>();
    let nextId = 1;
    for (let i = 0; i < n; i++) {
      const c = community[i];
      if (!commMap.has(c)) {
        commMap.set(c, nextId++);
      }
      this.analyticsBuffer[i * 3 + 2] = commMap.get(c)!;
    }
  }

  /**
   * Recompute all client-side analytics (anomaly + community detection).
   * Called when graph data changes or when analytics toggles are enabled.
   * Only computes if the server hasn't already provided analytics data.
   */
  recomputeAnalytics(): void {
    if (!this.analyticsBuffer) return;

    // Check if server already populated analytics (any non-zero values in buffer)
    // Check clusterId (i), anomalyScore (i+1), and communityId (i+2)
    let hasServerData = false;
    for (let i = 0; i < this.analyticsBuffer.length; i += 3) {
      if (this.analyticsBuffer[i] > 0 || this.analyticsBuffer[i + 1] > 0 || this.analyticsBuffer[i + 2] > 0) {
        hasServerData = true;
        break;
      }
    }

    // Only compute client-side if server didn't provide data
    if (!hasServerData) {
      workerLogger.info('Computing client-side analytics (anomaly + community detection)');
      this.computeAnomalyScores();
      this.computeCommunities();
    }
  }

  /**
   * Return per-node client-fallback analytics computed by this worker (degree
   * z-score + Louvain communities). Layout: Float32Array of [clusterId,
   * anomalyScore, communityId] per node, indexed by node position in
   * graphData.nodes (i.e., index * 3 + offset).
   *
   * NOTE (ADR-061): server-side analytics no longer ride the per-frame binary
   * stream — they arrive on the `analytics_update` side stream and merge into
   * useAnalyticsStore on the main thread. This buffer remains as a graceful
   * fallback for graphs that have not yet received a server analytics update.
   */
  async getAnalyticsBuffer(): Promise<Float32Array> {
    return this.analyticsBuffer ?? new Float32Array(0);
  }

  /**
   * Reheat the force simulation (restart physics from current positions).
   * Call this when user drags a node or wants to re-layout.
   */
  async reheatSimulation(alpha: number = 1.0): Promise<void> {
    this.forcePhysics.alpha = alpha;
    workerLogger.info(`Simulation reheated to alpha=${alpha}`);
  }

  /**
   * Update force-directed physics settings from UI.
   */
  async updateForcePhysicsSettings(settings: Partial<ForcePhysicsSettings>): Promise<void> {
    Object.assign(this.forcePhysics, settings);
    workerLogger.info('Force physics settings updated', this.forcePhysics);
  }

  /**
   * Get current force physics settings.
   */
  async getForcePhysicsSettings(): Promise<ForcePhysicsSettings> {
    return { ...this.forcePhysics };
  }
  
  async updateUserDrivenNodePosition(nodeId: number, position: Vec3): Promise<void> {
    const stringNodeId = this.reverseNodeIdMap.get(nodeId);
    if (stringNodeId) {
      const nodeIndex = this.nodeIndexMap.get(stringNodeId);
      if (nodeIndex !== undefined) {
        const i3 = nodeIndex * 3;

        this.currentPositions![i3] = position.x;
        this.currentPositions![i3 + 1] = position.y;
        this.currentPositions![i3 + 2] = position.z;
        this.targetPositions![i3] = position.x;
        this.targetPositions![i3 + 1] = position.y;
        this.targetPositions![i3 + 2] = position.z;

        this.velocities!.fill(0, i3, i3 + 3);

        // User drag position is applied optimistically on the client.
        // The position should also be sent to the server via REST API
        // so the server can apply it as a constraint and rebroadcast.
      }
    }
  }

  
  async tick(deltaTime: number): Promise<Float32Array> {
    if (!this.currentPositions || !this.targetPositions || !this.velocities) {
      return new Float32Array(0);
    }
    // Capture locals after null guard so TS narrows them as non-null
    const curPos = this.currentPositions;
    const tgtPos = this.targetPositions;
    const vel = this.velocities;

    // Clamp delta time for stability
    const dt = Math.min(deltaTime, 0.033); // Max 30fps equivalent

    this.frameCount = (this.frameCount || 0) + 1;

    // ====== SERVER-AUTHORITATIVE PHYSICS — Interpolate toward target positions ======
    // All graph types (visionflow, logseq) use server-computed positions as the
    // single source of truth. The client only performs optimistic tweening.
    {
      
      let hasAnyMovement = false;
      for (let i = 0; i < this.graphData.nodes.length && !hasAnyMovement; i++) {
        const i3 = i * 3;
        const dx = Math.abs(tgtPos[i3] - curPos[i3]);
        const dy = Math.abs(tgtPos[i3 + 1] - curPos[i3 + 1]);
        const dz = Math.abs(tgtPos[i3 + 2] - curPos[i3 + 2]);
        if (dx > 0.01 || dy > 0.01 || dz > 0.01) {
          hasAnyMovement = true;
        }
      }


      if (!hasAnyMovement) {
        // Performance: Removed per-frame logging
        this.syncToSharedBuffer();
        return curPos;
      }
      



      // deltaTime is already in seconds (from Three.js useFrame delta)
      // lerpBase and snapThreshold are configurable via ClientTweeningSettings.
      // Lower lerpBase = smoother/slower interpolation. Default 0.001.
      const lerpBase = this.tweenSettings.lerpBase;
      const lerpFactor = 1 - Math.pow(lerpBase, deltaTime); 
      
      
      let totalMovement = 0;
      
      // Performance: Removed interpolation logging - use DEBUG_PHYSICS if needed
      
      for (let i = 0; i < this.graphData.nodes.length; i++) {
        const i3 = i * 3;
        
        
        const nodeId = this.nodeIdMap.get(this.graphData.nodes[i].id);
        if (nodeId !== undefined && this.pinnedNodeIds.has(nodeId)) {
          
          continue;
        }
        
        
        const dx = tgtPos[i3] - curPos[i3];
        const dy = tgtPos[i3 + 1] - curPos[i3 + 1];
        const dz = tgtPos[i3 + 2] - curPos[i3 + 2];
        const distanceSq = dx * dx + dy * dy + dz * dz;
        
        
        
        
        
        
        
        
        
        
        
        
        

        
        
        
        
        
        const snapThreshold = this.tweenSettings.snapThreshold;
        const maxDiv = this.tweenSettings.maxDivergence;

        // Force snap when divergence exceeds maxDivergence (prevents runaway drift)
        if (distanceSq > maxDiv * maxDiv) {
          curPos[i3] = tgtPos[i3];
          curPos[i3 + 1] = tgtPos[i3 + 1];
          curPos[i3 + 2] = tgtPos[i3 + 2];
          vel[i3] = 0;
          vel[i3 + 1] = 0;
          vel[i3 + 2] = 0;
          totalMovement += Math.sqrt(distanceSq);
        } else if (distanceSq < snapThreshold * snapThreshold) {

          const positionChanged = Math.abs(curPos[i3] - tgtPos[i3]) > 0.01 ||
                                 Math.abs(curPos[i3 + 1] - tgtPos[i3 + 1]) > 0.01 ||
                                 Math.abs(curPos[i3 + 2] - tgtPos[i3 + 2]) > 0.01;

          if (positionChanged) {
            totalMovement += Math.sqrt(distanceSq);
            curPos[i3] = tgtPos[i3];
            curPos[i3 + 1] = tgtPos[i3 + 1];
            curPos[i3 + 2] = tgtPos[i3 + 2];
          }

          vel[i3] = 0;
          vel[i3 + 1] = 0;
          vel[i3 + 2] = 0;
        } else {

          const moveX = dx * lerpFactor;
          const moveY = dy * lerpFactor;
          const moveZ = dz * lerpFactor;

          totalMovement += Math.sqrt(moveX * moveX + moveY * moveY + moveZ * moveZ);

          curPos[i3] += moveX;
          curPos[i3 + 1] += moveY;
          curPos[i3 + 2] += moveZ;
        }
        
      }
      
      // Keep graphData.nodes[i].position in sync with currentPositions.
      // This ensures any future setGraphData() / getGraphData() uses the
      // latest interpolated positions — not stale DB values.
      // Only sync every 30 frames (~0.5s at 60fps) to limit overhead.
      if (this.frameCount % 30 === 0) {
        for (let i = 0; i < this.graphData.nodes.length; i++) {
          const i3 = i * 3;
          const node = this.graphData.nodes[i];
          node.position = {
            x: curPos[i3],
            y: curPos[i3 + 1],
            z: curPos[i3 + 2],
          };
        }
      }

      // Always sync to SharedArrayBuffer so main thread reads latest positions
      this.syncToSharedBuffer();

      return curPos;
    }
  }
}

// Expose the worker API using Comlink
const worker = new GraphWorker();
expose(worker);

export type GraphWorkerType = GraphWorker;