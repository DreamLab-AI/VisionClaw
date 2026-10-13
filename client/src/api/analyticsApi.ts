import { createLogger } from '../utils/loggerConfig';
import { createErrorMetadata } from '../utils/loggerConfig';
import { debugState } from '../utils/clientDebugState';
import { unifiedApiClient, isApiError } from '../services/api/UnifiedApiClient';
import { nostrAuth } from '../services/nostrAuthService';

const logger = createLogger('AnalyticsAPI');

// Type definitions for analytics API
export interface VisualAnalyticsParams {
  clustering_enabled: boolean;
  cluster_resolution: number;
  spatial_sampling: boolean;
  edge_bundling: boolean;
  bundling_strength: number;
  layout_algorithm: string;
  stress_majorization_enabled: boolean;
  community_detection_enabled: boolean;
  anomaly_detection_enabled: boolean;
  real_time_updates: boolean;
  performance_mode: string;
  gpu_acceleration: boolean;
}

export interface ConstraintSet {
  node_constraints: Array<{
    node_id: string;
    fixed_position?: { x: number; y: number; z?: number };
    movement_bounds?: { min_x: number; max_x: number; min_y: number; max_y: number };
  }>;
  edge_constraints: Array<{
    edge_id: string;
    target_length?: number;
    strength?: number;
  }>;
  global_constraints: {
    max_iterations: number;
    convergence_threshold: number;
    cooling_factor: number;
  };
}

export interface AnalysisTask {
  task_id: string;
  task_type: 'structural' | 'semantic' | 'clustering' | 'community';
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
  progress: number;
  start_time: string;
  estimated_completion?: string;
  result?: any;
  error?: string;
}

export interface StructuralAnalysisRequest {
  graph_data: {
    nodes: Array<{ id: string; [key: string]: any }>;
    edges: Array<{ source: string; target: string; [key: string]: any }>;
  };
  analysis_type: 'comprehensive' | 'centrality' | 'clustering' | 'connectivity';
  options?: {
    include_centrality?: boolean;
    include_clustering?: boolean;
    include_connectivity?: boolean;
    cluster_resolution?: number;
  };
}

export interface SemanticAnalysisRequest {
  graph_data: {
    nodes: Array<{ id: string; content?: string; metadata?: any }>;
    edges: Array<{ source: string; target: string; weight?: number }>;
  };
  analysis_type: 'similarity' | 'topics' | 'relationships';
  options?: {
    similarity_threshold?: number;
    topic_count?: number;
    embedding_model?: string;
  };
}

export interface GPUPerformanceStats {
  gpu_enabled: boolean;
  compute_mode: string;
  kernel_mode: string;
  nodes_count: number;
  edges_count: number;
  iteration_count: number;
  kinetic_energy: number;
  total_forces: number;
  num_constraints: number;
  gpu_failure_count: number;
  stress_majorization_stats: {
    total_runs: number;
    successful_runs: number;
    failed_runs: number;
    success_rate: number;
    average_computation_time_ms: number;
    is_emergency_stopped: boolean;
    emergency_stop_reason: string;
  };
}

export interface ClusteringRequest {
  algorithm: 'louvain' | 'leiden' | 'spectral' | 'kmeans';
  resolution?: number;
  iterations?: number;
  min_cluster_size?: number;
  options?: {
    gpu_accelerated?: boolean;
    real_time_updates?: boolean;
  };
}

export interface AnomalyDetectionConfig {
  enabled: boolean;
  sensitivity: number;
  detection_method: 'statistical' | 'ml' | 'hybrid';
  threshold_multiplier: number;
  update_interval_ms: number;
}


/** Server-computed directed cycles (`GET /api/analytics/cycles`). */
export interface CycleReport {
  nodeCount: number;
  edgeCount: number;
  /** `subsumption` (default), `all`, or the exact edge type requested. */
  edgeFilter: string;
  cyclicNodeIds: number[];
  /** Multi-node strongly connected components, largest first. */
  components: number[][];
}

export class AnalyticsAPI {
  private websocket: WebSocket | null = null;
  private taskSubscriptions = new Map<string, (task: AnalysisTask) => void>();
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;

  
  public async getAnalyticsParams(): Promise<VisualAnalyticsParams> {
    try {
      logger.debug('Fetching analytics parameters');
      const response = await unifiedApiClient.getData<{
        success: boolean;
        params: VisualAnalyticsParams;
        error?: string;
      }>('/analytics/params');

      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch analytics parameters');
      }

      return response.params;
    } catch (error) {
      logger.error('Failed to fetch analytics parameters:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async updateAnalyticsParams(params: Partial<VisualAnalyticsParams>): Promise<boolean> {
    try {
      logger.debug('Updating analytics parameters:', params);
      const response = await unifiedApiClient.postData<{
        success: boolean;
        error?: string;
      }>('/analytics/params', params);

      if (!response.success) {
        throw new Error(response.error || 'Failed to update analytics parameters');
      }

      return true;
    } catch (error) {
      logger.error('Failed to update analytics parameters:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async getConstraints(): Promise<ConstraintSet> {
    try {
      const response = await unifiedApiClient.getData<{
        success: boolean;
        constraints: ConstraintSet;
        error?: string;
      }>('/analytics/constraints');

      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch constraints');
      }

      return response.constraints;
    } catch (error) {
      logger.error('Failed to fetch constraints:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async updateConstraints(constraints: Partial<ConstraintSet>): Promise<boolean> {
    try {
      const response = await unifiedApiClient.postData<{
        success: boolean;
        error?: string;
      }>('/analytics/constraints', constraints);

      if (!response.success) {
        throw new Error(response.error || 'Failed to update constraints');
      }

      return true;
    } catch (error) {
      logger.error('Failed to update constraints:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async runStructuralAnalysis(request: StructuralAnalysisRequest): Promise<string> {
    try {
      logger.info('Starting structural analysis');
      const response = await unifiedApiClient.postData<{
        success: boolean;
        task_id: string;
        error?: string;
      }>('/analytics/clustering/run', {
        algorithm: 'louvain',
        resolution: request.options?.cluster_resolution || 1.0,
        gpu_accelerated: true,
        ...request
      });

      if (!response.success) {
        throw new Error(response.error || 'Failed to start structural analysis');
      }

      return response.task_id;
    } catch (error) {
      logger.error('Failed to run structural analysis:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async runSemanticAnalysis(request: SemanticAnalysisRequest): Promise<string> {
    try {
      logger.info('Starting semantic analysis');
      
      // Note: request already contains analysis_type, override it for semantic analysis
      const response = await unifiedApiClient.postData<{
        success: boolean;
        task_id: string;
        error?: string;
      }>('/analytics/insights', {
        ...request,
        analysis_type: 'semantic' as const
      });

      if (!response.success) {
        throw new Error(response.error || 'Failed to start semantic analysis');
      }

      return response.task_id;
    } catch (error) {
      logger.error('Failed to run semantic analysis:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async getTaskStatus(taskId: string): Promise<AnalysisTask> {
    try {
      const response = await unifiedApiClient.getData<{
        success: boolean;
        task: AnalysisTask;
        error?: string;
      }>(`/analytics/clustering/status?task_id=${encodeURIComponent(taskId)}`);

      if (!response.success) {
        throw new Error(response.error || 'Failed to get task status');
      }

      return response.task;
    } catch (error) {
      logger.error(`Failed to get task status for ${taskId}:`, createErrorMetadata(error));
      throw error;
    }
  }

  
  public async getAnalysisResults(taskId: string): Promise<any> {
    try {
      const task = await this.getTaskStatus(taskId);

      if (task.status === 'failed') {
        throw new Error(task.error || 'Analysis task failed');
      }

      if (task.status !== 'completed') {
        throw new Error('Analysis task not completed yet');
      }

      return task.result;
    } catch (error) {
      logger.error(`Failed to get analysis results for ${taskId}:`, createErrorMetadata(error));
      throw error;
    }
  }

  
  public async cancelTask(taskId: string): Promise<boolean> {
    try {
      const response = await unifiedApiClient.postData<{
        success: boolean;
        error?: string;
      }>(`/analytics/clustering/cancel?task_id=${encodeURIComponent(taskId)}`, {});

      return response.success;
    } catch (error) {
      logger.error(`Failed to cancel task ${taskId}:`, createErrorMetadata(error));
      return false;
    }
  }

  
  public async getPerformanceStats(): Promise<GPUPerformanceStats> {
    try {
      const response = await unifiedApiClient.getData<{
        success: boolean;
        stats: GPUPerformanceStats;
        error?: string;
      }>('/analytics/stats');

      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch performance stats');
      }

      return response.stats;
    } catch (error) {
      logger.error('Failed to fetch performance stats:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async getGPUStatus(): Promise<{
    gpu_available: boolean;
    compute_mode: string;
    features: string[];
    performance: GPUPerformanceStats;
  }> {
    try {
      const response = await unifiedApiClient.getData<{
        success: boolean;
        gpu_status: any;
        error?: string;
      }>('/analytics/gpu-status');

      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch GPU status');
      }

      return response.gpu_status;
    } catch (error) {
      logger.error('Failed to fetch GPU status:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async runClustering(request: ClusteringRequest): Promise<string> {
    try {
      const response = await unifiedApiClient.postData<{
        success: boolean;
        task_id: string;
        error?: string;
      }>('/analytics/clustering/run', request);

      if (!response.success) {
        throw new Error(response.error || 'Failed to start clustering analysis');
      }

      return response.task_id;
    } catch (error) {
      logger.error('Failed to run clustering:', createErrorMetadata(error));
      throw error;
    }
  }

  
  public async configureAnomalyDetection(config: AnomalyDetectionConfig): Promise<boolean> {
    try {
      const response = await unifiedApiClient.postData<{
        success: boolean;
        error?: string;
      }>('/analytics/anomaly/toggle', config);

      return response.success;
    } catch (error) {
      logger.error('Failed to configure anomaly detection:', createErrorMetadata(error));
      return false;
    }
  }

  
  /** Nodes on a directed cycle. Defaults to subClassOf-asserting edges; pass `'all'` for every edge. */
  public async getCycles(edgeType?: string): Promise<CycleReport> {
    const query = edgeType ? `?edgeType=${encodeURIComponent(edgeType)}` : '';
    const response = await unifiedApiClient.getData<{
      success: boolean;
      data?: CycleReport;
      error?: string;
    }>(`/analytics/cycles${query}`);

    if (!response.success || !response.data) {
      throw new Error(response.error || 'Failed to fetch cycle report');
    }
    return response.data;
  }

  public async getCurrentAnomalies(): Promise<any[]> {
    try {
      const response = await unifiedApiClient.getData<{
        success: boolean;
        anomalies: any[];
        error?: string;
      }>('/analytics/anomaly/current');

      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch anomalies');
      }

      return response.anomalies;
    } catch (error) {
      logger.error('Failed to fetch current anomalies:', createErrorMetadata(error));
      return [];
    }
  }

  
  public subscribeToTask(taskId: string, callback: (task: AnalysisTask) => void): () => void {
    this.taskSubscriptions.set(taskId, callback);
    this.ensureWebSocketConnection();

    return () => {
      this.taskSubscriptions.delete(taskId);
      if (this.taskSubscriptions.size === 0) {
        this.disconnectWebSocket();
      }
    };
  }

  
  private ensureWebSocketConnection(): void {
    if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
      return;
    }

    const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${wsProtocol}//${window.location.host}/ws/analytics`;

    logger.debug('Connecting to analytics WebSocket:', wsUrl);

    this.websocket = new WebSocket(wsUrl);

    this.websocket.onopen = () => {
      logger.info('Analytics WebSocket connected');
      this.reconnectAttempts = 0;

      // Send NIP-98 auth as first message
      const user = nostrAuth.getCurrentUser();
      if (user?.pubkey) {
        if (nostrAuth.isDevMode()) {
          this.websocket?.send(JSON.stringify({ type: 'auth', token: 'dev-session-token' }));
        } else {
          (async () => {
            try {
              const httpUrl = wsUrl.replace(/^ws(s?):\/\//, 'http$1://');
              const eventToken = await nostrAuth.signRequest(httpUrl, 'GET');
              this.websocket?.send(JSON.stringify({ type: 'authenticate', event: eventToken }));
            } catch (e) {
              logger.error('NIP-98 analytics WS auth failed:', e);
            }
          })();
        }
      }


      const taskIds = Array.from(this.taskSubscriptions.keys());
      for (const taskId of taskIds) {
        this.websocket?.send(JSON.stringify({
          type: 'subscribe',
          task_id: taskId
        }));
      }
    };

    this.websocket.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data.type === 'task_update' && data.task) {
          const callback = this.taskSubscriptions.get(data.task.task_id);
          if (callback) {
            callback(data.task);
          }
        }
      } catch (error) {
        logger.error('Failed to parse WebSocket message:', createErrorMetadata(error));
      }
    };

    this.websocket.onclose = () => {
      logger.warn('Analytics WebSocket disconnected');
      this.websocket = null;

      if (this.taskSubscriptions.size > 0 && this.reconnectAttempts < this.maxReconnectAttempts) {
        this.reconnectAttempts++;
        setTimeout(() => this.ensureWebSocketConnection(), 1000 * this.reconnectAttempts);
      }
    };

    this.websocket.onerror = (error) => {
      logger.error('Analytics WebSocket error:', error);
    };
  }

  
  private disconnectWebSocket(): void {
    if (this.websocket) {
      this.websocket.close();
      this.websocket = null;
    }
    this.reconnectAttempts = 0;
  }

  
  public async pollForCompletion(
    taskId: string,
    maxAttempts: number = 30,
    initialDelay: number = 1000
  ): Promise<any> {
    let attempts = 0;
    let delay = initialDelay;

    while (attempts < maxAttempts) {
      try {
        const task = await this.getTaskStatus(taskId);

        if (task.status === 'completed') {
          return task.result;
        } else if (task.status === 'failed') {
          throw new Error(task.error || 'Analysis task failed');
        } else if (task.status === 'cancelled') {
          throw new Error('Analysis task was cancelled');
        }

        await new Promise(resolve => setTimeout(resolve, delay));
        delay = Math.min(delay * 1.5, 5000); 
        attempts++;
      } catch (error) {
        if (attempts === maxAttempts - 1) {
          throw error;
        }
        await new Promise(resolve => setTimeout(resolve, delay));
        attempts++;
      }
    }

    throw new Error(`Task ${taskId} did not complete within ${maxAttempts} attempts`);
  }

  
  public cleanup(): void {
    this.disconnectWebSocket();
    this.taskSubscriptions.clear();
  }
}

// Export singleton instance
export const analyticsAPI = new AnalyticsAPI();

// Cleanup on page unload
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    analyticsAPI.cleanup();
  });
}