import { useEffect } from 'react';
import { webSocketService, WebSocketErrorFrame } from '../store/websocketStore';
import { useErrorHandler } from './useErrorHandler';
import { useSettingsStore } from '../store/settingsStore';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('WebSocketErrorHandler');

export function useWebSocketErrorHandler() {
  const { handleError, handleWebSocketError, handleSettingsError } = useErrorHandler();
  
  useEffect(() => {

    const unsubscribeErrorFrame = webSocketService.on('error-frame', (data: unknown) => {
      const error = data as WebSocketErrorFrame;
      logger.warn('WebSocket error frame received:', error);
      
      
      switch (error.category) {
        case 'validation':
          handleError(new Error(error.message), {
            title: 'Validation Error',
            category: 'validation',
            metadata: { 
              code: error.code,
              affectedPaths: error.affectedPaths,
              details: error.details
            }
          });
          break;
          
        case 'rate_limit':
          handleError(new Error(error.message), {
            title: 'Rate Limit Exceeded',
            category: 'network',
            retry: async () => {
              
              if (error.retryAfter) {
                await new Promise(resolve => setTimeout(resolve, error.retryAfter));
              }
              
              await (webSocketService as unknown as { processMessageQueue?: () => Promise<void> }).processMessageQueue?.();
            },
            metadata: {
              retryAfter: error.retryAfter,
              code: error.code
            }
          });
          break;
          
        case 'auth':
          handleError(new Error(error.message), {
            title: 'Authentication Error',
            category: 'network',
            actionLabel: 'Sign In',
            onAction: () => {
              
              window.location.href = '/auth/login';
            },
            metadata: {
              code: error.code
            }
          });
          break;
          
        case 'server':
          if (error.retryable) {
            handleError(new Error(error.message), {
              title: 'Server Error',
              category: 'network',
              retry: async () => {
                await webSocketService.connect();
              },
              metadata: {
                code: error.code,
                retryAfter: error.retryAfter
              }
            });
          } else {
            handleError(new Error(error.message), {
              title: 'Server Error',
              category: 'network',
              metadata: {
                code: error.code
              }
            });
          }
          break;
          
        case 'protocol':
          handleWebSocketError(new Error(error.message));
          break;
      }
    });
    
    
    const unsubscribeValidation = webSocketService.on('validation-error', (rawData: unknown) => {
      const data = rawData as { paths: string[], message: string };
      handleSettingsError(new Error(data.message), data.paths);
    });


    const unsubscribeRateLimit = webSocketService.on('rate-limit', (rawData: unknown) => {
      const data = rawData as { retryAfter: number, message: string };
      logger.info(`Rate limited. Will retry after ${data.retryAfter}ms`);
    });


    const unsubscribeAuth = webSocketService.on('auth-error', (rawData: unknown) => {
      const data = rawData as { code: string, message: string };
      
      useSettingsStore.getState().setAuthenticated(false);
      useSettingsStore.getState().setUser(null);
    });
    
    
    const unsubscribeConnectionState = webSocketService.onConnectionStateChange((state) => {
      if (state.status === 'failed') {
        handleWebSocketError(new Error(state.lastError || 'Connection failed'));
      }
    });
    
    return () => {
      unsubscribeErrorFrame();
      unsubscribeValidation();
      unsubscribeRateLimit();
      unsubscribeAuth();
      unsubscribeConnectionState();
    };
  }, [handleError, handleWebSocketError, handleSettingsError]);
}

// Helper to report client errors to server
export function reportClientError(error: Error, context?: Record<string, unknown>) {
  try {
    webSocketService.sendErrorFrame({
      code: 'CLIENT_ERROR',
      message: error.message,
      category: 'protocol',
      details: {
        stack: error.stack,
        context,
        userAgent: navigator.userAgent,
        timestamp: new Date().toISOString()
      },
      retryable: false
    });
  } catch (sendError) {
    
    logger.debug('Failed to report client error:', sendError);
  }
}