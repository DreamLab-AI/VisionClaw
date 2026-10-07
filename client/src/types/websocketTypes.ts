

// Base WebSocket message structure
export interface BaseWebSocketMessage {
  type: string;
  timestamp: number;
  clientId?: string;
  sessionId?: string;
}

// Workspace-related messages
export interface WorkspaceUpdateMessage extends BaseWebSocketMessage {
  type: 'workspace_update';
  data: {
    workspaceId: string;
    changes: Partial<{
      name: string;
      description: string;
      status: 'active' | 'archived';
      memberCount: number;
      favorite: boolean;
      lastAccessed: string;
      settings: Record<string, unknown>;
    }>;
    userId?: string;
    operation: 'create' | 'update' | 'delete' | 'favorite' | 'archive';
  };
}

export interface WorkspaceDeletedMessage extends BaseWebSocketMessage {
  type: 'workspace_deleted';
  data: {
    workspaceId: string;
    userId?: string;
  };
}

export interface WorkspaceCollaborationMessage extends BaseWebSocketMessage {
  type: 'workspace_collaboration';
  data: {
    workspaceId: string;
    action: 'user_joined' | 'user_left' | 'permission_changed';
    userId: string;
    userName?: string;
    permissions?: string[];
  };
}

// Analysis-related messages
export interface AnalysisProgressMessage extends BaseWebSocketMessage {
  type: 'analysis_progress';
  data: {
    analysisId: string;
    graphId?: string;
    progress: number;
    stage: string;
    estimatedTimeRemaining?: number;
    currentOperation: string;
    metrics?: {
      nodesProcessed: number;
      edgesProcessed: number;
      clustersFound?: number;
      similarityScore?: number;
    };
  };
}

export interface AnalysisCompleteMessage extends BaseWebSocketMessage {
  type: 'analysis_complete';
  data: {
    analysisId: string;
    graphId?: string;
    results: {
      similarity: {
        overall: number;
        structural: number;
        semantic: number;
      };
      matches: number;
      differences: number;
      clusters: number;
      centrality: {
        betweenness: number;
        closeness: number;
        eigenvector: number;
      };
      processing_time: number;
    };
    success: boolean;
    error?: string;
  };
}

export interface AnalysisErrorMessage extends BaseWebSocketMessage {
  type: 'analysis_error';
  data: {
    analysisId: string;
    graphId?: string;
    error: string;
    stage: string;
    retryable: boolean;
  };
}

// Optimization-related messages
export interface OptimizationUpdateMessage extends BaseWebSocketMessage {
  type: 'optimization_update';
  data: {
    optimizationId: string;
    graphId?: string;
    progress: number;
    algorithm: string;
    currentIteration: number;
    totalIterations: number;
    metrics: {
      performanceGain: number;
      confidence: number;
      energyLevel?: number;
      convergence?: number;
    };
    recommendations?: Array<{
      type: string;
      priority: 'low' | 'medium' | 'high';
      description: string;
    }>;
  };
}

export interface OptimizationResultMessage extends BaseWebSocketMessage {
  type: 'optimization_result';
  data: {
    optimizationId: string;
    graphId?: string;
    algorithm: string;
    confidence: number;
    performanceGain: number;
    clusters: number;
    recommendations: Array<{
      type: string;
      priority: 'low' | 'medium' | 'high';
      description: string;
    }>;
    layoutChanges?: {
      nodesRepositioned: number;
      clustersFormed: number;
      edgesOptimized: number;
    };
    success: boolean;
    error?: string;
  };
}

// Export-related messages
export interface ExportProgressMessage extends BaseWebSocketMessage {
  type: 'export_progress';
  data: {
    exportId: string;
    graphId?: string;
    format: string;
    progress: number;
    stage: 'preparing' | 'processing' | 'rendering' | 'finalizing' | 'uploading';
    size?: number;
    estimatedTimeRemaining?: number;
  };
}

export interface ExportReadyMessage extends BaseWebSocketMessage {
  type: 'export_ready';
  data: {
    exportId: string;
    graphId?: string;
    format: string;
    downloadUrl: string;
    size: number;
    expiresAt: string;
    metadata: {
      resolution?: string;
      compressionUsed: boolean;
      includedMetadata: boolean;
    };
  };
}

export interface ShareCreatedMessage extends BaseWebSocketMessage {
  type: 'share_created';
  data: {
    shareId: string;
    graphId?: string;
    shareUrl: string;
    expiresAt?: string;
    passwordProtected: boolean;
    permissions: string[];
    description?: string;
  };
}

export interface ShareAccessMessage extends BaseWebSocketMessage {
  type: 'share_access';
  data: {
    shareId: string;
    action: 'accessed' | 'downloaded' | 'expired';
    userId?: string;
    ipAddress?: string;
    userAgent?: string;
  };
}

// System and connection messages
export interface ConnectionStatusMessage extends BaseWebSocketMessage {
  type: 'connection_status';
  data: {
    status: 'connected' | 'disconnected' | 'reconnecting' | 'error';
    serverLoad?: number;
    latency?: number;
    features: string[];
  };
}

export interface SystemNotificationMessage extends BaseWebSocketMessage {
  type: 'system_notification';
  data: {
    level: 'info' | 'warning' | 'error';
    title: string;
    message: string;
    actions?: Array<{
      label: string;
      action: string;
    }>;
    persistent?: boolean;
  };
}

export interface UserActivityMessage extends BaseWebSocketMessage {
  type: 'user_activity';
  data: {
    userId: string;
    userName?: string;
    action: string;
    resource: string;
    resourceId?: string;
    metadata?: Record<string, unknown>;
  };
}

// Performance and metrics messages
export interface PerformanceMetricsMessage extends BaseWebSocketMessage {
  type: 'performance_metrics';
  data: {
    metrics: {
      cpu: number;
      memory: number;
      network: number;
      renderTime: number;
      frameRate: number;
    };
    graphId?: string;
    nodeCount?: number;
    edgeCount?: number;
  };
}

export interface ServerHealthMessage extends BaseWebSocketMessage {
  type: 'server_health';
  data: {
    status: 'healthy' | 'degraded' | 'unhealthy';
    services: Array<{
      name: string;
      status: 'up' | 'down' | 'degraded';
      latency?: number;
    }>;
    load: {
      cpu: number;
      memory: number;
      activeConnections: number;
    };
  };
}

// Graph Interaction messages
export interface GraphProcessingProgressMessage extends BaseWebSocketMessage {
  type: 'graph_processing_progress';
  data: {
    taskId: string;
    graphId?: string;
    progress: number;
    stage: string;
    currentOperation: string;
    estimatedTimeRemaining?: number;
    metrics?: {
      stepsProcessed: number;
      totalSteps: number;
      currentStep: string;
      operationsCompleted: number;
    };
  };
}

export interface GraphProcessingCompleteMessage extends BaseWebSocketMessage {
  type: 'graph_processing_complete';
  data: {
    taskId: string;
    graphId?: string;
    success: boolean;
    results?: unknown;
    processedSteps?: number;
    totalTime?: number;
    error?: string;
  };
}

export interface GraphProcessingErrorMessage extends BaseWebSocketMessage {
  type: 'graph_processing_error';
  data: {
    taskId: string;
    graphId?: string;
    error: string;
    stage: string;
    retryable: boolean;
  };
}

export interface TimeTraverseProgressMessage extends BaseWebSocketMessage {
  type: 'time_traverse_progress';
  data: {
    taskId: string;
    graphId?: string;
    progress: number;
    stage: string;
    currentStep: number;
    totalSteps: number;
    stepName?: string;
    estimatedTimeRemaining?: number;
  };
}

export interface TimeTraverseCompleteMessage extends BaseWebSocketMessage {
  type: 'time_traverse_complete';
  data: {
    taskId: string;
    graphId?: string;
    success: boolean;
    totalSteps: number;
    timeline?: unknown;
    processingTime?: number;
    error?: string;
  };
}

export interface CollaborationSessionMessage extends BaseWebSocketMessage {
  type: 'collaboration_session';
  data: {
    sessionId: string;
    action: 'created' | 'user_joined' | 'user_left' | 'ended';
    userId?: string;
    userName?: string;
    participantCount: number;
    shareUrl?: string;
  };
}

export interface VRARModeMessage extends BaseWebSocketMessage {
  type: 'vr_ar_mode';
  data: {
    sessionId: string;
    mode: 'vr' | 'ar' | 'disabled';
    userId?: string;
    features: {
      handTracking: boolean;
      hapticFeedback: boolean;
      spatialAudio?: boolean;
    };
  };
}

export interface ExplorationTourMessage extends BaseWebSocketMessage {
  type: 'exploration_tour';
  data: {
    tourId: string;
    action: 'created' | 'updated' | 'waypoint_added' | 'completed';
    waypoints?: Array<{
      nodeId: string;
      description: string;
      order: number;
    }>;
    progress?: number;
  };
}

// Node drag messages (server-side drag handling)
export interface NodeDragStartMessage extends BaseWebSocketMessage {
  type: 'nodeDragStart';
  data: {
    nodeId: number;
    position: { x: number; y: number; z: number };
  };
}

export interface NodeDragUpdateMessage extends BaseWebSocketMessage {
  type: 'nodeDragUpdate';
  data: {
    nodeId: number;
    position: { x: number; y: number; z: number };
    timestamp: number;
  };
}

export interface NodeDragEndMessage extends BaseWebSocketMessage {
  type: 'nodeDragEnd';
  data: {
    nodeId: number;
  };
}

export interface NodeDragStartAckMessage extends BaseWebSocketMessage {
  type: 'nodeDragStartAck';
  data: {
    nodeId: number;
  };
}

export interface NodeDragEndAckMessage extends BaseWebSocketMessage {
  type: 'nodeDragEndAck';
  data: {
    nodeId: number;
  };
}

// Connection and protocol messages
export interface ConnectionEstablishedMessage extends BaseWebSocketMessage {
  type: 'connection_established';
  data?: {
    serverVersion?: string;
    features?: string[];
  };
}

export interface ErrorMessage extends BaseWebSocketMessage {
  type: 'error';
  error: string;
  data?: Record<string, unknown>;
}

export interface FilterConfirmedMessage extends BaseWebSocketMessage {
  type: 'filter_update_success';
  data: {
    visible_nodes: number;
    total_nodes: number;
  };
}

export interface InitialGraphLoadMessage extends BaseWebSocketMessage {
  type: 'initialGraphLoad';
  nodes: Array<{
    id: string | number;
    label?: string;
    name?: string;
    position?: { x: number; y: number; z: number };
    x?: number;
    y?: number;
    z?: number;
    metadata?: Record<string, unknown>;
    quality_score?: number;
    authority_score?: number;
    color?: string;
    size?: number;
  }>;
  edges: Array<{
    id?: string;
    source: string | number;
    target: string | number;
    weight?: number;
    label?: string;
  }>;
}

// Memory flash event — RuVector memory access visualization
export interface MemoryFlashMessage extends BaseWebSocketMessage {
  type: 'memory_flash';
  data: {
    key: string;
    namespace: string;
    action: string;
    timestamp: number;
    /** wire id of the agent that touched memory, when the producer named it (ADR-2135) */
    agentId?: number;
  };
}

// Live linkage: server-side reasoned-graph change signal (sync reload,
// runtime node/edge mutation, ontology axiom write, inference results).
// Client responds with a debounced topology + inferred-axioms refetch.
export interface GraphUpdatedMessage extends BaseWebSocketMessage {
  type: 'graphUpdated';
  revision?: number;
  reason?: string;
}

// ADR-2047 (vc-core): live settings-change broadcast. Only the "nodeFilter"
// category carries a `settings` payload — other categories are a signal for
// the client to re-read that category from the settings API.
export interface SettingsUpdatedWireMessage extends BaseWebSocketMessage {
  type: 'settingsUpdated';
  category: 'physics' | 'rendering' | 'nodeFilter' | string;
  updatedBy: string;
  settings?: {
    enabled?: boolean;
    qualityThreshold?: number;
    authorityThreshold?: number;
    filterByQuality?: boolean;
    filterByAuthority?: boolean;
    filterMode?: string;
    includeLinkedPages?: boolean;
  };
}

// Union type of all possible WebSocket messages
export type WebSocketMessage =
  | GraphUpdatedMessage
  | SettingsUpdatedWireMessage
  | WorkspaceUpdateMessage
  | WorkspaceDeletedMessage
  | WorkspaceCollaborationMessage
  | AnalysisProgressMessage
  | AnalysisCompleteMessage
  | AnalysisErrorMessage
  | OptimizationUpdateMessage
  | OptimizationResultMessage
  | ExportProgressMessage
  | ExportReadyMessage
  | ShareCreatedMessage
  | ShareAccessMessage
  | ConnectionStatusMessage
  | SystemNotificationMessage
  | UserActivityMessage
  | PerformanceMetricsMessage
  | ServerHealthMessage
  | GraphProcessingProgressMessage
  | GraphProcessingCompleteMessage
  | GraphProcessingErrorMessage
  | TimeTraverseProgressMessage
  | TimeTraverseCompleteMessage
  | CollaborationSessionMessage
  | VRARModeMessage
  | ExplorationTourMessage
  | NodeDragStartMessage
  | NodeDragUpdateMessage
  | NodeDragEndMessage
  | NodeDragStartAckMessage
  | NodeDragEndAckMessage
  | ConnectionEstablishedMessage
  | ErrorMessage
  | FilterConfirmedMessage
  | InitialGraphLoadMessage
  | MemoryFlashMessage;

// Event handler types
export type MessageHandler<T extends WebSocketMessage = WebSocketMessage> = (message: T) => void;

export interface WebSocketEventHandlers {

  workspace_update: MessageHandler<WorkspaceUpdateMessage>;
  workspace_deleted: MessageHandler<WorkspaceDeletedMessage>;
  workspace_collaboration: MessageHandler<WorkspaceCollaborationMessage>;


  analysis_progress: MessageHandler<AnalysisProgressMessage>;
  analysis_complete: MessageHandler<AnalysisCompleteMessage>;
  analysis_error: MessageHandler<AnalysisErrorMessage>;


  optimization_update: MessageHandler<OptimizationUpdateMessage>;
  optimization_result: MessageHandler<OptimizationResultMessage>;


  export_progress: MessageHandler<ExportProgressMessage>;
  export_ready: MessageHandler<ExportReadyMessage>;
  share_created: MessageHandler<ShareCreatedMessage>;
  share_access: MessageHandler<ShareAccessMessage>;


  connection_status: MessageHandler<ConnectionStatusMessage>;
  system_notification: MessageHandler<SystemNotificationMessage>;
  user_activity: MessageHandler<UserActivityMessage>;
  performance_metrics: MessageHandler<PerformanceMetricsMessage>;
  server_health: MessageHandler<ServerHealthMessage>;


  graph_processing_progress: MessageHandler<GraphProcessingProgressMessage>;
  graph_processing_complete: MessageHandler<GraphProcessingCompleteMessage>;
  graph_processing_error: MessageHandler<GraphProcessingErrorMessage>;
  time_traverse_progress: MessageHandler<TimeTraverseProgressMessage>;
  time_traverse_complete: MessageHandler<TimeTraverseCompleteMessage>;
  collaboration_session: MessageHandler<CollaborationSessionMessage>;
  vr_ar_mode: MessageHandler<VRARModeMessage>;
  exploration_tour: MessageHandler<ExplorationTourMessage>;
}

// Configuration interfaces
export interface WebSocketConfig {
  url?: string;
  protocols?: string[];
  reconnect: {
    maxAttempts: number;
    baseDelay: number;
    maxDelay: number;
    backoffFactor: number;
  };
  heartbeat: {
    interval: number;
    timeout: number;
  };
  compression: boolean;
  binaryProtocol: boolean;
}

export interface WebSocketConnectionState {
  status: 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed';
  lastConnected?: number;
  lastError?: string;
  reconnectAttempts: number;
  serverFeatures: string[];
  latency?: number;
}

// Subscription management
export interface Subscription {
  id: string;
  type: keyof WebSocketEventHandlers;
  handler: MessageHandler;
  options?: {
    once?: boolean;
    filter?: (message: WebSocketMessage) => boolean;
  };
}

export interface SubscriptionFilters {
  workspaceId?: string;
  userId?: string;
  graphId?: string;
  analysisId?: string;
  optimizationId?: string;
  exportId?: string;
}

// Error types
export interface WebSocketError {
  code: string;
  message: string;
  type: 'connection' | 'protocol' | 'auth' | 'rate_limit' | 'server' | 'client';
  retryable: boolean;
  retryAfter?: number;
  context?: Record<string, unknown>;
}

// Statistics and monitoring
export interface WebSocketStatistics {
  messagesReceived: number;
  messagesSent: number;
  bytesReceived: number;
  bytesSent: number;
  connectionTime: number;
  reconnections: number;
  averageLatency: number;
  messagesByType: Record<string, number>;
  errors: number;
  lastActivity: number;
}