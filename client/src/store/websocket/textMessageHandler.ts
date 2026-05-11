/**
 * textMessageHandler.ts — JSON/text WebSocket message handling
 *
 * Processes parsed JSON messages: connection_established, error frames,
 * filter_update_success, analytics_update, memory_flash, etc.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import type { WebSocketMessage } from '../../types/websocketTypes';
import type { WebSocketErrorFrame } from './types';
import { emit, notifyMessageHandlers } from './connectionManager';
import { handleErrorFrame } from './binaryProtocol';
import { useAnalyticsStore, type AnalyticsUpdate } from '../analyticsStore';

const logger = createLogger('WebSocketStore');

/**
 * Process a parsed JSON WebSocket message, dispatching to the appropriate
 * handler based on message.type.
 */
export function handleTextMessage(
  message: WebSocketMessage,
  get: () => { forceReconnect: () => void },
  set: (partial: Record<string, unknown>) => void,
  processMessageQueueFn: () => void,
) {
  if (debugState.isDataDebugEnabled()) {
    logger.debug(`Received WebSocket message: ${message.type}`, (message as unknown as Record<string, unknown>).data);
  }

  if (message.type === 'connection_established') {
    set({ isServerReady: true });
    if (debugState.isEnabled()) {
      logger.info('Server connection established and ready');
    }
  }

  if (message.type === 'error' && (message as unknown as Record<string, unknown>).error) {
    handleErrorFrame(
      (message as unknown as Record<string, unknown>).error as WebSocketErrorFrame,
      get,
      processMessageQueueFn,
    );
    return;
  }

  if (message.type === 'filter_update_success') {
    if (debugState.isEnabled()) {
      logger.info(`Filter applied: ${message.data?.visible_nodes}/${message.data?.total_nodes} nodes visible`);
    }
    emit('filterApplied', {
      visibleNodes: message.data?.visible_nodes,
      totalNodes: message.data?.total_nodes
    });
  }

  if (message.type === 'initialGraphLoad') {
    logger.info('[WebSocket] Ignoring initialGraphLoad — graph data is served via REST /api/graph/data');
  }

  // Memory flash events -- forward to event bus for EmbeddingCloudLayer
  if (message.type === 'memory_flash' && (message as unknown as Record<string, unknown>).data) {
    emit('memoryFlash', (message as unknown as Record<string, unknown>).data);
  }

  // Analytics-side stream (ADR-061): sticky GPU outputs (cluster_id, anomaly_score,
  // sssp_*) arrive here at recompute cadence and merge into useAnalyticsStore.
  // Renderers read from the store, not from per-frame binary fields.
  if (message.type === 'analytics_update') {
    try {
      useAnalyticsStore.getState().merge(message as unknown as AnalyticsUpdate);
    } catch (error) {
      logger.error('Error merging analytics_update:', createErrorMetadata(error));
    }
    return;
  }

  notifyMessageHandlers(message);
}

// initialGraphLoad is no longer sent by the server. Graph data comes via
// REST /api/graph/data. WebSocket carries only binary position/velocity
// frames and lightweight JSON control messages.
