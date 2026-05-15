/**
 * textMessageHandler.ts — JSON/text WebSocket message handling
 *
 * Processes parsed JSON messages: connection_established, error frames,
 * filter_update_success, initialGraphLoad, memory_flash, etc.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { graphDataManager } from '../../features/graph/managers/graphDataManager';
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
    handleInitialGraphLoad(message);
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

function handleInitialGraphLoad(message: WebSocketMessage) {
  const msgData = message as unknown as { nodes?: unknown[]; edges?: unknown[] };
  const nodes = msgData.nodes || [];
  const edges = msgData.edges || [];
  logger.info(`[WebSocket] Received initialGraphLoad with ${nodes.length} nodes, ${edges.length} edges`);

  const existingNodeCount = graphDataManager.nodeIdMap.size;
  if (existingNodeCount > 0) {
    logger.info(
      `[WebSocket] Skipping initialGraphLoad setGraphData: REST already loaded ${existingNodeCount} nodes ` +
      `(WS sent ${nodes.length} unfiltered). Positions will arrive via binary stream.`
    );
    emit('graphDataUpdated', {
      nodeCount: existingNodeCount,
      edgeCount: 0,
      source: 'websocket_filter_skipped'
    });
    return;
  }

  const transformedNodes = nodes.map((node: unknown) => {
    const n = node as Record<string, unknown>;
    return {
      id: String(n.id),
      label: String(n.label || n.name || n.id),
      type: (n.node_type ?? n.nodeType ?? n.type) as string | undefined,
      position: (n.position as { x: number; y: number; z: number }) || { x: Number(n.x) || 0, y: Number(n.y) || 0, z: Number(n.z) || 0 },
      metadata: {
        ...(n.metadata as Record<string, unknown>),
        quality_score: n.quality_score ?? (n.metadata as Record<string, unknown>)?.quality_score,
        authority_score: n.authority_score ?? (n.metadata as Record<string, unknown>)?.authority_score,
      },
      color: n.color as string | undefined,
      size: n.size as number | undefined,
    };
  });

  const transformedEdges = edges.map((edge: unknown) => {
    const e = edge as Record<string, unknown>;
    let source = (e.source ?? e.from ?? e.from_node ?? e.sourceId ?? e.source_id) as string | undefined;
    let target = (e.target ?? e.to ?? e.to_node ?? e.targetId ?? e.target_id) as string | undefined;

    if (source === undefined || source === 'undefined' || source === 'null') source = undefined;
    if (target === undefined || target === 'undefined' || target === 'null') target = undefined;

    const edgeId = String(e.id || '');
    if ((source == null || target == null) && edgeId) {
      const parts = edgeId.split('-');
      if (parts.length >= 2) {
        if (source == null) source = parts[0];
        if (target == null) target = parts.slice(1).join('-');
      }
    }

    return {
      id: edgeId || `${source}-${target}`,
      source: String(source),
      target: String(target),
      weight: e.weight as number | undefined,
      label: e.label as string | undefined,
      edgeType: (e.edgeType ?? e.edge_type ?? e.relation_type) as string | undefined,
      owlPropertyIri: (e.owlPropertyIri ?? e.owl_property_iri) as string | undefined,
    };
  }).filter((edge: { source: string; target: string }) => edge.source !== 'undefined' && edge.target !== 'undefined');

  graphDataManager.setGraphData({
    nodes: transformedNodes,
    edges: transformedEdges,
  }).then(() => {
    logger.info(`[WebSocket] Graph updated with ${transformedNodes.length} nodes from server filter`);
    emit('graphDataUpdated', {
      nodeCount: transformedNodes.length,
      edgeCount: transformedEdges.length,
      source: 'websocket_filter'
    });
  }).catch(error => {
    logger.error('[WebSocket] Failed to update graph data from initialGraphLoad:', createErrorMetadata(error));
  });
}
