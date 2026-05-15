

import { wrap, Remote } from 'comlink';
import { GraphWorkerType, ForcePhysicsSettings } from '../workers/graph.worker';
import type { NodeMetadata } from '../workers/graph.worker';
import { createLogger } from '../../../utils/loggerConfig';
import { debugState } from '../../../utils/clientDebugState';

const logger = createLogger('GraphWorkerProxy');

export type { NodeMetadata } from '../workers/graph.worker';

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

// Add Vec3 to be used in updateUserDrivenNodePosition
export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

type GraphDataChangeListener = (data: GraphData) => void;
type PositionUpdateListener = (positions: Float32Array) => void;


class GraphWorkerProxy {
  private static instance: GraphWorkerProxy;
  private worker: Worker | null = null;
  private workerApi: Remote<GraphWorkerType> | null = null;
  private graphDataListeners: GraphDataChangeListener[] = [];
  private positionUpdateListeners: PositionUpdateListener[] = [];
  private sharedBuffer: SharedArrayBuffer | null = null;
  private isInitialized: boolean = false;
  private graphType: 'logseq' | 'visionflow' = 'logseq';
  private sharedPositionView: Float32Array | null = null;
  private lastReceivedPositions: Float32Array | null = null;
  private tickInFlight: boolean = false;
  private _consecutiveTickErrors: number = 0;
  private static readonly MAX_CONSECUTIVE_ERRORS = 10;

  private constructor() {}

  public static getInstance(): GraphWorkerProxy {
    if (!GraphWorkerProxy.instance) {
      GraphWorkerProxy.instance = new GraphWorkerProxy();
    }
    return GraphWorkerProxy.instance;
  }

  public async initialize(): Promise<void> {
    if (this.isInitialized) {
      logger.info('Already initialized, skipping');
      return;
    }
    
    logger.info('Starting worker initialization');
    try {

      logger.info('Creating worker');
      this.worker = new Worker(
        new URL('../workers/graph.worker.ts', import.meta.url),
        { type: 'module' }
      );

      
      this.worker.onerror = (error) => {
        const ev = error as ErrorEvent;
        logger.error('Worker error:', {
          message: ev.message,
          filename: ev.filename,
          lineno: ev.lineno,
          colno: ev.colno,
          errorName: ev.error?.name,
          errorMessage: ev.error?.message,
          errorStack: ev.error?.stack,
          type: ev.type,
        });
      };
      this.worker.addEventListener('messageerror', (e) => {
        logger.error('Worker messageerror:', e);
      });

      logger.info('Wrapping worker with Comlink');
      
      this.workerApi = wrap<GraphWorkerType>(this.worker);

      
      logger.info('Testing worker communication');
      try {
        await this.workerApi.initialize();
        logger.info('Worker communication test successful');
      } catch (commError) {
        logger.error('Worker communication failed:', commError);
        throw new Error(`Worker communication failed: ${commError}`);
      }

      
      // Capacity sized for KG + ontology + ADR-050 stub layer. With ~22k live
      // nodes today, 100k gives ~4× headroom; SAB cost is 1.6 MB.
      const maxNodes = 100000;
      const bufferSize = maxNodes * 4 * 4;

      if (!self.crossOriginIsolated) {
        logger.warn('Cross-origin isolation is NOT active. COOP/COEP headers may be missing or stripped. SharedArrayBuffer will be unavailable.');
      }

      if (typeof SharedArrayBuffer !== 'undefined') {
        try {
          logger.info('Setting up SharedArrayBuffer');
          this.sharedBuffer = new SharedArrayBuffer(bufferSize);
          this.sharedPositionView = new Float32Array(this.sharedBuffer);
          await this.workerApi.setupSharedPositions(this.sharedBuffer);
          logger.info(`SharedArrayBuffer initialized: ${bufferSize} bytes`);
          if (debugState.isEnabled()) {
            logger.info(`Initialized SharedArrayBuffer: ${bufferSize} bytes for ${maxNodes} nodes`);
          }
        } catch (sabError) {
          logger.warn('SharedArrayBuffer construction failed, falling back to message passing:', sabError);
          this.sharedBuffer = null;
          this.sharedPositionView = null;
        }
      } else {
        logger.warn('SharedArrayBuffer not available, falling back to regular message passing');
      }

      this.isInitialized = true;
      logger.info('Initialization complete');
      if (debugState.isEnabled()) {
        logger.info('Graph worker initialized successfully');
      }

      
      logger.info(`Setting initial graph type: ${this.graphType}`);
      await this.setGraphType(this.graphType);
    } catch (error) {
      logger.error('Failed to initialize graph worker:', error);
      throw error;
    }
  }

  
  public async setGraphType(type: 'logseq' | 'visionflow'): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }

    this.graphType = type;
    await this.workerApi.setGraphType(type);

    if (debugState.isEnabled()) {
      logger.info(`Graph type set to: ${type}`);
    }
  }

  
  public getGraphType(): 'logseq' | 'visionflow' {
    return this.graphType;
  }

  
  public async setGraphData(data: GraphData): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }

    await this.workerApi.setGraphData(data);
    this.notifyGraphDataListeners(data);

    if (debugState.isEnabled()) {
      logger.info(`Set ${this.graphType} graph data: ${data.nodes.length} nodes, ${data.edges.length} edges`);
    }
  }

  
  public async processBinaryData(data: ArrayBuffer): Promise<void> {
    // All graph types process binary position data from the server.
    // Server is the single source of truth for all node positions.
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }

    try {
      const positionArray = await this.workerApi.processBinaryData(data);
      this.notifyPositionUpdateListeners(positionArray);

      // When SharedArrayBuffer is unavailable, getPositionsSync() falls back to
      // lastReceivedPositions. Fetch currentPositions (3-float-per-node format)
      // from the worker so the renderer gets fresh positions without SAB.
      if (!this.sharedPositionView) {
        const currentPositions = await this.workerApi.getCurrentPositions();
        if (currentPositions && currentPositions.length > 0) {
          this.lastReceivedPositions = currentPositions;
        }
      }
      // Reset consecutive errors on success
      this._consecutiveTickErrors = 0;

      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Processed binary data: ${positionArray.length / 4} position updates`);
      }
    } catch (error) {
      logger.error('Error processing binary data in worker:', error);
      throw error;
    }
  }

  
  public async getGraphData(): Promise<GraphData> {
    if (!this.workerApi) {
      logger.error('Worker not initialized for getGraphData');
      throw new Error('Worker not initialized');
    }
    logger.info('Getting graph data from worker');
    try {
      const data = await this.workerApi.getGraphData();
      logger.info(`Got ${data.nodes.length} nodes, ${data.edges.length} edges from worker`);
      return data;
    } catch (error) {
      logger.error('Error getting graph data from worker:', error);
      throw error;
    }
  }

  
  /**
   * FIX 2: Check if binary stream has received positions for node IDs not in
   * the current graph data. Returns true if a REST re-fetch is recommended.
   * Clears the internal unknown set after reading to avoid redundant re-fetches.
   */
  public async hasUnknownNodes(): Promise<boolean> {
    if (!this.workerApi) {
      return false;
    }
    try {
      return await this.workerApi.hasUnknownNodes();
    } catch {
      return false;
    }
  }

  public async updateNode(node: Node): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }

    await this.workerApi.updateNode(node);

    
    const graphData = await this.workerApi.getGraphData();
    this.notifyGraphDataListeners(graphData);
  }

  
  public async removeNode(nodeId: string): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }

    await this.workerApi.removeNode(nodeId);

    
    const graphData = await this.workerApi.getGraphData();
    this.notifyGraphDataListeners(graphData);
  }

  public async updateSettings(settings: any): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.updateSettings(settings);
  }

  public async pinNode(nodeId: number): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.pinNode(nodeId);
  }

  public async unpinNode(nodeId: number): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.unpinNode(nodeId);
  }

  public async updateUserDrivenNodePosition(nodeId: number, position: Vec3): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.updateUserDrivenNodePosition(nodeId, position);
  }

  public async tick(deltaTime: number): Promise<Float32Array> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    return await this.workerApi.tick(deltaTime);
  }

  /**
   * Fire-and-forget tick with concurrency guard.
   * Only one tick RPC can be in flight at a time — subsequent calls are dropped.
   * Tracks consecutive errors for worker health monitoring.
   */
  public requestTick(deltaTime: number): void {
    if (!this.workerApi || this.tickInFlight) return;
    this.tickInFlight = true;
    this.workerApi.tick(deltaTime)
      .then((positions) => {
        this.tickInFlight = false;
        this.lastReceivedPositions = positions;
        this._consecutiveTickErrors = 0;
      })
      .catch((err) => {
        this.tickInFlight = false;
        this._consecutiveTickErrors++;
        logger.error(`[WorkerHealth] tick() failed (consecutive: ${this._consecutiveTickErrors}):`, err);
        if (this._consecutiveTickErrors >= GraphWorkerProxy.MAX_CONSECUTIVE_ERRORS) {
          logger.error(`[WorkerHealth] ${this._consecutiveTickErrors} consecutive tick errors — worker may be unhealthy`);
        }
      });
  }

  /**
   * Returns the number of consecutive tick errors (0 = healthy).
   */
  public getConsecutiveErrors(): number {
    return this._consecutiveTickErrors;
  }

  /**
   * Synchronous position read — returns SharedArrayBuffer view (zero-copy)
   * or cached positions from the last completed tick RPC as fallback.
   */
  public getPositionsSync(): Float32Array | null {
    return this.sharedPositionView || this.lastReceivedPositions;
  }

  /**
   * Return per-node client-fallback analytics (degree-based anomaly scoring +
   * Louvain community detection) computed by the worker. Layout: Float32Array
   * of [clusterId, anomalyScore, communityId] per node, indexed by node order
   * in graphData.nodes.
   *
   * NOTE (ADR-061): server-emitted analytics now ride the `analytics_update`
   * side stream into useAnalyticsStore — this method is retained only for the
   * worker's degree-based fallback when the server has not (yet) emitted any
   * cluster/community/anomaly data.
   */
  public async getAnalyticsBuffer(): Promise<Float32Array> {
    if (!this.workerApi) {
      return new Float32Array(0);
    }
    return await this.workerApi.getAnalyticsBuffer();
  }

  /**
   * Recompute client-side analytics (anomaly scores + community detection).
   * Only computes if the server hasn't already provided analytics data.
   */
  public async recomputeAnalytics(): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.recomputeAnalytics();
  }

  /**
   * Reheat the force simulation (restart physics from current positions).
   * Use this when user wants to re-layout or after significant changes.
   */
  public async reheatSimulation(alpha: number = 1.0): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.reheatSimulation(alpha);

    if (debugState.isEnabled()) {
      logger.info(`Reheated simulation to alpha=${alpha}`);
    }
  }

  /**
   * Update force-directed physics settings.
   */
  public async updateForcePhysicsSettings(settings: Partial<ForcePhysicsSettings>): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.updateForcePhysicsSettings(settings);
  }

  /**
   * Get current force physics settings.
   */
  public async getForcePhysicsSettings(): Promise<ForcePhysicsSettings> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    return await this.workerApi.getForcePhysicsSettings();
  }

  /**
   * Update client-side tweening configuration (does NOT affect server physics).
   * Controls how smoothly the client interpolates toward server-computed positions.
   */
  public async setTweeningSettings(settings: Partial<{
    enabled: boolean;
    lerpBase: number;
    snapThreshold: number;
    maxDivergence: number;
  }>): Promise<void> {
    if (!this.workerApi) {
      throw new Error('Worker not initialized');
    }
    await this.workerApi.setTweeningSettings(settings);
    if (debugState.isEnabled()) {
      logger.info('Tweening settings updated:', settings);
    }
  }

  
  public getSharedPositionBuffer(): Float32Array | null {
    return this.sharedPositionView;
  }

  
  public onGraphDataChange(listener: GraphDataChangeListener): () => void {
    this.graphDataListeners.push(listener);

    
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

  
  public isReady(): boolean {
    return this.isInitialized && this.workerApi !== null;
  }

  
  public dispose(): void {
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }

    this.workerApi = null;
    this.graphDataListeners = [];
    this.positionUpdateListeners = [];
    this.sharedBuffer = null;
    this.sharedPositionView = null;
    this.lastReceivedPositions = null;
    this.tickInFlight = false;
    this.isInitialized = false;

    if (debugState.isEnabled()) {
      logger.info('Graph worker disposed');
    }
  }

  private notifyGraphDataListeners(data: GraphData): void {
    this.graphDataListeners.forEach(listener => {
      try {
        listener(data);
      } catch (error) {
        logger.error('Error in graph data listener:', error);
      }
    });
  }

  private notifyPositionUpdateListeners(positions: Float32Array): void {
    this.positionUpdateListeners.forEach(listener => {
      try {
        listener(positions);
      } catch (error) {
        logger.error('Error in position update listener:', error);
      }
    });
  }
}

// Create singleton instance
export const graphWorkerProxy = GraphWorkerProxy.getInstance();

// Re-export types for convenience
export type { ForcePhysicsSettings } from '../workers/graph.worker';