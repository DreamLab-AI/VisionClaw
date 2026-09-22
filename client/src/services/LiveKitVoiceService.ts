/**
 * LiveKitVoiceService — WebRTC spatial voice chat via LiveKit SFU.
 *
 * Handles Plane 3 (user-to-user voice) and Plane 4 (agent spatial voice):
 *   - Connects to LiveKit room for the current Vircadia world
 *   - Publishes user microphone as a WebRTC audio track
 *   - Subscribes to remote participants (other users + agent virtual participants)
 *   - Applies spatial audio panning based on Vircadia entity positions
 *
 * Coordinate flow:
 *   Vircadia entity positions → CollaborativeGraphSync → this service → Web Audio panner
 *
 * Audio format: Opus 48kHz mono throughout.
 */

import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('LiveKitVoiceService');

export interface LiveKitConfig {
  /** LiveKit server WebSocket URL */
  serverUrl: string;
  /** JWT access token (generated server-side with LiveKit API key/secret) */
  token: string;
  /** Room name to join */
  roomName: string;
  /** Enable spatial audio panning */
  spatialAudio: boolean;
  /** Max distance for audio rolloff (Vircadia units) */
  maxDistance: number;
}

export interface SpatialPosition {
  x: number;
  y: number;
  z: number;
}

export interface RemoteParticipant {
  id: string;
  identity: string;
  /** Whether this is an agent virtual participant */
  isAgent: boolean;
  position: SpatialPosition;
  audioElement?: HTMLAudioElement;
  pannerNode?: PannerNode;
}

/** LiveKit Room instance shape from dynamically-imported SDK */
interface LiveKitRoomInstance {
  on: (event: string, handler: (...args: unknown[]) => void) => void;
  connect: (url: string, token: string) => Promise<void>;
  disconnect: () => Promise<void>;
  localParticipant: { setMicrophoneEnabled: (enabled: boolean) => Promise<void> };
}

/**
 * Manages the LiveKit WebRTC connection for spatial voice chat.
 *
 * Designed to work alongside VoiceWebSocketService:
 *   - VoiceWS handles agent commands (Plane 1+2, private)
 *   - LiveKit handles voice chat (Plane 3+4, public/spatial)
 *
 * The PushToTalkService coordinates which input goes where.
 */
export class LiveKitVoiceService {
  private static instance: LiveKitVoiceService;
  private config: LiveKitConfig | null = null;
  private audioContext: AudioContext | null = null;
  private listenerPosition: SpatialPosition = { x: 0, y: 0, z: 0 };
  private remoteParticipants: Map<string, RemoteParticipant> = new Map();
  private localStream: MediaStream | null = null;
  private isConnected = false;
  private listeners: Map<string, Set<(...args: unknown[]) => void>> = new Map();

  // LiveKit SDK room reference (lazy-loaded to avoid bundling when unused)
  /** LiveKit Room instance from dynamically-imported SDK */
  private room: LiveKitRoomInstance | null = null;

  private constructor() {}

  static getInstance(): LiveKitVoiceService {
    if (!LiveKitVoiceService.instance) {
      LiveKitVoiceService.instance = new LiveKitVoiceService();
    }
    return LiveKitVoiceService.instance;
  }

  /**
   * Connect to a LiveKit room for spatial voice chat.
   * Call this after the user has joined a Vircadia world.
   */
  async connect(config: LiveKitConfig): Promise<void> {
    this.config = config;

    try {
      // Dynamically import LiveKit client SDK (optional peer dependency)
      const livekitModule: string = 'livekit-client';
      // LiveKit SDK types from dynamic import
      // LiveKit SDK types from dynamic import
      interface LiveKitModule {
        Room: new (opts: Record<string, unknown>) => LiveKitRoomInstance;
        RoomEvent: Record<string, string>;
        Track: { Kind: Record<string, unknown> };
      }
      const { Room, RoomEvent, Track } = await (Function('m', 'return import(m)')(livekitModule)) as LiveKitModule;

      this.room = new Room({
        adaptiveStream: true,
        dynacast: true,
        audioCaptureDefaults: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
          sampleRate: 48000,
          channelCount: 1,
        },
      });

      // Set up event handlers
      this.room.on(RoomEvent.TrackSubscribed, (...args: unknown[]) => {
        const track = args[0] as Record<string, unknown>;
        const participant = args[2] as Record<string, unknown>;
        if (track.kind === Track.Kind.Audio) {
          this.handleRemoteAudio(track, participant);
        }
      });

      this.room.on(RoomEvent.TrackUnsubscribed, (...args: unknown[]) => {
        const participant = args[2] as Record<string, unknown>;
        this.removeRemoteParticipant(participant.identity as string);
      });

      this.room.on(RoomEvent.ParticipantDisconnected, (...args: unknown[]) => {
        const participant = args[0] as Record<string, unknown>;
        this.removeRemoteParticipant(participant.identity as string);
      });

      this.room.on(RoomEvent.Disconnected, () => {
        this.isConnected = false;
        this.emit('disconnected');
        logger.info('Disconnected from LiveKit room');
      });

      // Connect to the room
      await this.room.connect(config.serverUrl, config.token);
      this.isConnected = true;

      // Set up audio context for spatial processing
      if (config.spatialAudio) {
        this.audioContext = new AudioContext({ sampleRate: 48000 });
        // Set listener position (will be updated from Vircadia)
        const listener = this.audioContext.listener;
        if (listener.positionX) {
          listener.positionX.value = 0;
          listener.positionY.value = 0;
          listener.positionZ.value = 0;
        }
      }

      logger.info(`Connected to LiveKit room: ${config.roomName}`);
      this.emit('connected', { roomName: config.roomName });
    } catch (error) {
      logger.error('Failed to connect to LiveKit:', error);
      throw error;
    }
  }

  /**
   * Start publishing local microphone to the LiveKit room.
   * Called when PTT is released (switching to voice chat mode).
   */
  async startPublishing(): Promise<void> {
    if (!this.room || !this.isConnected) {
      logger.warn('Cannot publish: not connected to LiveKit');
      return;
    }

    try {
      await this.room.localParticipant.setMicrophoneEnabled(true);
      logger.debug('Started publishing microphone to LiveKit');
      this.emit('publishingStarted');
    } catch (error) {
      logger.error('Failed to start microphone publishing:', error);
    }
  }

  /**
   * Stop publishing local microphone.
   * Called when PTT is pressed (switching to agent command mode).
   */
  async stopPublishing(): Promise<void> {
    if (!this.room || !this.isConnected) return;

    try {
      await this.room.localParticipant.setMicrophoneEnabled(false);
      logger.debug('Stopped publishing microphone to LiveKit');
      this.emit('publishingStopped');
    } catch (error) {
      logger.error('Failed to stop microphone publishing:', error);
    }
  }

  /**
   * Update the listener's position (the local user's position in Vircadia world).
   * This drives the spatial audio panning for all remote participants.
   */
  updateListenerPosition(position: SpatialPosition): void {
    this.listenerPosition = position;

    if (this.audioContext?.listener) {
      const listener = this.audioContext.listener;
      if (listener.positionX) {
        listener.positionX.value = position.x;
        listener.positionY.value = position.y;
        listener.positionZ.value = position.z;
      }
    }
  }

  /**
   * Update a remote participant's spatial position.
   * Called when Vircadia entity positions change (from CollaborativeGraphSync).
   */
  updateParticipantPosition(participantId: string, position: SpatialPosition): void {
    const participant = this.remoteParticipants.get(participantId);
    if (!participant) return;

    participant.position = position;

    // Update the Web Audio panner node position
    if (participant.pannerNode) {
      participant.pannerNode.positionX.value = position.x;
      participant.pannerNode.positionY.value = position.y;
      participant.pannerNode.positionZ.value = position.z;
    }
  }

  /** Handle incoming audio track from a remote participant */
  private handleRemoteAudio(track: Record<string, unknown>, participant: Record<string, unknown>): void {
    const participantId = participant.identity as string;
    const isAgent = participantId.startsWith('agent-');

    // Create or update the remote participant entry
    const remote: RemoteParticipant = {
      id: participant.sid as string,
      identity: participantId,
      isAgent,
      position: { x: 0, y: 0, z: 0 },
    };

    if (this.config?.spatialAudio && this.audioContext) {
      // Set up spatial audio: track → panner → destination
      const mediaStream = new MediaStream([track.mediaStreamTrack as MediaStreamTrack]);
      const source = this.audioContext.createMediaStreamSource(mediaStream);

      const panner = this.audioContext.createPanner();
      panner.panningModel = 'HRTF';
      panner.distanceModel = 'inverse';
      panner.maxDistance = this.config.maxDistance;
      panner.refDistance = 1;
      panner.rolloffFactor = 1;
      panner.coneInnerAngle = 360;
      panner.coneOuterAngle = 360;

      source.connect(panner);
      panner.connect(this.audioContext.destination);

      remote.pannerNode = panner;
    } else {
      // Non-spatial: just attach to an audio element
      const audioEl = (track.attach as () => HTMLAudioElement)();
      audioEl.volume = 1.0;
      remote.audioElement = audioEl;
    }

    this.remoteParticipants.set(participantId, remote);
    logger.info(`Remote ${isAgent ? 'agent' : 'user'} audio: ${participantId}`);
    this.emit('participantJoined', { identity: participantId, isAgent });
  }

  /** Remove a remote participant and clean up their audio resources */
  private removeRemoteParticipant(identity: string): void {
    const participant = this.remoteParticipants.get(identity);
    if (!participant) return;

    if (participant.audioElement) {
      participant.audioElement.pause();
      participant.audioElement.srcObject = null;
    }
    if (participant.pannerNode) {
      participant.pannerNode.disconnect();
    }

    this.remoteParticipants.delete(identity);
    logger.info(`Remote participant removed: ${identity}`);
    this.emit('participantLeft', { identity });
  }

  /** Disconnect from the LiveKit room */
  async disconnect(): Promise<void> {
    if (this.room) {
      await this.room.disconnect();
      this.room = null;
    }

    // Clean up all remote participants
    for (const [id] of this.remoteParticipants) {
      this.removeRemoteParticipant(id);
    }

    if (this.audioContext) {
      await this.audioContext.close();
      this.audioContext = null;
    }

    this.isConnected = false;
    logger.info('LiveKit voice service disconnected');
  }

  /** Get the list of currently connected remote participants */
  getRemoteParticipants(): RemoteParticipant[] {
    return Array.from(this.remoteParticipants.values());
  }

  getIsConnected(): boolean {
    return this.isConnected;
  }

  // --- Event emitter ---
  on(event: string, callback: (...args: unknown[]) => void): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback);
  }

  off(event: string, callback: (...args: unknown[]) => void): void {
    this.listeners.get(event)?.delete(callback);
  }

  private emit(event: string, ...args: unknown[]): void {
    this.listeners.get(event)?.forEach(cb => {
      try { cb(...args); } catch (err) {
        logger.error(`LiveKit event listener error (${event}):`, err);
      }
    });
  }
}
