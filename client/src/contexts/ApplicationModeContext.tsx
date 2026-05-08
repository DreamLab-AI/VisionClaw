import React, { createContext, useContext, useState, useEffect, useMemo, useCallback, ReactNode } from 'react';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('ApplicationModeContext');

type ApplicationMode = 'desktop' | 'mobile' | 'xr';

interface LayoutSettings {
  showPanels: boolean;
  showViewport: boolean;
  showControls: boolean;
}

interface ApplicationModeContextValue {
  mode: ApplicationMode;
  previousMode: ApplicationMode | null;
  isXRMode: boolean;
  isMobileView: boolean;
  setMode: (mode: ApplicationMode) => void;
  layoutSettings: LayoutSettings;
}

const defaultContext: ApplicationModeContextValue = {
  mode: 'desktop',
  previousMode: null,
  isXRMode: false,
  isMobileView: false,
  setMode: () => { },
  layoutSettings: {
    showPanels: true,
    showViewport: true,
    showControls: true
  }
};

// Create the context
const ApplicationModeContext = createContext<ApplicationModeContextValue>(defaultContext);

interface ApplicationModeProviderProps {
  children: ReactNode;
}

export const ApplicationModeProvider: React.FC<ApplicationModeProviderProps> = ({ children }) => {
  const [mode, setMode] = useState<ApplicationMode>('desktop');
  const [previousMode, setPreviousMode] = useState<ApplicationMode | null>(null);
  const [isMobileView, setIsMobileView] = useState(false);

  useEffect(() => {
    const handleResize = () => {
      const isMobile = window.innerWidth < 768;
      setIsMobileView(isMobile);

      if (isMobile && mode !== 'xr') {
        setMode('mobile');
      }
      else if (!isMobile && mode === 'mobile') {
        setMode('desktop');
      }
    };

    handleResize();

    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
    };
  }, [mode]);

  const handleModeChange = useCallback((newMode: ApplicationMode) => {
    logger.info(`Changing mode: ${mode} -> ${newMode}`);
    setPreviousMode(mode);
    setMode(newMode);
  }, [mode]);
    
  const getLayoutSettings = (): LayoutSettings => {
    switch (mode) {
      case 'desktop':
        return {
          showPanels: true,
          showViewport: true,
          showControls: true
        };
      case 'mobile':
        return {
          showPanels: true,
          showViewport: true,
          showControls: true
        };
      case 'xr':
        return {
          showPanels: false,
          showViewport: true,
          showControls: false
        };
      default:
        return {
          showPanels: true,
          showViewport: true,
          showControls: true
        };
    }
  };

  const layoutSettings = useMemo(() => getLayoutSettings(), [mode]);

  const contextValue = useMemo<ApplicationModeContextValue>(() => ({
    mode,
    previousMode,
    isXRMode: mode === 'xr',
    isMobileView,
    setMode: handleModeChange,
    layoutSettings
  }), [mode, previousMode, isMobileView, handleModeChange, layoutSettings]);

  return (
    <ApplicationModeContext.Provider value={contextValue}>
      {children}
    </ApplicationModeContext.Provider>
  );
};

export const useApplicationMode = () => {
    const context = useContext(ApplicationModeContext);
    if (!context) {
        throw new Error('useApplicationMode must be used within an ApplicationModeProvider');
    }
    return context;
};
