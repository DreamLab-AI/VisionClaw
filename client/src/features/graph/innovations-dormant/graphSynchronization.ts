/**
 * @deprecated DORMANT SERVICE -- registered in InnovationManager but never
 * imported or called by any UI component, hook, or other module outside of
 * InnovationManager.initialize(). 276 lines of unused code (camera sync,
 * selection sync, cross-graph state synchronisation). Consider removing
 * in the next dead-code cleanup pass.  Audited 2026-05-09.
 */

import { Camera, Vector3, OrthographicCamera, PerspectiveCamera } from 'three';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('GraphSynchronization');

export interface SyncState {
  camera: {
    position: Vector3;
    target: Vector3;
    zoom: number;
  };
  selection: {
    selectedNodes: Set<string>;
    hoveredNode: string | null;
  };
  interaction: {
    isPanning: boolean;
    isZooming: boolean;
    lastUpdate: number;
  };
}

export interface SyncOptions {
  enableCameraSync: boolean;
  enableSelectionSync: boolean;
  enableZoomSync: boolean;
  enablePanSync: boolean;
  smoothTransitions: boolean;
  transitionDuration: number;
}

export class GraphSynchronization {
  private static instance: GraphSynchronization;
  private syncState: SyncState;
  private syncOptions: SyncOptions;
  private listeners: Map<string, Set<(state: SyncState) => void>> = new Map();
  private animationFrameId: number | null = null;

  private constructor() {
    this.syncState = {
      camera: {
        position: new Vector3(0, 0, 10),
        target: new Vector3(0, 0, 0),
        zoom: 1
      },
      selection: {
        selectedNodes: new Set(),
        hoveredNode: null
      },
      interaction: {
        isPanning: false,
        isZooming: false,
        lastUpdate: Date.now()
      }
    };

    this.syncOptions = {
      enableCameraSync: true,
      enableSelectionSync: true,
      enableZoomSync: true,
      enablePanSync: true,
      smoothTransitions: true,
      transitionDuration: 300
    };
  }

  public static getInstance(): GraphSynchronization {
    if (!GraphSynchronization.instance) {
      GraphSynchronization.instance = new GraphSynchronization();
    }
    return GraphSynchronization.instance;
  }

  
  public updateSyncOptions(options: Partial<SyncOptions>): void {
    this.syncOptions = { ...this.syncOptions, ...options };
    logger.info('Sync options updated:', this.syncOptions);
  }

  
  public getSyncOptions(): SyncOptions {
    return { ...this.syncOptions };
  }

  
  public syncCamera(graphId: string, camera: Camera, target?: Vector3): void {
    if (!this.syncOptions.enableCameraSync) return;

    const newState = {
      ...this.syncState,
      camera: {
        position: camera.position.clone(),
        target: target ? target.clone() : this.syncState.camera.target,
        zoom: this.syncOptions.enableZoomSync ? (camera as OrthographicCamera | PerspectiveCamera).zoom : this.syncState.camera.zoom
      },
      interaction: {
        ...this.syncState.interaction,
        lastUpdate: Date.now()
      }
    };

    this.updateState(newState);
    this.notifyOtherGraphs(graphId, 'camera');
  }

  
  public syncSelection(graphId: string, selectedNodes: Set<string>, hoveredNode?: string | null): void {
    if (!this.syncOptions.enableSelectionSync) return;

    const newState = {
      ...this.syncState,
      selection: {
        selectedNodes: new Set(selectedNodes),
        hoveredNode: hoveredNode !== undefined ? hoveredNode : this.syncState.selection.hoveredNode
      },
      interaction: {
        ...this.syncState.interaction,
        lastUpdate: Date.now()
      }
    };

    this.updateState(newState);
    this.notifyOtherGraphs(graphId, 'selection');
  }

  
  public syncPan(graphId: string, delta: Vector3): void {
    if (!this.syncOptions.enablePanSync) return;

    const newState = {
      ...this.syncState,
      camera: {
        ...this.syncState.camera,
        position: this.syncState.camera.position.clone().add(delta),
        target: this.syncState.camera.target.clone().add(delta)
      },
      interaction: {
        ...this.syncState.interaction,
        isPanning: true,
        lastUpdate: Date.now()
      }
    };

    this.updateState(newState);
    this.notifyOtherGraphs(graphId, 'pan');
  }

  
  public syncZoom(graphId: string, zoomFactor: number): void {
    if (!this.syncOptions.enableZoomSync) return;

    const newState = {
      ...this.syncState,
      camera: {
        ...this.syncState.camera,
        zoom: this.syncState.camera.zoom * zoomFactor
      },
      interaction: {
        ...this.syncState.interaction,
        isZooming: true,
        lastUpdate: Date.now()
      }
    };

    this.updateState(newState);
    this.notifyOtherGraphs(graphId, 'zoom');
  }

  
  public subscribe(graphId: string, callback: (state: SyncState) => void): () => void {
    if (!this.listeners.has(graphId)) {
      this.listeners.set(graphId, new Set());
    }
    
    this.listeners.get(graphId)!.add(callback);

    
    return () => {
      const graphListeners = this.listeners.get(graphId);
      if (graphListeners) {
        graphListeners.delete(callback);
        if (graphListeners.size === 0) {
          this.listeners.delete(graphId);
        }
      }
    };
  }

  
  public getState(): SyncState {
    return {
      camera: {
        position: this.syncState.camera.position.clone(),
        target: this.syncState.camera.target.clone(),
        zoom: this.syncState.camera.zoom
      },
      selection: {
        selectedNodes: new Set(this.syncState.selection.selectedNodes),
        hoveredNode: this.syncState.selection.hoveredNode
      },
      interaction: { ...this.syncState.interaction }
    };
  }

  
  private updateState(newState: SyncState): void {
    this.syncState = newState;
  }

  
  private notifyOtherGraphs(senderGraphId: string, syncType: string): void {
    this.listeners.forEach((callbacks, graphId) => {
      if (graphId !== senderGraphId) {
        callbacks.forEach(callback => {
          try {
            if (this.syncOptions.smoothTransitions) {
              this.smoothTransition(callback);
            } else {
              callback(this.getState());
            }
          } catch (error) {
            logger.error(`Error notifying graph ${graphId}:`, error);
          }
        });
      }
    });
  }

  
  private smoothTransition(callback: (state: SyncState) => void): void {
    if (this.animationFrameId !== null) {
      cancelAnimationFrame(this.animationFrameId);
    }

    const startTime = Date.now();
    const duration = this.syncOptions.transitionDuration;

    const animate = () => {
      const elapsed = Date.now() - startTime;
      const progress = Math.min(elapsed / duration, 1);
      
      
      const easeProgress = 1 - Math.pow(1 - progress, 3);
      
      callback(this.getState());

      if (progress < 1) {
        this.animationFrameId = requestAnimationFrame(animate);
      } else {
        this.animationFrameId = null;
      }
    };

    this.animationFrameId = requestAnimationFrame(animate);
  }

  
  public resetInteractionState(): void {
    this.syncState.interaction.isPanning = false;
    this.syncState.interaction.isZooming = false;
  }

  
  public dispose(): void {
    if (this.animationFrameId !== null) {
      cancelAnimationFrame(this.animationFrameId);
      this.animationFrameId = null;
    }
    this.listeners.clear();
    logger.info('Graph synchronization disposed');
  }
}

// Export singleton instance
export const graphSynchronization = GraphSynchronization.getInstance();