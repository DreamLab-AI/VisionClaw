

import { AudioOutputService } from './AudioOutputService';
import { AudioInputService, AudioChunk, AudioInputState } from './AudioInputService';
import { useSettingsStore } from '../store/settingsStore';
import { gatedConsole } from '../utils/console';
import { createLogger } from '../utils/loggerConfig';
import { webSocketRegistry } from './WebSocketRegistry';
import { webSocketEventBus } from './WebSocketEventBus';
import { nostrAuth } from './nostrAuthService';

const logger = createLogger('VoiceWebSocketService');

const REGISTRY_NAME = 'voice';

export interface VoiceMessage {
  type: 'tts' | 'stt' | 'audio_chunk' | 'transcription' | 'error' | 'connected' | 'authenticate_success' | 'authenticate_error';
  data?: any;
}

export interface TTSRequest {
  text: string;
  voice?: string;
  speed?: number;
  stream?: boolean;
}

export interface TranscriptionResult {
  text: string;
  isFinal: boolean;
  confidence?: number;
  timestamp?: number;
}

export class VoiceWebSocketService {
  private static instance: VoiceWebSocketService;
  private socket: WebSocket | null = null;
  private audioOutput: AudioOutputService;
  private audioInput: AudioInputService;
  private isStreamingAudio = false;
  private pcmRemainder = new Uint8Array(0);
  private transcriptionCallback?: (result: TranscriptionResult) => void;
  private listeners: Map<string, Set<Function>> = new Map();
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 2000;

  private constructor() {
    this.audioOutput = AudioOutputService.getInstance();
    this.audioInput = AudioInputService.getInstance();

    
    this.setupAudioInputListeners();
  }

  static getInstance(): VoiceWebSocketService {
    if (!VoiceWebSocketService.instance) {
      VoiceWebSocketService.instance = new VoiceWebSocketService();
    }
    return VoiceWebSocketService.instance;
  }

  
  async connectToSpeech(baseUrl: string): Promise<void> {
    const wsUrl = baseUrl.replace(/^http/, 'ws') + '/ws/speech';
    await this.connect(wsUrl);
  }

  
  async connect(url: string): Promise<void> {
    
    if (this.socket && (this.socket.readyState === WebSocket.OPEN || this.socket.readyState === WebSocket.CONNECTING)) {
      return;
    }

    
    if (this.socket) {
      this.socket.close();
      this.socket = null;
    }

    return new Promise((resolve, reject) => {
      try {
        this.socket = new WebSocket(url);

        this.socket.onopen = () => {
          gatedConsole.voice.log('Voice WebSocket connected');
          this.reconnectAttempts = 0;
          webSocketRegistry.register(REGISTRY_NAME, url, this.socket!);
          webSocketEventBus.emit('connection:open', { name: REGISTRY_NAME, url });
          void this.sendAuthenticate(url);
          this.emit('connected');
          resolve();
        };

        this.socket.onmessage = (event) => {
          this.handleMessage(event);
        };

        this.socket.onclose = (event) => {
          gatedConsole.voice.log('Voice WebSocket disconnected');
          webSocketRegistry.unregister(REGISTRY_NAME);
          webSocketEventBus.emit('connection:close', {
            name: REGISTRY_NAME,
            code: event.code,
            reason: event.reason,
          });
          this.emit('disconnected', event);
          if (event.code !== 1000) {
            this.attemptReconnect(url);
          }
        };

        this.socket.onerror = (error) => {
          gatedConsole.voice.error('Voice WebSocket error:', error);
          webSocketEventBus.emit('connection:error', { name: REGISTRY_NAME, error });
          this.emit('error', error);
          reject(error);
        };
      } catch (error) {
        reject(error);
      }
    });
  }

  
  /**
   * ADR-2075: `/ws/speech` authenticates AFTER the upgrade, because browsers
   * cannot set WebSocket request headers. The server refuses every
   * command-bearing frame until this arrives and closes the socket if it does
   * not. Same frame shape as the graph socket and `analyticsApi`.
   */
  private async sendAuthenticate(wsUrl: string): Promise<void> {
    if (!this.isConnected()) return;
    if (!nostrAuth.isAuthenticated()) {
      logger.warn('Voice socket: not authenticated — /ws/speech will refuse commands');
      return;
    }
    try {
      if (nostrAuth.isDevMode()) {
        const user = nostrAuth.getCurrentUser();
        this.send(
          JSON.stringify({
            type: 'authenticate',
            token: 'dev-session-token',
            pubkey: user?.pubkey,
          })
        );
        return;
      }
      // The server validates the NIP-98 `u` tag against the HTTP-equivalent of
      // this socket's own URL, so sign exactly that.
      const httpUrl = wsUrl.replace(/^ws(s?):\/\//, 'http$1://');
      const event = await nostrAuth.signRequest(httpUrl, 'GET');
      this.send(JSON.stringify({ type: 'authenticate', event }));
    } catch (e) {
      logger.warn('Voice socket NIP-98 authenticate failed:', e);
    }
  }

  private handleMessage(event: MessageEvent): void {
    
    if (event.data instanceof ArrayBuffer || event.data instanceof Blob) {
      this.handleAudioData(event.data);
      return;
    }

    
    try {
      const message: VoiceMessage = JSON.parse(event.data);
      
      
      if (message.type === 'error') {
        logger.error('Raw error message from server', message);
      }

      // Emit to cross-service event bus for any listener
      webSocketEventBus.emit('message:voice', { data: message });

      switch (message.type) {
        case 'connected':
          gatedConsole.voice.log('Connected to voice service:', message.data);
          this.emit('voiceConnected', message.data);
          break;

        case 'transcription':
          this.handleTranscription(message.data);
          break;

        case 'error':
          const errorMsg = (message as VoiceMessage).data || (message as VoiceMessage & { error?: string }).error || 'Unknown voice service error';
          gatedConsole.voice.error('Voice service error:', errorMsg);
          this.emit('voiceError', errorMsg);
          break;

        // ADR-2075 post-upgrade auth outcome.
        case 'authenticate_success':
          gatedConsole.voice.log(
            'Voice socket authenticated:',
            (message as VoiceMessage & { pubkey?: string }).pubkey
          );
          this.emit('voiceAuthenticated', message);
          break;

        case 'authenticate_error':
          gatedConsole.voice.error(
            'Voice socket authentication rejected:',
            (message as VoiceMessage & { error?: string }).error
          );
          this.emit('voiceError', (message as VoiceMessage & { error?: string }).error);
          break;

        default:
          this.emit('message', message);
      }
    } catch (error) {
      gatedConsole.voice.error('Failed to parse voice message:', error);
    }
  }

  
  private async handleAudioData(data: ArrayBuffer | Blob) {
    try {
      
      const buffer = data instanceof Blob ? await data.arrayBuffer() : data;

      
      const joined = new Uint8Array(this.pcmRemainder.length + buffer.byteLength);
      joined.set(this.pcmRemainder);
      joined.set(new Uint8Array(buffer), this.pcmRemainder.length);
      const length = joined.length - joined.length % 2;
      this.pcmRemainder = joined.slice(length);
      await this.audioOutput.queuePcm(joined.slice(0, length).buffer, 24000);
      this.emit('audioReceived', buffer);
    } catch (error) {
      gatedConsole.voice.error('Failed to handle audio data:', error);
      this.emit('audioError', error);
    }
  }

  
  private handleTranscription(data: TranscriptionResult) {
    if (this.transcriptionCallback) {
      this.transcriptionCallback(data);
    }
    this.emit('transcription', data);
  }

  
  async sendTextForTTS(request: TTSRequest): Promise<void> {
    if (!this.isConnected()) {
      throw new Error('Not connected to voice service');
    }

    this.audioOutput.stop();
    this.pcmRemainder = new Uint8Array(0);
    const message: VoiceMessage = {
      type: 'tts',
      data: { ...request, voice: request.voice || 'alba', stream: true }
    };

    this.send(JSON.stringify(message));
    this.emit('ttsSent', request);
  }

  /**
   * COM-15 / D6: the PTT-start binding message. Threads the selected agent's
   * `did:nostr` onto the server session so a following spoken command has a
   * verifiable target (`AudioRouter.set_ptt_with_target`). Fire-and-forget: a
   * dropped socket is a no-op (the binding re-sends on the next PTT edge).
   */
  setPtt(active: boolean, actorDid?: string | null): void {
    if (!this.isConnected()) return;
    if (active) {
      this.audioOutput.stop();
      this.pcmRemainder = new Uint8Array(0);
      this.send(JSON.stringify({ type: 'cancel_tts' }));
    }
    this.send(
      JSON.stringify({
        type: 'set_ptt',
        active,
        actorDid: actorDid ?? null,
      }),
    );
  }

  /**
   * COM-15 / V1: dispatch a spoken command. When `actorDid` is a bound agent's
   * `did:nostr`, the server takes the GOVERNED path (signed 31402 →
   * `/v1/voice-intent` → PocketTts ack); otherwise it reaches the settings
   * assistant. The `actorDid` is carried per-command so a mid-utterance
   * re-selection cannot mis-address it.
   */
  sendVoiceCommand(text: string, actorDid?: string | null): void {
    if (!this.isConnected()) {
      throw new Error('Not connected to voice service');
    }
    this.send(
      JSON.stringify({
        type: 'voice_command',
        text,
        actorDid: actorDid ?? null,
      }),
    );
  }


  async startAudioStreaming(options?: { language?: string; model?: string }): Promise<void> {
    if (!this.isConnected()) {
      throw new Error('Not connected to voice service');
    }

    if (this.isStreamingAudio) {
      gatedConsole.voice.warn('Audio streaming already active');
      return;
    }

    
    const support = AudioInputService.getBrowserSupport();
    logger.warn('DEVELOPER MODE: Browser support checks bypassed', support);
    
    
    
    
    
    
    
    
    
    
    
    
    
    

    try {
      
      const micAccess = await this.audioInput.requestMicrophoneAccess();
      if (!micAccess) {
        throw new Error('Microphone access denied');
      }

      
      await this.audioInput.startRecording();
      this.isStreamingAudio = true;

      
      const message = {
        type: 'stt',
        action: 'start',
        language: options?.language || 'en',
        model: options?.model || 'whisper-1',
        ...options
      };

      this.send(JSON.stringify(message));
      this.emit('audioStreamingStarted');
    } catch (error) {
      
      this.isStreamingAudio = false;
      this.audioInput.stopRecording();
      throw error;
    }
  }

  
  stopAudioStreaming() {
    if (!this.isStreamingAudio) {
      return;
    }

    
    this.audioInput.stopRecording();
    
    
    
    
    if (this.isConnected()) {
      
      const message = {
        type: 'stt',
        action: 'stop'
      };

      this.send(JSON.stringify(message));
    }

    
    setTimeout(() => {
      this.isStreamingAudio = false;
      this.emit('audioStreamingStopped');
    }, 100);
  }

  
  private setupAudioInputListeners() {
    
    this.audioInput.on('recordingComplete', async (completeAudio: Blob) => {
      logger.debug('Recording complete event received', { blobSize: completeAudio.size });
      if (this.isStreamingAudio && this.isConnected()) {
        
        try {
          const arrayBuffer = await completeAudio.arrayBuffer();
          logger.debug('Sending complete audio file', { bytes: arrayBuffer.byteLength });
          gatedConsole.voice.log('Sending complete audio file:', {
            size: arrayBuffer.byteLength,
            type: completeAudio.type
          });
          
          this.sendBinary(arrayBuffer);
        } catch (error) {
          logger.error('Failed to send audio:', error);
          gatedConsole.voice.error('Failed to send audio:', error);
        }
      } else {
        logger.debug('Not sending audio', { streaming: this.isStreamingAudio, connected: this.isConnected() });
      }
    });

    
    this.audioInput.on('audioChunk', (chunk: AudioChunk) => {
      
      
    });

    this.audioInput.on('error', (error: any) => {
      gatedConsole.voice.error('Audio input error:', error);
      this.emit('audioInputError', error);
    });

    this.audioInput.on('audioLevel', (level: number) => {
      this.emit('audioLevel', level);
    });

    this.audioInput.on('stateChange', (state: AudioInputState) => {
      this.emit('audioInputStateChange', state);
    });
  }

  
  private isConnected(): boolean {
    return this.socket?.readyState === WebSocket.OPEN;
  }

  
  private send(data: string): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(data);
    }
  }

  
  private sendBinary(data: ArrayBuffer): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(data);
    }
  }

  
  private attemptReconnect(url: string) {
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      setTimeout(() => {
        gatedConsole.voice.log(`Attempting to reconnect voice WebSocket (${this.reconnectAttempts}/${this.maxReconnectAttempts})`);
        this.connect(url).catch((error) => gatedConsole.voice.error('Reconnect failed:', error));
      }, this.reconnectDelay);
    }
  }

  
  on(event: string, callback: Function) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(callback);
  }

  off(event: string, callback: Function) {
    if (this.listeners.has(event)) {
      this.listeners.get(event)!.delete(callback);
    }
  }

  private emit(event: string, ...args: any[]) {
    if (this.listeners.has(event)) {
      this.listeners.get(event)!.forEach(callback => {
        callback(...args);
      });
    }
  }

  
  onTranscription(callback: (result: TranscriptionResult) => void) {
    this.transcriptionCallback = callback;
  }

  
  stopAllAudio() {
    this.stopAudioStreaming();
    this.audioOutput.stop();
  }

  
  async disconnect(): Promise<void> {
    this.stopAllAudio();
    webSocketRegistry.unregister(REGISTRY_NAME);
    if (this.socket) {
      this.socket.close(1000, 'Normal closure');
      this.socket = null;
    }

    this.reconnectAttempts = this.maxReconnectAttempts;
  }

  
  getAudioOutput(): AudioOutputService {
    return this.audioOutput;
  }

  
  getAudioInput(): AudioInputService {
    return this.audioInput;
  }
}
