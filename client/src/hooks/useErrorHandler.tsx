import { useCallback, useRef } from 'react';
import { useToast, ToastAction } from '../features/design-system/components';
import { createLogger } from '../utils/loggerConfig';
import { webSocketService } from '../store/websocketStore';
import { unifiedApiClient } from '../services/api/UnifiedApiClient';

const logger = createLogger('ErrorHandler');

export interface ErrorOptions {
  title?: string;
  showDetails?: boolean;
  actionLabel?: string;
  onAction?: () => void;
  retry?: () => Promise<void>;
  maxRetries?: number;
  category?: 'network' | 'validation' | 'settings' | 'websocket' | 'general';
  metadata?: Record<string, any>;
}

export interface ErrorContext {
  timestamp: number;
  category: string;
  message: string;
  count: number;
  lastOccurrence: number;
}

export function useErrorHandler() {
  const { toast } = useToast();
  const errorHistory = useRef<Map<string, ErrorContext>>(new Map());
  // Use ref instead of state to avoid infinite re-render loops
  const retryAttempts = useRef<Map<string, number>>(new Map());

  const handleError = useCallback((error: unknown, options: ErrorOptions = {}) => {
    const {
      title = 'Something went wrong',
      showDetails = true,
      actionLabel,
      onAction,
      retry,
      maxRetries = 3,
      category = 'general',
      metadata
    } = options;

    
    let message = 'An unexpected error occurred';
    let details: string | undefined;
    let errorCode: string | undefined;

    if (error instanceof Error) {
      message = error.message;
      if (showDetails && error.stack) {
        details = error.stack;
      }
    } else if (typeof error === 'string') {
      message = error;
    } else if (error && typeof error === 'object' && 'message' in error) {
      message = String(error.message);
      
      if ('code' in error) {
        errorCode = String(error.code);
      }
    }

    
    const errorKey = `${category}:${message}`;
    const errorContext = errorHistory.current.get(errorKey) || {
      timestamp: Date.now(),
      category,
      message,
      count: 0,
      lastOccurrence: Date.now()
    };
    errorContext.count++;
    errorContext.lastOccurrence = Date.now();
    errorHistory.current.set(errorKey, errorContext);

    // Evict oldest entry when the map exceeds capacity to prevent unbounded growth
    if (errorHistory.current.size > 100) {
      const oldestKey = errorHistory.current.keys().next().value;
      if (oldestKey !== undefined) {
        errorHistory.current.delete(oldestKey);
      }
    }

    
    const userFriendlyMessages: Record<string, string> = {
      'Network request failed': 'Unable to connect to the server. Please check your internet connection.',
      'Failed to fetch': 'Could not load data. Please try again later.',
      'Unauthorized': 'You need to sign in to access this feature.',
      'Forbidden': 'You don\'t have permission to perform this action.',
      'Not found': 'The requested resource was not found.',
      'Internal server error': 'The server encountered an error. Please try again later.',
      'Timeout': 'The request took too long. Please try again.',
      'Invalid input': 'Please check your input and try again.',
      'Quota exceeded': 'You\'ve reached the usage limit. Please try again later.',
      '500 Internal Server Error': 'Server error. We\'re working on fixing this.',
      '502 Bad Gateway': 'Connection issue. Please try again in a moment.',
      '503 Service Unavailable': 'Service temporarily unavailable. Please try again later.',
      'WebSocket': 'Real-time connection issue. Attempting to reconnect...',
      'ECONNREFUSED': 'Cannot connect to server. Please check if the server is running.',
      'ETIMEDOUT': 'Connection timed out. Please check your network.',
      'batch update failed': 'Failed to save multiple settings. Trying individual updates...',
      'validation failed': 'Invalid data format. Please check your input.',
      'settings sync failed': 'Settings synchronization failed. Your changes may not be saved.'
    };

    
    let friendlyMessage = message;
    let showRetry = false;
    
    switch (category) {
      case 'network':
        showRetry = true;
        if (message.includes('ECONNREFUSED')) {
          friendlyMessage = 'Cannot connect to server. Please ensure the server is running.';
        }
        break;
      
      case 'websocket':
        showRetry = true;
        friendlyMessage = 'Real-time updates temporarily unavailable.';
        break;
      
      case 'settings':
        if (message.includes('batch')) {
          friendlyMessage = 'Settings update partially failed. Some changes may not be saved.';
          showRetry = true;
        }
        break;
      
      case 'validation':
        friendlyMessage = 'Invalid data provided. Please check your input and try again.';
        break;
    }

    
    for (const [pattern, friendly] of Object.entries(userFriendlyMessages)) {
      if (message.toLowerCase().includes(pattern.toLowerCase())) {
        friendlyMessage = friendly;
        break;
      }
    }


    const shouldAutoRetry = showRetry && retry && errorContext.count <= maxRetries;
    const currentRetries = retryAttempts.current.get(errorKey) || 0;

    if (shouldAutoRetry && currentRetries < maxRetries) {
      retryAttempts.current.set(errorKey, currentRetries + 1);
      
      
      const delay = Math.min(1000 * Math.pow(2, currentRetries), 10000);
      
      setTimeout(async () => {
        try {
          await retry();
          // Clear retry count on success
          retryAttempts.current.delete(errorKey);
          
          toast({
            title: 'Success',
            description: 'Operation completed successfully',
            variant: 'default',
          });
        } catch (retryError) {
          
          handleError(retryError, { ...options, category });
        }
      }, delay);
      
      friendlyMessage += ` Retrying in ${delay / 1000}s... (${currentRetries + 1}/${maxRetries})`;
    }

    
    const toastId = toast({
      title,
      description: friendlyMessage,
      variant: 'destructive',
      action: actionLabel && onAction ? (
        <ToastAction altText={actionLabel} onClick={onAction}>
          {actionLabel}
        </ToastAction>
      ) : retry && !shouldAutoRetry ? (
        <ToastAction 
          altText="Retry" 
          onClick={async () => {
            try {
              await retry();
              toast({
                title: 'Success',
                description: 'Operation completed successfully',
                variant: 'default',
              });
            } catch (retryError) {
              handleError(retryError, options);
            }
          }}
        >
          Retry
        </ToastAction>
      ) : undefined,
    });

    
    logger.error('Error handled:', {
      error,
      message,
      details,
      friendlyMessage,
      category,
      errorCode,
      metadata,
      errorCount: errorContext.count,
      retryAttempts: currentRetries
    });

    
    if (category !== 'general' || errorContext.count > 3) {
      sendErrorTelemetry({
        message,
        category,
        errorCode,
        metadata,
        errorCount: errorContext.count,
        timestamp: Date.now()
      });
    }
  }, [toast]);

  const handleAsyncError = useCallback((promise: Promise<any>, options?: ErrorOptions) => {
    return promise.catch(error => {
      handleError(error, options);
      throw error; 
    });
  }, [handleError]);

  
  const handleWebSocketError = useCallback((error: unknown) => {
    const connectionState = webSocketService.getConnectionState();
    
    handleError(error, {
      title: 'Connection Error',
      category: 'websocket',
      retry: async () => {
        await webSocketService.connect();
      },
      metadata: {
        connectionState,
        queuedMessages: webSocketService.getQueuedMessageCount()
      }
    });
  }, [handleError]);

  
  const handleSettingsError = useCallback((error: unknown, failedPaths?: string[]) => {
    handleError(error, {
      title: 'Settings Update Failed',
      category: 'settings',
      metadata: { failedPaths },
      retry: async () => {
        
        throw new Error('Settings retry not implemented');
      }
    });
  }, [handleError]);

  
  const getErrorStats = useCallback(() => {
    const stats: Record<string, { count: number; lastOccurrence: number }> = {};
    errorHistory.current.forEach((context, key) => {
      stats[key] = {
        count: context.count,
        lastOccurrence: context.lastOccurrence
      };
    });
    return stats;
  }, []);


  const clearErrorHistory = useCallback(() => {
    errorHistory.current.clear();
    retryAttempts.current.clear();
  }, []);

  return {
    handleError,
    handleAsyncError,
    handleWebSocketError,
    handleSettingsError,
    getErrorStats,
    clearErrorHistory
  };
}

// Helper function to send error telemetry
async function sendErrorTelemetry(errorData: Record<string, any>) {
  try {
    
    if (process.env.NODE_ENV !== 'production') return;
    
    await unifiedApiClient.post('/telemetry/errors', errorData);
  } catch (error) {
    
    logger.debug('Failed to send error telemetry:', error);
  }
}

// Hook for wrapping async functions with error handling
export function useWithErrorHandler() {
  const { toast } = useToast();

  const withErrorHandler = useCallback(<T extends (...args: any[]) => Promise<any>>(
    fn: T,
    options?: ErrorOptions
  ): T => {
    return (async (...args: Parameters<T>) => {
      try {
        return await fn(...args);
      } catch (error) {
        let message = 'An unexpected error occurred';
        if (error instanceof Error) {
          message = error.message;
        } else if (typeof error === 'string') {
          message = error;
        }

        toast({
          title: options?.title || 'Error',
          description: message,
          variant: 'destructive',
        });

        throw error;
      }
    }) as T;
  }, [toast]);

  return { withErrorHandler };
}