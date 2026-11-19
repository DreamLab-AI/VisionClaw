import './devMeasureGuard'; // dev only, and first: must wrap performance.measure before React loads
import '../rendering/troikaConfig';
import { extend } from '@react-three/fiber';
import { GeodesicPolyhedronGeometry } from '../utils/three-geometries';

extend({ GeodesicPolyhedronGeometry });
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { initializeDebugSystem } from '../utils/debugConfig';
import { unifiedApiClient } from '../services/api/UnifiedApiClient';
import { initializeAuthInterceptor, setupAuthStateListener } from '../services/api/authInterceptor';
import { useSettingsStore } from '../store/settingsStore';
import { useWebSocketStore } from '../store/websocketStore';
import '../styles/index.css';


// Initialize debug system before app starts
initializeDebugSystem();

// Expose stores for testing/debugging (dev mode only)
if (import.meta.env.DEV) {
  const devWindow = window as unknown as Record<string, unknown>;
  devWindow.useSettingsStore = useSettingsStore;
  devWindow.useWebSocketStore = useWebSocketStore;
}

// Initialize authentication interceptor for all API calls
initializeAuthInterceptor(unifiedApiClient);
setupAuthStateListener();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
