

import { createLogger as createBaseLogger, createErrorMetadata, createDataMetadata, DEFAULT_LOG_LEVEL } from './baseLogger';
import type { LoggerOptions, LogEntry } from './baseLogger';
import { clientDebugState } from './clientDebugState';

// Re-export utility functions and types for backward compatibility
export { createErrorMetadata, createDataMetadata };
export type { LogEntry };

// Agent-specific telemetry types
export interface AgentTelemetryData {
  agentId: string;
  agentType: string;
  action: string;
  timestamp: Date;
  metadata?: Record<string, any>;
  position?: { x: number; y: number; z: number };
  performance?: {
    renderTime?: number;
    frameRate?: number;
    meshCount?: number;
    triangleCount?: number;
  };
}

export interface WebSocketTelemetryData {
  messageType: string;
  direction: 'incoming' | 'outgoing';
  timestamp: Date;
  size?: number;
  metadata?: Record<string, any>;
}

export interface ThreeJSTelemetryData {
  action: 'position_update' | 'mesh_create' | 'animation_frame' | 'force_applied';
  objectId: string;
  position?: { x: number; y: number; z: number };
  rotation?: { x: number; y: number; z: number };
  timestamp: Date;
  metadata?: Record<string, any>;
}

type LogLevel = 'debug' | 'info' | 'warn' | 'error';

const MAX_TELEMETRY_ENTRIES = 1000;

// Enhanced agent logger with telemetry capabilities
function createBaseAgentLogger(namespace: string, options: LoggerOptions = {}) {
  const baseLogger = createBaseLogger(namespace, options);

  const agentTelemetry: AgentTelemetryData[] = [];
  const webSocketTelemetry: WebSocketTelemetryData[] = [];
  const threeJSTelemetry: ThreeJSTelemetryData[] = [];

  function trimTelemetryArray<T>(array: T[]) {
    if (array.length > MAX_TELEMETRY_ENTRIES) {
      array.splice(0, array.length - MAX_TELEMETRY_ENTRIES);
    }
  }

  return {
    ...baseLogger,

    
    logAgentAction(agentId: string, agentType: string, action: string, metadata?: Record<string, any>, position?: { x: number; y: number; z: number }) {
      const telemetryData: AgentTelemetryData = {
        agentId,
        agentType,
        action,
        timestamp: new Date(),
        metadata,
        position
      };

      agentTelemetry.push(telemetryData);
      trimTelemetryArray(agentTelemetry);

      baseLogger.debug(`[AGENT:${agentType}:${agentId}] ${action}`, { metadata, position });
    },

    
    logWebSocketMessage(messageType: string, direction: 'incoming' | 'outgoing', metadata?: Record<string, any>, size?: number) {
      const telemetryData: WebSocketTelemetryData = {
        messageType,
        direction,
        timestamp: new Date(),
        size,
        metadata
      };

      webSocketTelemetry.push(telemetryData);
      trimTelemetryArray(webSocketTelemetry);

      baseLogger.debug(`[WS:${direction.toUpperCase()}:${messageType}] ${size ? `${size} bytes` : 'no size'}`, metadata);
    },

    
    logThreeJSAction(action: ThreeJSTelemetryData['action'], objectId: string, position?: { x: number; y: number; z: number }, rotation?: { x: number; y: number; z: number }, metadata?: Record<string, any>) {
      const telemetryData: ThreeJSTelemetryData = {
        action,
        objectId,
        position,
        rotation,
        timestamp: new Date(),
        metadata
      };

      threeJSTelemetry.push(telemetryData);
      trimTelemetryArray(threeJSTelemetry);

      baseLogger.debug(`[THREE.JS:${action.toUpperCase()}:${objectId}]`, { position, rotation, metadata });
    },

    
    logPerformance(operation: string, duration: number, metadata?: Record<string, any>) {
      baseLogger.info(`[PERF:${operation}] ${duration.toFixed(2)}ms`, metadata);
    },

    
    getAgentTelemetry: () => [...agentTelemetry],
    getWebSocketTelemetry: () => [...webSocketTelemetry],
    getThreeJSTelemetry: () => [...threeJSTelemetry],

    
    clearTelemetry() {
      agentTelemetry.length = 0;
      webSocketTelemetry.length = 0;
      threeJSTelemetry.length = 0;
      baseLogger.info('Telemetry data cleared');
    }
  };
}

// Track created loggers for dynamic updates
const createdLoggers = new Set<{
  logger: any;
  namespace: string;
  options: LoggerOptions;
  updateLevel: (level: LogLevel, enabled: boolean) => void;
}>();


function isValidLogLevel(level: any): level is LogLevel {
  const validLevels: LogLevel[] = ['debug', 'info', 'warn', 'error'];
  return validLevels.includes(level);
}


function getEffectiveLogLevel(options: LoggerOptions = {}): LogLevel {
  
  if (options.respectRuntimeSettings === false) {
    return DEFAULT_LOG_LEVEL;
  }

  
  const consoleLoggingEnabled = clientDebugState.get('consoleLogging');
  if (!consoleLoggingEnabled) {
    
    return options.level || DEFAULT_LOG_LEVEL;
  }

  
  const runtimeLogLevel = clientDebugState.get('logLevel') as LogLevel;
  if (runtimeLogLevel && isValidLogLevel(runtimeLogLevel)) {
    return runtimeLogLevel;
  }

  
  return options.level || DEFAULT_LOG_LEVEL;
}


function shouldEnableLogging(options: LoggerOptions): boolean {
  if (options.respectRuntimeSettings === false) {
    return !options.disabled;
  }

  const consoleLoggingEnabled = clientDebugState.get('consoleLogging') === true;
  return !options.disabled && consoleLoggingEnabled;
}


export function createLogger(namespace: string, options: LoggerOptions = {}) {
  return createDynamicLogger(namespace, options);
}


export function createDynamicLogger(namespace: string, options: LoggerOptions = {}) {
  const effectiveLevel = getEffectiveLogLevel(options);
  const enabled = shouldEnableLogging(options);

  
  const logger = createBaseLogger(namespace, {
    ...options,
    level: effectiveLevel,
    disabled: !enabled
  });

  const updateLevel = (newLevel: LogLevel, newEnabled: boolean) => {
    
    logger.updateLevel(newLevel);
    logger.setEnabled(newEnabled);
  };

  
  const loggerInfo = {
    logger,
    namespace,
    options,
    updateLevel
  };
  createdLoggers.add(loggerInfo);

  
  const dynamicLogger = {
    ...logger, 

    
    updateSettings: () => {
      const newLevel = getEffectiveLogLevel(options);
      const newEnabled = shouldEnableLogging(options);
      updateLevel(newLevel, newEnabled);
    },

    
    destroy: () => {
      createdLoggers.delete(loggerInfo);
    }
  };

  return dynamicLogger;
}


export function createAgentLogger(namespace: string, options: LoggerOptions = {}) {
  return createDynamicAgentLogger(namespace, options);
}


export function createDynamicAgentLogger(namespace: string, options: LoggerOptions = {}) {
  const effectiveLevel = getEffectiveLogLevel(options);
  const enabled = shouldEnableLogging(options);

  
  const agentLogger = createBaseAgentLogger(namespace, {
    ...options,
    level: effectiveLevel,
    disabled: !enabled
  });

  const updateLevel = (newLevel: LogLevel, newEnabled: boolean) => {
    
    agentLogger.updateLevel(newLevel);
    agentLogger.setEnabled(newEnabled);
  };

  
  const loggerInfo = {
    logger: agentLogger,
    namespace,
    options,
    updateLevel
  };
  createdLoggers.add(loggerInfo);

  
  const dynamicAgentLogger = {
    ...agentLogger, 

    
    updateSettings: () => {
      const newLevel = getEffectiveLogLevel(options);
      const newEnabled = shouldEnableLogging(options);
      updateLevel(newLevel, newEnabled);
    },

    
    destroy: () => {
      createdLoggers.delete(loggerInfo);
    }
  };

  return dynamicAgentLogger;
}


export function updateAllLoggers() {
  createdLoggers.forEach(({ updateLevel, options }) => {
    const newLevel = getEffectiveLogLevel(options);
    const newEnabled = shouldEnableLogging(options);
    updateLevel(newLevel, newEnabled);
  });
}


function setupAutoUpdate() {
  
  clientDebugState.subscribe('consoleLogging', () => {
    updateAllLoggers();
  });

  
  clientDebugState.subscribe('logLevel', () => {
    updateAllLoggers();
  });
}

// Initialize auto-update when this module is loaded
if (typeof window !== 'undefined') {
  setupAutoUpdate();
}

// Export the current effective log level for reference
export { DEFAULT_LOG_LEVEL };

// Dynamic telemetry logger for system-wide telemetry that respects Control Center settings
export const dynamicAgentTelemetryLogger = createDynamicAgentLogger('agent-telemetry');

// Default logger instance for backward compatibility
export const logger = createDynamicLogger('app');