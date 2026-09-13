

import { useState, useEffect, useCallback, useRef } from 'react';
import { VoiceWebSocketService, TranscriptionResult } from '../services/VoiceWebSocketService';
import { useSettingsStore } from '../store/settingsStore';
import { gatedConsole } from '../utils/console';

export interface UseVoiceInteractionOptions {
  autoConnect?: boolean;
  onTranscription?: (text: string, isFinal: boolean) => void;
  onError?: (error: any) => void;
  language?: string;
}

export interface UseVoiceInteractionReturn {
  isConnected: boolean;
  isListening: boolean;
  isSpeaking: boolean;
  transcription: string;
  partialTranscription: string;
  startListening: () => Promise<void>;
  stopListening: () => void;
  speak: (text: string) => Promise<void>;
  toggleListening: () => Promise<void>;
  connect: () => Promise<void>;
  disconnect: () => Promise<void>;
}

export function useVoiceInteraction(options: UseVoiceInteractionOptions = {}): UseVoiceInteractionReturn {
  const { autoConnect = true, onTranscription, onError, language } = options;
  const settings = useSettingsStore((state) => state.settings);

  
  const [isConnected, setIsConnected] = useState(false);
  const [isListening, setIsListening] = useState(false);
  const [isSpeaking, setIsSpeaking] = useState(false);
  const [transcription, setTranscription] = useState('');
  const [partialTranscription, setPartialTranscription] = useState('');

  
  const voiceServiceRef = useRef<VoiceWebSocketService | undefined>(undefined);
  const autoConnectAttemptedRef = useRef(false);
  const onTranscriptionRef = useRef(onTranscription);
  const onErrorRef = useRef(onError);

  
  onTranscriptionRef.current = onTranscription;
  onErrorRef.current = onError;

  
  useEffect(() => {
    voiceServiceRef.current = VoiceWebSocketService.getInstance();
    const voiceService = voiceServiceRef.current;

    
    const handleConnected = () => setIsConnected(true);
    const handleDisconnected = () => {
      setIsConnected(false);
      setIsListening(false);
    };

    const handleTranscription = (result: TranscriptionResult) => {
      if (result.isFinal) {
        setTranscription(result.text);
        setPartialTranscription('');
        onTranscriptionRef.current?.(result.text, true);
      } else {
        setPartialTranscription(result.text);
        onTranscriptionRef.current?.(result.text, false);
      }
    };

    const handleAudioStarted = () => setIsSpeaking(true);
    const handleAudioEnded = () => setIsSpeaking(false);

    const handleError = (error: any) => {
      gatedConsole.voice.error('Voice interaction error:', error);
      onErrorRef.current?.(error);
    };

    
    voiceService.on('connected', handleConnected);
    voiceService.on('disconnected', handleDisconnected);
    voiceService.on('transcription', handleTranscription);
    voiceService.on('voiceError', handleError);

    const audioOutput = voiceService.getAudioOutput();
    audioOutput.on('audioStarted', handleAudioStarted);
    audioOutput.on('audioEnded', handleAudioEnded);

    
    return () => {
      voiceService.off('connected', handleConnected);
      voiceService.off('disconnected', handleDisconnected);
      voiceService.off('transcription', handleTranscription);
      voiceService.off('voiceError', handleError);
      audioOutput.off('audioStarted', handleAudioStarted);
      audioOutput.off('audioEnded', handleAudioEnded);
    };
  }, []); 

  
  useEffect(() => {
    if (autoConnect && !autoConnectAttemptedRef.current && voiceServiceRef.current && (settings?.system?.customBackendUrl || window.location.origin)) {
      autoConnectAttemptedRef.current = true;
      connect().catch((error) => gatedConsole.voice.error('Auto-connect failed:', error));
    }
  }, [autoConnect, settings?.system?.customBackendUrl]);

  const connect = useCallback(async () => {
    if (!voiceServiceRef.current || isConnected) return;

    try {
      const baseUrl = settings?.system?.customBackendUrl || window.location.origin;
      await voiceServiceRef.current.connectToSpeech(baseUrl);
    } catch (error) {
      gatedConsole.voice.error('Failed to connect to voice service:', error);
      onError?.(error);
    }
  }, [isConnected, settings?.system?.customBackendUrl]);

  const disconnect = useCallback(async () => {
    if (!voiceServiceRef.current) return;

    try {
      await voiceServiceRef.current.disconnect();
      autoConnectAttemptedRef.current = false; 
    } catch (error) {
      gatedConsole.voice.error('Failed to disconnect from voice service:', error);
      onError?.(error);
    }
  }, []);

  const startListening = useCallback(async () => {
    if (!voiceServiceRef.current || !isConnected || isListening) return;

    try {
      await voiceServiceRef.current.startAudioStreaming({ language });
      setIsListening(true);
    } catch (error) {
      gatedConsole.voice.error('Failed to start listening:', error);
      onError?.(error);
    }
  }, [isConnected, isListening, language]);

  const stopListening = useCallback(() => {
    if (!voiceServiceRef.current || !isListening) return;

    voiceServiceRef.current.stopAudioStreaming();
    setIsListening(false);
  }, [isListening]);

  const speak = useCallback(async (text: string) => {
    if (!voiceServiceRef.current || !isConnected) {
      throw new Error('Not connected to voice service');
    }

    try {
      await voiceServiceRef.current.sendTextForTTS({
        text,
        voice: settings?.pocketTts?.defaultVoice,
        speed: settings?.pocketTts?.defaultSpeed,
        stream: settings?.pocketTts?.stream ?? true
      });
    } catch (error) {
      gatedConsole.voice.error('Failed to speak:', error);
      onError?.(error);
      throw error;
    }
  }, [isConnected, settings?.pocketTts?.defaultVoice, settings?.pocketTts?.defaultSpeed, settings?.pocketTts?.stream]);

  const toggleListening = useCallback(async () => {
    if (isListening) {
      stopListening();
    } else {
      await startListening();
    }
  }, [isListening, startListening, stopListening]);

  return {
    isConnected,
    isListening,
    isSpeaking,
    transcription,
    partialTranscription,
    startListening,
    stopListening,
    speak,
    toggleListening,
    connect,
    disconnect
  };
}
