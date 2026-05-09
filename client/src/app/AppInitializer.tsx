import React, { useEffect } from 'react';
import { createLogger, createErrorMetadata } from '../utils/loggerConfig';
import { debugState } from '../utils/clientDebugState';
import { useSettingsStore } from '../store/settingsStore';
import { useWorkerErrorStore } from '../store/workerErrorStore';
import { webSocketService } from '../store/websocketStore';
import { graphWorkerProxy } from '../features/graph/managers/graphWorkerProxy';
import { graphDataManager } from '../features/graph/managers/graphDataManager';

const logger = createLogger('AppInitializer');

interface AppInitializerProps {
  onInitialized: () => void;
  onError: (error: Error) => void;
}

const AppInitializer: React.FC<AppInitializerProps> = ({ onInitialized, onError }) => {
  const { initialize } = useSettingsStore();

  useEffect(() => {
    const initApp = async () => {
      const t0 = performance.now();

      // Set up retry handler for worker initialization
      const initializeWorker = async (): Promise<boolean> => {
        try {
          logger.info('Step 1: Initializing graphWorkerProxy');
          await graphWorkerProxy.initialize();
          logger.info('Step 1b: graphWorkerProxy initialized, ensuring graphDataManager worker connection');

          const workerReady = await graphDataManager.ensureWorkerReady();
          logger.info(`Step 1c: graphDataManager worker ready: ${workerReady}`);

          if (!workerReady) {
            throw new Error('Graph worker failed to become ready after initialization');
          }

          return true;
        } catch (workerError) {
          logger.error('Worker initialization failed:', createErrorMetadata(workerError));
          const errorMessage = workerError instanceof Error ? workerError.message : String(workerError);

          let details = errorMessage;
          if (typeof SharedArrayBuffer === 'undefined') {
            details = 'SharedArrayBuffer is not available. This is required for the graph engine to function properly.';
          } else if (errorMessage.includes('Worker') || errorMessage.includes('worker')) {
            details = `Worker initialization error: ${errorMessage}`;
          }

          useWorkerErrorStore.getState().setWorkerError(
            'The graph visualization engine failed to initialize.',
            details
          );

          logger.warn('Continuing without fully initialized worker');
          return false;
        }
      };

      useWorkerErrorStore.getState().setRetryHandler(async () => {
        const success = await initializeWorker();
        if (!success) {
          throw new Error('Worker initialization retry failed');
        }
      });

      try {
        await initializeWorker();
        logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  worker ready`);

        logger.info('Step 2: graphWorkerProxy initialized, calling settings initialize');
        await initialize();
        logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  settings initialized`);

        // Apply debug settings
        const currentSettings = useSettingsStore.getState().settings;
        const systemDebug = (currentSettings as unknown as Record<string, Record<string, Record<string, unknown>>>)?.system?.debug;
        if (systemDebug) {
          try {
            const debugSettings = systemDebug;
            debugState.enableDebug(!!debugSettings.enabled);
            if (debugSettings.enabled) {
              debugState.enableDataDebug(!!debugSettings.enableDataDebug);
              debugState.enablePerformanceDebug(!!debugSettings.enablePerformanceDebug);
            }
          } catch (debugError) {
            logger.warn('Error applying debug settings:', createErrorMetadata(debugError));
          }
        }

        // Run WebSocket init and graph data fetch IN PARALLEL
        // WebSocket is for live position updates; graph data fetch is for initial load.
        // Neither depends on the other — parallelize to cut startup time.
        const wsPromise = (async () => {
          if (typeof graphDataManager !== 'undefined') {
            try {
              const settings = useSettingsStore.getState().settings;
              await initializeWebSocket(settings);
              logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  WebSocket connected`);
            } catch (wsError) {
              logger.error('WebSocket initialization failed, continuing with UI only:', createErrorMetadata(wsError));
              logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  WebSocket FAILED (non-fatal)`);
            }
          }
        })();

        const dataPromise = (async () => {
          try {
            logger.info('Fetching initial graph data via REST API');
            const graphData = await graphDataManager.fetchInitialData();
            logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  graph data: ${graphData.nodes.length} nodes, ${graphData.edges.length} edges`);
          } catch (fetchError) {
            logger.error('Failed to fetch initial graph data:', createErrorMetadata(fetchError));
            logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  graph data FAILED`);
            await graphDataManager.setGraphData({ nodes: [], edges: [] });
          }
        })();

        // Wait for BOTH, but neither blocks the other
        await Promise.all([wsPromise, dataPromise]);

        logger.debug(`+${((performance.now() - t0) / 1000).toFixed(1)}s  initialization complete`);
        onInitialized();
        logger.info('onInitialized called successfully');

      } catch (error) {
        logger.error('Failed to initialize application components:', createErrorMetadata(error as Error));
        onError(error as Error);
      }
    };

    initApp();
  }, []);

  
  const initializeWebSocket = async (settingsParam: Record<string, unknown> | null | undefined): Promise<void> => {
    // Use passed settings or fall back to fresh store read
    const settings = (settingsParam ?? useSettingsStore.getState().settings) as Record<string, unknown>;
    try {
      const websocketService = webSocketService;

      
      // Binary position updates are handled by the websocket store's binaryProtocol.ts
      // pipeline (processBinaryData → graphDataManager.updateNodePositions). No duplicate
      // handler needed here — registering one causes double-processing and incorrect
      // updateCount cadence for REST re-fetch of unknown nodes.

      
      websocketService.onConnectionStatusChange((connected) => {
        if (debugState.isEnabled()) {
          logger.info(`WebSocket connection status changed: ${connected}`);
        }

        
        if (connected) {
          try {
            if (websocketService.isReady()) {
              
              logger.info('WebSocket is connected and fully established - enabling binary updates');
              graphDataManager.setBinaryUpdatesEnabled(true);

              
              logger.info('Sending subscribe_position_updates message to server');
              const sys = settings?.system as Record<string, unknown> | undefined;
              const ws = sys?.websocket as Record<string, unknown> | undefined;
              const updateRate = (ws?.updateRate as number | undefined) || 60;
              websocketService.sendMessage('subscribe_position_updates', {
                binary: true,
                interval: updateRate
              });

              if (debugState.isDataDebugEnabled()) {
                logger.debug('Binary updates enabled and subscribed to position updates');
              }
            } else {
              logger.info('WebSocket connected but not fully established yet - waiting for readiness');



              graphDataManager.enableBinaryUpdates();


              const unsubscribe = websocketService.onMessage((message) => {
                if (message.type === 'connection_established') {

                  logger.info('Connection established message received, sending subscribe_position_updates');
                  const sys2 = settings?.system as Record<string, unknown> | undefined;
                  const ws2 = sys2?.websocket as Record<string, unknown> | undefined;
                  const updateRate2 = (ws2?.updateRate as number | undefined) || 60;
                  websocketService.sendMessage('subscribe_position_updates', {
                    binary: true,
                    interval: updateRate2
                  });
                  unsubscribe(); 

                  if (debugState.isDataDebugEnabled()) {
                    logger.debug('Connection established, subscribed to position updates');
                  }
                }
              });
            }
          } catch (connectionError) {
            logger.error('Error during WebSocket status change handling:', createErrorMetadata(connectionError));
          }
        }
      });

      
      if (websocketService) {
        const wsAdapter = {
          send: (data: ArrayBuffer) => {
            websocketService.sendRawBinaryData(data);
          },
          isReady: () => websocketService.isReady()
        };
        graphDataManager.setWebSocketService(wsAdapter);
      }

      try {
        
        await websocketService.connect();
      } catch (connectError) {
        logger.error('Failed to connect to WebSocket:', createErrorMetadata(connectError));
      }
    } catch (error) {
      logger.error('Failed during WebSocket/data initialization:', createErrorMetadata(error));
      throw error;
    }
  };

  return null; 
};

export default AppInitializer;
