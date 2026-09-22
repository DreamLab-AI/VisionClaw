import { expose, transfer } from 'comlink';
import { BinaryNodeData, parseBinaryFrameData, createBinaryNodeData, Vec3 } from '../../../types/binaryProtocol';
import { workerLogger } from './lib/logger';
import { findFreeMappedId } from './lib/id-mapping';
import { isZlibCompressed, decompressZlib } from './lib/compression';
import { tickTween } from './lib/tween';
import { processFrameUpdates, warnUnknownNodes, WORKER_ANALYTICS_STRIDE } from './lib/binary-processor';
import { reallocateAfterRemoval, syncToSharedBuffer, initPositionBuffers } from './lib/buffer-manager';
import {
  Node, Edge, GraphData, ForcePhysicsSettings, TweenSettings, PhysicsSettings,
} from './lib/types';

// Re-export types consumed by the main thread via GraphWorkerType
export type { Node, Edge, GraphData, ForcePhysicsSettings, NodeMetadata } from './lib/types';

class GraphWorker {
  private graphData: GraphData = { nodes: [], edges: [] };
  private nodeIdMap: Map<string, number> = new Map();
  private reverseNodeIdMap: Map<number, string> = new Map();
  private graphType: 'knowledge' | 'visionclaw' = 'knowledge';

  private nodeIndexMap: Map<string, number> = new Map();
  // Pre-cached parallel array of node IDs — kept in sync with nodeIndexMap to
  // avoid allocating a fresh string[] on every tick() call (hot path).
  private nodeIdCache: string[] = [];

  private currentPositions: Float32Array | null = null;
  private targetPositions: Float32Array | null = null;
  private velocities: Float32Array | null = null;
  private pinnedNodeIds: Set<number> = new Set();
  private physicsSettings: PhysicsSettings = {
    springStrength: 0.001,
    damping: 0.98,
    maxVelocity: 0.5,
    updateThreshold: 0.05,
  };

  // Server physics is ALWAYS authoritative — all graph types use server positions.
  // This flag is kept for API compatibility but always returns true.
  private useServerPhysics: boolean = true;

  // Client-side tweening toward server targets. lerpFactor = 1 - lerpBase^dt.
  private tweenSettings: TweenSettings = {
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

  // Self-driven interpolation loop. Server frames arrive at broadcast cadence
  // (~5Hz); the renderer runs at 60fps reading the SAB. Without a worker-side
  // tween tick, positions step once per server frame — visible stutter despite
  // fast rendering. (The old main-thread tick() caller was removed along with
  // client physics, so interpolation must self-drive here.)
  private tweenTimer: ReturnType<typeof setInterval> | null = null;
  private lastTweenTs: number = 0;

  // Retained for API compatibility — server (Rust/CUDA) now owns all force-directed layout.
  private forcePhysics: ForcePhysicsSettings = {
    repulsionStrength: 500, attractionStrength: 0.05, centerGravity: 0.01,
    damping: 0.85, maxVelocity: 5.0, idealEdgeLength: 30, theta: 0.8,
    enabled: true, alpha: 1.0, alphaDecay: 0.0228, alphaMin: 0.001,
    clusterStrength: 0.3, enableClustering: true,
  };

  // Idempotency guard: skip updateSettings when physics values haven't changed
  private _lastPhysicsKey: string = '';

  // Edge lookup for O(1) neighbour access (kept for graph structure queries)
  private edgeSourceMap: Map<string, string[]> = new Map();
  private edgeTargetMap: Map<string, string[]> = new Map();

  // Pre-allocated buffer for binary position output (reused every processBinaryData call)
  private binaryOutputBuffer: Float32Array | null = null;
  private binaryOutputBufferSize: number = 0;

  // Per-node analytics from binary protocol V3 (ADR-031 D2): server is the sole
  // source of truth — no client-side recompute. Indexed by nodeIndex (same order
  // as graphData.nodes), stride WORKER_ANALYTICS_STRIDE (5):
  // [clusterId, anomalyScore, communityId, centrality, ssspDistance].
  // Updated every processBinaryData call from decoded server frames only.
  private analyticsBuffer: Float32Array | null = null;

  // FIX 4: JSON/binary race guard — binary frames arriving before setGraphData()
  // are queued here and replayed once graph data is loaded.
  private graphDataLoaded: boolean = false;
  private pendingBinaryFrames: ArrayBuffer[] = [];

  // FIX 2: Track unknown node IDs from binary stream. If binary positions arrive
  // for nodes not in reverseNodeIdMap, it means the server added nodes (via graph
  // mutation) that the client doesn't know about.
  private unknownNodeIds: Set<number> = new Set();
  private lastUnknownNodeAlert: number = 0;


  async initialize(): Promise<void> {
    workerLogger.info('Initialize method called');
    return Promise.resolve();
  }

  // ADR-2041: `logseq` accepted and normalised to `knowledge` for one release.
  async setGraphType(type: 'knowledge' | 'visionclaw' | 'logseq'): Promise<void> {
    this.graphType = type === 'visionclaw' ? 'visionclaw' : 'knowledge';
    this.useServerPhysics = true;
    workerLogger.info(`Graph type set to ${this.graphType} - using SERVER-AUTHORITATIVE physics (single source of truth)`);
  }

  async setGraphData(data: GraphData): Promise<void> {
    this.graphData = {
      nodes: data.nodes.map(node => this.ensureNodeHasValidPosition(node)),
      edges: data.edges
    };

    // Capture old state BEFORE clearing maps — needed for position preservation.
    const nodeCount = data.nodes.length;
    const oldCurrentPos = this.currentPositions;
    const oldTargetPos = this.targetPositions;
    const oldNodeIndexMap = new Map(this.nodeIndexMap);

    this.nodeIdMap.clear();
    this.reverseNodeIdMap.clear();
    this.nodeIndexMap.clear();
    this.nodeIdCache = new Array(nodeCount);
    this.graphData.nodes.forEach((node, index) => {
      const nodeId = String(node.id);
      node.id = nodeId;

      // Server sends compact IDs (0..N-1) as the node.id directly.
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
      this.nodeIdCache[index] = nodeId;
    });

    // Build edge adjacency maps for O(1) neighbour lookup
    this.edgeSourceMap.clear();
    this.edgeTargetMap.clear();
    for (const edge of data.edges) {
      if (!this.edgeSourceMap.has(edge.source)) this.edgeSourceMap.set(edge.source, []);
      this.edgeSourceMap.get(edge.source)!.push(edge.target);
      if (!this.edgeTargetMap.has(edge.target)) this.edgeTargetMap.set(edge.target, []);
      this.edgeTargetMap.get(edge.target)!.push(edge.source);
    }

    // Initialise position buffers, preserving positions for nodes that existed before
    const { currentPositions: newCurrentPositions, targetPositions: newTargetPositions,
            velocities: newVelocities, preservedCount } =
      initPositionBuffers(this.graphData.nodes, oldCurrentPos, oldTargetPos, oldNodeIndexMap);

    this.currentPositions = newCurrentPositions;
    this.targetPositions  = newTargetPositions;
    this.velocities       = newVelocities;

    // Allocate per-node analytics buffer (stride 5; ADR-031 D2). Server frames
    // are the sole writer — no client-side recompute (ADR-031 D6/D8). Values
    // stay zeroed until the first V3/V5 analytics frame arrives.
    this.analyticsBuffer = new Float32Array(nodeCount * WORKER_ANALYTICS_STRIDE);

    // Write preserved positions back into graphData
    for (let i = 0; i < nodeCount; i++) {
      const i3 = i * 3;
      this.graphData.nodes[i].position = {
        x: newCurrentPositions[i3],
        y: newCurrentPositions[i3 + 1],
        z: newCurrentPositions[i3 + 2],
      };
    }

    workerLogger.info(`Initialized ${this.graphType} graph with ${nodeCount} nodes, ${data.edges.length} edges (${preservedCount} positions preserved, server-authoritative physics)`);

    this.syncToSharedBuffer();

    // FIX 4: Mark graph data as loaded and replay any queued binary frames.
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

  /**
   * D7 binary entry point — ADR-03 Phase 4.
   *
   * SAB mode (positionView set): returns `undefined`; the renderer reads the SAB view directly.
   * Comlink mode: returns a fresh transferred ArrayBuffer with the FULL stride-3,
   *   node-index-ordered currentPositions — the same layout the renderer reads from the
   *   SAB (positions[nodeIndex*3 + {0,1,2}]). Returning the stride-4 [nodeId,x,y,z] update
   *   array here would make the renderer read node-ID fields as coordinates.
   */
  async processBinaryFrame(data: ArrayBuffer): Promise<ArrayBuffer | void> {
    await this.processBinaryData(data);
    if (this.positionView) {
      return;
    }
    const src = this.currentPositions;
    if (!src) return;
    const out = new ArrayBuffer(src.byteLength);
    new Float32Array(out).set(src);
    return transfer(out, [out]) as unknown as ArrayBuffer;
  }

  private syncToSharedBuffer(): void {
    syncToSharedBuffer(this.positionView, this.currentPositions);
  }

  async updateSettings(settings: Record<string, unknown>): Promise<void> {
    const vis = settings?.visualisation as Record<string, unknown> | undefined;
    const graphs = vis?.graphs as Record<string, Record<string, unknown>> | undefined;
    const graphSettings = graphs?.[this.graphType]?.physics as Record<string, unknown> | undefined ??
                         vis?.physics as Record<string, unknown> | undefined;
    const vfPhysics = (this.graphType === 'visionclaw')
      ? (graphs?.visionclaw?.physics as Record<string, unknown> | undefined ?? {})
      : null;

    // Idempotency: bail if physics values haven't changed
    const physicsKey = JSON.stringify({ gs: graphSettings, vf: vfPhysics });
    if (physicsKey === this._lastPhysicsKey) return;
    this._lastPhysicsKey = physicsKey;

    this.physicsSettings = {
      springStrength: (graphSettings?.springStrength as number | undefined) ?? 0.001,
      damping: (graphSettings?.damping as number | undefined) ?? 0.98,
      maxVelocity: (graphSettings?.maxVelocity as number | undefined) ?? 0.5,
      updateThreshold: (graphSettings?.updateThreshold as number | undefined) ?? 0.05
    };

    const tweening = graphs?.[this.graphType]?.tweening as Record<string, unknown> | undefined;
    if (tweening) {
      await this.setTweeningSettings({
        enabled: tweening.enabled as boolean | undefined,
        lerpBase: tweening.lerpBase as number | undefined,
        snapThreshold: tweening.snapThreshold as number | undefined,
        maxDivergence: tweening.maxDivergence as number | undefined,
      });
    }
  }

  async processBinaryData(data: ArrayBuffer): Promise<Float32Array> {
    // FIX 4: queue binary frames that arrive before setGraphData()
    if (!this.graphDataLoaded) {
      this.pendingBinaryFrames.push(data.slice(0));
      workerLogger.info(`Binary frame queued (graphData not yet loaded), queue size: ${this.pendingBinaryFrames.length}`);
      return new Float32Array(0);
    }

    this.binaryUpdateCount = (this.binaryUpdateCount || 0) + 1;
    this.lastBinaryUpdate = Date.now();

    if (isZlibCompressed(data)) {
      data = await decompressZlib(data);
    }

    const frame = parseBinaryFrameData(data);
    const nodeUpdates = frame.nodes;
    const isDelta = frame.type === 'delta';

    // Reuse binary output buffer, only reallocate if size changed
    const requiredBinarySize = nodeUpdates.length * 4;
    if (!this.binaryOutputBuffer || this.binaryOutputBufferSize !== requiredBinarySize) {
      this.binaryOutputBuffer = new Float32Array(requiredBinarySize);
      this.binaryOutputBufferSize = requiredBinarySize;
    }
    const positionArray = this.binaryOutputBuffer;

    // Delegate per-node update loop to binary-processor (zero allocation, typed-array only)
    const unknownCount = processFrameUpdates(
      {
        nodeUpdates,
        isDelta,
        targetPositions: this.targetPositions!,
        analyticsBuffer: this.analyticsBuffer,
        positionArray,
        reverseNodeIdMap: this.reverseNodeIdMap,
        nodeIndexMap: this.nodeIndexMap,
        pinnedNodeIds: this.pinnedNodeIds,
      },
      this.unknownNodeIds,
    );

    // FIX 2: throttled warning for unknown node IDs
    this.lastUnknownNodeAlert = warnUnknownNodes(
      unknownCount, this.unknownNodeIds.size, this.lastUnknownNodeAlert,
    );

    // Server is authoritative: incoming frames land in targetPositions only, and
    // the self-driven tween loop interpolates currentPositions toward them at
    // ~60Hz, publishing to the SAB each tick. The previous hard snap here (D7
    // hotfix for the orphaned tick() caller) made positions step at server
    // broadcast cadence (~5Hz) — the long-standing stutter despite fast
    // rendering. Snap remains only for tweening-disabled mode.
    if (!this.tweenSettings.enabled) {
      if (this.currentPositions && this.currentPositions.length === this.targetPositions!.length) {
        this.currentPositions.set(this.targetPositions!);
      }
      this.syncToSharedBuffer();
    } else {
      this.ensureTweenLoop();
    }

    return positionArray;
  }

  /**
   * Start (idempotently) the worker-internal interpolation loop. Runs at ~60Hz,
   * advancing currentPositions toward targetPositions via tickTween and
   * publishing to the SAB. Stops itself after the layout has fully converged
   * AND no fresh server frame has arrived for 2s, so an idle graph costs
   * nothing; the next incoming frame restarts it. Per-client: this loop is
   * purely local interpolation — multi-client safe by construction.
   */
  private ensureTweenLoop(): void {
    if (this.tweenTimer !== null) return;
    this.lastTweenTs = performance.now();
    let idleTicks = 0;
    this.tweenTimer = setInterval(() => {
      const now = performance.now();
      const dt = Math.min((now - this.lastTweenTs) / 1000, 0.033);
      this.lastTweenTs = now;
      if (!this.currentPositions || !this.targetPositions || !this.velocities) return;
      const result = tickTween({
        curPos: this.currentPositions,
        tgtPos: this.targetPositions,
        vel: this.velocities,
        nodeCount: this.graphData.nodes.length,
        pinnedNodeIds: this.pinnedNodeIds,
        nodeIdMap: this.nodeIdMap,
        nodeIds: this.nodeIdCache,
        tweenSettings: this.tweenSettings,
        deltaTime: dt,
      });
      this.syncToSharedBuffer();
      // lastBinaryUpdate is epoch ms (Date.now), distinct from performance.now.
      if (result.hadMovement || Date.now() - this.lastBinaryUpdate < 2000) {
        idleTicks = 0;
      } else if (++idleTicks > 120) {
        clearInterval(this.tweenTimer!);
        this.tweenTimer = null;
      }
    }, 16);
  }

  /**
   * FIX 2: Returns true if the binary stream has received positions for node IDs
   * not in the current graph data. Clears the unknown set after reading.
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
      this.nodeIdCache[newIndex] = node.id;
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
    if (!this.currentPositions || !this.targetPositions || !this.velocities) return;
    const oldIndexMap = new Map(this.nodeIndexMap);
    const bufs = reallocateAfterRemoval(
      this.graphData,
      { currentPositions: this.currentPositions, targetPositions: this.targetPositions, velocities: this.velocities },
      oldIndexMap,
      this.nodeIndexMap,
      this.nodeIdCache,
    );
    this.currentPositions = bufs.currentPositions;
    this.targetPositions  = bufs.targetPositions;
    this.velocities       = bufs.velocities;
  }

  async createBinaryData(nodes: BinaryNodeData[]): Promise<ArrayBuffer> {
    return createBinaryNodeData(nodes);
  }

  private ensureNodeHasValidPosition(node: Node): Node {
    if (!node.position) return { ...node, position: { x: 0, y: 0, z: 0 } };
    return {
      ...node,
      position: {
        x: typeof node.position.x === 'number' ? node.position.x : 0,
        y: typeof node.position.y === 'number' ? node.position.y : 0,
        z: typeof node.position.z === 'number' ? node.position.z : 0
      }
    };
  }

  async pinNode(nodeId: number): Promise<void> { this.pinnedNodeIds.add(nodeId); }
  async unpinNode(nodeId: number): Promise<void> { this.pinnedNodeIds.delete(nodeId); }

  async setUseServerPhysics(useServer: boolean): Promise<void> {
    this.useServerPhysics = true;
    if (!useServer) {
      workerLogger.warn('Client-side physics requested but server is authoritative — ignoring');
    }
    workerLogger.info('Physics mode: server-authoritative (single source of truth)');
  }

  async getPhysicsMode(): Promise<boolean> {
    return this.useServerPhysics;
  }

  async setTweeningSettings(settings: Partial<TweenSettings>): Promise<void> {
    if (settings.enabled !== undefined) this.tweenSettings.enabled = settings.enabled;
    if (settings.lerpBase !== undefined) this.tweenSettings.lerpBase = Math.max(0.0001, Math.min(0.5, settings.lerpBase));
    if (settings.snapThreshold !== undefined) this.tweenSettings.snapThreshold = Math.max(0.01, settings.snapThreshold);
    if (settings.maxDivergence !== undefined) this.tweenSettings.maxDivergence = Math.max(1, settings.maxDivergence);
    workerLogger.info(`Tweening settings updated: lerpBase=${this.tweenSettings.lerpBase}, snap=${this.tweenSettings.snapThreshold}`);
  }

  recomputeAnalytics(): void {
    // ADR-031 D6/D8: the client-side Louvain/analytics shadow is removed. The
    // server is the sole source of truth; analytics arrive on V3/V5 frames and
    // are written by processFrameUpdates. This RPC is retained as a no-op so the
    // worker proxy contract is unchanged, but it never fabricates client data.
  }

  async getAnalyticsBuffer(): Promise<Float32Array> {
    return this.analyticsBuffer ?? new Float32Array(0);
  }

  async reheatSimulation(alpha: number = 1.0): Promise<void> {
    this.forcePhysics.alpha = alpha;
    workerLogger.info(`Simulation reheated to alpha=${alpha}`);
  }

  async updateForcePhysicsSettings(settings: Partial<ForcePhysicsSettings>): Promise<void> {
    Object.assign(this.forcePhysics, settings);
    workerLogger.info('Force physics settings updated', this.forcePhysics);
  }

  async getForcePhysicsSettings(): Promise<ForcePhysicsSettings> {
    return { ...this.forcePhysics };
  }

  async updateUserDrivenNodePosition(nodeId: number, position: Vec3): Promise<void> {
    const stringNodeId = this.reverseNodeIdMap.get(nodeId);
    if (stringNodeId) {
      const nodeIndex = this.nodeIndexMap.get(stringNodeId);
      if (nodeIndex !== undefined) {
        const i3 = nodeIndex * 3;
        this.currentPositions![i3]     = position.x;
        this.currentPositions![i3 + 1] = position.y;
        this.currentPositions![i3 + 2] = position.z;
        this.targetPositions![i3]      = position.x;
        this.targetPositions![i3 + 1]  = position.y;
        this.targetPositions![i3 + 2]  = position.z;
        this.velocities!.fill(0, i3, i3 + 3);
      }
    }
  }

  async tick(deltaTime: number): Promise<Float32Array> {
    if (!this.currentPositions || !this.targetPositions || !this.velocities) {
      return new Float32Array(0);
    }

    const curPos = this.currentPositions;
    const tgtPos = this.targetPositions;
    const vel    = this.velocities;
    const dt     = Math.min(deltaTime, 0.033); // clamp to max 30fps equivalent

    this.frameCount = (this.frameCount || 0) + 1;

    const result = tickTween({
      curPos,
      tgtPos,
      vel,
      nodeCount: this.graphData.nodes.length,
      pinnedNodeIds: this.pinnedNodeIds,
      nodeIdMap: this.nodeIdMap,
      nodeIds: this.nodeIdCache,
      tweenSettings: this.tweenSettings,
      deltaTime: dt,
    });

    if (!result.hadMovement) {
      this.syncToSharedBuffer();
      return curPos;
    }

    // Sync graphData.nodes[i].position every 30 frames (~0.5s at 60fps)
    if (this.frameCount % 30 === 0) {
      for (let i = 0; i < this.graphData.nodes.length; i++) {
        const i3 = i * 3;
        this.graphData.nodes[i].position = {
          x: curPos[i3],
          y: curPos[i3 + 1],
          z: curPos[i3 + 2],
        };
      }
    }

    this.syncToSharedBuffer();
    return curPos;
  }
}

const worker = new GraphWorker();
expose(worker);

export type GraphWorkerType = GraphWorker;
