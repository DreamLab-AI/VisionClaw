
import { createLogger } from '../../../utils/loggerConfig';
import { unifiedApiClient } from '../../../services/api/UnifiedApiClient';
import type { BotsAgent, BotsEdge } from '../types/BotsTypes';
import { PollingPerformanceMonitor } from '../utils/pollingPerformance';

const logger = createLogger('AgentPollingService');

export interface AgentSwarmData {
  nodes: Array<{
    id: number;
    metadataId: string; 
    label: string;
    type?: string; 
    data?: {
      nodeId?: number;
      x?: number;
      y?: number;
      z?: number;
      vx?: number;
      vy?: number;
      vz?: number;
      position?: { x: number; y: number; z: number };
      velocity?: { x: number; y: number; z: number };
    };
    metadata?: {
      agent_type?: string;
      status?: string;
      health?: string;
      cpu_usage?: string;
      memory_usage?: string;
      workload?: string;
      tokens?: string;
      created_at?: string;
      age?: string;
      swarm_id?: string;
      parent_queen_id?: string;
      capabilities?: string;
    };
  }>;
  edges: Array<{
    id: string;
    source: number;
    target: number;
    weight: number;
  }>;
  metadata?: {
    total_agents: number;
    active_agents: number;
    total_tasks: number;
    completed_tasks: number;
    avg_success_rate: number;
    total_tokens: number;
  };
}

export interface PollingConfig {
  activePollingInterval: number; 
  idlePollingInterval: number;   
  enableSmartPolling: boolean;   
  maxRetries: number;
  retryDelay: number;
}

type PollingCallback = (data: AgentSwarmData) => void;
type ErrorCallback = (error: Error) => void;

export class AgentPollingService {
  private static instance: AgentPollingService;
  private pollingTimer: ReturnType<typeof setTimeout> | null = null;
  private startDelayTimer: ReturnType<typeof setTimeout> | null = null;
  private config: PollingConfig;
  private callbacks: Set<PollingCallback> = new Set();
  private errorCallbacks: Set<ErrorCallback> = new Set();
  private isPolling: boolean = false;
  private lastPollTime: number = 0;
  private lastDataHash: string = '';
  private retryCount: number = 0;
  private currentInterval: number;
  private activityLevel: 'active' | 'idle' = 'idle';
  private lastActivityCheck: number = 0;
  private performanceMonitor: PollingPerformanceMonitor;
  private subscriberCount: number = 0;

  private constructor() {
    this.config = {
      activePollingInterval: 2000,  
      idlePollingInterval: 10000,   
      enableSmartPolling: true,
      maxRetries: 3,
      retryDelay: 2000
    };
    this.currentInterval = this.config.idlePollingInterval;
    this.performanceMonitor = new PollingPerformanceMonitor();
  }

  public static getInstance(): AgentPollingService {
    if (!AgentPollingService.instance) {
      AgentPollingService.instance = new AgentPollingService();
    }
    return AgentPollingService.instance;
  }

  
  public configure(config: Partial<PollingConfig>): void {
    this.config = { ...this.config, ...config };
    logger.debug('Polling configuration updated:', this.config);
    
    
    if (this.isPolling) {
      this.stop();
      this.start();
    }
  }

  public start(): void {
    this.subscriberCount++;
    if (this.subscriberCount === 1) {
      if (this.isPolling) {
        logger.warn('Polling already active');
        return;
      }
      logger.debug('Starting agent swarm polling');
      this.isPolling = true;
      this.startDelayTimer = setTimeout(() => {
        this.startDelayTimer = null;
        if (this.isPolling) this.poll();
      }, 5000);
    } else {
      logger.debug(`Polling subscriber added (count: ${this.subscriberCount})`);
    }
  }

  public stop(): void {
    this.subscriberCount = Math.max(0, this.subscriberCount - 1);
    if (this.subscriberCount === 0) {
      if (this.startDelayTimer) {
        clearTimeout(this.startDelayTimer);
        this.startDelayTimer = null;
      }
      if (this.pollingTimer) {
        clearTimeout(this.pollingTimer);
        this.pollingTimer = null;
      }
      this.isPolling = false;
      logger.debug('Agent swarm polling stopped');
    } else {
      logger.debug(`Polling subscriber removed (count: ${this.subscriberCount})`);
    }
  }

  
  public subscribe(callback: PollingCallback, errorCallback?: ErrorCallback): () => void {
    this.callbacks.add(callback);
    if (errorCallback) {
      this.errorCallbacks.add(errorCallback);
    }

    
    return () => {
      this.callbacks.delete(callback);
      if (errorCallback) {
        this.errorCallbacks.delete(errorCallback);
      }
    };
  }

  
  public async pollNow(): Promise<void> {
    if (!this.isPolling) {
      logger.warn('Cannot poll now - polling is not active');
      return;
    }
    
    
    if (this.pollingTimer) {
      clearTimeout(this.pollingTimer);
      this.pollingTimer = null;
    }
    
    await this.poll();
  }

  
  public getPerformanceMetrics() {
    return this.performanceMonitor.getMetrics();
  }

  
  public resetPerformanceMetrics(): void {
    this.performanceMonitor.reset();
  }

  
  public getStatus() {
    return {
      isPolling: this.isPolling,
      currentInterval: this.currentInterval,
      activityLevel: this.activityLevel,
      lastPollTime: this.lastPollTime,
      retryCount: this.retryCount,
      subscriberCount: this.callbacks.size,
      performance: this.performanceMonitor.getSummary()
    };
  }

  
  private async poll(): Promise<void> {
    if (!this.isPolling) return;

    try {
      const startTime = Date.now();
      
      
      const data = await unifiedApiClient.getData<AgentSwarmData>('/graph/data');
      
      const pollDuration = Date.now() - startTime;
      this.lastPollTime = Date.now();
      this.retryCount = 0; 

      
      const dataHash = this.hashData(data);
      const hasChanged = dataHash !== this.lastDataHash;
      this.lastDataHash = dataHash;
      
      
      this.performanceMonitor.recordPoll(pollDuration, hasChanged);

      
      if (this.config.enableSmartPolling) {
        this.updateActivityLevel(data, hasChanged);
      }

      
      if (hasChanged || Date.now() - this.lastActivityCheck > 10000) {
        logger.debug('Poll completed', {
          duration: pollDuration,
          hasChanged,
          activityLevel: this.activityLevel,
          nodeCount: data.nodes?.length || 0,
          activeAgents: data.metadata?.active_agents || 0
        });
        this.lastActivityCheck = Date.now();
      }

      
      if (hasChanged) {
        this.callbacks.forEach(callback => {
          try {
            callback(data);
          } catch (error) {
            logger.error('Error in polling callback:', error);
          }
        });
      }

    } catch (error) {
      this.handlePollingError(error as Error);
    }

    
    if (this.isPolling) {
      this.pollingTimer = setTimeout(() => this.poll(), this.currentInterval);
    }
  }

  
  private updateActivityLevel(data: AgentSwarmData, hasChanged: boolean): void {
    const activeAgents = data.metadata?.active_agents || 0;
    const totalAgents = data.metadata?.total_agents || 0;
    const activeRatio = totalAgents > 0 ? activeAgents / totalAgents : 0;

    
    const wasActive = this.activityLevel === 'active';
    
    
    
    
    
    if (activeRatio > 0.2 || hasChanged || (data.metadata?.total_tasks || 0) > (data.metadata?.completed_tasks || 0)) {
      this.activityLevel = 'active';
      this.currentInterval = this.config.activePollingInterval;
    } else {
      this.activityLevel = 'idle';
      this.currentInterval = this.config.idlePollingInterval;
    }

    
    if (wasActive !== (this.activityLevel === 'active')) {
      logger.info(`Activity level changed to ${this.activityLevel}, interval: ${this.currentInterval}ms`);
    }
  }

  
  private handlePollingError(error: Error): void {
    this.retryCount++;
    logger.error('Polling error:', error, { retryCount: this.retryCount });
    
    
    this.performanceMonitor.recordError();

    
    this.errorCallbacks.forEach(callback => {
      try {
        callback(error);
      } catch (err) {
        logger.error('Error in error callback:', err);
      }
    });

    if (this.retryCount >= this.config.maxRetries) {
      logger.error('Max retries reached, stopping polling');
      this.stop();
      return;
    }

    
    const retryDelay = this.config.retryDelay * Math.pow(2, this.retryCount - 1);
    logger.info(`Retrying poll in ${retryDelay}ms`);
    
    if (this.isPolling) {
      this.pollingTimer = setTimeout(() => this.poll(), retryDelay);
    }
  }

  
  private hashData(data: AgentSwarmData): string {
    const relevant = {
      nodeCount: data.nodes?.length || 0,
      edgeCount: data.edges?.length || 0,
      activeAgents: data.metadata?.active_agents || 0,
      totalTasks: data.metadata?.total_tasks || 0,
      completedTasks: data.metadata?.completed_tasks || 0,
      // Structural identity only — positions change every physics tick
      // and must NOT be included or the service stays permanently "active"
      nodeIds: data.nodes?.map(n => n.id).join(',')
    };
    return JSON.stringify(relevant);
  }
}

// Export singleton instance
export const agentPollingService = AgentPollingService.getInstance();