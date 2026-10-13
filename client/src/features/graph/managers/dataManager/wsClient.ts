import { createLogger, createErrorMetadata } from '../../../../utils/loggerConfig';
import { debugState } from '../../../../utils/clientDebugState';
import { useSettingsStore } from '../../../../store/settingsStore';
import { BinaryNodeData, createBinaryNodeData, Vec3, BINARY_NODE_SIZE } from '../../../../types/binaryProtocol';
import { binaryProtocol } from '../../../../services/BinaryWebSocketProtocol';
import { graphWorkerProxy } from '../graphWorkerProxy';
import { useWorkerErrorStore } from '../../../../store/workerErrorStore';
import type { WebSocketAdapter } from '../../../../store/websocketStore';
import type { GraphData } from '../graphWorkerProxy';

const logger = createLogger('GraphDataManager.wsClient');

/**
 * Process a raw binary WebSocket frame that carries physics positions.
 *
 * Throttled to ~60 fps (16 ms gate). Delegates zero-copy to the worker via
 * `graphWorkerProxy.processBinaryFrame` (ADR-03 D7).
 */
export async function handleBinaryFrame(
  positionData: ArrayBuffer,
  lastUpdateTime: number,
  onUpdateTime: (t: number) => void,
): Promise<void> {
  if (!positionData || positionData.byteLength === 0) return;

  const now = Date.now();
  if (now - lastUpdateTime < 16) return; // throttle to ~60 fps
  onUpdateTime(now);

  try {
    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Received binary data: ${positionData.byteLength} bytes`);

      const remainder = (positionData.byteLength - 1) % BINARY_NODE_SIZE;
      if (remainder !== 0) {
        logger.warn(`Binary data size (${positionData.byteLength} bytes) is not a multiple of ${BINARY_NODE_SIZE}. Remainder: ${remainder}`);
      }
    }

    // ADR-03 D7: single binary entry point — newest-wins discipline is inside
    // processBinaryFrame; no caller gating needed. Frame transferred zero-copy.
    const frame = new Uint8Array(positionData);
    await graphWorkerProxy.processBinaryFrame(frame);
    useWorkerErrorStore.getState().resetTransientErrors();

    const settings = useSettingsStore.getState().settings;
    if (settings?.system?.debug?.enabled && (settings.system.debug.enablePhysicsDebug || settings.system.debug.enableNodeDebug)) {
      const view = new DataView(positionData);
      const nodeCount = Math.min(3, positionData.byteLength / BINARY_NODE_SIZE);
      for (let i = 0; i < nodeCount; i++) {
        const offset = i * BINARY_NODE_SIZE;
        const x = view.getFloat32(offset + 4, true);
        const y = view.getFloat32(offset + 8, true);
        const z = view.getFloat32(offset + 12, true);
        logger.info(`[Physics Debug] Node ${i}: position(${x.toFixed(2)}, ${y.toFixed(2)}, ${z.toFixed(2)})`);
      }
    }

    if (debugState.isDataDebugEnabled()) {
      logger.debug('Processed binary data through worker');
    }
  } catch (error) {
    logger.error('Error processing binary position data:', createErrorMetadata(error));
    useWorkerErrorStore.getState().recordTransientError('updateNodePositions');

    if (debugState.isEnabled()) {
      try {
        const view = new DataView(positionData);
        const maxBytes = Math.min(64, positionData.byteLength);
        const hex: string[] = [];
        for (let i = 0; i < maxBytes; i++) {
          hex.push(view.getUint8(i).toString(16).padStart(2, '0'));
        }
        logger.debug(`First ${maxBytes} bytes: ${hex.join(' ')}${positionData.byteLength > maxBytes ? '...' : ''}`);
      } catch (_e) {
        logger.debug('Could not display binary data preview');
      }
    }
  }
}

/**
 * Send current node positions to the backend over WebSocket.
 * Only fires while binary updates are enabled AND the user is actively
 * interacting (drag/pan) to avoid unnecessary upstream traffic.
 */
export async function sendNodePositions(
  binaryUpdatesEnabled: boolean,
  webSocketService: WebSocketAdapter | null,
  isUserInteracting: boolean,
  lastGraphData: GraphData | null,
  nodeIdMap: Map<string, number>,
  ensureNodeHasValidPosition: (node: import('../graphWorkerProxy').Node) => import('../graphWorkerProxy').Node,
): Promise<void> {
  if (!binaryUpdatesEnabled || !webSocketService || !isUserInteracting) return;

  try {
    const currentData = lastGraphData ?? { nodes: [], edges: [] };

    const binaryNodes: BinaryNodeData[] = currentData.nodes
      .filter(node => node && node.id)
      .map(node => {
        const validatedNode = ensureNodeHasValidPosition(node);
        const numericId = nodeIdMap.get(validatedNode.id) || 0;
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
            z: validatedNode.position.z || 0,
          },
          velocity,
        };
      })
      .filter((node): node is BinaryNodeData => node !== null);

    const buffer = createBinaryNodeData(binaryNodes);
    webSocketService.send(buffer);

    if (debugState.isDataDebugEnabled()) {
      logger.debug(`Sent positions for ${binaryNodes.length} nodes`);
    }
  } catch (error) {
    logger.error('Error sending node positions:', createErrorMetadata(error));
  }
}

/**
 * Activate binary position updates once the WebSocket is ready.
 * Polls with a 500 ms back-off when the socket is not yet open.
 *
 * @param webSocketService  Live WebSocket adapter.
 * @param setBinaryEnabled  Callback to flip the enabled flag.
 * @param retryTimerRef     Current retry timer handle (may be null).
 * @param onRetryTimer      Called with the new timer handle so the orchestrator can cancel it.
 */
export function enableBinaryUpdates(
  webSocketService: WebSocketAdapter | null,
  setBinaryEnabled: (v: boolean) => void,
  retryTimerRef: number | null,
  onRetryTimer: (handle: number | null) => void,
): void {
  if (!webSocketService) {
    logger.warn('Cannot enable binary updates: WebSocket service not set');
    return;
  }

  if (webSocketService.isReady()) {
    setBinaryEnabled(true);
    return;
  }

  if (retryTimerRef !== null) window.clearTimeout(retryTimerRef);

  const handle = window.setTimeout(() => {
    onRetryTimer(null);
    if (webSocketService.isReady()) {
      setBinaryEnabled(true);
      if (debugState.isEnabled()) logger.info('WebSocket ready, binary updates enabled');
    } else {
      if (debugState.isEnabled()) logger.info('WebSocket not ready yet, retrying...');
      enableBinaryUpdates(webSocketService, setBinaryEnabled, null, onRetryTimer);
    }
  }, 500) as unknown as number;

  onRetryTimer(handle);
}
