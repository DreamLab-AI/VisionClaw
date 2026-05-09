import { useEffect, useCallback, useState } from 'react'
import AppInitializer from './AppInitializer'
import { ApplicationModeProvider } from '../contexts/ApplicationModeContext';
import { useSettingsStore } from '../store/settingsStore';
import { createLogger } from '../utils/loggerConfig';
import MainLayout from './MainLayout';
import { useQuest3Integration } from '../hooks/useQuest3Integration';
import { BotsDataProvider } from '../features/bots/contexts/BotsDataContext';
import { CommandPalette } from '../features/command-palette/components/CommandPalette';
import { initializeCommandPalette } from '../features/command-palette/defaultCommands';
import { HelpProvider } from '../features/help/components/HelpProvider';
import { registerSettingsHelp } from '../features/help/settingsHelp';
import { OnboardingProvider } from '../features/onboarding/components/OnboardingProvider';
import { registerOnboardingCommands } from '../features/onboarding/flows/defaultFlows';
import { TooltipProvider } from '../features/design-system/components/Tooltip';
import { useBotsWebSocketIntegration } from '../features/bots/hooks/useBotsWebSocketIntegration';
import { DebugControlPanel } from '../components/DebugControlPanel';
import { ConnectionWarning } from '../components/ConnectionWarning';
import { useAutoBalanceNotifications } from '../hooks/useAutoBalanceNotifications';
import ErrorBoundary from '../components/ErrorBoundary';
import FeatureErrorBoundary from '../components/FeatureErrorBoundary';
import { useNostrAuth } from '../hooks/useNostrAuth';
import { OnboardingWizard } from '../components/OnboardingWizard';
import { LoadingScreen } from '../components/LoadingScreen';
import { WorkerErrorModal } from '../components/WorkerErrorModal';
import solidPodService from '../services/SolidPodService';
import { useHashRoute } from '../hooks/useHashRoute';
import { EnterpriseFullPage, EnterpriseDrawerMount } from '../features/enterprise';
import { BrokerInbox } from '../features/broker/BrokerInbox';
import { MigrationEventToast } from '../features/migration/MigrationEventToast';

const logger = createLogger('App');


function App() {
  const route = useHashRoute();
  const [initializationState, setInitializationState] = useState<'loading' | 'initialized' | 'error'>('loading');
  const [initializationError, setInitializationError] = useState<Error | null>(null);
  const initialized = useSettingsStore(state => state.initialized);

  // Auth state
  const { authenticated, isLoading: isAuthLoading, user } = useNostrAuth();

  const { shouldUseQuest3Layout, isQuest3Detected, autoStartSuccessful } = useQuest3Integration({
    enableAutoStart: false
  });


  const botsConnectionStatus = useBotsWebSocketIntegration();


  useAutoBalanceNotifications();

  // Update settings store with auth state and connect Solid WebSocket
  useEffect(() => {
    if (authenticated && user) {
      const settingsStore = useSettingsStore.getState();
      settingsStore.setAuthenticated(true);
      settingsStore.setUser({
        isPowerUser: user.isPowerUser,
        pubkey: user.pubkey
      });

      // Connect Solid WebSocket for real-time pod notifications
      solidPodService.connectWebSocket();
    } else {
      const settingsStore = useSettingsStore.getState();
      settingsStore.setAuthenticated(false);
      settingsStore.setUser(null);

      // Disconnect Solid WebSocket when user logs out
      solidPodService.disconnect();
    }

    // Cleanup on unmount
    return () => {
      solidPodService.disconnect();
    };
  }, [authenticated, user]);

  
  const shouldUseImmersiveClient = () => {
    // Check for desktop force FIRST - allows overriding VR detection
    const forceDesktop = window.location.search.includes('force=desktop') ||
                         window.location.search.includes('vr=false') ||
                         window.location.search.includes('mode=desktop');

    if (forceDesktop) {
      logger.info('[App] Desktop mode forced via URL parameter');
      return false;
    }

    const userAgent = navigator.userAgent;

    const isQuest3Browser = userAgent.includes('Quest 3') ||
                            userAgent.includes('Quest3') ||
                            userAgent.includes('OculusBrowser') ||
                            (userAgent.includes('VR') && userAgent.includes('Quest')) ||
                            userAgent.toLowerCase().includes('meta quest');


    const forceQuest3 = window.location.search.includes('force=quest3') ||
                        window.location.search.includes('directar=true') ||
                        window.location.search.includes('immersive=true');

    return (isQuest3Browser || forceQuest3 || shouldUseQuest3Layout) && initialized;
  };

  useEffect(() => {
    
    if (initialized) {
      initializeCommandPalette();
      registerSettingsHelp();
      registerOnboardingCommands();

      const hasVisited = localStorage.getItem('hasVisited');
      if (!hasVisited) {
        localStorage.setItem('hasVisited', 'true');
        setTimeout(() => {
          window.dispatchEvent(new CustomEvent('start-onboarding', {
            detail: { flowId: 'welcome' }
          }));
        }, 1000);
      }
    }
  }, [initialized])

  const handleInitialized = useCallback(() => {
    setInitializationState('initialized');
    const settings = useSettingsStore.getState().settings;
    const debugEnabled = settings?.system?.debug?.enabled === true;
    if (debugEnabled) {
      logger.debug('Application initialized');
      logger.debug('Bots WebSocket connection status:', botsConnectionStatus);
    }
  }, [botsConnectionStatus]);

  const handleInitializationError = useCallback((error: Error) => {
    setInitializationError(error);
    setInitializationState('error');
  }, []);

  // Show loading screen while checking auth
  if (isAuthLoading) {
    return <LoadingScreen message="Checking authentication..." />;
  }

  // Allow bypass for visual testing via URL parameter (DEVELOPMENT ONLY)
  const skipAuth = import.meta.env.DEV && (
    window.location.search.includes('skipAuth=true') ||
    window.location.search.includes('test=visual')
  );

  // Show login screen if not authenticated (unless testing bypass in dev mode)
  if (!authenticated && !skipAuth) {
    return <OnboardingWizard onComplete={() => {
      // Auth state update happens inside the wizard via nostrAuth
      // React re-renders automatically via useNostrAuth subscription
    }} />;
  }

  const renderContent = () => {
    switch (initializationState) {
      case 'loading':
        return <LoadingScreen message="Connecting to server..." />;
      case 'error':
        return (
          <div>
            <h2>Error Initializing Application</h2>
            <p>{initializationError?.message || 'An unknown error occurred.'}</p>
            <button onClick={() => window.location.reload()}>Retry</button>
          </div>
        );
      case 'initialized':
        // Enterprise route: full-viewport enterprise shell, no graph behind it
        if (route.startsWith('/enterprise')) {
          return <EnterpriseFullPage />;
        }

        // PRD-008 / ADR-071: Immersive (XR) client now ships as a Godot APK.
        // The browser path always renders the desktop MainLayout. The Quest 3
        // detection helper is retained to surface a sideload prompt in a
        // future W3 deliverable, but it no longer routes to a WebXR scene.
        if (shouldUseImmersiveClient()) {
          logger.info('[App] XR-capable client detected; Godot APK launcher pending (PRD-008 W3). Falling back to desktop layout.');
        }
        return (
          <BotsDataProvider>
            <MainLayout />
          </BotsDataProvider>
        );
    }
  };

  return (
    <>
      <a href="#main-content" className="sr-only focus:not-sr-only focus:absolute focus:z-50 focus:p-4 focus:bg-white focus:text-black">Skip to graph</a>
      <TooltipProvider delayDuration={300} skipDelayDuration={100}>
        <HelpProvider>
          <OnboardingProvider>
            <ErrorBoundary>
              <ApplicationModeProvider>
                <div id="main-content">
                {renderContent()}
                </div>
                {initializationState === 'loading' && (
                  <AppInitializer onInitialized={handleInitialized} onError={handleInitializationError} />
                )}
                {initializationState === 'initialized' && (
                  <>
                    <ConnectionWarning />
                    <FeatureErrorBoundary feature="Command Palette">
                      <CommandPalette />
                    </FeatureErrorBoundary>
                    <FeatureErrorBoundary feature="Debug Panel">
                      <DebugControlPanel />
                    </FeatureErrorBoundary>
                    <WorkerErrorModal />
                    {!route.startsWith('/enterprise') && (
                      <FeatureErrorBoundary feature="Enterprise Drawer">
                        <EnterpriseDrawerMount />
                      </FeatureErrorBoundary>
                    )}
                    {!route.startsWith('/enterprise') && (
                      <FeatureErrorBoundary feature="Broker Inbox">
                        <BrokerInbox compact />
                      </FeatureErrorBoundary>
                    )}
                    {!route.startsWith('/enterprise') && (
                      <FeatureErrorBoundary feature="Migration Toast">
                        <MigrationEventToast />
                      </FeatureErrorBoundary>
                    )}
                  </>
                )}
              </ApplicationModeProvider>
            </ErrorBoundary>
          </OnboardingProvider>
        </HelpProvider>
      </TooltipProvider>
    </>
  );
}

export default App
