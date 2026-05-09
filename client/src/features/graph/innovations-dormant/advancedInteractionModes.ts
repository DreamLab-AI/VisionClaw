/**
 * @deprecated DORMANT SERVICE -- registered in InnovationManager but never
 * imported or called by any UI component, hook, or other module outside of
 * InnovationManager.initialize(). 862 lines of unused code (time-travel,
 * collaborative editing, VR/AR spatial UI, guided exploration). Consider
 * removing in the next dead-code cleanup pass.  Audited 2026-05-09.
 */

import { Vector3, Color, Camera, Raycaster, Object3D } from 'three';
import { createLogger } from '../../../utils/loggerConfig';
import type { GraphData } from '../managers/graphDataManager';

const logger = createLogger('AdvancedInteractionModes');

export interface TimeTravelState {
  isActive: boolean;
  currentStep: number;
  totalSteps: number;
  graphStates: GraphData[];
  animationSpeed: number;
  isPlaying: boolean;
  playbackDirection: 'forward' | 'backward';
  onStateChange?: (step: number, graphData: GraphData) => void;
}

export interface ExplorationState {
  isActive: boolean;
  currentTour: string | null;
  currentWaypoint: number;
  isGuidedMode: boolean;
  highlightedElements: Set<string>;
  narrativeText: string;
  autoAdvance: boolean;
  onWaypointReached?: (waypoint: ExplorationWaypoint) => void;
}

export interface ExplorationWaypoint {
  id: string;
  position: Vector3;
  target: Vector3;
  duration: number;
  title: string;
  description: string;
  highlightNodes: string[];
  highlightEdges: string[];
  interactiveElements: InteractiveElement[];
  triggers: WaypointTrigger[];
}

export interface InteractiveElement {
  type: 'info_panel' | 'mini_graph' | 'statistics' | 'comparison' | 'quiz';
  position: Vector3;
  content: any;
  isVisible: boolean;
}

export interface WaypointTrigger {
  type: 'proximity' | 'interaction' | 'time' | 'completion';
  parameters: any;
  action: 'advance' | 'highlight' | 'show_element' | 'play_animation';
}

export interface CollaborationState {
  isActive: boolean;
  sessionId: string;
  participants: CollaborationParticipant[];
  sharedCursor: Map<string, Vector3>;
  sharedSelections: Map<string, Set<string>>;
  chatMessages: ChatMessage[];
  annotations: GraphAnnotation[];
  permissions: CollaborationPermissions;
}

export interface CollaborationParticipant {
  id: string;
  name: string;
  color: Color;
  isActive: boolean;
  lastActivity: number;
  cursorPosition: Vector3;
  currentSelection: Set<string>;
  permissions: string[];
}

export interface ChatMessage {
  id: string;
  participantId: string;
  message: string;
  timestamp: number;
  type: 'text' | 'annotation' | 'highlight' | 'suggestion';
  attachedElements?: string[];
}

export interface GraphAnnotation {
  id: string;
  creatorId: string;
  position: Vector3;
  content: string;
  type: 'note' | 'question' | 'explanation' | 'warning';
  attachedNodes: string[];
  visibility: 'private' | 'shared' | 'public';
  reactions: Map<string, string>; 
}

export interface CollaborationPermissions {
  canEdit: boolean;
  canAnnotate: boolean;
  canHighlight: boolean;
  canUseVoiceChat: boolean;
  canModifyLayout: boolean;
  canCreateTours: boolean;
}

export interface VRARState {
  isActive: boolean;
  mode: 'VR' | 'AR' | 'mixed';
  handTracking: boolean;
  eyeTracking: boolean;
  hapticFeedback: boolean;
  spatialAudio: boolean;
  immersiveUI: boolean;
  roomScale: boolean;
  passthrough: boolean;
}

export interface ImmersiveInteraction {
  type: 'hand_grab' | 'eye_select' | 'voice_command' | 'gesture' | 'haptic_tap';
  targetElement: string;
  parameters: any;
  confidence: number;
  timestamp: number;
}

export interface SpatialUI {
  panels: SpatialPanel[];
  menus: SpatialMenu[];
  notifications: SpatialNotification[];
  workspace: SpatialWorkspace;
}

export interface SpatialPanel {
  id: string;
  position: Vector3;
  rotation: Vector3;
  size: { width: number; height: number };
  content: 'graph_controls' | 'information' | 'tools' | 'collaboration' | 'settings';
  isVisible: boolean;
  canMove: boolean;
  canResize: boolean;
}

export interface SpatialMenu {
  id: string;
  triggerType: 'hand' | 'eye' | 'voice';
  position: Vector3;
  items: SpatialMenuItem[];
  isVisible: boolean;
  autoHide: boolean;
}

export interface SpatialMenuItem {
  id: string;
  label: string;
  icon: string;
  action: () => void;
  isEnabled: boolean;
  submenu?: SpatialMenuItem[];
}

export interface SpatialNotification {
  id: string;
  type: 'info' | 'warning' | 'error' | 'success';
  message: string;
  position: Vector3;
  duration: number;
  isVisible: boolean;
}

export interface SpatialWorkspace {
  center: Vector3;
  bounds: { min: Vector3; max: Vector3 };
  scale: number;
  orientation: Vector3;
  snapPoints: Vector3[];
}

export class AdvancedInteractionModes {
  private static instance: AdvancedInteractionModes;

  private timeTravelState!: TimeTravelState;
  private explorationState!: ExplorationState;
  private collaborationState!: CollaborationState;
  private vrArState!: VRARState;
  private spatialUI!: SpatialUI;

  private tours: Map<string, ExplorationWaypoint[]> = new Map();
  private activeAnimations: Map<string, any> = new Map();
  private eventListeners: Map<string, Set<Function>> = new Map();

  private constructor() {
    this.initializeStates();
  }

  public static getInstance(): AdvancedInteractionModes {
    if (!AdvancedInteractionModes.instance) {
      AdvancedInteractionModes.instance = new AdvancedInteractionModes();
    }
    return AdvancedInteractionModes.instance;
  }

  private initializeStates(): void {
    this.timeTravelState = {
      isActive: false,
      currentStep: 0,
      totalSteps: 0,
      graphStates: [],
      animationSpeed: 1.0,
      isPlaying: false,
      playbackDirection: 'forward'
    };

    this.explorationState = {
      isActive: false,
      currentTour: null,
      currentWaypoint: 0,
      isGuidedMode: true,
      highlightedElements: new Set(),
      narrativeText: '',
      autoAdvance: false
    };

    this.collaborationState = {
      isActive: false,
      sessionId: '',
      participants: [],
      sharedCursor: new Map(),
      sharedSelections: new Map(),
      chatMessages: [],
      annotations: [],
      permissions: {
        canEdit: false,
        canAnnotate: true,
        canHighlight: true,
        canUseVoiceChat: false,
        canModifyLayout: false,
        canCreateTours: false
      }
    };

    this.vrArState = {
      isActive: false,
      mode: 'VR',
      handTracking: false,
      eyeTracking: false,
      hapticFeedback: false,
      spatialAudio: false,
      immersiveUI: false,
      roomScale: false,
      passthrough: false
    };

    this.spatialUI = {
      panels: [],
      menus: [],
      notifications: [],
      workspace: {
        center: new Vector3(0, 0, 0),
        bounds: { 
          min: new Vector3(-50, -50, -50), 
          max: new Vector3(50, 50, 50) 
        },
        scale: 1.0,
        orientation: new Vector3(0, 0, 0),
        snapPoints: []
      }
    };
  }

  
  public activateTimeTravelMode(
    graphStates: GraphData[],
    options: {
      animationSpeed?: number;
      startStep?: number;
      onStateChange?: (step: number, graphData: GraphData) => void;
    } = {}
  ): void {
    logger.info('Activating time-travel mode');

    this.timeTravelState = {
      isActive: true,
      currentStep: options.startStep || 0,
      totalSteps: graphStates.length,
      graphStates,
      animationSpeed: options.animationSpeed || 1.0,
      isPlaying: false,
      playbackDirection: 'forward',
      onStateChange: options.onStateChange
    };

    this.emit('timeTravelActivated', this.timeTravelState);
  }

  public playTimeTravel(): void {
    if (!this.timeTravelState.isActive) return;

    this.timeTravelState.isPlaying = true;
    this.startTimeTravelAnimation();
    this.emit('timeTravelPlay', this.timeTravelState);
  }

  public pauseTimeTravel(): void {
    this.timeTravelState.isPlaying = false;
    this.emit('timeTravelPause', this.timeTravelState);
  }

  public seekTimeTravel(step: number): void {
    if (!this.timeTravelState.isActive) return;

    this.timeTravelState.currentStep = Math.max(0, Math.min(step, this.timeTravelState.totalSteps - 1));
    const currentGraph = this.timeTravelState.graphStates[this.timeTravelState.currentStep];
    
    this.timeTravelState.onStateChange?.(this.timeTravelState.currentStep, currentGraph);
    this.emit('timeTravelSeek', this.timeTravelState);
  }

  private startTimeTravelAnimation(): void {
    if (!this.timeTravelState.isPlaying) return;

    const stepDuration = 1000 / this.timeTravelState.animationSpeed;
    
    const animate = () => {
      if (!this.timeTravelState.isPlaying) return;

      const direction = this.timeTravelState.playbackDirection === 'forward' ? 1 : -1;
      const nextStep = this.timeTravelState.currentStep + direction;

      if (nextStep >= 0 && nextStep < this.timeTravelState.totalSteps) {
        this.seekTimeTravel(nextStep);
        setTimeout(animate, stepDuration);
      } else {
        this.pauseTimeTravel();
      }
    };

    setTimeout(animate, stepDuration);
  }

  
  public createExplorationTour(
    tourId: string,
    waypoints: ExplorationWaypoint[]
  ): void {
    logger.info(`Creating exploration tour: ${tourId}`);
    this.tours.set(tourId, waypoints);
    this.emit('tourCreated', { tourId, waypoints });
  }

  public startExplorationTour(
    tourId: string,
    options: {
      autoAdvance?: boolean;
      onWaypointReached?: (waypoint: ExplorationWaypoint) => void;
    } = {}
  ): void {
    const tour = this.tours.get(tourId);
    if (!tour) {
      logger.error(`Tour not found: ${tourId}`);
      return;
    }

    logger.info(`Starting exploration tour: ${tourId}`);

    this.explorationState = {
      isActive: true,
      currentTour: tourId,
      currentWaypoint: 0,
      isGuidedMode: true,
      highlightedElements: new Set(),
      narrativeText: tour[0]?.description || '',
      autoAdvance: options.autoAdvance || false,
      onWaypointReached: options.onWaypointReached
    };

    this.moveToWaypoint(0);
    this.emit('explorationStarted', this.explorationState);
  }

  public nextWaypoint(): void {
    if (!this.explorationState.isActive || !this.explorationState.currentTour) return;

    const tour = this.tours.get(this.explorationState.currentTour)!;
    const nextIndex = this.explorationState.currentWaypoint + 1;

    if (nextIndex < tour.length) {
      this.moveToWaypoint(nextIndex);
    } else {
      this.finishExploration();
    }
  }

  public previousWaypoint(): void {
    if (!this.explorationState.isActive) return;

    const prevIndex = this.explorationState.currentWaypoint - 1;
    if (prevIndex >= 0) {
      this.moveToWaypoint(prevIndex);
    }
  }

  private moveToWaypoint(index: number): void {
    if (!this.explorationState.currentTour) return;

    const tour = this.tours.get(this.explorationState.currentTour)!;
    const waypoint = tour[index];

    this.explorationState.currentWaypoint = index;
    this.explorationState.narrativeText = waypoint.description;
    
    
    this.explorationState.highlightedElements.clear();
    
    
    waypoint.highlightNodes.forEach(nodeId => {
      this.explorationState.highlightedElements.add(nodeId);
    });
    waypoint.highlightEdges.forEach(edgeId => {
      this.explorationState.highlightedElements.add(edgeId);
    });

    
    this.executeWaypointTriggers(waypoint);

    this.explorationState.onWaypointReached?.(waypoint);
    this.emit('waypointReached', { waypoint, index });
  }

  private executeWaypointTriggers(waypoint: ExplorationWaypoint): void {
    waypoint.triggers.forEach(trigger => {
      switch (trigger.action) {
        case 'advance':
          if (this.explorationState.autoAdvance) {
            setTimeout(() => this.nextWaypoint(), trigger.parameters.delay || 3000);
          }
          break;
        case 'highlight':
          
          break;
        case 'show_element':
          
          waypoint.interactiveElements.forEach(element => {
            element.isVisible = true;
          });
          break;
        case 'play_animation':
          
          break;
      }
    });
  }

  private finishExploration(): void {
    this.explorationState.isActive = false;
    this.explorationState.highlightedElements.clear();
    this.emit('explorationFinished', this.explorationState);
  }

  
  public startCollaborationSession(
    sessionId: string,
    permissions: Partial<CollaborationPermissions> = {}
  ): void {
    logger.info(`Starting collaboration session: ${sessionId}`);

    this.collaborationState = {
      isActive: true,
      sessionId,
      participants: [],
      sharedCursor: new Map(),
      sharedSelections: new Map(),
      chatMessages: [],
      annotations: [],
      permissions: {
        ...this.collaborationState.permissions,
        ...permissions
      }
    };

    this.emit('collaborationStarted', this.collaborationState);
  }

  public addParticipant(participant: Omit<CollaborationParticipant, 'lastActivity'>): void {
    const fullParticipant: CollaborationParticipant = {
      ...participant,
      lastActivity: Date.now()
    };

    this.collaborationState.participants.push(fullParticipant);
    this.emit('participantJoined', fullParticipant);
  }

  public updateParticipantCursor(participantId: string, position: Vector3): void {
    this.collaborationState.sharedCursor.set(participantId, position);
    
    const participant = this.collaborationState.participants.find(p => p.id === participantId);
    if (participant) {
      participant.cursorPosition = position;
      participant.lastActivity = Date.now();
    }

    this.emit('cursorUpdated', { participantId, position });
  }

  public updateParticipantSelection(participantId: string, selection: Set<string>): void {
    this.collaborationState.sharedSelections.set(participantId, selection);
    
    const participant = this.collaborationState.participants.find(p => p.id === participantId);
    if (participant) {
      participant.currentSelection = selection;
      participant.lastActivity = Date.now();
    }

    this.emit('selectionUpdated', { participantId, selection });
  }

  public sendChatMessage(participantId: string, message: string, type: ChatMessage['type'] = 'text'): void {
    const chatMessage: ChatMessage = {
      id: `msg-${Date.now()}-${Math.random()}`,
      participantId,
      message,
      timestamp: Date.now(),
      type
    };

    this.collaborationState.chatMessages.push(chatMessage);
    this.emit('chatMessage', chatMessage);
  }

  public createAnnotation(
    creatorId: string,
    position: Vector3,
    content: string,
    type: GraphAnnotation['type'],
    attachedNodes: string[] = []
  ): string {
    const annotation: GraphAnnotation = {
      id: `annotation-${Date.now()}-${Math.random()}`,
      creatorId,
      position,
      content,
      type,
      attachedNodes,
      visibility: 'shared',
      reactions: new Map()
    };

    this.collaborationState.annotations.push(annotation);
    this.emit('annotationCreated', annotation);
    
    return annotation.id;
  }

  
  public activateVRMode(options: Partial<VRARState> = {}): void {
    logger.info('Activating VR mode');

    this.vrArState = {
      ...this.vrArState,
      isActive: true,
      mode: 'VR',
      ...options
    };

    this.initializeSpatialUI();
    this.setupVRControls();
    this.emit('vrActivated', this.vrArState);
  }

  public activateARMode(options: Partial<VRARState> = {}): void {
    logger.info('Activating AR mode');

    this.vrArState = {
      ...this.vrArState,
      isActive: true,
      mode: 'AR',
      passthrough: true,
      ...options
    };

    this.initializeSpatialUI();
    this.setupARControls();
    this.emit('arActivated', this.vrArState);
  }

  private initializeSpatialUI(): void {
    
    this.spatialUI.panels = [
      {
        id: 'main-controls',
        position: new Vector3(-2, 1, -1),
        rotation: new Vector3(0, 0.3, 0),
        size: { width: 1.5, height: 1 },
        content: 'graph_controls',
        isVisible: true,
        canMove: true,
        canResize: false
      },
      {
        id: 'information',
        position: new Vector3(2, 1, -1),
        rotation: new Vector3(0, -0.3, 0),
        size: { width: 1.2, height: 0.8 },
        content: 'information',
        isVisible: true,
        canMove: true,
        canResize: false
      }
    ];

    
    this.spatialUI.workspace = {
      center: new Vector3(0, 0, 0),
      bounds: { 
        min: new Vector3(-20, -10, -20), 
        max: new Vector3(20, 10, 20) 
      },
      scale: 1.0,
      orientation: new Vector3(0, 0, 0),
      snapPoints: [
        new Vector3(0, 0, 0),
        new Vector3(-5, 0, 0),
        new Vector3(5, 0, 0),
        new Vector3(0, 0, -5),
        new Vector3(0, 0, 5)
      ]
    };
  }

  private setupVRControls(): void {
    if (this.vrArState.handTracking) {
      logger.info('Hand tracking enabled');
    }
    if (this.vrArState.eyeTracking) {
      logger.info('Eye tracking enabled');
    }
    if (this.vrArState.hapticFeedback) {
      logger.info('Haptic feedback enabled');
    }
  }

  private setupARControls(): void {
    logger.info('World tracking setup');
    logger.info('Occlusion setup');
    logger.info('Light estimation setup');
  }

  public processImmersiveInteraction(interaction: ImmersiveInteraction): void {
    logger.info(`Processing immersive interaction: ${interaction.type}`);

    switch (interaction.type) {
      case 'hand_grab':
        this.handleHandGrab(interaction);
        break;
      case 'eye_select':
        this.handleEyeSelect(interaction);
        break;
      case 'voice_command':
        this.handleVoiceCommand(interaction);
        break;
      case 'gesture':
        this.handleGesture(interaction);
        break;
      case 'haptic_tap':
        this.handleHapticTap(interaction);
        break;
    }

    this.emit('immersiveInteraction', interaction);
  }

  private handleHandGrab(interaction: ImmersiveInteraction): void {
    
    const target = interaction.targetElement;
    
    
    if (target.startsWith('node-')) {
      this.selectNode(target.replace('node-', ''));
    } else if (target.startsWith('panel-')) {
      this.movePanel(target.replace('panel-', ''), interaction.parameters.position);
    }
  }

  private handleEyeSelect(interaction: ImmersiveInteraction): void {
    
    if (interaction.confidence > 0.8) {
      this.highlightElement(interaction.targetElement);
    }
  }

  private handleVoiceCommand(interaction: ImmersiveInteraction): void {
    
    const command = interaction.parameters.command;
    
    switch (command) {
      case 'show information':
        this.showPanel('information');
        break;
      case 'hide controls':
        this.hidePanel('main-controls');
        break;
      case 'start tour':
        this.startExplorationTour('default');
        break;
      case 'reset view':
        this.resetWorkspaceView();
        break;
    }
  }

  private handleGesture(interaction: ImmersiveInteraction): void {
    
    const gesture = interaction.parameters.gesture;
    
    switch (gesture) {
      case 'pinch':
        this.scaleWorkspace(interaction.parameters.scale);
        break;
      case 'swipe_left':
        this.nextWaypoint();
        break;
      case 'swipe_right':
        this.previousWaypoint();
        break;
      case 'point':
        this.highlightElement(interaction.targetElement);
        break;
    }
  }

  private handleHapticTap(interaction: ImmersiveInteraction): void {
    
    this.selectElement(interaction.targetElement);
    this.provideHapticFeedback('tap');
  }

  

  private selectNode(nodeId: string): void {
    this.emit('nodeSelected', { nodeId });
  }

  private movePanel(panelId: string, position: Vector3): void {
    const panel = this.spatialUI.panels.find(p => p.id === panelId);
    if (panel && panel.canMove) {
      panel.position = position;
      this.emit('panelMoved', { panelId, position });
    }
  }

  private highlightElement(elementId: string): void {
    this.emit('elementHighlighted', { elementId });
  }

  private showPanel(panelId: string): void {
    const panel = this.spatialUI.panels.find(p => p.id === panelId);
    if (panel) {
      panel.isVisible = true;
      this.emit('panelShown', { panelId });
    }
  }

  private hidePanel(panelId: string): void {
    const panel = this.spatialUI.panels.find(p => p.id === panelId);
    if (panel) {
      panel.isVisible = false;
      this.emit('panelHidden', { panelId });
    }
  }

  private resetWorkspaceView(): void {
    this.spatialUI.workspace.scale = 1.0;
    this.spatialUI.workspace.orientation = new Vector3(0, 0, 0);
    this.emit('workspaceReset');
  }

  private scaleWorkspace(scale: number): void {
    this.spatialUI.workspace.scale *= scale;
    this.emit('workspaceScaled', { scale: this.spatialUI.workspace.scale });
  }

  private selectElement(elementId: string): void {
    this.emit('elementSelected', { elementId });
  }

  private provideHapticFeedback(type: 'tap' | 'pulse' | 'vibrate'): void {
    
    this.emit('hapticFeedback', { type });
  }

  

  private emit(event: string, data?: any): void {
    const listeners = this.eventListeners.get(event);
    if (listeners) {
      listeners.forEach(listener => {
        try {
          listener(data);
        } catch (error) {
          logger.error(`Error in event listener for ${event}:`, error);
        }
      });
    }
  }

  public on(event: string, listener: Function): () => void {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, new Set());
    }
    
    this.eventListeners.get(event)!.add(listener);
    
    
    return () => {
      const listeners = this.eventListeners.get(event);
      if (listeners) {
        listeners.delete(listener);
      }
    };
  }

  

  public getTimeTravelState(): TimeTravelState {
    return { ...this.timeTravelState };
  }

  public getExplorationState(): ExplorationState {
    return { ...this.explorationState };
  }

  public getCollaborationState(): CollaborationState {
    return { ...this.collaborationState };
  }

  public getVRARState(): VRARState {
    return { ...this.vrArState };
  }

  public getSpatialUI(): SpatialUI {
    return { ...this.spatialUI };
  }

  
  public dispose(): void {
    this.timeTravelState.isPlaying = false;
    this.explorationState.isActive = false;
    this.collaborationState.isActive = false;
    this.vrArState.isActive = false;
    
    this.tours.clear();
    this.activeAnimations.clear();
    this.eventListeners.clear();
    
    logger.info('Advanced interaction modes disposed');
  }
}

// Export singleton instance
export const advancedInteractionModes = AdvancedInteractionModes.getInstance();