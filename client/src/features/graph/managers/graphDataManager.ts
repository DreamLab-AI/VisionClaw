import { createLogger, createErrorMetadata } from '../../../utils/loggerConfig';
import { debugState } from '../../../utils/clientDebugState';
import { useSettingsStore } from '../../../store/settingsStore';
import type { WebSocketAdapter } from '../../../store/websocketStore';
import { binaryProtocol } from '../../../services/BinaryWebSocketProtocol';
import { graphWorkerProxy } from './graphWorkerProxy';
import type { GraphData, Node, Edge } from './graphWorkerProxy';
import { useWorkerErrorStore } from '../../../store/workerErrorStore';

// Sub-modules
import { ensureNodeHasValidPosition, validateNodeMappings, dropLinkedPageStubs } from './dataManager/nodeUtils';
import { buildNodeIdMaps, upsertNodeIdEntry, setDataAndNotify, topologyHash } from './dataManager/topology';
import { ListenerRegistry } from './dataManager/listeners';
import type { GraphType } from '../types/graphTypes';
import { fetchGraphData, scheduleEmptyDataRetry, type GraphTypeFilter } from './dataManager/restClient';
import { handleBinaryFrame, sendNodePositions as _sendNodePositions, enableBinaryUpdates as _enableBinaryUpdates } from './dataManager/wsClient';
import { parseBinaryNodeData, getActualNodeId } from '../../../types/binaryProtocol';

// Re-export types for backward compat
export type { Node, Edge, GraphData, NodeMetadata } from './graphWorkerProxy';
export type GraphNode = Node;

const logger = createLogger('GraphDataManager');

class GraphDataManager {
  private static instance: GraphDataManager;

  // ── WebSocket / binary ──────────────────────────────────────────────────
  private binaryUpdatesEnabled: boolean = false;
  public webSocketService: WebSocketAdapter | null = null;
  private lastBinaryUpdateTime: number = 0;

  // ── Listeners (ADR-03 D5) ───────────────────────────────────────────────
  private listenerRegistry = new ListenerRegistry();

  // ── Topology cache (ADR-03 D5) ─────────────────────────────────────────
  private lastGraphData:     GraphData | null = null;
  private lastGraphDataHash: string | null    = null;

  // ── Node ID maps ────────────────────────────────────────────────────────
  public  nodeIdMap:        Map<string, number> = new Map();
  private reverseNodeIdMap: Map<number, string> = new Map();

  // ── Worker / graph type ─────────────────────────────────────────────────
  private workerInitialized: boolean = false;
  private graphType: 'knowledge' | 'visionclaw' = 'knowledge';
  // Server-side population filter (PRD-018 WS-4). null/'all' = whole graph.
  private graphTypeFilter: GraphTypeFilter = null;
  private workerUnsubscribers: Array<() => void> = [];

  // ── User interaction state ──────────────────────────────────────────────
  private isUserInteracting: boolean = false;
  private interactionTimeoutRef: number | null = null;

  // ── Shared retry timer (T6 + WS back-off share this slot) ──────────────
  private retryTimeout: number | null = null;

  private updateCount: number = 0;

  // ── Live-linkage self-heal state (see maybeSelfHealUnknownNodes) ───────
  private selfHealFrameCounter: number = 0;
  private lastSelfHealRefetch: number = 0;

  private constructor() {
    this.waitForWorker();
  }

  // ── Worker initialisation ─────────────────────────────────────────────

  private async waitForWorker(): Promise<void> {
    try {
      if (debugState.isEnabled()) logger.debug('Waiting for worker to be ready...');

      let attempts = 0;
      const maxAttempts = 1000; // 10 seconds
      while (!graphWorkerProxy.isReady() && attempts < maxAttempts) {
        await new Promise(resolve => setTimeout(resolve, 10));
        attempts++;
      }

      if (!graphWorkerProxy.isReady()) {
        logger.warn('Graph worker proxy not ready after timeout, attempting recovery...');
        await new Promise(resolve => setTimeout(resolve, 2000));
        if (!graphWorkerProxy.isReady()) {
          logger.warn('Graph worker proxy still not ready after recovery attempt');
          this.workerInitialized = false;
          useWorkerErrorStore.getState().setWorkerError(
            'The graph visualization worker failed to initialize.',
            'Worker initialization timed out after 12 seconds. Click retry to attempt reinitialization.',
          );
          return;
        }
      }

      this.workerInitialized = true;
      if (debugState.isEnabled()) logger.info('Worker is ready!');
      this.setupWorkerListeners();
      if (debugState.isEnabled()) logger.info('Graph worker proxy is ready');
    } catch (error) {
      logger.error('Failed to wait for graph worker proxy:', createErrorMetadata(error));
      this.workerInitialized = false;
    }
  }

  private setupWorkerListeners(): void {
    // ADR-03 D7: worker no longer emits graph-data or position-update events.
    // Retained as a no-op for legacy call paths (e.g. ensureWorkerReady).
    this.workerUnsubscribers.forEach(unsub => unsub());
    this.workerUnsubscribers = [];
  }

  public static getInstance(): GraphDataManager {
    if (!GraphDataManager.instance) {
      GraphDataManager.instance = new GraphDataManager();
    }
    return GraphDataManager.instance;
  }

  // ── Public accessors ──────────────────────────────────────────────────

  public get reverseNodeIds(): Map<number, string> {
    return this.reverseNodeIdMap;
  }

  /** Always returns null — worker data is async-only. Callers should use fallback positioning. */
  public getCachedGraphData(): GraphData | null {
    return null;
  }

  /** Public read-only accessor for the cached topology (ADR-03 D5). */
  public getLastGraphData(): GraphData | null {
    return this.lastGraphData;
  }

  public async ensureWorkerReady(): Promise<boolean> {
    if (this.workerInitialized) return true;

    if (debugState.isEnabled()) logger.debug('ensureWorkerReady called, checking worker status...');

    if (graphWorkerProxy.isReady()) {
      this.workerInitialized = true;
      this.setupWorkerListeners();
      if (debugState.isEnabled()) logger.info('Worker is now ready (late initialization)');
      return true;
    }

    for (let i = 0; i < 100; i++) {
      await new Promise(resolve => setTimeout(resolve, 10));
      if (graphWorkerProxy.isReady()) {
        this.workerInitialized = true;
        this.setupWorkerListeners();
        if (debugState.isEnabled()) logger.info('Worker became ready after additional wait');
        return true;
      }
    }

    logger.warn('Worker still not ready after ensureWorkerReady');
    return false;
  }

  // ── Configuration ─────────────────────────────────────────────────────

  public setWebSocketService(service: WebSocketAdapter): void {
    this.webSocketService = service;
    if (debugState.isDataDebugEnabled()) logger.debug('WebSocket service set');
  }

  // Send-side API: only the current vocabulary (ADR-2115 retired `logseq`).
  public setGraphType(type: GraphType): void {
    this.graphType = type;
    if (debugState.isEnabled()) logger.info(`Graph type set to: ${this.graphType}`);
  }

  public getGraphType(): 'knowledge' | 'visionclaw' {
    return this.graphType;
  }

  /**
   * Set the server-side population filter (PRD-018 WS-4). When set to a concrete
   * population the next `fetchInitialData` sends `?graph_type=` so only that
   * subset is transferred instead of the whole graph. `null`/`'all'` clears it.
   */
  public setGraphTypeFilter(filter: GraphTypeFilter): void {
    this.graphTypeFilter = filter;
    if (debugState.isEnabled()) logger.info(`Graph type filter set to: ${filter ?? 'all'}`);
  }

  public getGraphTypeFilter(): GraphTypeFilter {
    return this.graphTypeFilter;
  }

  // ── REST data fetch ───────────────────────────────────────────────────

  public async fetchInitialData(): Promise<GraphData> {
    // Drop linked_page stubs at source unless the user has opted to include
    // them — mirrors the ingestion gate in setGraphData and the server param.
    const includeLinkedPages =
      useSettingsStore.getState().settings?.nodeFilter?.includeLinkedPages ?? false;
    const validatedData = await fetchGraphData(
      this.graphType,
      this.graphTypeFilter,
      !includeLinkedPages,
    );

    await this.setGraphData(validatedData);

    const currentData = this.lastGraphData ?? validatedData;
    if (debugState.isEnabled()) logger.info(`Graph data loaded: ${currentData.nodes.length} nodes`);

    // T6 fix: backend is up but Oxigraph is empty. Schedule periodic re-fetches.
    if (currentData.nodes.length === 0) {
      scheduleEmptyDataRetry(
        1,
        this.retryTimeout,
        () => this.fetchInitialData(),
        handle => { this.retryTimeout = handle; },
        this.graphTypeFilter,
      );
    }

    return currentData;
  }

  // ── Topology management ───────────────────────────────────────────────

  public async setGraphData(data: GraphData): Promise<void> {
    if (debugState.isEnabled()) {
      logger.info(`Setting ${this.graphType} graph data: ${data.nodes.length} nodes, ${data.edges.length} edges`);
    }

    const storeState = useSettingsStore.getState();
    const includeLinkedPages = storeState.settings?.nodeFilter?.includeLinkedPages ?? false;

    // Population gate (ingestion): drop linked_page wikilink-stub nodes before
    // they reach the worker/topology/render pipeline. This is the dominant
    // node-count reduction — ~14.7k of 17.1k nodes are linked_page stubs that
    // the render gate already hides; dropping them here removes the worker and
    // edge-buffer churn that otherwise blocks the constrained sidecar.
    const gatedData = dropLinkedPageStubs(data, includeLinkedPages);

    let validatedData = gatedData;

    if (gatedData && gatedData.nodes) {
      validatedData = {
        ...gatedData,
        nodes: gatedData.nodes.map(node => ensureNodeHasValidPosition(node)),
      };

      if (debugState.isEnabled()) logger.info(`Validated ${validatedData.nodes.length} nodes with positions`);
    } else {
      validatedData = { nodes: [], edges: data?.edges || [] };
      logger.warn('Initialized with empty graph data');
    }

    buildNodeIdMaps(validatedData.nodes, this.nodeIdMap, this.reverseNodeIdMap);

    // ADR-03 D5: single cached delivery path
    setDataAndNotify(
      validatedData,
      this.lastGraphDataHash,
      this.listenerRegistry.graphDataSet,
      (data, hash) => {
        this.lastGraphData     = data;
        this.lastGraphDataHash = hash;
      },
    );

    // ADR-03 D7: deliver topology to worker for binary frame resolution
    if (graphWorkerProxy.isReady()) {
      try {
        await graphWorkerProxy.setGraphTopology(validatedData);
      } catch (err) {
        logger.warn('Failed to deliver topology to worker:', createErrorMetadata(err));
      }
    }

    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Graph data updated: ${validatedData.nodes.length} nodes, ${validatedData.edges.length} edges`);
    }
  }

  // ── Graph queries ─────────────────────────────────────────────────────

  public async getGraphData(): Promise<GraphData> {
    return this.lastGraphData ?? { nodes: [], edges: [] };
  }

  public async getVisibleNodes(): Promise<Node[]> {
    return this.lastGraphData?.nodes ?? [];
  }

  // ── Mutation helpers ──────────────────────────────────────────────────

  public async addNode(node: Node): Promise<void> {
    upsertNodeIdEntry(node, this.nodeIdMap, this.reverseNodeIdMap);
    const current = this.lastGraphData ?? { nodes: [], edges: [] };
    await this.setGraphData({
      nodes: [...current.nodes.filter(n => n.id !== node.id), node],
      edges: current.edges,
    });
  }

  public async addEdge(edge: Edge): Promise<void> {
    const current = this.lastGraphData ?? { nodes: [], edges: [] };
    const edges = [...current.edges];
    const idx = edges.findIndex(e => e.id === edge.id);
    if (idx >= 0) edges[idx] = { ...edges[idx], ...edge };
    else edges.push(edge);
    await this.setGraphData({ nodes: current.nodes, edges });
  }

  /**
   * Additive expansion merge (Graph2VR desktop migration). Merges freshly
   * fetched nodes/edges into the current graph WITHOUT re-laying-out the
   * existing population: every existing node object is preserved verbatim (its
   * live position, metadata, and worker index stay put), and only genuinely new
   * nodes are appended. New nodes are seeded near the anchor so physics settles
   * just the additions instead of teleporting them from the origin.
   *
   * Id-type discipline (a known trap here): expansion payload ids are numeric
   * u32; every comparison and map key is String()-coerced to match the
   * string-keyed topology.
   *
   * @param anchorPosition The anchor's LIVE world position (from the worker/SAB
   *   buffer, captured at right-click time). Required to avoid seeding from the
   *   stale cached `node.position` — the origin-spawn / zero-length-edge trap
   *   this codebase has hit before (physics owns positions post-load; the cached
   *   value is the initial-load snapshot, not where the node currently is).
   *   Falls back to the cached anchor position, then the origin, when absent.
   * @returns counts of nodes/edges actually added (post-dedup).
   */
  public async mergeGraphData(
    newNodes: Node[],
    newEdges: Edge[],
    anchorNodeId?: string,
    anchorPosition?: { x: number; y: number; z: number },
  ): Promise<{ nodesAdded: number; edgesAdded: number }> {
    const current = this.lastGraphData ?? { nodes: [], edges: [] };

    const existingNodeIds = new Set(current.nodes.map(n => String(n.id)));
    const cachedAnchor = anchorNodeId
      ? current.nodes.find(n => String(n.id) === String(anchorNodeId))
      : undefined;
    // Prefer the live worker/SAB position passed by the caller; the cached
    // node.position is a last resort (it may be the stale load-time value).
    const anchorPos = anchorPosition ?? cachedAnchor?.position ?? { x: 0, y: 0, z: 0 };

    // Seed new nodes on a small jittered shell around the anchor. Deterministic
    // enough to avoid a degenerate all-at-one-point cluster, cheap, and
    // physics-friendly (the settler pulls them onto their edges from here).
    const SEED_RADIUS = 12;
    const addedNodes: Node[] = [];
    let seedIndex = 0;
    for (const node of newNodes) {
      const id = String(node.id);
      if (existingNodeIds.has(id)) continue;
      existingNodeIds.add(id);
      const angle = seedIndex * 2.399963; // golden angle — even angular spread
      const ring = 1 + Math.floor(seedIndex / 8);
      seedIndex++;
      const seeded: Node = {
        ...node,
        id,
        position: node.position && (node.position.x || node.position.y || node.position.z)
          ? node.position
          : {
              x: anchorPos.x + Math.cos(angle) * SEED_RADIUS * ring * 0.4,
              y: anchorPos.y + Math.sin(angle * 1.7) * SEED_RADIUS * 0.5,
              z: anchorPos.z + Math.sin(angle) * SEED_RADIUS * ring * 0.4,
            },
      };
      addedNodes.push(seeded);
    }

    // Merge edges, deduped by id (fall back to a source-target-type key when the
    // payload omits ids). Existing edges are kept untouched.
    const edgeKey = (e: Edge): string =>
      e.id ?? `${String(e.source)}-${String(e.target)}-${e.edgeType ?? e.label ?? ''}`;
    const existingEdgeKeys = new Set(current.edges.map(edgeKey));
    const addedEdges: Edge[] = [];
    for (const edge of newEdges) {
      const normalised: Edge = { ...edge, source: String(edge.source), target: String(edge.target) };
      const key = edgeKey(normalised);
      if (existingEdgeKeys.has(key)) continue;
      existingEdgeKeys.add(key);
      addedEdges.push(normalised);
    }

    if (addedNodes.length === 0 && addedEdges.length === 0) {
      return { nodesAdded: 0, edgesAdded: 0 };
    }

    await this.setGraphData({
      nodes: [...current.nodes, ...addedNodes],
      edges: [...current.edges, ...addedEdges],
    });

    if (debugState.isEnabled()) {
      logger.info(
        `[expansion] additive merge: +${addedNodes.length} nodes, +${addedEdges.length} edges ` +
          `(anchor=${anchorNodeId ?? 'none'})`,
      );
    }
    return { nodesAdded: addedNodes.length, edgesAdded: addedEdges.length };
  }

  public async removeNode(nodeId: string): Promise<void> {
    const numericId = this.nodeIdMap.get(nodeId);
    const current = this.lastGraphData ?? { nodes: [], edges: [] };
    await this.setGraphData({
      nodes: current.nodes.filter(n => n.id !== nodeId),
      edges: current.edges.filter(
        e => String(e.source) !== String(nodeId) && String(e.target) !== String(nodeId),
      ),
    });
    if (numericId !== undefined) {
      this.nodeIdMap.delete(nodeId);
      this.reverseNodeIdMap.delete(numericId);
    }
  }

  public async removeEdge(edgeId: string): Promise<void> {
    const current = this.lastGraphData ?? { nodes: [], edges: [] };
    await this.setGraphData({
      nodes: current.nodes,
      edges: current.edges.filter(e => e.id !== edgeId),
    });
  }

  // ── WebSocket / binary ────────────────────────────────────────────────

  public async updateNodePositions(positionData: ArrayBuffer): Promise<void> {
    this.updateCount = (this.updateCount || 0) + 1;
    this.maybeSelfHealUnknownNodes(positionData);
    await handleBinaryFrame(
      positionData,
      this.lastBinaryUpdateTime,
      t => { this.lastBinaryUpdateTime = t; },
    );
  }

  /**
   * Live-linkage self-heal: if the position stream references node ids this
   * client has never learned, the server's topology is ahead of ours (a
   * graphUpdated signal was missed, or binary frames raced the refetch).
   * Sample-check roughly every 2s of frames; on detection, refetch topology —
   * the topology hash makes a false-positive refetch a no-op. Backstops the
   * signal path rather than replacing it.
   */
  private maybeSelfHealUnknownNodes(positionData: ArrayBuffer): void {
    this.selfHealFrameCounter = (this.selfHealFrameCounter + 1) % 120;
    if (this.selfHealFrameCounter !== 0) return;
    if (this.nodeIdMap.size === 0) return; // topology not loaded yet
    // A server-side node filter makes the local graph an intentional SUBSET:
    // binary frames may legitimately reference ids we dropped, and a refetch
    // here would fight the filter — re-expanding the view the user just
    // narrowed. Self-heal only applies to the unfiltered view.
    const nf = useSettingsStore.getState().settings?.nodeFilter;
    if (nf?.enabled && (nf.filterByQuality || nf.filterByAuthority)) return;
    const now = Date.now();
    if (now - this.lastSelfHealRefetch < 30_000) return; // refetch cooldown
    try {
      const nodes = parseBinaryNodeData(positionData);
      const unknown = nodes.some(n => !this.reverseNodeIdMap.has(getActualNodeId(n.nodeId)));
      if (unknown) {
        this.lastSelfHealRefetch = now;
        logger.info('[LiveLinkage] binary stream references unknown node ids — refetching topology');
        this.fetchInitialData().catch(err =>
          logger.error('[LiveLinkage] self-heal refetch failed:', createErrorMetadata(err)),
        );
      }
    } catch {
      // Frame parse issues are the binary pipeline's concern, not self-heal's.
    }
  }

  public async sendNodePositions(): Promise<void> {
    await _sendNodePositions(
      this.binaryUpdatesEnabled,
      this.webSocketService,
      this.isUserInteracting,
      this.lastGraphData,
      this.nodeIdMap,
      ensureNodeHasValidPosition,
    );
  }

  public enableBinaryUpdates(): void {
    _enableBinaryUpdates(
      this.webSocketService,
      enabled => this.setBinaryUpdatesEnabled(enabled),
      this.retryTimeout,
      handle => { this.retryTimeout = handle; },
    );
  }

  public setBinaryUpdatesEnabled(enabled: boolean): void {
    this.binaryUpdatesEnabled = enabled;
    if (debugState.isEnabled()) logger.info(`Binary updates ${enabled ? 'enabled' : 'disabled'}`);
  }

  // ── Listeners ─────────────────────────────────────────────────────────

  /**
   * Subscribe to graph-data changes (ADR-03 D5).
   * Duplicate registrations of the same callback are coalesced.
   * New subscribers receive the cached snapshot via queueMicrotask.
   */
  public onGraphDataChange(listener: (data: GraphData) => void): () => void {
    return this.listenerRegistry.onGraphDataChange(listener, this.lastGraphData);
  }

  public onPositionUpdate(listener: (positions: Float32Array) => void): () => void {
    return this.listenerRegistry.onPositionUpdate(listener);
  }

  /** Alias kept for backward compatibility. */
  public subscribeToUpdates(listener: (data: GraphData) => void): () => void {
    return this.onGraphDataChange(listener);
  }

  // ── Node utilities ────────────────────────────────────────────────────

  public ensureNodeHasValidPosition(node: Node): Node {
    return ensureNodeHasValidPosition(node);
  }

  // ── User interaction ──────────────────────────────────────────────────

  public setUserInteracting(isInteracting: boolean): void {
    if (this.isUserInteracting === isInteracting) return;
    this.isUserInteracting = isInteracting;

    if (isInteracting) {
      if (this.interactionTimeoutRef) {
        window.clearTimeout(this.interactionTimeoutRef);
        this.interactionTimeoutRef = null;
      }
      binaryProtocol.setUserInteracting(true);
      if (debugState.isEnabled()) logger.debug('User interaction started - WebSocket position updates enabled');
    } else {
      this.interactionTimeoutRef = window.setTimeout(() => {
        this.isUserInteracting = false;
        this.interactionTimeoutRef = null;

        const flushedBuffer = binaryProtocol.setUserInteracting(false);
        if (flushedBuffer && this.webSocketService && this.webSocketService.isReady()) {
          this.webSocketService.send(flushedBuffer);
        }

        if (debugState.isEnabled()) logger.debug('User interaction ended - WebSocket position updates disabled');
      }, 200);
    }
  }

  public isUserCurrentlyInteracting(): boolean {
    return this.isUserInteracting;
  }

  // ── Dispose ───────────────────────────────────────────────────────────

  public dispose(): void {
    if (this.retryTimeout !== null) {
      window.clearTimeout(this.retryTimeout);
      this.retryTimeout = null;
    }
    if (this.interactionTimeoutRef !== null) {
      window.clearTimeout(this.interactionTimeoutRef);
      this.interactionTimeoutRef = null;
    }

    this.workerUnsubscribers.forEach(unsub => unsub());
    this.workerUnsubscribers = [];

    this.listenerRegistry.clear();
    this.lastGraphData     = null;
    this.lastGraphDataHash = null;
    this.webSocketService  = null;
    this.nodeIdMap.clear();
    this.reverseNodeIdMap.clear();
    this.isUserInteracting = false;

    if (debugState.isEnabled()) logger.info('GraphDataManager disposed');
  }
}

export const graphDataManager = GraphDataManager.getInstance();
