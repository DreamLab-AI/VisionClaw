import { createAgentLogger } from '../utils/loggerConfig';
import { AgentTelemetryData, WebSocketTelemetryData, ThreeJSTelemetryData } from '../utils/loggerConfig';
import { unifiedApiClient } from '../services/api/UnifiedApiClient';

export interface TelemetryMetrics {
  agentSpawns: number;
  webSocketMessages: number;
  threeJSOperations: number;
  renderCycles: number;
  averageFrameTime: number;
  memoryUsage?: number;
  errorCount: number;
}

export interface TelemetryUploadPayload {
  sessionId: string;
  timestamp: Date;
  metrics: TelemetryMetrics;
  agentTelemetry: AgentTelemetryData[];
  webSocketTelemetry: WebSocketTelemetryData[];
  threeJSTelemetry: ThreeJSTelemetryData[];
  systemInfo: {
    userAgent: string;
    viewport: { width: number; height: number };
    pixelRatio: number;
    webglRenderer?: string;
  };
}


export class AgentTelemetryService {
  private static instance: AgentTelemetryService;
  private logger = createAgentLogger('AgentTelemetryService');
  private sessionId: string;
  private metrics: TelemetryMetrics;
  private uploadInterval: NodeJS.Timeout | null = null;
  private memoryInterval: NodeJS.Timeout | null = null;
  private frameTimeBuffer = new Float64Array(60);
  private frameTimeIndex = 0;
  private frameTimeCount = 0;
  private frameTimeSum = 0;
  private lastFrameTime = 0;

  // Stored handler references for cleanup
  private errorHandler = () => { this.metrics.errorCount++; };
  private rejectionHandler = () => { this.metrics.errorCount++; };

  private enabled = false;

  private constructor() {
    this.sessionId = this.generateSessionId();
    this.metrics = this.initializeMetrics();
  }

  enable() {
    if (this.enabled) return;
    this.enabled = true;
    this.setupPerformanceObserver();
    this.startAutoUpload();
  }

  static getInstance(): AgentTelemetryService {
    if (!AgentTelemetryService.instance) {
      AgentTelemetryService.instance = new AgentTelemetryService();
    }
    return AgentTelemetryService.instance;
  }

  private generateSessionId(): string {
    return `session_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  private initializeMetrics(): TelemetryMetrics {
    return {
      agentSpawns: 0,
      webSocketMessages: 0,
      threeJSOperations: 0,
      renderCycles: 0,
      averageFrameTime: 0,
      errorCount: 0
    };
  }

  private setupPerformanceObserver() {

    /** Chrome-specific performance.memory API */
    interface PerformanceMemory { usedJSHeapSize: number; totalJSHeapSize: number; jsHeapSizeLimit: number; }
    const perfWithMemory = performance as Performance & { memory?: PerformanceMemory };
    if (perfWithMemory.memory) {
      const updateMemory = () => {
        this.metrics.memoryUsage = perfWithMemory.memory?.usedJSHeapSize;
      };
      this.memoryInterval = setInterval(updateMemory, 5000);
    }


    window.addEventListener('error', this.errorHandler);
    window.addEventListener('unhandledrejection', this.rejectionHandler);
  }

  private startAutoUpload() {
    
    
    
    this.uploadInterval = setInterval(() => {
      this.fetchAgentTelemetry().catch(error => {
        this.logger.error('Failed to fetch agent telemetry:', error);
      });
    }, 30000); 
  }

  
  logAgentSpawn(agentId: string, agentType: string, metadata?: Record<string, any>) {
    this.metrics.agentSpawns++;
    this.logger.logAgentAction(agentId, agentType, 'spawn', metadata);

    this.logger.debug('Agent Spawned', {
      agentType,
      agentId,
      metadata,
      totalSpawned: this.metrics.agentSpawns
    });
  }

  logAgentAction(agentId: string, agentType: string, action: string, metadata?: Record<string, any>, position?: { x: number; y: number; z: number }) {
    this.logger.logAgentAction(agentId, agentType, action, metadata, position);
  }

  logWebSocketMessage(messageType: string, direction: 'incoming' | 'outgoing', data?: any, size?: number) {
    this.metrics.webSocketMessages++;

    const metadata = {
      hasData: !!data,
      dataKeys: data && typeof data === 'object' ? Object.keys(data) : []
    };

    this.logger.logWebSocketMessage(messageType, direction, metadata, size);

    this.logger.debug('WebSocket Message', {
      messageType,
      direction,
      size: size ? `${size} bytes` : 'unknown',
      data
    });
  }

  logThreeJSOperation(action: ThreeJSTelemetryData['action'], objectId: string, position?: { x: number; y: number; z: number }, rotation?: { x: number; y: number; z: number }, metadata?: Record<string, any>) {
    this.metrics.threeJSOperations++;
    this.logger.logThreeJSAction(action, objectId, position, rotation, metadata);
  }

  logRenderCycle(frameTime: number) {
    this.metrics.renderCycles++;

    // O(1) circular buffer update
    if (this.frameTimeCount >= 60) {
      this.frameTimeSum -= this.frameTimeBuffer[this.frameTimeIndex];
    }
    this.frameTimeBuffer[this.frameTimeIndex] = frameTime;
    this.frameTimeSum += frameTime;
    this.frameTimeIndex = (this.frameTimeIndex + 1) % 60;
    if (this.frameTimeCount < 60) {
      this.frameTimeCount++;
    }

    this.metrics.averageFrameTime = this.frameTimeSum / this.frameTimeCount;

    if (frameTime > 50) {
      this.logger.warn(`PERFORMANCE: Slow frame detected - ${frameTime.toFixed(2)}ms`);
    }

    this.logger.logPerformance('render_cycle', frameTime);
  }

  logUserInteraction(interactionType: string, target: string, metadata?: Record<string, any>) {
    this.logger.debug('User Interaction', {
      interactionType,
      target,
      metadata
    });

    this.logger.logAgentAction('user', 'interaction', interactionType, { target, ...metadata });
  }

  private getRecentFrameTimes(count: number): number[] {
    const n = Math.min(count, this.frameTimeCount);
    const result: number[] = new Array(n);
    for (let i = 0; i < n; i++) {
      // Walk backwards from the most recent entry
      const idx = (this.frameTimeIndex - 1 - i + 60) % 60;
      result[n - 1 - i] = this.frameTimeBuffer[idx];
    }
    return result;
  }


  getDebugOverlayData() {
    return {
      sessionId: this.sessionId,
      metrics: { ...this.metrics },
      recentFrameTimes: this.getRecentFrameTimes(10),
      agentTelemetry: this.logger.getAgentTelemetry().slice(-10),
      webSocketTelemetry: this.logger.getWebSocketTelemetry().slice(-10),
      threeJSTelemetry: this.logger.getThreeJSTelemetry().slice(-10)
    };
  }

  
  
  
  async fetchAgentTelemetry(): Promise<any> {
    try {
      
      const [statusResponse, dataResponse] = await Promise.all([
        unifiedApiClient.get('/bots/status'),
        unifiedApiClient.get('/bots/data')
      ]);

      
      const telemetryData = statusResponse.data;
      const agentData = dataResponse.data;

        
        const mergedData = {
          ...telemetryData,
          agents: agentData.agents || telemetryData.agents || []
        };

        this.logger.debug(`Fetched telemetry for ${mergedData.agents?.length || 0} agents`);

        
        if (mergedData.agents) {
          this.processAgentTelemetry(mergedData.agents);
          this.cacheAgentTelemetry(mergedData);
        }

        return mergedData;
    } catch (error) {
      this.logger.error('Failed to fetch agent telemetry:', error);
      
      return this.getCachedTelemetry();
    }
  }

  
  private cacheAgentTelemetry(data: any) {
    try {
      const cacheKey = `agent-telemetry-cache-${this.sessionId}`;
      localStorage.setItem(cacheKey, JSON.stringify({
        timestamp: Date.now(),
        data: data
      }));
    } catch (e) {
      
    }
  }


  private processAgentTelemetry(agents: any[]) {
    agents.forEach(agent => {

      this.logger.logAgentAction(
        agent.id,
        agent.type,
        'telemetry-update',
        {
          status: agent.status,
          cpuUsage: agent.cpuUsage,
          memoryUsage: agent.memoryUsage,
          health: agent.health,
          workload: agent.workload
        }
      );
    });
  }

  
  private getCachedTelemetry(): any {
    try {
      const cacheKey = `agent-telemetry-cache-${this.sessionId}`;
      const cached = localStorage.getItem(cacheKey);
      return cached ? JSON.parse(cached) : null;
    } catch (e) {
      return null;
    }
  }

  private getWebGLRenderer(): string | undefined {
    try {
      const canvas = document.createElement('canvas');
      const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
      if (!gl) return undefined;

      const webglContext = gl as WebGLRenderingContext;
      const debugInfo = webglContext.getExtension('WEBGL_debug_renderer_info');
      let renderer: string | undefined;
      if (debugInfo) {
        renderer = webglContext.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL);
      }

      // Release the WebGL context and detached canvas to prevent GPU leak
      const loseCtx = webglContext.getExtension('WEBGL_lose_context');
      loseCtx?.loseContext();
      canvas.remove();

      return renderer;
    } catch (e) {
      return undefined;
    }
  }

  private storeOfflineTelemetry() {
    try {
      const offlineKey = `offline-telemetry-${this.sessionId}`;
      const data = {
        metrics: this.metrics,
        agentTelemetry: this.logger.getAgentTelemetry(),
        webSocketTelemetry: this.logger.getWebSocketTelemetry(),
        threeJSTelemetry: this.logger.getThreeJSTelemetry(),
        timestamp: new Date().toISOString()
      };
      localStorage.setItem(offlineKey, JSON.stringify(data));
    } catch (e) {
      this.logger.warn('Failed to store offline telemetry:', e);
    }
  }

  
  destroy() {
    if (this.uploadInterval) {
      clearInterval(this.uploadInterval);
      this.uploadInterval = null;
    }
    if (this.memoryInterval) {
      clearInterval(this.memoryInterval);
      this.memoryInterval = null;
    }
    window.removeEventListener('error', this.errorHandler);
    window.removeEventListener('unhandledrejection', this.rejectionHandler);
    this.enabled = false;
  }
}

// Export singleton instance
export const agentTelemetry = AgentTelemetryService.getInstance();