import React, { useState, useEffect } from 'react';
import GraphCanvasWrapper from '../features/graph/components/GraphCanvasWrapper';
import { ControlCenter, StatusSurface } from '../features/control-center';
import { useSettingsStore } from '../store/settingsStore';
import { useBotsData } from '../features/bots/contexts/BotsDataContext';
import { graphDataManager, type GraphData } from '../features/graph/managers/graphDataManager';
import { NodeContextMenu } from '../features/graph/components/NodeContextMenu';
import { AgentOpsSurface } from '../features/control-center/agents/AgentOpsSurface';
import { AcspCaseQueue } from '../features/control-center/governance/AcspCaseQueue';
import { KpiPanel } from '../features/control-center/kpi/KpiPanel';
import { createLogger } from '../utils/loggerConfig';

const logger = createLogger('MainLayout');

const MainLayoutContent: React.FC = () => {
  
  const { settings } = useSettingsStore();
  const { botsData } = useBotsData();
  const showStats = settings?.system?.debug?.enablePerformanceDebug ?? false;
  const enableBloom = settings?.visualisation?.glow?.enabled ?? false;
  
  
  const [graphData, setGraphData] = useState<GraphData>({ nodes: [], edges: [] });
  const [otherGraphData, setOtherGraphData] = useState<GraphData | undefined>();

  
  
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
      aria-label="VisionClaw Graph Visualization"
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
        <GraphCanvasWrapper />
      </section>

      <nav id="control-panel" aria-label="Visualization controls">
        <ControlCenter
          showStats={showStats}
          enableBloom={enableBloom}
          onOrbitControlsToggle={() => {}}
          botsData={botsData ?? undefined}
          graphData={graphData}
          otherGraphData={otherGraphData}
        />
      </nav>

      {/* Node detail lives in the control centre's right-hand drawer (ADR-2189 §3). */}

      {/* Right-click additive-expansion menu — driven by visionclaw:node-contextmenu */}
      <NodeContextMenu />

      {/* ADR-2189 §4: the corner chips share one bottom row below the dock, so
          no chip can cover a dock control. Left: agents, then governance. */}
      <div className="cc-corner cc-corner--left" data-testid="corner-left">
        {/* D2/D8 (PRD-023 WP-3): per-agent steering + swarm observability, opened
            behind an agent-node selection or the ambient "Agents" pill. */}
        <AgentOpsSurface />

        {/* REC-2/D3 (PRD-023 WP-4): broker case queue + ambient ACSP open-case
            indicator, driven by broker:* WS events. */}
        <AcspCaseQueue />
      </div>

      {/* Right: status (outermost), then KPIs. */}
      <div className="cc-corner cc-corner--right" data-testid="corner-right">
        {/* Unified system-status surface: connection telemetry, agent summary,
            the SpacePilot connect + secure-context guidance, the layout mode and
            an honest layout-motion readout. */}
        <aside aria-label="System status">
          <StatusSurface />
        </aside>

        {/* REC-4 (PRD-023 WP-8, ADR-043 resurrection): four-KPI dashboard —
            Augmentation Ratio + Trust Variance computed live; Mesh Velocity +
            HITL Precision render "awaiting data source". */}
        <KpiPanel />
      </div>

      {/* OntologyPanel is accessed via the control panel's ontology tab, not rendered as overlay */}

      {/* ADR-2189 §4: voice availability is reported in the Status flyout (a dot on
          the Status chip), not as a fixed warning over the dock. */}
    </main>
  );
};

const MainLayout: React.FC = () => {
  // BotsDataProvider is mounted in App.tsx — no duplicate here
  return <MainLayoutContent />;
};

export default MainLayout;