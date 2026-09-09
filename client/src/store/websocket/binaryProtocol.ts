/**
 * binaryProtocol.ts — Binary message handling
 *
 * Handles: validateBinaryData, processBinaryData, protocol version dispatch,
 * position update parsing, batch queue, all binary frame processing.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { useSettingsStore } from '../settingsStore';
import { graphDataManager } from '../../features/graph/managers/graphDataManager';
import {
  parseBinaryNodeData,
  parseBinaryFrameData,
  isAgentNode,
  type BinaryNodeData,
  getNodeType,
  getActualNodeId,
  NodeType,
  PROTOCOL_V3,
  PROTOCOL_V5,
} from '../../types/binaryProtocol';
import { NodePositionBatchQueue, createWebSocketBatchProcessor } from '../../utils/BatchQueue';
import { validateNodePositions, createValidationMiddleware } from '../../utils/validation';
import { binaryProtocol, MessageType, GraphTypeFlag } from '../../services/BinaryWebSocketProtocol';
import type { WebSocketErrorFrame, NodePositionUpdate } from './types';
import {
  emit,
  notifyBinaryMessageHandlers,
} from './connectionManager';
import { pushTransientBeams } from '../transientBeamStore';
import { useAgentTargetStore } from '../agentTargetStore';
import { nodeAnalyticsStore } from '../../features/analytics/store/nodeAnalyticsStore';

const logger = createLogger('WebSocketStore');

// ── Constants ──────────────────────────────────────────────────────────
const ACK_BATCH_SIZE = 10;
// Upper bound on the node-type cache. The graph tops out near ~30k nodes;
// agent ids churn (spawn/despawn) so without a bound this Map grows for the
// life of the session. Cap well above the live node count and evict oldest
// (insertion-order) entries when exceeded.
const MAX_NODE_TYPE_ENTRIES = 65536;

// ── Encapsulated module-level state ────────────────────────────────────
let positionBatchQueue: NodePositionBatchQueue | null = null;
let binaryMessageCount = 0;
let currentNodeTypeMap: Map<number, NodeType> = new Map();
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
  currentNodeTypeMap = new Map();
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
    maxNodes: 10000,
    maxCoordinate: 10000,
    minCoordinate: -10000,
    maxVelocity: 1000
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
    onSuccess: batchProcessor.onSuccess
  });

  logger.info('Position batch queue initialized');
}

export function destroyBatchQueue() {
  if (positionBatchQueue) {
    positionBatchQueue.destroy();
    positionBatchQueue = null;
  }
}

// ── Position ACK ───────────────────────────────────────────────────────

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

// ── Node type map ──────────────────────────────────────────────────────

function updateNodeTypeMapFromParsed(
  parsedNodes: BinaryNodeData[],
  set: (partial: { nodeTypeMap: Map<number, NodeType> }) => void,
) {
  for (const node of parsedNodes) {
    const nodeType = getNodeType(node.nodeId);
    if (nodeType !== NodeType.Unknown) {
      const actualId = getActualNodeId(node.nodeId);
      // Re-insert to keep recently-seen ids at the tail (LRU-ish for eviction).
      if (currentNodeTypeMap.has(actualId)) {
        currentNodeTypeMap.delete(actualId);
      }
      currentNodeTypeMap.set(actualId, nodeType);
    }
  }

  // Bound the cache: evict oldest entries (Map preserves insertion order).
  while (currentNodeTypeMap.size > MAX_NODE_TYPE_ENTRIES) {
    const oldest = currentNodeTypeMap.keys().next().value;
    if (oldest === undefined) break;
    currentNodeTypeMap.delete(oldest);
  }

  set({ nodeTypeMap: new Map(currentNodeTypeMap) });
}

export function getCurrentNodeTypeMap(): Map<number, NodeType> {
  return new Map(currentNodeTypeMap);
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

  const firstByte = new DataView(data).getUint8(0);
  // Legacy frames lead with the protocol version (3/5); agent-action frames
  // lead with the 0x23 MessageType tag (the server never wraps them in the
  // V4 header). Anything else is unknown and rejected.
  if (firstByte !== 3 && firstByte !== 5 && firstByte !== MessageType.AGENT_ACTION) {
    console.warn(`[WS] Unexpected binary frame lead byte: ${firstByte}`);
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
          message: error.message
        });
      }
      break;

    case 'rate_limit':
      if (error.retryAfter) {
        logger.warn(`Rate limited. Retry after ${error.retryAfter}ms`);
        emit('rate-limit', {
          retryAfter: error.retryAfter,
          message: error.message
        });
      }
      break;

    case 'auth':
      emit('auth-error', {
        code: error.code,
        message: error.message
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

// ── Binary message processing handlers ─────────────────────────────────

async function handleGraphUpdate(data: ArrayBuffer, header: ReturnType<typeof binaryProtocol.parseHeader>) {
  if (!header) return;

  const graphTypeFlag = header.graphTypeFlag as GraphTypeFlag;
  const currentMode = useSettingsStore.getState().get<'knowledge_graph' | 'ontology'>('visualisation.graphs.mode') || 'knowledge_graph';

  const shouldProcess =
    (currentMode === 'knowledge_graph' && graphTypeFlag === GraphTypeFlag.KNOWLEDGE_GRAPH) ||
    (currentMode === 'ontology' && graphTypeFlag === GraphTypeFlag.ONTOLOGY);

  if (!shouldProcess) {
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Skipping graph update - mode mismatch: current=${currentMode}, flag=${graphTypeFlag}`);
    }
    return;
  }

  const payload = binaryProtocol.extractPayload(data, header);

  emit('graph-update', {
    graphType: graphTypeFlag === GraphTypeFlag.ONTOLOGY ? 'ontology' : 'knowledge_graph',
    data: payload
  });

  if (debugState.isDataDebugEnabled()) {
    logger.debug(`Processed graph update: mode=${currentMode}, size=${payload.byteLength}`);
  }
}

async function handleVoiceData(data: ArrayBuffer, header: ReturnType<typeof binaryProtocol.parseHeader>) {
  if (!header) return;

  const payload = binaryProtocol.extractPayload(data, header);

  emit('voice-data', payload);

  if (debugState.isDataDebugEnabled()) {
    logger.debug(`Processed voice data: size=${payload.byteLength}`);
  }
}

async function handlePositionUpdate(
  data: ArrayBuffer,
  header: ReturnType<typeof binaryProtocol.parseHeader>,
  get: () => { socket: WebSocket | null },
  set: (partial: { nodeTypeMap: Map<number, NodeType> }) => void,
) {
  if (!header) return;

  const payload = binaryProtocol.extractPayload(data, header);
  const estimatedNodeCount = Math.floor(payload.byteLength / 28);

  let parsedNodes: BinaryNodeData[] | null = null;
  try {
    parsedNodes = parseBinaryNodeData(payload);
  } catch (error) {
    logger.error('Error parsing binary node data:', createErrorMetadata(error));
    return;
  }

  updateNodeTypeMapFromParsed(parsedNodes, set);

  const hasBotsData = parsedNodes.some(node => isAgentNode(node.nodeId));

  if (hasBotsData) {
    emit('bots-position-update', payload);
    if (debugState.isDataDebugEnabled()) {
      logger.debug('Emitted bots-position-update event');
    }
  }

  const graphType = graphDataManager.getGraphType();

  binaryMessageCount = (binaryMessageCount || 0) + 1;
  if (binaryMessageCount % 100 === 1) {
    logger.debug('Position update received', { graphType, dataSize: payload.byteLength, msgCount: binaryMessageCount });
  }

  try {
    await graphDataManager.updateNodePositions(payload);
    if (binaryMessageCount % 100 === 1) {
      logger.debug('Node positions updated successfully', { graphType });
    }
  } catch (error) {
    logger.error('[WebSocketStore] Error updating positions:', createErrorMetadata(error));
    logger.error('Error processing position data in graphDataManager:', createErrorMetadata(error));
  }

  positionUpdateSequence++;
  if (positionUpdateSequence - lastAckSentSequence >= ACK_BATCH_SIZE) {
    sendPositionAck(get, positionUpdateSequence, estimatedNodeCount);
    lastAckSentSequence = positionUpdateSequence;
  }
}

async function handleLegacyBinaryData(
  data: ArrayBuffer,
  get: () => { socket: WebSocket | null },
  set: (partial: { nodeTypeMap: Map<number, NodeType> }) => void,
) {
  const estimatedNodeCount = Math.floor(data.byteLength / 28);

  let frame: ReturnType<typeof parseBinaryFrameData>;
  try {
    frame = parseBinaryFrameData(data);
  } catch (error) {
    logger.error('Error parsing legacy binary data:', createErrorMetadata(error));
    return;
  }

  const parsedNodes = frame.nodes;

  // ADR-03 D7 "Phase 5": ingest per-node analytics on the main thread.
  // Only V3/V5 full frames carry cluster_id/anomaly/community (offsets 36/40/44);
  // V2 and V4 delta frames omit them and must NOT overwrite live analytics.
  if (frame.type === 'full') {
    const protoByte = new DataView(data).getUint8(0);
    if (protoByte === PROTOCOL_V3 || protoByte === PROTOCOL_V5) {
      nodeAnalyticsStore.ingest(parsedNodes);
    }
  }

  updateNodeTypeMapFromParsed(parsedNodes, set);

  const hasBotsData = parsedNodes.some(node => isAgentNode(node.nodeId));

  if (hasBotsData) {
    emit('bots-position-update', data);
    if (debugState.isDataDebugEnabled()) {
      logger.debug('Emitted bots-position-update event (legacy)');
    }
  }

  const graphType = graphDataManager.getGraphType();
  binaryMessageCount = (binaryMessageCount || 0) + 1;

  if (binaryMessageCount % 100 === 1) {
    logger.debug('Legacy binary data received', { graphType, dataSize: data.byteLength, msgCount: binaryMessageCount });
  }

  try {
    await graphDataManager.updateNodePositions(data);
    if (binaryMessageCount % 100 === 1) {
      logger.debug('Node positions updated successfully (legacy)', { graphType });
    }
  } catch (error) {
    logger.error('Error processing legacy binary data:', createErrorMetadata(error));
  }

  positionUpdateSequence++;
  const ackSequence = frame.broadcastSequence ?? positionUpdateSequence;
  if (positionUpdateSequence - lastAckSentSequence >= ACK_BATCH_SIZE) {
    sendPositionAck(get, ackSequence, estimatedNodeCount);
    lastAckSentSequence = positionUpdateSequence;
  }
}

/**
 * The ONLY agent-action wire frame is a bare type tag, not a 6-byte framed
 * header: `[0x23][count:u16][(len:u16)(event 15B+payload)]…`
 * (encode_agent_actions, src/utils/binary_protocol.rs:1233). Stripping
 * MESSAGE_HEADER_SIZE would misalign the batch count, so the tag is peeled with
 * a 1-byte offset instead of routing through parseHeader/extractPayload.
 *
 * ADR-2099: there is no second, framed-header decode path for 0x23, and there
 * cannot be one. The framed header writes its type byte at offset 0
 * (createMessage, BinaryWebSocketProtocol.ts:84) and parseHeader reads it back
 * from offset 0, so `header.type === AGENT_ACTION` implies `firstByte === 0x23`
 * — which processBinaryData has already routed here and returned on. A
 * `case MessageType.AGENT_ACTION` in that switch is unreachable by
 * construction; it was deleted, not kept as a fallback.
 */
function handleAgentActionTagged(data: ArrayBuffer) {
  const actions = data.byteLength >= 18
    ? binaryProtocol.decodeAgentActions(data.slice(1))
    : [];
  dispatchAgentActions(actions);
}

function dispatchAgentActions(actions: ReturnType<typeof binaryProtocol.decodeAgentActions>) {
  if (actions.length === 0) return;
  // Live transcript + attention heat consume this event.
  emit('agent-action', actions);
  // Embodied transient beams (0x23 → TransientBeamsLayer). Distinct sink,
  // pushed in parallel so neither path can starve the other.
  pushTransientBeams(actions);
  // Agent→target tracking (momentum nudge in BotsVisualization).
  useAgentTargetStore.getState().pushActions(actions);

  if (debugState.isDataDebugEnabled()) {
    logger.debug(`Processed ${actions.length} agent action(s)`);
  }
}

// ── Main binary data processor ─────────────────────────────────────────

export async function processBinaryData(
  data: ArrayBuffer,
  get: () => { socket: WebSocket | null },
  set: (partial: { nodeTypeMap: Map<number, NodeType> }) => void,
) {
  try {
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Processing binary data: ${data.byteLength} bytes`);
    }

    if (data.byteLength >= 1) {
      const firstByte = new DataView(data).getUint8(0);
      // ADR-2057: V2 is not accepted — the server rejects it outright.
      if (firstByte === PROTOCOL_V3 || firstByte === PROTOCOL_V5) {
        await handleLegacyBinaryData(data, get, set);
        notifyBinaryMessageHandlers(data);
        return;
      }
      // Total for 0x23: parseHeader reads its type from offset 0 too, so no
      // agent-action frame can reach the switch below (ADR-2099).
      if (firstByte === MessageType.AGENT_ACTION) {
        handleAgentActionTagged(data);
        return;
      }
    }

    const header = binaryProtocol.parseHeader(data);
    if (!header) {
      logger.error('Failed to parse binary message header');
      return;
    }

    switch (header.type) {
      case MessageType.GRAPH_UPDATE:
        await handleGraphUpdate(data, header);
        break;

      case MessageType.VOICE_DATA:
        await handleVoiceData(data, header);
        break;

      case MessageType.POSITION_UPDATE:
      case MessageType.AGENT_POSITIONS:
        await handlePositionUpdate(data, header, get, set);
        break;

      default:
        await handleLegacyBinaryData(data, get, set);
        break;
    }

    notifyBinaryMessageHandlers(data);
  } catch (error) {
    logger.error('Error processing binary data:', createErrorMetadata(error));
  }
}

// ── Position update sending ────────────────────────────────────────────

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
      ssspDistance: 0,
      ssspParent: -1
    }));

    const validation = validateNodePositions(binaryNodes, {
      maxNodes: updates.length + 100
    });

    if (!validation.valid) {
      logger.error('Position updates failed validation:', validation.errors);
      return;
    }

    binaryNodes.forEach(node => {
      const priority = isAgentNode(node.nodeId) ? 10 : 0;
      positionBatchQueue!.enqueuePositionUpdate(node, priority);
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
