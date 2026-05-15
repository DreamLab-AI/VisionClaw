/**
 * websocket/index.ts — Zustand store combining all WebSocket modules
 *
 * Re-exports the store hook, service compat class, utility hooks, and all
 * public types so that consumers can import from this directory.
 */

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import { createLogger, createErrorMetadata } from '../../utils/loggerConfig';
import { debugState } from '../../utils/clientDebugState';
import { useSettingsStore } from '../settingsStore';
import type { WebSocketMessage } from '../../types/websocketTypes';
import { webSocketRegistry } from '../../services/WebSocketRegistry';
import { webSocketEventBus } from '../../services/WebSocketEventBus';
import { useAnalyticsStore } from '../analyticsStore';

// ── Module imports ─────────────────────────────────────────────────────
import {
  determineWebSocketUrl,
  getUrlFromSettings,
  emit,
  notifyConnectionStatusHandlers,
  notifyConnectionStateHandlers,
  queueMessage,
  processMessageQueue,
  startHeartbeat,
  stopHeartbeat,
  handleHeartbeatResponse,
  attemptReconnect,
  sendAuthOnConnect,
  registerMessageHandler,
  registerBinaryMessageHandler,
  registerConnectionStatusHandler,
  registerConnectionStateHandler,
  registerEventHandler,
  connectSolidWebSocket,
  disconnectSolidWebSocket,
  getHandlerState,
  resetHandlerState,
} from './connectionManager';

import {
  validateBinaryData,
  processBinaryData,
  initializeBatchQueue,
  destroyBatchQueue,
  sendNodePositionUpdates as sendNodePositionUpdatesFn,
  flushPositionUpdates as flushPositionUpdatesFn,
  getPositionQueueMetrics as getPositionQueueMetricsFn,
  getPositionBatchQueue,
  resetBinaryState,
} from './binaryProtocol';

import {
  setupFilterSubscription,
  cleanupFilterSubscriptions,
  forceRefreshFilter as forceRefreshFilterFn,
  resetFilterState,
} from './filterSync';

import { handleTextMessage } from './textMessageHandler';

import type {
  WebSocketState,
  WebSocketErrorFrame,
  ConnectionState,
  FilterUpdateParams,
  SolidNotificationCallback,
  QueuedMessage,
} from './types';

// Re-export all public types
export type {
  WebSocketAdapter,
  WebSocketErrorFrame,
  QueuedMessage,
  ConnectionState,
  SolidNotification,
  SolidNotificationCallback,
  WebSocketState,
  FilterUpdateParams,
  NodePositionUpdate,
  WebSocketInternals,
  FilterSnapshot,
  LegacyMessageHandler,
  BinaryMessageHandler,
  ConnectionStatusHandler,
  ConnectionStateHandler,
  EventHandler,
} from './types';

const logger = createLogger('WebSocketStore');

// ── Zustand store ──────────────────────────────────────────────────────

export const useWebSocketStore = create<WebSocketState>()(
  subscribeWithSelector((set, get) => {
    // Helper: update connection state in store + notify handlers
    const updateConnectionState = (
      status: ConnectionState['status'],
      lastError?: string,
      lastConnected?: number
    ) => {
      set(state => ({
        connectionState: {
          ...state.connectionState,
          status,
          lastError,
          lastConnected,
          reconnectAttempts: state.reconnectAttempts
        }
      }));
      notifyConnectionStateHandlers(get().connectionState);
    };

    const validateMessage = (message: unknown): message is WebSocketMessage => {
      return (
        message !== null &&
        typeof message === 'object' &&
        typeof (message as WebSocketMessage).type === 'string' &&
        (message as WebSocketMessage).type.length > 0 &&
        (message as WebSocketMessage).type.length <= 100
      );
    };

    // Defer settings subscription to avoid circular import initialization crash.
    queueMicrotask(() => {
      let previousCustomBackendUrl = useSettingsStore.getState().settings?.system?.customBackendUrl;
      set({ url: getUrlFromSettings() });

      useSettingsStore.subscribe((state) => {
        const newCustomBackendUrl = state.settings?.system?.customBackendUrl;
        if (newCustomBackendUrl !== previousCustomBackendUrl) {
          if (debugState.isEnabled()) {
            logger.info(`customBackendUrl setting changed from "${previousCustomBackendUrl}" to "${newCustomBackendUrl}", re-evaluating WebSocket URL.`);
          }
          previousCustomBackendUrl = newCustomBackendUrl;
          const wsState = get();
          set({ url: getUrlFromSettings() });
          if (wsState.isConnected || (wsState.socket && wsState.socket.readyState === WebSocket.CONNECTING)) {
            logger.info('Reconnecting WebSocket due to customBackendUrl change.');
            wsState.close();
            setTimeout(() => {
              get().connect().catch(error => {
                logger.error('Failed to reconnect WebSocket after URL change:', createErrorMetadata(error));
              });
            }, 100);
          }
        }
      });
    });

    return {
      // ── Initial state ────────────────────────────────────────────
      socket: null,
      isConnected: false,
      isServerReady: false,
      connectionState: {
        status: 'disconnected',
        reconnectAttempts: 0
      },
      url: getUrlFromSettings(),
      solidSocket: null,
      isSolidConnected: false,
      solidSubscriptions: new Map(),
      messageQueue: [],
      statistics: {
        messagesReceived: 0,
        messagesSent: 0,
        bytesReceived: 0,
        bytesSent: 0,
        connectionTime: 0,
        reconnections: 0,
        averageLatency: 0,
        messagesByType: {},
        errors: 0,
        lastActivity: Date.now()
      },
      reconnectInterval: 1000,
      maxReconnectAttempts: 10,
      reconnectAttempts: 0,

      // ── Actions ──────────────────────────────────────────────────

      connect: async () => {
        const state = get();
        if (state.socket && (state.socket.readyState === WebSocket.CONNECTING || state.socket.readyState === WebSocket.OPEN)) {
          return;
        }

        try {
          if (debugState.isEnabled()) {
            logger.info(`Connecting to WebSocket at ${state.url}`);
          }

          const socket = new WebSocket(state.url);
          socket.binaryType = 'arraybuffer';

          socket.onopen = () => {
            if (get().socket !== null && get().socket !== socket) return;

            set({ socket, isConnected: true, reconnectAttempts: 0 });
            updateConnectionState('connected', undefined, Date.now());

            webSocketRegistry.register('graph', state.url, socket);
            webSocketEventBus.emit('connection:open', { name: 'graph', url: state.url });

            if (debugState.isEnabled()) {
              logger.info('WebSocket connection established');
            }

            sendAuthOnConnect(socket, state.url);

            const currentFilter = useSettingsStore.getState().settings?.nodeFilter;
            if (currentFilter) {
              get().sendMessage('filter_update', {
                enabled: currentFilter.enabled,
                quality_threshold: currentFilter.qualityThreshold,
                authority_threshold: currentFilter.authorityThreshold,
                filter_by_quality: currentFilter.filterByQuality,
                filter_by_authority: currentFilter.filterByAuthority,
                filter_mode: currentFilter.filterMode,
              });
            }

            initializeBatchQueue(get);
            setupFilterSubscription(get);
            notifyConnectionStatusHandlers(true);
            startHeartbeat(get);
            processMessageQueue(get, set);
          };

          // --- Binary frame velocity management ---
          let _pendingBinaryFrame: ArrayBuffer | null = null;
          let _binaryFrameScheduled = false;
          let _binaryDropCount = 0;

          const scheduleBinaryProcessing = (buffer: ArrayBuffer) => {
            if (_pendingBinaryFrame !== null) {
              _binaryDropCount++;
              if (_binaryDropCount % 100 === 1) {
                logger.debug(`[BinaryVelocity] Dropped ${_binaryDropCount} stale binary frames (keeping latest)`);
              }
            }
            _pendingBinaryFrame = buffer;

            if (!_binaryFrameScheduled) {
              _binaryFrameScheduled = true;
              queueMicrotask(() => {
                _binaryFrameScheduled = false;
                const frame = _pendingBinaryFrame;
                _pendingBinaryFrame = null;
                if (frame) {
                  try {
                    processBinaryData(frame, get);
                  } catch (err) {
                    logger.error('Error in binary frame processing:', createErrorMetadata(err));
                  }
                }
              });
            }
          };

          socket.onmessage = (event: MessageEvent) => {
            if (get().socket !== socket) return;

            if (event.data === 'pong') {
              handleHeartbeatResponse();
              return;
            }

            if (event.data instanceof Blob) {
              if (debugState.isDataDebugEnabled()) {
                logger.debug('Received binary blob data');
              }

              event.data.arrayBuffer().then(buffer => {
                if (validateBinaryData(buffer)) {
                  scheduleBinaryProcessing(buffer);
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
              if (validateBinaryData(event.data)) {
                scheduleBinaryProcessing(event.data);
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

              const message = JSON.parse(event.data) as WebSocketMessage;

              if (!validateMessage(message)) {
                logger.warn('Received malformed message, skipping processing');
                return;
              }

              handleTextMessage(message, get, set, () => processMessageQueue(get, set));
            } catch (error) {
              logger.error('Error parsing WebSocket message:', createErrorMetadata(error));
            }
          };

          socket.onclose = (event: CloseEvent) => {
            if (get().socket !== null && get().socket !== socket) return;

            set({ isConnected: false, isServerReady: false });
            stopHeartbeat();

            webSocketRegistry.unregister('graph');
            webSocketEventBus.emit('connection:close', {
              name: 'graph',
              code: event.code,
              reason: event.reason,
            });

            if (debugState.isEnabled()) {
              logger.info(`WebSocket connection closed: ${event.code} ${event.reason}`);
            }

            notifyConnectionStatusHandlers(false);

            const isNormalClosure = event.code === 1000 || event.code === 1001;
            const wasCleanShutdown = event.wasClean;

            if (!isNormalClosure || !wasCleanShutdown) {
              updateConnectionState('reconnecting', event.reason);
              attemptReconnect(get, set, updateConnectionState);
            } else {
              updateConnectionState('disconnected');
            }
          };

          socket.onerror = (event: Event) => {
            if (get().socket !== null && get().socket !== socket) return;

            const errorMessage = event instanceof ErrorEvent ? event.message : 'Unknown WebSocket error';
            logger.error('WebSocket error:', { event, message: errorMessage });
            webSocketEventBus.emit('connection:error', { name: 'graph', error: errorMessage });
            updateConnectionState('failed', errorMessage);
          };

          set({ socket });

          return new Promise<void>((resolve, reject) => {
            socket.addEventListener('open', () => resolve(), { once: true });
            socket.addEventListener('error', () => {
              if (socket.readyState !== WebSocket.OPEN) {
                reject(new Error('WebSocket connection failed'));
              }
            }, { once: true });
          });
        } catch (error) {
          logger.error('Error establishing WebSocket connection:', createErrorMetadata(error));
          throw error;
        }
      },

      disconnect: () => {
        get().close();
        get().disconnectSolid();
      },

      close: () => {
        const state = get();
        if (state.socket) {
          stopHeartbeat();
          webSocketRegistry.unregister('graph');
          destroyBatchQueue();
          cleanupFilterSubscriptions();

          try {
            state.socket.close(1000, 'Normal closure');
            if (debugState.isEnabled()) {
              logger.info('WebSocket connection closed by client');
            }
          } catch (error) {
            logger.error('Error closing WebSocket:', createErrorMetadata(error));
          } finally {
            set({
              socket: null,
              isConnected: false,
              isServerReady: false,
              reconnectAttempts: 0,
              messageQueue: []
            });
            updateConnectionState('disconnected');
            notifyConnectionStatusHandlers(false);
          }
        }
      },

      sendMessage: (type: string, data?: unknown) => {
        const state = get();
        const message = { type, data };
        const messageStr = JSON.stringify(message);

        if (!state.isConnected || !state.socket) {
          queueMessage(set, 'text', messageStr);
          logger.warn(`Message queued: ${type} (WebSocket not connected)`);
          return;
        }

        try {
          state.socket.send(messageStr);

          if (debugState.isDataDebugEnabled()) {
            logger.debug(`Sent message: ${type}`);
          }
        } catch (error) {
          logger.error('Error sending WebSocket message:', createErrorMetadata(error));
          queueMessage(set, 'text', messageStr);
        }
      },

      sendRawBinaryData: (data: ArrayBuffer) => {
        const state = get();
        if (!state.isConnected || !state.socket) {
          queueMessage(set, 'binary', data);
          logger.warn(`Binary data queued: ${data.byteLength} bytes (WebSocket not connected)`);
          return;
        }

        try {
          state.socket.send(data);

          if (debugState.isDataDebugEnabled()) {
            logger.debug(`Sent binary data: ${data.byteLength} bytes`);
          }
        } catch (error) {
          logger.error('Error sending binary data:', createErrorMetadata(error));
          queueMessage(set, 'binary', data);
        }
      },

      sendFilterUpdate: (filter: FilterUpdateParams) => {
        const state = get();
        if (!state.isConnected) {
          logger.warn('Cannot send filter update: WebSocket not connected');
          return;
        }

        state.sendMessage('filter_update', {
          enabled: filter.enabled,
          quality_threshold: filter.qualityThreshold,
          authority_threshold: filter.authorityThreshold,
          filter_by_quality: filter.filterByQuality,
          filter_by_authority: filter.filterByAuthority,
          filter_mode: filter.filterMode,
        });

        logger.info('Filter update sent to server', filter);
      },

      forceRefreshFilter: () => forceRefreshFilterFn(get),

      forceReconnect: () => {
        logger.info('Forcing WebSocket reconnection');
        const state = get();
        if (state.socket) {
          state.socket.close(4001, 'Forced reconnection');
        }
      },

      clearMessageQueue: () => {
        const queueSize = get().messageQueue.length;
        set({ messageQueue: [] });
        if (queueSize > 0) {
          logger.info(`Cleared ${queueSize} messages from queue`);
        }
      },

      setCustomBackendUrl: (backendUrl: string | null) => {
        if (!backendUrl) {
          set({ url: determineWebSocketUrl() });
          if (debugState.isEnabled()) {
            logger.info(`Reset to default WebSocket URL: ${get().url}`);
          }
          return;
        }

        const protocol = backendUrl.startsWith('https://') ? 'wss://' : 'ws://';
        const hostWithProtocol = backendUrl.replace(/^(https?:\/\/)?/, '');
        const newUrl = `${protocol}${hostWithProtocol}/wss`;

        set({ url: newUrl });

        if (debugState.isEnabled()) {
          logger.info(`Set custom WebSocket URL: ${newUrl}`);
        }

        const state = get();
        if (state.isConnected && state.socket) {
          if (debugState.isEnabled()) {
            logger.info('Reconnecting with new WebSocket URL');
          }
          state.close();
          state.connect().catch(error => {
            logger.error('Failed to reconnect with new URL:', createErrorMetadata(error));
          });
        }
      },

      sendNodePositionUpdates: (updates) => sendNodePositionUpdatesFn(updates),
      flushPositionUpdates: () => flushPositionUpdatesFn(),
      getPositionQueueMetrics: () => getPositionQueueMetricsFn(),

      onMessage: (handler) => registerMessageHandler(handler),
      onBinaryMessage: (handler) => registerBinaryMessageHandler(handler),
      onConnectionStatusChange: (handler) => registerConnectionStatusHandler(handler, get().isConnected),
      onConnectionStateChange: (handler) => registerConnectionStateHandler(handler, get().connectionState),
      on: (eventName, handler) => registerEventHandler(eventName, handler),
      emit,

      connectSolid: () => {
        connectSolidWebSocket(get, set, () => get().connectSolid());
      },

      disconnectSolid: () => {
        disconnectSolidWebSocket(get, set);
      },

      subscribeSolidResource: (resourceUrl: string, callback: SolidNotificationCallback) => {
        set(state => {
          const newSubscriptions = new Map(state.solidSubscriptions);
          if (!newSubscriptions.has(resourceUrl)) {
            newSubscriptions.set(resourceUrl, new Set());

            if (state.solidSocket?.readyState === WebSocket.OPEN) {
              state.solidSocket.send(`sub ${resourceUrl}`);
              logger.debug('Subscribed to Solid resource', { url: resourceUrl });
            }
          }

          newSubscriptions.get(resourceUrl)!.add(callback);
          return { solidSubscriptions: newSubscriptions };
        });

        return () => {
          set(state => {
            const newSubscriptions = new Map(state.solidSubscriptions);
            const callbacks = newSubscriptions.get(resourceUrl);
            if (callbacks) {
              callbacks.delete(callback);

              if (callbacks.size === 0) {
                if (state.solidSocket?.readyState === WebSocket.OPEN) {
                  state.solidSocket.send(`unsub ${resourceUrl}`);
                  logger.debug('Unsubscribed from Solid resource', { url: resourceUrl });
                }
                newSubscriptions.delete(resourceUrl);
              }
            }
            return { solidSubscriptions: newSubscriptions };
          });
        };
      },

      unsubscribeSolidResource: (resourceUrl: string) => {
        set(state => {
          const newSubscriptions = new Map(state.solidSubscriptions);
          if (newSubscriptions.has(resourceUrl)) {
            if (state.solidSocket?.readyState === WebSocket.OPEN) {
              state.solidSocket.send(`unsub ${resourceUrl}`);
              logger.debug('Unsubscribed from Solid resource (all callbacks)', { url: resourceUrl });
            }
            newSubscriptions.delete(resourceUrl);
          }
          return { solidSubscriptions: newSubscriptions };
        });
      },

      isSolidWebSocketConnected: () => {
        const state = get();
        return state.isSolidConnected && state.solidSocket?.readyState === WebSocket.OPEN;
      },

      getSolidSubscriptions: () => {
        return Array.from(get().solidSubscriptions.keys());
      },

      isReady: () => {
        const state = get();
        return state.isConnected && state.isServerReady;
      },

      getConnectionState: () => {
        return { ...get().connectionState };
      },

      getQueuedMessageCount: () => {
        return get().messageQueue.length;
      },

      _reset: () => {
        const state = get();
        state.close();
        state.disconnectSolid();

        resetHandlerState();
        resetBinaryState();
        resetFilterState();
        useAnalyticsStore.getState().reset();

        set({
          socket: null,
          isConnected: false,
          isServerReady: false,
          connectionState: {
            status: 'disconnected',
            reconnectAttempts: 0
          },
          url: getUrlFromSettings(),
          solidSocket: null,
          isSolidConnected: false,
          solidSubscriptions: new Map(),
          messageQueue: [],
          statistics: {
            messagesReceived: 0,
            messagesSent: 0,
            bytesReceived: 0,
            bytesSent: 0,
            connectionTime: 0,
            reconnections: 0,
            averageLatency: 0,
            messagesByType: {},
            errors: 0,
            lastActivity: Date.now()
          },
          reconnectAttempts: 0
        });
      },

      _getInternals: () => {
        const handlerState = getHandlerState();
        return {
          ...handlerState,
          positionBatchQueue: getPositionBatchQueue(),
        };
      }
    };
  })
);

// ── Backward-compatible service wrapper (from serviceCompat.ts) ────────
export { webSocketService, WebSocketServiceCompat } from './serviceCompat';

// Utility hooks for common selections
export const useWebSocketConnection = () => useWebSocketStore(state => ({
  isConnected: state.isConnected,
  isServerReady: state.isServerReady,
  connectionState: state.connectionState
}));

export const useWebSocketActions = () => useWebSocketStore(state => ({
  connect: state.connect,
  disconnect: state.disconnect,
  sendMessage: state.sendMessage
}));

export default useWebSocketStore;
