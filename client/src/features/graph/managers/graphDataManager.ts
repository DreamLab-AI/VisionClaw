import { createLogger, createErrorMetadata } from '../../../utils/loggerConfig';
import { debugState } from '../../../utils/clientDebugState';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import { WebSocketAdapter } from '../../../store/websocketStore';
import { useSettingsStore } from '../../../store/settingsStore';
import {
  BinaryNodeData,
  createBinaryNodeData,
  Vec3,
  BINARY_NODE_SIZE,
  BINARY_FRAME_HEADER_SIZE,
} from '../../../types/binaryProtocol';
import { binaryProtocol } from '../../../services/BinaryWebSocketProtocol';
import { stringToU32 } from '../../../types/idMapping';
import { graphWorkerProxy } from './graphWorkerProxy';
import type { GraphData, Node, Edge } from './graphWorkerProxy';
import { startTransition } from 'react';
import { useWorkerErrorStore } from '../../../store/workerErrorStore';

const logger = createLogger('GraphDataManager');

// Re-export types from worker proxy for compatibility
export type { Node, Edge, GraphData, NodeMetadata } from './graphWorkerProxy';

// Alias for backward compatibility
export type KGNode = Node;

type GraphDataChangeListener = (data: GraphData) => void;
type PositionUpdateListener = (positions: Float32Array) => void;

class GraphDataManager {
  private static instance: GraphDataManager;
  private binaryUpdatesEnabled: boolean = false;
  public webSocketService: WebSocketAdapter | null = null;
  private graphDataListeners: GraphDataChangeListener[] = [];
  private positionUpdateListeners: PositionUpdateListener[] = [];
  private lastBinaryUpdateTime: number = 0;
  private retryTimeout: number | null = null;
  public nodeIdMap: Map<string, number> = new Map();
  private reverseNodeIdMap: Map<number, string> = new Map();
  private workerInitialized: boolean = false;
  private graphType: 'logseq' | 'visionflow' = 'logseq'; 
  private isUserInteracting: boolean = false; 
  private interactionTimeoutRef: number | null = null;
  private updateCount: number = 0; 

  // Track worker subscription unsubscribe functions for cleanup
  private workerUnsubscribers: Array<() => void> = [];

  private constructor() {

    this.waitForWorker();
  }

  private async waitForWorker(): Promise<void> {
    try {
      if (debugState.isEnabled()) {
        logger.debug('Waiting for worker to be ready...');
      }
      let attempts = 0;
      const maxAttempts = 1000; // 10 seconds total (increased from 3s)

      while (!graphWorkerProxy.isReady() && attempts < maxAttempts) {
        await new Promise(resolve => setTimeout(resolve, 10));
        attempts++;
      }

      if (!graphWorkerProxy.isReady()) {
        logger.warn('Graph worker proxy not ready after timeout, attempting recovery...');
        // Try one more time with a longer wait before giving up
        await new Promise(resolve => setTimeout(resolve, 2000));
        if (!graphWorkerProxy.isReady()) {
          logger.warn('Graph worker proxy still not ready after recovery attempt');
          this.workerInitialized = false;
          useWorkerErrorStore.getState().setWorkerError(
            'The graph visualization worker failed to initialize.',
            'Worker initialization timed out after 12 seconds. Click retry to attempt reinitialization.'
          );
          return;
        }
      }

      this.workerInitialized = true;
      if (debugState.isEnabled()) {
        logger.info('Worker is ready!');
      }
      
      
      this.setupWorkerListeners();
      
      if (debugState.isEnabled()) {
        logger.info('Graph worker proxy is ready');
      }
    } catch (error) {
      logger.error('Failed to wait for graph worker proxy:', createErrorMetadata(error));
      this.workerInitialized = false;
    }
  }

  private setupWorkerListeners(): void {
    // Clean up any previous subscriptions before re-subscribing
    this.workerUnsubscribers.forEach(unsub => unsub());
    this.workerUnsubscribers = [];

    const unsubGraphData = graphWorkerProxy.onGraphDataChange((data) => {
      this.graphDataListeners.forEach(listener => {
        try {
          startTransition(() => {
            listener(data);
          });
        } catch (error) {
          logger.error('Error in forwarded graph data listener:', createErrorMetadata(error));
        }
      });
    });
    this.workerUnsubscribers.push(unsubGraphData);

    const unsubPositions = graphWorkerProxy.onPositionUpdate((positions) => {
      this.positionUpdateListeners.forEach(listener => {
        try {
          listener(positions);
        } catch (error) {
          logger.error('Error in forwarded position update listener:', createErrorMetadata(error));
        }
      });
    });
    this.workerUnsubscribers.push(unsubPositions);

    // Listen for physics parameter updates from settingsStore and forward to worker.
    // settingsStore.notifyPhysicsUpdate() dispatches a CustomEvent with the latest
    // physics settings; we forward them to the worker so tweening/interpolation
    // parameters stay in sync with the UI controls.
    const handlePhysicsUpdate = (e: Event) => {
      const detail = (e as CustomEvent)?.detail;
      if (detail && this.workerInitialized) {
        graphWorkerProxy.updateSettings(detail).catch((err: unknown) => {
          logger.warn('Failed to forward physics settings to worker:', createErrorMetadata(err));
        });
        // Server sync handled by autoSaveManager -> settingsApi.updatePhysics() (GET-merge-PUT)
      }
    };
    window.addEventListener('physicsParametersUpdated', handlePhysicsUpdate);
    this.workerUnsubscribers.push(() => {
      window.removeEventListener('physicsParametersUpdated', handlePhysicsUpdate);
    });

    // Listen for client-side tweening setting changes and forward to worker.
    // Tweening controls how smoothly the client interpolates toward server positions.
    const handleTweeningUpdate = (e: Event) => {
      const detail = (e as CustomEvent)?.detail;
      if (detail && this.workerInitialized) {
        graphWorkerProxy.setTweeningSettings(detail).catch((err: unknown) => {
          logger.warn('Failed to forward tweening settings to worker:', createErrorMetadata(err));
        });
      }
    };
    window.addEventListener('tweeningSettingsUpdated', handleTweeningUpdate);
    this.workerUnsubscribers.push(() => {
      window.removeEventListener('tweeningSettingsUpdated', handleTweeningUpdate);
    });
  }

  public static getInstance(): GraphDataManager {
    if (!GraphDataManager.instance) {
      GraphDataManager.instance = new GraphDataManager();
    }
    return GraphDataManager.instance;
  }

  /**
   * Get the reverse node ID map (numeric ID -> string ID)
   * Used for resolving node positions from binary protocol IDs.
   */
  public get reverseNodeIds(): Map<number, string> {
    return this.reverseNodeIdMap;
  }

  /**
   * Get cached graph data synchronously (may be stale or null)
   * Used for fast position lookups during animation.
   * Note: Returns null since worker data requires async access.
   * Callers should use fallback positioning when null.
   */
  public getCachedGraphData(): GraphData | null {
    // Worker data is async-only; callers should use fallback positioning
    // For real-time visualization, ActionConnectionsLayer uses deterministic
    // position generation based on node IDs when positions aren't available.
    return null;
  }

  // Allow re-checking worker readiness after AppInitializer completes
  public async ensureWorkerReady(): Promise<boolean> {
    if (this.workerInitialized) {
      return true;
    }

    if (debugState.isEnabled()) {
      logger.debug('ensureWorkerReady called, checking worker status...');
    }

    if (graphWorkerProxy.isReady()) {
      this.workerInitialized = true;
      this.setupWorkerListeners();
      if (debugState.isEnabled()) {
        logger.info('Worker is now ready (late initialization)');
      }
      return true;
    }

    // Wait a bit more for worker
    for (let i = 0; i < 100; i++) {
      await new Promise(resolve => setTimeout(resolve, 10));
      if (graphWorkerProxy.isReady()) {
        this.workerInitialized = true;
        this.setupWorkerListeners();
        if (debugState.isEnabled()) {
          logger.info('Worker became ready after additional wait');
        }
        return true;
      }
    }

    logger.warn('Worker still not ready after ensureWorkerReady');
    return false;
  }

  
  public setWebSocketService(service: WebSocketAdapter): void {
    this.webSocketService = service;
    if (debugState.isDataDebugEnabled()) {
      logger.debug('WebSocket service set');
    }
  }

  
  public setGraphType(type: 'logseq' | 'visionflow'): void {
    this.graphType = type;
    if (debugState.isEnabled()) {
      logger.info(`Graph type set to: ${type}`);
    }
  }

  
  public getGraphType(): 'logseq' | 'visionflow' {
    return this.graphType;
  }

  
  
  public async fetchInitialData(): Promise<GraphData> {
    const maxRetries = 3;
    const initialDelay = 500;

    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        if (debugState.isEnabled()) {
          logger.info(`Fetching initial ${this.graphType} graph data with physics positions (Attempt ${attempt}/${maxRetries})`);
        }

        const response = await unifiedApiClient.get('/graph/data', { timeout: 10000 });

        // Handle response structure: { success: true, data: { nodes: [], edges: [] } }
        const responseData = response.data.data || response.data;

        if (!responseData || typeof responseData !== 'object') {
          throw new Error('Invalid graph data format: data is not an object');
        }

        const nodes = Array.isArray(responseData.nodes) ? responseData.nodes : [];

        // One-time diagnostic: use console.log directly (custom logger may filter)
        if (Array.isArray(responseData.edges) && responseData.edges.length > 0) {
          const rawEdge = responseData.edges[0];
          logger.debug('[graphDataManager] RAW API edge[0]:',
            'keys=', Object.keys(rawEdge),
            'source=', rawEdge.source, '(type:', typeof rawEdge.source + ')',
            'target=', rawEdge.target, '(type:', typeof rawEdge.target + ')',
            'id=', rawEdge.id,
            'full=', JSON.stringify(rawEdge).slice(0, 300));
        }

        // Convert edge source/target to strings.  Rust API returns `source: u32`,
        // but the client Edge interface expects `source: string`.
        // Also recover from pre-broken "undefined" strings and missing fields.
        const edges = Array.isArray(responseData.edges)
          ? responseData.edges.map((edge: Edge & Record<string, unknown>) => {
              let source: string | undefined = (edge.source ?? edge.from ?? edge.from_node ?? edge.sourceId ?? edge.source_id) as string | undefined;
              let target: string | undefined = (edge.target ?? edge.to ?? edge.to_node ?? edge.targetId ?? edge.target_id) as string | undefined;

              // Guard: if source/target are literally the string "undefined" (from a previous
              // String(undefined) coercion), treat them as missing
              if (source === 'undefined' || source === 'null') source = undefined;
              if (target === 'undefined' || target === 'null') target = undefined;

              // Extract from id if still missing (format: "source-target", e.g. "798-861")
              if ((source == null || target == null) && edge.id && typeof edge.id === 'string') {
                const parts = edge.id.split('-');
                if (parts.length >= 2) {
                  if (source == null) source = parts[0];
                  if (target == null) target = parts.slice(1).join('-');
                }
              }

              return {
                ...edge,
                source: String(source),
                target: String(target)
              };
            }).filter((edge: { source: string; target: string }) => edge.source !== 'undefined' && edge.target !== 'undefined')
          : [];
        const metadata = responseData.metadata || {};
        const settlementState = responseData.settlementState || { isSettled: false, stableFrameCount: 0, kineticEnergy: 0 };

        if (debugState.isEnabled()) {
          logger.debug(`Received settlement state: settled=${settlementState.isSettled}, frames=${settlementState.stableFrameCount}, KE=${settlementState.kineticEnergy}`);
        }

        
        
        const enrichedNodes = nodes.map((node: Node) => {
          // Normalize node ID to string — API returns u32 numeric IDs but
          // edge source/target are already String()-coerced above.  Without this,
          // Map/Set lookups using === fail: Set.has("42") misses number 42.
          const normalizedNode = { ...node, id: String(node.id) };

          // Ensure position property exists (API may send flat x/y/z instead of nested position)
          if (!normalizedNode.position) {
            const raw = node as unknown as Record<string, unknown> & { position?: { x?: number; y?: number; z?: number } };
            normalizedNode.position = {
              x: Number(raw.x) || Number(raw.position?.x) || 0,
              y: Number(raw.y) || Number(raw.position?.y) || 0,
              z: Number(raw.z) || Number(raw.position?.z) || 0,
            };
          }

          const nodeWithMeta = normalizedNode as unknown as { metadata_id?: string; metadataId?: string };
          const nodeMetadata = metadata[nodeWithMeta.metadata_id || nodeWithMeta.metadataId || ''];
          if (nodeMetadata) {
            return { ...normalizedNode, metadata: { ...normalizedNode.metadata, ...nodeMetadata } };
          }
          return normalizedNode;
        });

        const validatedData = { nodes: enrichedNodes, edges };

        if (debugState.isEnabled()) {
          logger.info(`Received initial graph data: ${validatedData.nodes.length} nodes, ${validatedData.edges.length} edges (physics settled: ${settlementState.isSettled})`);
        }

        await this.setGraphData(validatedData);

        const currentData = await graphWorkerProxy.getGraphData();
        if (debugState.isEnabled()) {
          logger.info(`Graph data loaded: ${currentData.nodes.length} nodes`);
        }
        return currentData;

      } catch (error) {
        logger.error(`Attempt ${attempt} failed to fetch initial graph data:`, createErrorMetadata(error));
        if (attempt === maxRetries) {
          logger.error('All attempts to fetch initial graph data failed.');
          throw error; 
        }

        const delay = initialDelay * Math.pow(2, attempt - 1);
        if (debugState.isEnabled()) {
          logger.debug(`Retrying in ${delay}ms...`);
        }
        await new Promise(resolve => setTimeout(resolve, delay));
      }
    }

    
    return { nodes: [], edges: [] };
  }

  
  public async setGraphData(data: GraphData): Promise<void> {
    if (debugState.isEnabled()) {
      logger.info(`Setting ${this.graphType} graph data: ${data.nodes.length} nodes, ${data.edges.length} edges`);
    }

    // Get quality gate settings for filtering
    const storeState = useSettingsStore.getState();
    const qualityGates = storeState.settings?.qualityGates;
    // Use settings value, default to Infinity (no limit) if not set
    const maxNodeCount = qualityGates?.maxNodeCount ?? Infinity;
    // Performance: Removed per-call logging

    // ADR-048: no client-side tier filter here. Filtering the nodes list would
    // desync `reverseNodeIdMap` in graph.worker.ts from the binary position
    // stream — the server sends updates for ALL loaded nodes (KG + OwlClass),
    // so every unknown ID triggers the unknown-node handler and the view stops
    // updating without a manual refresh. Tier visibility is controlled at the
    // backend via `LOAD_ONTOLOGY_TIER` env (default true). To hide the ontology
    // tier, unset that env rather than filtering on the client.
    let validatedData = data;
    if (data && data.nodes) {
      let nodesToUse = data.nodes;

      // Apply node count filtering if we exceed maxNodeCount
      if (nodesToUse.length > maxNodeCount) {
        logger.info(`Filtering nodes: ${nodesToUse.length} exceeds maxNodeCount ${maxNodeCount}`);

        // Sort by authority_score or quality_score (higher = more important)
        const scoredNodes = nodesToUse.map(node => ({
          node,
          score: (node.metadata?.authority_score ?? 0) + (node.metadata?.quality_score ?? 0)
        }));

        // Sort descending by score, keep top N
        scoredNodes.sort((a, b) => b.score - a.score);
        nodesToUse = scoredNodes.slice(0, maxNodeCount).map(s => s.node);

        logger.info(`Filtered to ${nodesToUse.length} nodes (by authority/quality score)`);

        // Filter edges to only include connections between kept nodes
        const keptNodeIds = new Set(nodesToUse.map(n => String(n.id)));
        const filteredEdges = (data.edges || []).filter(
          edge => keptNodeIds.has(String(edge.source)) && keptNodeIds.has(String(edge.target))
        );

        logger.info(`Filtered edges: ${data.edges?.length ?? 0} -> ${filteredEdges.length}`);

        validatedData = {
          nodes: nodesToUse.map(node => this.ensureNodeHasValidPosition(node)),
          edges: filteredEdges
        };
      } else {
        const validatedNodes = nodesToUse.map(node => this.ensureNodeHasValidPosition(node));
        validatedData = {
          ...data,
          nodes: validatedNodes
        };
      }

      if (debugState.isEnabled()) {
        logger.info(`Validated ${validatedData.nodes.length} nodes with positions`);
      }
    } else {

      validatedData = { nodes: [], edges: data?.edges || [] };
      logger.warn('Initialized with empty graph data');
    }
    
    
    this.nodeIdMap.clear();
    this.reverseNodeIdMap.clear();
    
    
    validatedData.nodes.forEach((node) => {
      const numericId = parseInt(node.id, 10);
      if (!isNaN(numericId) && numericId >= 0 && numericId <= 0xFFFFFFFF) {
        this.nodeIdMap.set(node.id, numericId);
        this.reverseNodeIdMap.set(numericId, node.id);
      } else {
        let mappedId = stringToU32(node.id);
        while (this.reverseNodeIdMap.has(mappedId) && this.reverseNodeIdMap.get(mappedId) !== node.id) {
          mappedId = (mappedId + 1) >>> 0;
        }
        this.nodeIdMap.set(node.id, mappedId);
        this.reverseNodeIdMap.set(mappedId, node.id);
      }
    });
    
    
    await graphWorkerProxy.setGraphData(validatedData);
    
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Graph data updated: ${validatedData.nodes.length} nodes, ${validatedData.edges.length} edges`);
    }
  }

  
  private validateNodeMappings(nodes: Node[]): void {
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Validated ${nodes.length} nodes with ID mapping`);
    }
  }

  
  public enableBinaryUpdates(): void {
    if (!this.webSocketService) {
      logger.warn('Cannot enable binary updates: WebSocket service not set');
      return;
    }

    
    if (this.webSocketService.isReady()) {
      this.setBinaryUpdatesEnabled(true);
      return;
    }

    
    if (this.retryTimeout) {
      window.clearTimeout(this.retryTimeout);
    }

    this.retryTimeout = window.setTimeout(() => {
      if (this.webSocketService && this.webSocketService.isReady()) {
        this.setBinaryUpdatesEnabled(true);
        if (debugState.isEnabled()) {
          logger.info('WebSocket ready, binary updates enabled');
        }
      } else {
        if (debugState.isEnabled()) {
          logger.info('WebSocket not ready yet, retrying...');
        }
        this.enableBinaryUpdates();
      }
    }, 500);
  }

  public setBinaryUpdatesEnabled(enabled: boolean): void {
    this.binaryUpdatesEnabled = enabled;
    
    if (debugState.isEnabled()) {
      logger.info(`Binary updates ${enabled ? 'enabled' : 'disabled'}`);
    }
  }

  
  public async getGraphData(): Promise<GraphData> {
    // Check both local flag AND proxy ready state (handles race condition)
    if (!this.workerInitialized && !graphWorkerProxy.isReady()) {
      if (debugState.isEnabled()) {
        logger.warn('Worker not initialized, returning empty data');
      }
      return { nodes: [], edges: [] };
    }

    // Update local flag if proxy is ready but we missed initialization
    if (!this.workerInitialized && graphWorkerProxy.isReady()) {
      this.workerInitialized = true;
      this.setupWorkerListeners();
    }

    try {
      return await graphWorkerProxy.getGraphData();
    } catch (error) {
      logger.error('Error getting graph data from worker:', createErrorMetadata(error));
      return { nodes: [], edges: [] };
    }
  }

  
  public async addNode(node: Node): Promise<void> {
    
    const numericId = parseInt(node.id, 10);
    if (!isNaN(numericId)) {
      this.nodeIdMap.set(node.id, numericId);
      this.reverseNodeIdMap.set(numericId, node.id);
    } else {
      let mappedId = stringToU32(node.id);
      while (this.reverseNodeIdMap.has(mappedId) && this.reverseNodeIdMap.get(mappedId) !== node.id) {
        mappedId = (mappedId + 1) >>> 0;
      }
      this.nodeIdMap.set(node.id, mappedId);
      this.reverseNodeIdMap.set(mappedId, node.id);
    }
    
    await graphWorkerProxy.updateNode(node);
  }

  
  public async addEdge(edge: Edge): Promise<void> {
    
    const currentData = await graphWorkerProxy.getGraphData();
    const existingIndex = currentData.edges.findIndex(e => e.id === edge.id);
    
    if (existingIndex >= 0) {
      currentData.edges[existingIndex] = {
        ...currentData.edges[existingIndex],
        ...edge
      };
    } else {
      currentData.edges.push(edge);
    }
    
    await graphWorkerProxy.setGraphData(currentData);
  }

  
  public async removeNode(nodeId: string): Promise<void> {
    
    const numericId = this.nodeIdMap.get(nodeId);
    
    await graphWorkerProxy.removeNode(nodeId);
    
    
    if (numericId !== undefined) {
      this.nodeIdMap.delete(nodeId);
      this.reverseNodeIdMap.delete(numericId);
    }
  }

  
  public async removeEdge(edgeId: string): Promise<void> {
    
    const currentData = await graphWorkerProxy.getGraphData();
    currentData.edges = currentData.edges.filter(edge => edge.id !== edgeId);
    await graphWorkerProxy.setGraphData(currentData);
  }

  
  public async updateNodePositions(positionData: ArrayBuffer): Promise<void> {
    this.updateCount = (this.updateCount || 0) + 1;

    if (!positionData || positionData.byteLength === 0) {
      return;
    }

    // All graph types process binary position updates from the server.
    // Server is the single source of truth for all node positions.

    // Throttle to ~60fps
    const now = Date.now();
    if (now - this.lastBinaryUpdateTime < 16) {
      return;
    }
    this.lastBinaryUpdateTime = now;

    try {
      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Received binary data: ${positionData.byteLength} bytes`);

        // ADR-061: 9-byte envelope (preamble + broadcast_sequence) + N × 24 B nodes.
        if (positionData.byteLength >= BINARY_FRAME_HEADER_SIZE) {
          const remainder = (positionData.byteLength - BINARY_FRAME_HEADER_SIZE) % BINARY_NODE_SIZE;
          if (remainder !== 0) {
            logger.warn(
              `Binary data size (${positionData.byteLength} bytes) does not match ` +
              `${BINARY_FRAME_HEADER_SIZE} + N × ${BINARY_NODE_SIZE}. Remainder: ${remainder} bytes`,
            );
          }
        }
      }

      await graphWorkerProxy.processBinaryData(positionData);
      // Successful processing — reset transient error counter
      useWorkerErrorStore.getState().resetTransientErrors();

      // FIX 2: Periodically check if the binary stream delivered positions for
      // unknown node IDs (graph mutation on server). If so, re-fetch graph data
      // via REST to pick up new nodes and edges. Checked every ~100 binary frames
      // to avoid excessive async overhead.
      if (this.updateCount % 100 === 0) {
        graphWorkerProxy.hasUnknownNodes().then(async (hasUnknown) => {
          if (hasUnknown) {
            logger.info('[graphDataManager] Unknown nodes detected in binary stream — re-fetching graph data via REST');
            try {
              // Use unifiedApiClient (carries auth cookies/headers) so the
              // recovery path works when /api/graph/data is auth-gated.
              // Raw fetch() returned 401/403 and silently swallowed, which
              // meant reverseNodeIdMap never rebuilt and positions stayed
              // stale until the user hard-refreshed the page.
              const resp = await unifiedApiClient.get<{ nodes?: Node[]; edges?: Edge[]; data?: GraphData }>('/graph/data', { timeout: 10000 });
              const payload = resp.data;
              const freshData: GraphData | undefined =
                (payload && Array.isArray((payload as GraphData).nodes))
                  ? (payload as GraphData)
                  : (payload && (payload as { data?: GraphData }).data);
              if (freshData?.nodes && freshData.nodes.length > 0) {
                await this.setGraphData(freshData);
                logger.info(`[graphDataManager] REST re-fetch complete: ${freshData.nodes.length} nodes, ${freshData.edges?.length ?? 0} edges`);
              }
            } catch (err) {
              logger.error('[graphDataManager] REST re-fetch failed:', err);
            }
          }
        }).catch(() => { /* ignore polling errors */ });
      }

      const settings = useSettingsStore.getState().settings;
      const debugEnabled = settings?.system?.debug?.enabled;
      const physicsDebugEnabled = settings?.system?.debug?.enablePhysicsDebug;
      const nodeDebugEnabled = settings?.system?.debug?.enableNodeDebug;
      
      if (debugEnabled && (physicsDebugEnabled || nodeDebugEnabled)) {
        const view = new DataView(positionData);
        const payloadBytes = Math.max(0, positionData.byteLength - BINARY_FRAME_HEADER_SIZE);
        const nodeCount = Math.min(3, Math.floor(payloadBytes / BINARY_NODE_SIZE));
        for (let i = 0; i < nodeCount; i++) {
          const offset = BINARY_FRAME_HEADER_SIZE + i * BINARY_NODE_SIZE;
          const x = view.getFloat32(offset + 4, true);
          const y = view.getFloat32(offset + 8, true);
          const z = view.getFloat32(offset + 12, true);
          logger.info(`[Physics Debug] Node ${i}: position(${x.toFixed(2)}, ${y.toFixed(2)}, ${z.toFixed(2)})`);
        }
      }
      
      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Processed binary data through worker`);
      }
    } catch (error) {
      logger.error('Error processing binary position data:', createErrorMetadata(error));
      // Track transient errors — only escalate to red screen after sustained failures
      useWorkerErrorStore.getState().recordTransientError('updateNodePositions');

      if (debugState.isEnabled()) {
        try {
          
          const view = new DataView(positionData);
          const byteArray = [];
          const maxBytesToShow = Math.min(64, positionData.byteLength);
          
          for (let i = 0; i < maxBytesToShow; i++) {
            byteArray.push(view.getUint8(i).toString(16).padStart(2, '0'));
          }
          
          logger.debug(`First ${maxBytesToShow} bytes of binary data: ${byteArray.join(' ')}${positionData.byteLength > maxBytesToShow ? '...' : ''}`);
        } catch (e) {
          logger.debug('Could not display binary data preview:', e);
        }
      }
    }
  }

  
  
  public async sendNodePositions(): Promise<void> {
    if (!this.binaryUpdatesEnabled || !this.webSocketService || !this.isUserInteracting) {
      return;
    }

    try {
      
      const currentData = await graphWorkerProxy.getGraphData();
      
      
      const binaryNodes: BinaryNodeData[] = currentData.nodes
        .filter(node => node && node.id) 
        .map(node => {
          
          const validatedNode = this.ensureNodeHasValidPosition(node);
          
          
          const numericId = this.nodeIdMap.get(validatedNode.id) || 0;
          if (numericId === 0) {
            logger.warn(`No numeric ID found for node ${validatedNode.id}, skipping`);
            return null;
          }
          
          
          const velocity: Vec3 = (validatedNode.metadata?.velocity as Vec3) || { x: 0, y: 0, z: 0 };
          
          return {
            nodeId: numericId,
            position: {
              x: validatedNode.position.x || 0,
              y: validatedNode.position.y || 0,
              z: validatedNode.position.z || 0
            },
            velocity
          };
        })
        .filter((node): node is BinaryNodeData => node !== null);

      
      const buffer = createBinaryNodeData(binaryNodes);
      
      
      this.webSocketService.send(buffer);
      
      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Sent positions for ${binaryNodes.length} nodes using binary protocol`);
      }
    } catch (error) {
      logger.error('Error sending node positions:', createErrorMetadata(error));
    }
  }

  
  public onGraphDataChange(listener: GraphDataChangeListener): () => void {
    this.graphDataListeners.push(listener);

    // Provide initial data to new listener
    graphWorkerProxy.getGraphData().then(data => {
      if (debugState.isEnabled()) {
        logger.debug(`Calling listener with current data: ${data.nodes.length} nodes`);
      }
      listener(data);
    }).catch(error => {
      logger.error('Error getting initial graph data for listener:', createErrorMetadata(error));
      listener({ nodes: [], edges: [] });
    });

    return () => {
      this.graphDataListeners = this.graphDataListeners.filter(l => l !== listener);
    };
  }

  
  public onPositionUpdate(listener: PositionUpdateListener): () => void {
    this.positionUpdateListeners.push(listener);
    
    
    return () => {
      this.positionUpdateListeners = this.positionUpdateListeners.filter(l => l !== listener);
    };
  }

  
  private async notifyGraphDataListeners(): Promise<void> {
    try {
      const currentData = await graphWorkerProxy.getGraphData();
      this.graphDataListeners.forEach(listener => {
        try {
          listener(currentData);
        } catch (error) {
          logger.error('Error in graph data listener:', createErrorMetadata(error));
        }
      });
    } catch (error) {
      logger.error('Error getting graph data for listeners:', createErrorMetadata(error));
    }
  }

  
  private notifyPositionUpdateListeners(positions: Float32Array): void {
    this.positionUpdateListeners.forEach(listener => {
      try {
        listener(positions);
      } catch (error) {
        logger.error('Error in position update listener:', createErrorMetadata(error));
      }
    });
  }

  
  public ensureNodeHasValidPosition(node: Node): Node {
    if (!node.position) {
      // Only log in debug mode to avoid spam
      if (debugState.isDataDebugEnabled()) {
        logger.warn(`Node ${node.id} missing position - server should provide this!`);
      }
      return {
        ...node,
        position: { x: 0, y: 0, z: 0 }
      };
    } else if (typeof node.position.x !== 'number' ||
               typeof node.position.y !== 'number' ||
               typeof node.position.z !== 'number') {
      if (debugState.isDataDebugEnabled()) {
        logger.warn(`Node ${node.id} has invalid position coordinates - fixing`);
      }
      // Return a new object to avoid mutating the input
      return {
        ...node,
        position: {
          x: typeof node.position.x === 'number' && isFinite(node.position.x) ? node.position.x : 0,
          y: typeof node.position.y === 'number' && isFinite(node.position.y) ? node.position.y : 0,
          z: typeof node.position.z === 'number' && isFinite(node.position.z) ? node.position.z : 0,
        },
      };
    }
    return node;
  }

  
  public subscribeToUpdates(listener: GraphDataChangeListener): () => void {
    return this.onGraphDataChange(listener);
  }

  /**
   * Get visible nodes asynchronously from the worker.
   * Previously this was synchronous and always returned [] because of an async race.
   */
  public async getVisibleNodes(): Promise<Node[]> {
    try {
      const data = await graphWorkerProxy.getGraphData();
      return data.nodes;
    } catch (error) {
      logger.error('Error getting visible nodes:', createErrorMetadata(error));
      return [];
    }
  }

  
  public setUserInteracting(isInteracting: boolean): void {
    if (this.isUserInteracting === isInteracting) {
      return;
    }

    this.isUserInteracting = isInteracting;

    if (isInteracting) {

      if (this.interactionTimeoutRef) {
        window.clearTimeout(this.interactionTimeoutRef);
        this.interactionTimeoutRef = null;
      }

      binaryProtocol.setUserInteracting(true);

      if (debugState.isEnabled()) {
        logger.debug('User interaction started - WebSocket position updates enabled');
      }
    } else {


      this.interactionTimeoutRef = window.setTimeout(() => {
        this.isUserInteracting = false;
        this.interactionTimeoutRef = null;

        // Flush any pending binary protocol position updates before disabling
        const flushedBuffer = binaryProtocol.setUserInteracting(false);
        if (flushedBuffer && this.webSocketService && this.webSocketService.isReady()) {
          this.webSocketService.send(flushedBuffer);
        }

        if (debugState.isEnabled()) {
          logger.debug('User interaction ended - WebSocket position updates disabled');
        }
      }, 200);
    }
  }

  
  public isUserCurrentlyInteracting(): boolean {
    return this.isUserInteracting;
  }

  
  public dispose(): void {
    if (this.retryTimeout) {
      window.clearTimeout(this.retryTimeout);
      this.retryTimeout = null;
    }

    if (this.interactionTimeoutRef) {
      window.clearTimeout(this.interactionTimeoutRef);
      this.interactionTimeoutRef = null;
    }

    // Clean up worker subscriptions to prevent leaks
    this.workerUnsubscribers.forEach(unsub => unsub());
    this.workerUnsubscribers = [];

    this.graphDataListeners = [];
    this.positionUpdateListeners = [];
    this.webSocketService = null;
    this.nodeIdMap.clear();
    this.reverseNodeIdMap.clear();
    this.isUserInteracting = false;

    if (debugState.isEnabled()) {
      logger.info('GraphDataManager disposed');
    }
  }
}

// Create singleton instance
export const graphDataManager = GraphDataManager.getInstance();

