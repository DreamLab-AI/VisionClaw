/**
 * connectionManager.ts — WebSocket connection lifecycle
 *
 * Handles: connect, disconnect, reconnect with exponential backoff,
 * heartbeat, message queuing, and connection state management.
 */

import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { useSettingsStore } from '../settingsStore';
import { nostrAuth } from '../../services/nostrAuthService';
import type {
  ConnectionState,
  QueuedMessage,
  LegacyMessageHandler,
  BinaryMessageHandler,
  ConnectionStatusHandler,
  ConnectionStateHandler,
  EventHandler,
} from './types';

const logger = createLogger('WebSocketStore');

// ── Constants ──────────────────────────────────────────────────────────
const HEARTBEAT_INTERVAL_MS = 30000;
const HEARTBEAT_TIMEOUT_MS = 45000;
const MAX_QUEUE_SIZE = 100;
const MAX_RECONNECT_DELAY = 30000;

// ── Encapsulated module-level state ────────────────────────────────────
let messageHandlers: LegacyMessageHandler[] = [];
let binaryMessageHandlers: BinaryMessageHandler[] = [];
let connectionStatusHandlers: ConnectionStatusHandler[] = [];
let connectionStateHandlers: ConnectionStateHandler[] = [];
let eventHandlers: Map<string, EventHandler[]> = new Map();
let heartbeatInterval: number | null = null;
let heartbeatTimeout: number | null = null;
let reconnectTimeout: number | null = null;

// ── Handler state accessors (used by index.ts for _getInternals / _reset) ──

export function getHandlerState() {
  return {
    messageHandlers,
    binaryMessageHandlers,
    connectionStatusHandlers,
    connectionStateHandlers,
    eventHandlers,
    heartbeatInterval,
    reconnectTimeout,
  };
}

export function resetHandlerState() {
  messageHandlers = [];
  binaryMessageHandlers = [];
  connectionStatusHandlers = [];
  connectionStateHandlers = [];
  eventHandlers = new Map();
  heartbeatInterval = null;
  heartbeatTimeout = null;
  reconnectTimeout = null;
}

// ── URL helpers ────────────────────────────────────────────────────────

export function determineWebSocketUrl(): string {
  if (typeof window === 'undefined') {
    return 'ws://localhost:3001/wss';
  }

  const isDev = import.meta.env.DEV;
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.hostname;
  const port = window.location.port;
  const baseUrl = `${protocol}//${host}:${port}`;
  const wsUrl = `${baseUrl}/wss`;

  if (debugState.isEnabled()) {
    logger.info(`Determined WebSocket URL (${isDev ? 'dev' : 'prod'}): ${wsUrl}`);
  }

  return wsUrl;
}

export function getUrlFromSettings(): string {
  let newUrl = determineWebSocketUrl();

  try {
    const state = useSettingsStore.getState();
    const settings = state.settings;

    if (settings?.system?.customBackendUrl &&
        settings.system.customBackendUrl.trim() !== '') {
      const customUrl = settings.system.customBackendUrl.trim();
      const protocol = customUrl.startsWith('https://') ? 'wss://' : 'ws://';
      const hostAndPath = customUrl.replace(/^(https?:\/\/)?/, '');
      newUrl = `${protocol}${hostAndPath.replace(/\/$/, '')}/wss`;
      if (debugState.isEnabled()) {
        logger.info(`Using custom backend WebSocket URL: ${newUrl}`);
      }
    }
  } catch {
    // Settings store not yet initialized — use default URL.
  }

  return newUrl;
}

// ── Event emitter ──────────────────────────────────────────────────────

export function emit(eventName: string, data: unknown) {
  const handlers = eventHandlers.get(eventName);
  if (handlers) {
    handlers.forEach(handler => {
      try {
        handler(data);
      } catch (error) {
        logger.error(`Error in event handler for ${eventName}:`, createErrorMetadata(error));
      }
    });
  }
}

// ── Notification helpers ───────────────────────────────────────────────

export function notifyConnectionStatusHandlers(connected: boolean) {
  connectionStatusHandlers.forEach(handler => {
    try {
      handler(connected);
    } catch (error) {
      logger.error('Error in connection status handler:', createErrorMetadata(error));
    }
  });
}

export function notifyConnectionStateHandlers(state: ConnectionState) {
  connectionStateHandlers.forEach(handler => {
    try {
      handler(state);
    } catch (error) {
      logger.error('Error in connection state handler:', createErrorMetadata(error));
    }
  });
}

// ── Message queuing ────────────────────────────────────────────────────

export function queueMessage(
  set: (partial: Record<string, unknown> | ((s: { messageQueue: QueuedMessage[] }) => { messageQueue: QueuedMessage[] })) => void,
  type: 'text' | 'binary',
  data: string | ArrayBuffer
) {
  set((state: { messageQueue: QueuedMessage[] }) => {
    const newQueue = [...state.messageQueue];
    if (newQueue.length >= MAX_QUEUE_SIZE) {
      newQueue.shift();
      logger.warn('Message queue full, removed oldest message');
    }
    newQueue.push({
      type,
      data,
      timestamp: Date.now(),
      retries: 0
    });
    return { messageQueue: newQueue };
  });
}

export async function processMessageQueue(
  get: () => { isConnected: boolean; socket: WebSocket | null; messageQueue: QueuedMessage[] },
  set: (partial: Record<string, unknown> | ((s: { messageQueue: QueuedMessage[] }) => { messageQueue: QueuedMessage[] })) => void,
) {
  const state = get();
  if (!state.isConnected || !state.socket || state.messageQueue.length === 0) {
    return;
  }

  const messagesToProcess = [...state.messageQueue];
  set({ messageQueue: [] });

  for (const queuedMessage of messagesToProcess) {
    try {
      if (queuedMessage.type === 'text') {
        state.socket.send(queuedMessage.data as string);
      } else {
        state.socket.send(queuedMessage.data as ArrayBuffer);
      }

      if (debugState.isDataDebugEnabled()) {
        logger.debug(`Processed queued ${queuedMessage.type} message`);
      }
    } catch (error) {
      queuedMessage.retries++;
      if (queuedMessage.retries < 3) {
        set((s: { messageQueue: QueuedMessage[] }) => ({ messageQueue: [...s.messageQueue, queuedMessage] }));
        logger.warn(`Failed to send queued message, retry ${queuedMessage.retries}/3`);
      } else {
        logger.error('Failed to send queued message after 3 retries, dropping:', createErrorMetadata(error));
      }
    }
  }
}

// ── Heartbeat ──────────────────────────────────────────────────────────
// Liveness is ping/pong only. Binary silence is NOT a failure signal: with acknowledged
// streaming (ADR-2141) a client that holds the latest positions is sent nothing at all.

export function startHeartbeat(
  get: () => { isConnected: boolean; socket: WebSocket | null },
) {
  stopHeartbeat();
  heartbeatInterval = window.setInterval(() => {
    sendHeartbeat(get);
  }, HEARTBEAT_INTERVAL_MS);
}

export function stopHeartbeat() {
  if (heartbeatInterval) {
    window.clearInterval(heartbeatInterval);
    heartbeatInterval = null;
  }
  if (heartbeatTimeout) {
    window.clearTimeout(heartbeatTimeout);
    heartbeatTimeout = null;
  }
}

/**
 * Cancel any pending reconnect timer. Must be called on explicit
 * disconnect/teardown — otherwise a scheduled reconnect fires after
 * close() and revives the socket (leak + correctness bug).
 */
export function clearReconnectTimeout() {
  if (reconnectTimeout) {
    window.clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }
}

function sendHeartbeat(
  get: () => { isConnected: boolean; socket: WebSocket | null },
) {
  const state = get();
  if (!state.isConnected || !state.socket) {
    return;
  }

  try {
    state.socket.send('ping');

    heartbeatTimeout = window.setTimeout(() => {
      logger.warn('Heartbeat timeout - server not responding');
      handleHeartbeatTimeout(get);
    }, HEARTBEAT_TIMEOUT_MS);

    if (debugState.isDataDebugEnabled()) {
      logger.debug('Sent heartbeat ping');
    }
  } catch (error) {
    logger.error('Error sending heartbeat:', createErrorMetadata(error));
    handleHeartbeatTimeout(get);
  }
}

export function handleHeartbeatResponse() {
  if (heartbeatTimeout) {
    window.clearTimeout(heartbeatTimeout);
    heartbeatTimeout = null;
  }

  if (debugState.isDataDebugEnabled()) {
    logger.debug('Received heartbeat pong');
  }
}

function handleHeartbeatTimeout(
  get: () => { socket: WebSocket | null },
) {
  logger.warn('Heartbeat timeout detected, connection may be dead');
  const state = get();
  if (state.socket) {
    state.socket.close(4000, 'Heartbeat timeout');
  }
}

// ── Reconnect with exponential backoff ─────────────────────────────────

export function attemptReconnect(
  get: () => { reconnectAttempts: number; maxReconnectAttempts: number; connect: () => Promise<void> },
  set: (partial: Record<string, unknown> | ((s: { reconnectAttempts: number }) => { reconnectAttempts: number })) => void,
  updateConnectionState: (status: ConnectionState['status'], lastError?: string) => void,
) {
  if (reconnectTimeout) {
    window.clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }

  const state = get();
  if (state.reconnectAttempts < state.maxReconnectAttempts) {
    set((s: { reconnectAttempts: number }) => ({ reconnectAttempts: s.reconnectAttempts + 1 }));

    const baseDelay = 1000;
    const jitter = Math.random() * 500;
    const delay = Math.min(baseDelay * Math.pow(2, state.reconnectAttempts) + jitter, MAX_RECONNECT_DELAY);

    updateConnectionState('reconnecting', `Reconnecting in ${delay}ms`);

    if (debugState.isEnabled()) {
      logger.info(`Attempting to reconnect in ${delay}ms (attempt ${state.reconnectAttempts + 1}/${state.maxReconnectAttempts})`);
    }

    reconnectTimeout = window.setTimeout(() => {
      get().connect().catch(error => {
        logger.error('Reconnect attempt failed:', createErrorMetadata(error));
        attemptReconnect(get, set, updateConnectionState);
      });
    }, delay);
  } else {
    logger.error(`Maximum reconnect attempts (${state.maxReconnectAttempts}) reached. Giving up.`);
    updateConnectionState('failed', 'Maximum reconnect attempts reached');
  }
}

// ── Auth helper ────────────────────────────────────────────────────────

export function sendAuthOnConnect(socket: WebSocket, url: string) {
  const user = nostrAuth.getCurrentUser();
  if (user?.pubkey) {
    if (nostrAuth.isDevMode()) {
      const isEphemeral = !!sessionStorage.getItem('ephemeral_session_pubkey');
      socket.send(JSON.stringify({
        type: 'authenticate',
        token: 'dev-session-token',
        pubkey: user.pubkey,
        ephemeral: isEphemeral,
      }));
    } else {
      (async () => {
        try {
          const httpUrl = url.replace(/^ws(s?):\/\//, 'http$1://');
          const eventToken = await nostrAuth.signRequest(httpUrl, 'GET');
          socket.send(JSON.stringify({ type: 'authenticate', event: eventToken }));
        } catch (e) {
          logger.error('NIP-98 WS auth signing failed:', e);
        }
      })();
    }
  }
}

// ── Handler registration helpers ───────────────────────────────────────

export function registerMessageHandler(handler: LegacyMessageHandler): () => void {
  messageHandlers.push(handler);
  return () => {
    messageHandlers = messageHandlers.filter(h => h !== handler);
  };
}

export function registerBinaryMessageHandler(handler: BinaryMessageHandler): () => void {
  binaryMessageHandlers.push(handler);
  return () => {
    binaryMessageHandlers = binaryMessageHandlers.filter(h => h !== handler);
  };
}

export function registerConnectionStatusHandler(handler: ConnectionStatusHandler, currentConnected: boolean): () => void {
  connectionStatusHandlers.push(handler);
  handler(currentConnected);
  return () => {
    connectionStatusHandlers = connectionStatusHandlers.filter(h => h !== handler);
  };
}

export function registerConnectionStateHandler(handler: ConnectionStateHandler, currentState: ConnectionState): () => void {
  connectionStateHandlers.push(handler);
  handler(currentState);
  return () => {
    connectionStateHandlers = connectionStateHandlers.filter(h => h !== handler);
  };
}

export function registerEventHandler(eventName: string, handler: EventHandler): () => void {
  if (!eventHandlers.has(eventName)) {
    eventHandlers.set(eventName, []);
  }
  eventHandlers.get(eventName)!.push(handler);

  return () => {
    const handlers = eventHandlers.get(eventName);
    if (handlers) {
      const index = handlers.indexOf(handler);
      if (index > -1) {
        handlers.splice(index, 1);
      }
    }
  };
}

export function notifyBinaryMessageHandlers(data: ArrayBuffer) {
  binaryMessageHandlers.forEach(handler => {
    try {
      handler(data);
    } catch (error) {
      logger.error('Error in binary message handler:', createErrorMetadata(error));
    }
  });
}

export function notifyMessageHandlers(message: Parameters<LegacyMessageHandler>[0]) {
  messageHandlers.forEach(handler => {
    try {
      handler(message);
    } catch (error) {
      logger.error('Error in message handler:', createErrorMetadata(error));
    }
  });
}

// ── Solid WebSocket (re-exported from solidWebSocket.ts) ───────────────
export {
  connectSolidWebSocket,
  disconnectSolidWebSocket,
  resetSolidReconnect,
  subscribeSolidResource,
  unsubscribeSolidResource,
} from './solidWebSocket';
