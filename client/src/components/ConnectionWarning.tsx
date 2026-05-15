import React, { useState, useEffect } from 'react';
import { createLogger } from '../utils/loggerConfig';
import { webSocketService } from '../store/websocketStore';
import { useSettingsStore } from '../store/settingsStore';
import { AlertCircle, WifiOff, RefreshCw } from 'lucide-react';

const logger = createLogger('ConnectionWarning');


export const ConnectionWarning: React.FC = () => {
  const [isConnected, setIsConnected] = useState(false);
  const [settingsSource, setSettingsSource] = useState<'server' | 'local'>('server');
  const [isReconnecting, setIsReconnecting] = useState(false);
  const [isDismissed, setIsDismissed] = useState(false);
  const debugEnabled = useSettingsStore(s => (s.settings as any)?.system?.debug?.enabled ?? false);

  useEffect(() => {

    const handleConnectionChange = (connected: boolean) => {
      setIsConnected(connected);
      if (!connected) {
        logger.warn('Lost connection to backend server');
      }
    };


    const unsubscribe = webSocketService.onConnectionStatusChange(handleConnectionChange);


    setIsConnected(webSocketService.isReady());

    
    const checkSettingsSource = () => {
      const localStorageSettings = localStorage.getItem('settings');
      if (localStorageSettings && !isConnected) {
        setSettingsSource('local');
        logger.warn('Using cached settings from local storage - server settings unavailable');
      }
    };

    checkSettingsSource();

    // Auto-dismiss after 8 seconds so the banner doesn't permanently block the UI
    const autoDismissTimer = setTimeout(() => {
      setIsDismissed(true);
    }, 8000);

    return () => {
      clearTimeout(autoDismissTimer);
      if (typeof unsubscribe === 'function') {
        unsubscribe();
      }
    };
  }, []);

  const handleReconnect = async () => {
    setIsReconnecting(true);
    try {
      logger.info('Attempting manual reconnection...');
      await webSocketService.connect();
      
      
      const { initialize } = useSettingsStore.getState();
      await initialize();
      
      logger.info('Reconnection successful');
    } catch (error) {
      logger.error('Manual reconnection failed:', error);
    } finally {
      setIsReconnecting(false);
    }
  };


  if (isConnected && settingsSource === 'server') {
    return null;
  }

  // When dismissed, show a minimal indicator instead of blocking the entire UI
  if (isDismissed) {
    return (
      <button
        onClick={() => setIsDismissed(false)}
        className="fixed top-2 right-2 z-40 flex items-center space-x-1 px-2 py-1 bg-orange-600/80 hover:bg-orange-600 text-white rounded-md text-xs transition-colors backdrop-blur-sm"
        title="Backend disconnected - click to show details"
      >
        <WifiOff className="h-3 w-3" />
        <span>Offline</span>
      </button>
    );
  }

  return (
    <div className="fixed top-0 left-0 right-0 z-40 bg-gradient-to-r from-orange-600/95 to-red-600/95 text-white px-4 py-2 shadow-lg backdrop-blur-sm pointer-events-auto">
      <div className="max-w-7xl mx-auto flex items-center justify-between">
        <div className="flex items-center space-x-3">
          {!isConnected ? (
            <WifiOff className="h-4 w-4 animate-pulse" />
          ) : (
            <AlertCircle className="h-4 w-4" />
          )}

          <div className="flex flex-col">
            <div className="font-semibold text-xs">
              {!isConnected
                ? 'Connection to Backend Failed'
                : 'Using Cached Settings'}
            </div>
            <div className="text-[10px] opacity-90">
              {!isConnected
                ? 'Running in offline mode. Real-time features disabled.'
                : 'Using local storage fallback.'}
            </div>
          </div>
        </div>

        <div className="flex items-center space-x-2">
          <button
            onClick={handleReconnect}
            disabled={isReconnecting}
            className="flex items-center space-x-1 px-2 py-1 bg-white/20 hover:bg-white/30 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title="Attempt to reconnect to server"
          >
            <RefreshCw className={`h-3 w-3 ${isReconnecting ? 'animate-spin' : ''}`} />
            <span className="text-xs font-medium">
              {isReconnecting ? 'Reconnecting...' : 'Retry'}
            </span>
          </button>
          <button
            onClick={() => setIsDismissed(true)}
            className="flex items-center px-1.5 py-1 bg-white/10 hover:bg-white/20 rounded-md transition-colors"
            title="Dismiss - controls remain accessible"
            aria-label="Dismiss connection warning"
          >
            <span className="text-xs">✕</span>
          </button>
        </div>
      </div>

      {}
      {debugEnabled && (
        <div className="max-w-7xl mx-auto mt-1 text-[10px] opacity-75 font-mono">
          <div>Settings Source: {settingsSource === 'local' ? 'localStorage' : 'server'} | WebSocket: {isConnected ? 'connected' : 'disconnected'}</div>
        </div>
      )}
    </div>
  );
};

export default ConnectionWarning;