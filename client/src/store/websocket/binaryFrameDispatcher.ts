/**
 * binaryFrameDispatcher.ts — ordered, single-flight binary frame discipline
 *
 * At most one frame is processed across the `await` inside processBinaryData (which crosses the
 * Comlink boundary into the worker); frames that arrive meanwhile wait in arrival order and are
 * drained via queueMicrotask. NOTHING is dropped or replaced: since ADR-2141 a position frame is
 * one CHUNK of a client's stream (only the nodes that changed since its last acknowledgement),
 * not a full snapshot a newer frame could supersede, and the server sends the next chunk only
 * after this client acknowledges the previous one, so the queue stays at most a few frames deep.
 * The bound below is a backstop against a misbehaving sender, not a normal path.
 *
 * Each WebSocket connection gets its own dispatcher instance so queue state never leaks across
 * reconnects.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import type { WebSocketMessage } from '../../types/websocketTypes';
import type { NodeType } from '../../types/binaryProtocol';
import { processBinaryData, validateBinaryData } from './binaryProtocol';
import { handleHeartbeatResponse } from './connectionManager';
import { handleTextMessage } from './textMessageHandler';
import type { WebSocketStatistics } from '../../types/websocketTypes';

const logger = createLogger('WebSocketStore');

/** Backstop against a misbehaving sender; the acknowledged stream keeps the real depth tiny. */
const MAX_QUEUED_FRAMES = 256;

export interface BinaryFrameDispatcher {
  handle(buffer: ArrayBuffer): void;
}

/** Minimal store getter shape required by the message handler's downstream
 *  consumers (processBinaryData needs `socket`; handleTextMessage needs
 *  `forceReconnect`). */
type MessageHandlerGet = () => { socket: WebSocket | null; forceReconnect: () => void };
/** Store setter accepting binary node-type updates plus arbitrary text-message
 *  state partials. */
type MessageHandlerSet = (partial: Record<string, unknown>) => void;

/**
 * Create a per-connection binary frame dispatcher. The in-flight/pending
 * state is captured in this closure, so disposing of the dispatcher (by
 * dropping the reference on socket teardown) releases any pending buffer.
 */
export function createBinaryFrameDispatcher(
  get: () => { socket: WebSocket | null },
  set: (partial: { nodeTypeMap: Map<number, NodeType> }) => void,
): BinaryFrameDispatcher {
  let inFlight = false;
  const queue: ArrayBuffer[] = [];
  let overflowed = 0;

  const pump = (): void => {
    if (inFlight) return;
    const buffer = queue.shift();
    if (buffer === undefined) return;
    inFlight = true;
    Promise.resolve(processBinaryData(buffer, get, set))
      .catch(err => {
        logger.error('Error in binary frame processing:', createErrorMetadata(err));
      })
      .finally(() => {
        inFlight = false;
        // Microtask yield between frames so React render loop gets a chance.
        if (queue.length > 0) queueMicrotask(pump);
      });
  };

  const handle = (buffer: ArrayBuffer): void => {
    if (queue.length >= MAX_QUEUED_FRAMES) {
      queue.shift();
      overflowed++;
      if (overflowed % 100 === 1) {
        logger.warn(`Binary frame queue overflowed ${overflowed} times; oldest frame dropped (the server rebuilds it after its ack timeout)`);
      }
    }
    queue.push(buffer);
    pump();
  };

  return { handle };
}

function validateMessage(message: unknown): message is WebSocketMessage {
  return (
    message !== null &&
    typeof message === 'object' &&
    typeof (message as WebSocketMessage).type === 'string' &&
    (message as WebSocketMessage).type.length > 0 &&
    (message as WebSocketMessage).type.length <= 100
  );
}

/**
 * Build the `socket.onmessage` handler for a single connection. The handler
 * guards against stale sockets (post-reconnect), routes pong/binary/blob/text
 * frames, and feeds binary frames through the single-flight dispatcher.
 */
export function createMessageHandler(
  socket: WebSocket,
  get: MessageHandlerGet,
  set: MessageHandlerSet,
  dispatcher: BinaryFrameDispatcher,
  processMessageQueueFn: () => void,
): (event: MessageEvent) => void {
  // Honest statistics: every inbound message bumps the counters that the
  // status indicators read. Previously statistics stayed zeroed forever, so
  // "All systems synchronized" displayed even with a dead feed (Front 4).
  const bumpRx = (bytes: number) => {
    const stats = (get() as unknown as { statistics?: WebSocketStatistics }).statistics;
    if (!stats) return;
    set({
      statistics: {
        ...stats,
        messagesReceived: stats.messagesReceived + 1,
        bytesReceived: stats.bytesReceived + bytes,
        lastActivity: Date.now(),
      },
    });
  };

  return (event: MessageEvent) => {
    if (get().socket !== socket) return;

    if (event.data === 'pong') {
      handleHeartbeatResponse();
      bumpRx(4);
      return;
    }

    if (event.data instanceof Blob) {
      if (debugState.isDataDebugEnabled()) {
        logger.debug('Received binary blob data');
      }
      bumpRx(event.data.size);

      event.data.arrayBuffer().then(buffer => {
        if (validateBinaryData(buffer)) {
          dispatcher.handle(buffer);
        } else {
          logger.warn('Invalid binary data received, skipping processing');
        }
      }).catch(error => {
        logger.error('Error converting Blob to ArrayBuffer:', createErrorMetadata(error));
      });
      return;
    }

    if (event.data instanceof ArrayBuffer) {
      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Received binary ArrayBuffer data: ${event.data.byteLength} bytes`);
      }
      bumpRx(event.data.byteLength);
      if (validateBinaryData(event.data)) {
        dispatcher.handle(event.data);
      } else {
        logger.warn('Invalid binary data received, skipping processing');
      }
      return;
    }

    try {
      if (typeof event.data !== 'string' || event.data.trim() === '') {
        logger.warn('Received empty or invalid message data');
        return;
      }
      bumpRx(event.data.length);

      const message = JSON.parse(event.data) as WebSocketMessage;

      if (!validateMessage(message)) {
        logger.warn('Received malformed message, skipping processing');
        return;
      }

      handleTextMessage(message, get, set, processMessageQueueFn);
    } catch (error) {
      logger.error('Error parsing WebSocket message:', createErrorMetadata(error));
    }
  };
}
