import React, { useState, useEffect } from 'react';
import GraphCanvasWrapper from '../features/graph/components/GraphCanvasWrapper';
import { IntegratedControlPanel } from '../features/visualisation/components/IntegratedControlPanel';
import { useSettingsStore } from '../store/settingsStore';
import { useBotsData } from '../features/bots/contexts/BotsDataContext';
import { BrowserSupportWarning } from '../components/BrowserSupportWarning';
import { SpaceMouseStatus } from '../components/SpaceMouseStatus';
import { AudioInputService } from '../services/AudioInputService';
import { graphDataManager, type GraphData } from '../features/graph/managers/graphDataManager';
import { NodeDetailPanel } from '../features/graph/components/NodeDetailPanel';
import FeatureErrorBoundary from '../components/FeatureErrorBoundary';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('MainLayout');

const MainLayoutContent: React.FC = () => {
  
  const showStats = useSettingsStore(
    s => (s.settings as any)?.system?.debug?.enablePerformanceDebug ?? false
  );
  const enableBloom = useSettingsStore(
    s => (s.settings as any)?.visualisation?.glow?.enabled ?? false
  );
  const { botsData } = useBotsData();
  const [hasVoiceSupport, setHasVoiceSupport] = useState(true);
  
  
  const [graphData, setGraphData] = useState<GraphData>({ nodes: [], edges: [] });
  const [otherGraphData, setOtherGraphData] = useState<GraphData | undefined>();

  useEffect(() => {
    const support = AudioInputService.getBrowserSupport();
    const isSupported = support.getUserMedia && support.isHttps && support.audioContext && support.mediaRecorder;
    setHasVoiceSupport(isSupported);
  }, []);
  
  
  useEffect(() => {
    const unsubscribe = graphDataManager.onGraphDataChange((data: GraphData) => {
      setGraphData(data);
      
      
      if (data.nodes.length > 0) {
        setOtherGraphData({
          nodes: data.nodes.slice(0, Math.floor(data.nodes.length / 2)),
          edges: data.edges.slice(0, Math.floor(data.edges.length / 2))
        });
      }
    });
    
    return unsubscribe;
  }, []);

  return (
    <main
      role="main"
      aria-label="VisionFlow Graph Visualization"
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        width: '100vw',
        height: '100vh',
        backgroundColor: '#000022'
      }}
    >
      {/* Skip link for keyboard navigation */}
      <a
        href="#control-panel"
        className="sr-only focus:not-sr-only focus:absolute focus:z-50 focus:top-2 focus:left-2 focus:bg-white focus:text-black focus:p-2 focus:rounded"
      >
        Skip to controls
      </a>

      <section aria-label="Graph visualization canvas">
        <FeatureErrorBoundary feature="Graph Canvas">
          <GraphCanvasWrapper />
        </FeatureErrorBoundary>
      </section>

      <nav id="control-panel" aria-label="Visualization controls">
        <FeatureErrorBoundary feature="Control Panel">
          <IntegratedControlPanel
            showStats={showStats}
            enableBloom={enableBloom}
            onOrbitControlsToggle={() => {}}
            botsData={botsData ?? undefined}
            graphData={graphData}
            otherGraphData={otherGraphData}
          />
        </FeatureErrorBoundary>
      </nav>

      <FeatureErrorBoundary feature="Node Detail Panel">
        <NodeDetailPanel />
      </FeatureErrorBoundary>

      {/* OntologyPanel is accessed via the control panel's ontology tab, not rendered as overlay */}

      <aside aria-label="Device status">
        <SpaceMouseStatus />
      </aside>

      {!hasVoiceSupport && (
        <aside
          className="fixed bottom-20 left-4 z-40 max-w-sm pointer-events-auto"
          role="alert"
          aria-live="polite"
        >
          <BrowserSupportWarning className="shadow-lg" />
        </aside>
      )}
    </main>
  );
};

const MainLayout: React.FC = () => {
  // BotsDataProvider is mounted in App.tsx — no duplicate here
  return <MainLayoutContent />;
};

export default MainLayout;