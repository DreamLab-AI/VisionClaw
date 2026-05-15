/**
 * binaryProtocol.ts — Position-frame decoder + outbound batch queue (ADR-061 / PRD-007).
 *
 * Inbound: a single straight path. Every binary frame is a position frame
 * (28 B/node, fixed). No version dispatch. No flag-bit decode. Sticky GPU
 * outputs (cluster_id, anomaly_score, etc.) arrive on the separate
 * `analytics_update` text-message channel — see analyticsStore.
 *
 * Outbound: position-batch-queue scaffolding for user-driven node drag updates.
 * Unrelated to the inbound decoder; preserved as-is from the prior design.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { graphDataManager } from '../../features/graph/managers/graphDataManager';
import {
  isPositionFrame,
  BINARY_PROTOCOL_PREAMBLE,
  BINARY_FRAME_HEADER_SIZE,
  BINARY_NODE_SIZE,
  type BinaryNodeData,
} from '../../types/binaryProtocol';
import { NodePositionBatchQueue, createWebSocketBatchProcessor } from '../../utils/BatchQueue';
import { validateNodePositions, createValidationMiddleware } from '../../utils/validation';
import { binaryProtocol } from '../../services/BinaryWebSocketProtocol';
import type { WebSocketErrorFrame, NodePositionUpdate } from './types';
import { emit, notifyBinaryMessageHandlers } from './connectionManager';

const logger = createLogger('WebSocketStore');

// ── Constants ──────────────────────────────────────────────────────────
const ACK_BATCH_SIZE = 10;

// ── Encapsulated module-level state ────────────────────────────────────
let positionBatchQueue: NodePositionBatchQueue | null = null;
let binaryMessageCount = 0;
let positionUpdateSequence = 0;
let lastAckSentSequence = 0;

// ── State accessors (used by index.ts for _getInternals / _reset) ──

export function getPositionBatchQueue(): NodePositionBatchQueue | null {
  return positionBatchQueue;
}

export function resetBinaryState() {
  if (positionBatchQueue) {
    positionBatchQueue.destroy();
  }
  positionBatchQueue = null;
  binaryMessageCount = 0;
  positionUpdateSequence = 0;
  lastAckSentSequence = 0;
}

// ── Batch queue initialization ─────────────────────────────────────────

export function initializeBatchQueue(
  get: () => { isConnected: boolean; socket: WebSocket | null },
) {
  if (positionBatchQueue) {
    positionBatchQueue.destroy();
  }

  const validationMiddleware = createValidationMiddleware({
    maxNodes: 100000,
    maxCoordinate: 10000,
    minCoordinate: -10000,
    maxVelocity: 1000,
  });

  const batchProcessor = createWebSocketBatchProcessor((data: ArrayBuffer) => {
    const state = get();
    if (!state.isConnected || !state.socket) {
      logger.warn('Cannot send batch: WebSocket not connected');
      return;
    }

    try {
      state.socket.send(data);

      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Sent binary batch: ${data.byteLength} bytes`);
      }
    } catch (error) {
      logger.error('Error sending batch:', createErrorMetadata(error));
      throw error;
    }
  });

  positionBatchQueue = new NodePositionBatchQueue({
    processBatch: async (batch: BinaryNodeData[]) => {
      const validatedBatch = validationMiddleware(batch);

      if (validatedBatch.length === 0) {
        logger.warn('All nodes in batch failed validation');
        return;
      }

      await batchProcessor.processBatch(validatedBatch);
    },
    onError: batchProcessor.onError,
    onSuccess: batchProcessor.onSuccess,
  });

  logger.info('Position batch queue initialized');
}

export function destroyBatchQueue() {
  if (positionBatchQueue) {
    positionBatchQueue.destroy();
    positionBatchQueue = null;
  }
}

// ── Position ACK (backpressure) ────────────────────────────────────────

function sendPositionAck(
  get: () => { socket: WebSocket | null },
  sequenceId: number,
  nodesReceived: number,
) {
  const state = get();
  if (!state.socket || state.socket.readyState !== WebSocket.OPEN) {
    return;
  }

  try {
    const ackMessage = binaryProtocol.createBroadcastAck(sequenceId, nodesReceived);
    state.socket.send(ackMessage);

    if (debugState.isDataDebugEnabled() && sequenceId % 100 === 0) {
      logger.debug(`Sent BroadcastAck: seq=${sequenceId}, nodes=${nodesReceived}`);
    }
  } catch (error) {
    logger.error('Error sending position ACK:', createErrorMetadata(error));
  }
}

// ── Validation ─────────────────────────────────────────────────────────

export function validateBinaryData(data: ArrayBuffer): boolean {
  if (!data || data.byteLength === 0) {
    return false;
  }

  if (data.byteLength > 50 * 1024 * 1024) {
    logger.warn(`Binary data too large: ${data.byteLength} bytes`);
    return false;
  }

  if (!isPositionFrame(data)) {
    const firstByte = new DataView(data).getUint8(0);
    logger.warn(
      `Invalid binary protocol preamble: 0x${firstByte.toString(16)} ` +
      `(expected 0x${BINARY_PROTOCOL_PREAMBLE.toString(16)})`,
    );
    return false;
  }

  return true;
}

// ── Error frame handling ───────────────────────────────────────────────

export function handleErrorFrame(
  error: WebSocketErrorFrame,
  get: () => { forceReconnect: () => void },
  processMessageQueueFn: () => void,
) {
  logger.error('Received error frame from server:', error);

  emit('error-frame', error);

  switch (error.category) {
    case 'validation':
      if (error.affectedPaths && error.affectedPaths.length > 0) {
        emit('validation-error', {
          paths: error.affectedPaths,
          message: error.message,
        });
      }
      break;

    case 'rate_limit':
      if (error.retryAfter) {
        logger.warn(`Rate limited. Retry after ${error.retryAfter}ms`);
        emit('rate-limit', {
          retryAfter: error.retryAfter,
          message: error.message,
        });
      }
      break;

    case 'auth':
      emit('auth-error', {
        code: error.code,
        message: error.message,
      });
      break;

    case 'server':
      if (error.retryable && error.retryAfter) {
        setTimeout(() => {
          processMessageQueueFn();
        }, error.retryAfter);
      }
      break;

    case 'protocol':
      logger.error('Protocol error - considering reconnection');
      if (error.code === 'PROTOCOL_VERSION_MISMATCH') {
        get().forceReconnect();
      }
      break;
  }
}

// ── Main binary data processor — single straight path ──────────────────

export async function processBinaryData(
  data: ArrayBuffer,
  get: () => { socket: WebSocket | null },
) {
  try {
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Processing binary data: ${data.byteLength} bytes`);
    }

    // Header-only validation: preamble + broadcast_sequence + node count.
    // Full per-node decode happens ONLY in the worker thread.
    if (data.byteLength < BINARY_FRAME_HEADER_SIZE) {
      return;
    }
    const headerView = new DataView(data);
    if (headerView.getUint8(0) !== BINARY_PROTOCOL_PREAMBLE) {
      return;
    }
    const broadcastSequence = Number(headerView.getBigUint64(1, true));
    const nodeCount = Math.floor((data.byteLength - BINARY_FRAME_HEADER_SIZE) / BINARY_NODE_SIZE);

    binaryMessageCount = (binaryMessageCount || 0) + 1;
    if (binaryMessageCount % 100 === 1) {
      logger.debug('Position frame received', {
        bytes: data.byteLength,
        nodes: nodeCount,
        broadcastSequence,
        msgCount: binaryMessageCount,
      });
    }

    // Forward the raw buffer to graphDataManager → worker for SAB writeback.
    // The worker does the full per-node decode via the canonical decoder.
    try {
      await graphDataManager.updateNodePositions(data);
    } catch (error) {
      logger.error('Error updating positions:', createErrorMetadata(error));
    }

    // Notify any registered raw-binary listeners (BotsWebSocketIntegration etc.)
    notifyBinaryMessageHandlers(data);

    // Backpressure ack — every Nth frame, ack the latest broadcast_sequence.
    positionUpdateSequence++;
    if (positionUpdateSequence - lastAckSentSequence >= ACK_BATCH_SIZE) {
      sendPositionAck(get, broadcastSequence, nodeCount);
      lastAckSentSequence = positionUpdateSequence;
    }
  } catch (error) {
    logger.error('Error processing binary data:', createErrorMetadata(error));
  }
}

// ── Position update sending (outbound batch queue) ─────────────────────

export function sendNodePositionUpdates(updates: NodePositionUpdate[]) {
  if (!positionBatchQueue) {
    logger.warn('Position batch queue not initialized');
    return;
  }

  try {
    const binaryNodes: BinaryNodeData[] = updates.map(update => ({
      nodeId: update.nodeId,
      position: update.position,
      velocity: update.velocity || { x: 0, y: 0, z: 0 },
    }));

    const validation = validateNodePositions(binaryNodes, {
      maxNodes: updates.length + 100,
    });

    if (!validation.valid) {
      logger.error('Position updates failed validation:', validation.errors);
      return;
    }

    binaryNodes.forEach(node => {
      positionBatchQueue!.enqueuePositionUpdate(node, 0);
    });

    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Queued ${updates.length} position updates for batching`);
    }
  } catch (error) {
    logger.error('Error queuing position updates:', createErrorMetadata(error));
  }
}

export function flushPositionUpdates(): Promise<void> {
  if (positionBatchQueue) {
    return positionBatchQueue.flush();
  }
  return Promise.resolve();
}

export function getPositionQueueMetrics(): ReturnType<NodePositionBatchQueue['getMetrics']> | null {
  if (positionBatchQueue) {
    return positionBatchQueue.getMetrics();
  }
  return null;
}
