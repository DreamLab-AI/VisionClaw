import type { WebSocketMessage, WebSocketStatistics } from '../../types/websocketTypes';
import type { NodePositionBatchQueue } from '../../utils/BatchQueue';
import type { NodeType } from '../../types/binaryProtocol';

// WebSocketAdapter interface for components that need to send binary data
// plus typed JSON control messages (node drag pin/unpin protocol).
export interface WebSocketAdapter {
  send: (data: ArrayBuffer) => void;
  sendMessage: (type: string, data?: unknown) => void;
  isReady: () => boolean;
}

// Re-export types for backward compatibility
export interface WebSocketErrorFrame {
  code: string;
  message: string;
  category: 'validation' | 'server' | 'protocol' | 'auth' | 'rate_limit';
  details?: unknown;
  retryable: boolean;
  retryAfter?: number;
  affectedPaths?: string[];
  timestamp: number;
}

export interface QueuedMessage {
  type: 'text' | 'binary';
  data: string | ArrayBuffer;
  timestamp: number;
  retries: number;
}

export interface ConnectionState {
  status: 'disconnected' | 'connecting' | 'connected' | 'reconnecting' | 'failed';
  lastConnected?: number;
  lastError?: string;
  reconnectAttempts: number;
}

export interface SolidNotification {
  type: 'pub' | 'ack';
  url: string;
}

export type SolidNotificationCallback = (notification: SolidNotification) => void;

// ADR-2047 (vc-core): live settings-change broadcast. Only the "nodeFilter"
// category carries a `settings` payload — other categories are a signal to
// re-read that category from the settings API.
export type SettingsUpdatedCategory = 'physics' | 'rendering' | 'nodeFilter' | (string & {});

export interface SettingsUpdatedMessage {
  type: 'settingsUpdated';
  category: SettingsUpdatedCategory;
  updatedBy: string;
  timestamp: number;
  settings?: FilterSnapshot;
}

export type LegacyMessageHandler = (message: WebSocketMessage) => void;
export type BinaryMessageHandler = (data: ArrayBuffer) => void;
export type ConnectionStatusHandler = (connected: boolean) => void;
export type ConnectionStateHandler = (state: ConnectionState) => void;
export type EventHandler = (data: unknown) => void;

export interface FilterUpdateParams {
  enabled?: boolean;
  qualityThreshold?: number;
  authorityThreshold?: number;
  filterByQuality?: boolean;
  filterByAuthority?: boolean;
  filterMode?: string;
  includeLinkedPages?: boolean;
  /** Opt-in: the server also removes non-matching nodes (default false). */
  serverRemove?: boolean;
}

export interface NodePositionUpdate {
  nodeId: number;
  position: { x: number; y: number; z: number };
  velocity?: { x: number; y: number; z: number };
}

export interface WebSocketInternals {
  messageHandlers: LegacyMessageHandler[];
  binaryMessageHandlers: BinaryMessageHandler[];
  connectionStatusHandlers: ConnectionStatusHandler[];
  connectionStateHandlers: ConnectionStateHandler[];
  eventHandlers: Map<string, EventHandler[]>;
  positionBatchQueue: NodePositionBatchQueue | null;
  heartbeatInterval: number | null;
  reconnectTimeout: number | null;
}

// P2 PERFORMANCE FIX: Track individual filter fields instead of serializing
// the entire settings tree on every state change.
export interface FilterSnapshot {
  enabled?: boolean;
  qualityThreshold?: number;
  authorityThreshold?: number;
  filterByQuality?: boolean;
  filterByAuthority?: boolean;
  filterMode?: string;
  includeLinkedPages?: boolean;
  serverRemove?: boolean;
}

// Store state interface
export interface WebSocketState {
  // Connection state
  socket: WebSocket | null;
  isConnected: boolean;
  isServerReady: boolean;
  connectionState: ConnectionState;
  url: string;

  // Solid WebSocket state
  solidSocket: WebSocket | null;
  isSolidConnected: boolean;
  solidSubscriptions: Map<string, Set<SolidNotificationCallback>>;

  // Message queuing
  messageQueue: QueuedMessage[];

  // Statistics
  statistics: WebSocketStatistics;

  // Configuration
  reconnectInterval: number;
  maxReconnectAttempts: number;
  reconnectAttempts: number;

  // Actions
  connect: () => Promise<void>;
  disconnect: () => void;
  close: () => void;
  sendMessage: (type: string, data?: unknown) => void;
  sendRawBinaryData: (data: ArrayBuffer) => void;
  sendFilterUpdate: (filter: FilterUpdateParams) => void;
  forceRefreshFilter: () => Promise<void>;
  forceReconnect: () => void;
  clearMessageQueue: () => void;
  setCustomBackendUrl: (backendUrl: string | null) => void;

  // Node position updates
  sendNodePositionUpdates: (updates: NodePositionUpdate[]) => void;
  flushPositionUpdates: () => Promise<void>;
  getPositionQueueMetrics: () => ReturnType<NodePositionBatchQueue['getMetrics']> | null;

  // Event handling
  onMessage: (handler: LegacyMessageHandler) => () => void;
  onBinaryMessage: (handler: BinaryMessageHandler) => () => void;
  onConnectionStatusChange: (handler: ConnectionStatusHandler) => () => void;
  onConnectionStateChange: (handler: ConnectionStateHandler) => () => void;
  on: (eventName: string, handler: EventHandler) => () => void;
  emit: (eventName: string, data: unknown) => void;

  // Solid WebSocket
  connectSolid: () => void;
  disconnectSolid: () => void;
  subscribeSolidResource: (resourceUrl: string, callback: SolidNotificationCallback) => () => void;
  unsubscribeSolidResource: (resourceUrl: string) => void;
  isSolidWebSocketConnected: () => boolean;
  getSolidSubscriptions: () => string[];

  // Per-node type map from binary protocol flags
  nodeTypeMap: Map<number, NodeType>;
  getNodeTypeMap: () => Map<number, NodeType>;

  // Utility methods
  isReady: () => boolean;
  getConnectionState: () => ConnectionState;
  getQueuedMessageCount: () => number;

  // Testing/cleanup
  _reset: () => void;
  _getInternals: () => WebSocketInternals;
}
