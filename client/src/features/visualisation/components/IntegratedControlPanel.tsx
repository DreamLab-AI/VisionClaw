
import React, { useState, useEffect, useMemo } from 'react';
import { SpaceDriver } from '../../../services/SpaceDriverService';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../design-system/components/Tabs';
import { TooltipProvider } from '../../design-system/components/Tooltip';
import ErrorBoundary from '../../../components/ErrorBoundary';
// Import to trigger scrollbar-hiding CSS injection
import '../../design-system/components/ScrollArea';
import { createLogger } from '../../../utils/loggerConfig';

const logger = createLogger('IntegratedControlPanel');

// Control Panel Components
import { ControlPanelHeader } from './ControlPanel/ControlPanelHeader';
import { SystemInfo } from './ControlPanel/SystemInfo';
import { BotsStatusPanel } from './ControlPanel/BotsStatusPanel';
import { SpacePilotStatus } from './ControlPanel/SpacePilotStatus';
import { TabNavigation } from './ControlPanel/TabNavigation';
import type { ControlPanelProps } from './ControlPanel/types';

// Unified Control Center Components
import { SystemHealthIndicator } from './ControlPanel/SystemHealthIndicator';
import { AdvancedModeToggle } from './ControlPanel/AdvancedModeToggle';
import { UnifiedSettingsTabContent } from './ControlPanel/UnifiedSettingsTabContent';
import { UNIFIED_TABS, filterTabs } from './ControlPanel/unifiedSettingsConfig';
import { SolidTabContent } from '../../solid/components/SolidTabContent';
import { ControlPanelProvider, useControlPanelContext } from '../../settings/components/control-panel-context';
import { useSettingsStore } from '../../../store/settingsStore';
import { Lock, Star } from 'lucide-react';
import { CommandInput } from './CommandInput';


// Inner component that uses context
const IntegratedControlPanelInner: React.FC<ControlPanelProps> = ({
  showStats,
  enableBloom,
  onOrbitControlsToggle,
  botsData,
  graphData,
  otherGraphData
}) => {

  const [isExpanded, setIsExpanded] = useState(true);
  const [activeTab, setActiveTab] = useState<string>('graph');

  // Unified Control Center state
  const { advancedMode } = useControlPanelContext();
  const isPowerUser = useSettingsStore(state => state.isPowerUser);

  // Filter visible tabs based on advanced mode and power user status
  const visibleTabs = useMemo(() => {
    return filterTabs(UNIFIED_TABS, advancedMode, isPowerUser);
  }, [advancedMode, isPowerUser]);

  // Calculate grid columns for tab layout
  const gridColumns = useMemo(() => {
    const count = visibleTabs.length;
    if (count <= 4) return count;
    if (count <= 6) return 3;
    if (count <= 9) return Math.ceil(count / 2);
    return 4;
  }, [visibleTabs.length]);

  
  const [webHidAvailable, setWebHidAvailable] = useState(false);
  const [spacePilotConnected, setSpacePilotConnected] = useState(false);
  const [spacePilotButtons, setSpacePilotButtons] = useState<string[]>([]);

  
  useEffect(() => {
    setWebHidAvailable('hid' in navigator);
  }, []);

  
  useEffect(() => {
    const handleConnect = () => {
      setSpacePilotConnected(true);
      onOrbitControlsToggle?.(false);
    };

    const handleDisconnect = () => {
      setSpacePilotConnected(false);
      setSpacePilotButtons([]);
      onOrbitControlsToggle?.(true);
    };

    const handleButtons = (event: any) => {
      const buttons = event.detail.buttons || [];
      setSpacePilotButtons(buttons);
    };

    SpaceDriver.addEventListener('connect', handleConnect);
    SpaceDriver.addEventListener('disconnect', handleDisconnect);
    SpaceDriver.addEventListener('buttons', handleButtons);

    return () => {
      SpaceDriver.removeEventListener('connect', handleConnect);
      SpaceDriver.removeEventListener('disconnect', handleDisconnect);
      SpaceDriver.removeEventListener('buttons', handleButtons);
    };
  }, [onOrbitControlsToggle]);

  const handleConnectSpacePilot = async () => {
    try {
      await SpaceDriver.scan();
    } catch (error) {
      
    }
  };

  if (!isExpanded) {
    return (
      <>
        <div
          onClick={() => setIsExpanded(true)}
          style={{
            position: 'fixed',
            top: '10px',
            left: '10px',
            width: '40px',
            height: '40px',
            background: 'rgba(0,0,0,0.8)',
            border: '1px solid rgba(255,255,255,0.3)',
            borderRadius: '4px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
            zIndex: 1000
          }}
        >
          <div style={{
            width: '12px',
            height: '12px',
            background: '#f87171',
            borderRadius: '50%',
            boxShadow: '0 0 5px rgba(248,113,113,0.5)'
          }} />
        </div>
        <CommandInput isCollapsed={true} />
      </>
    );
  }

  
  return (
    <div style={{
      position: 'fixed',
      top: '10px',
      left: '10px',
      color: 'white',
      fontFamily: 'sans-serif',
      fontSize: '11px',
      background: 'rgba(0,0,0,0.92)',
      padding: '12px',
      borderRadius: '6px',
      border: '1px solid rgba(255,255,255,0.2)',
      width: '360px',
      maxWidth: '360px',
      maxHeight: '88vh',
      display: 'flex',
      flexDirection: 'column',
      backdropFilter: 'blur(12px)',
      boxShadow: '0 8px 32px rgba(0,0,0,0.4)',
      zIndex: 1000,
      overflow: 'hidden'
    }}>
      {}
      <div style={{
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        marginBottom: '8px',
        paddingBottom: '8px',
        borderBottom: '1px solid rgba(255,255,255,0.15)'
      }}>
        <div style={{
          display: 'flex',
          alignItems: 'center',
          gap: '6px'
        }}>
          <div style={{
            width: '6px',
            height: '6px',
            background: '#10b981',
            borderRadius: '50%',
            boxShadow: '0 0 6px rgba(16,185,129,0.6)'
          }} />
          <h2 style={{
            fontSize: '12px',
            fontWeight: 'bold',
            color: '#10b981',
            margin: 0
          }}>
            Control Center
          </h2>
        </div>
        <button
          onClick={() => setIsExpanded(false)}
          style={{
            background: 'rgba(255,255,255,0.1)',
            border: '1px solid rgba(255,255,255,0.2)',
            color: 'white',
            width: '20px',
            height: '20px',
            borderRadius: '3px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
            fontSize: '14px',
            lineHeight: '1'
          }}
        >
          ×
        </button>
      </div>

      {/* System Health and Advanced Mode Toggle - NEW UNIFIED COMPONENTS */}
      <div style={{
        display: 'flex',
        gap: '6px',
        marginBottom: '8px',
        fontSize: '10px'
      }}>
        <div style={{ flex: 1 }}>
          <SystemHealthIndicator
            graphData={graphData}
            mcpConnected={false}
            websocketStatus="connected"
            metadataStatus={(graphData?.nodes?.length ?? 0) > 0 ? 'loaded' : 'loading'}
          />
        </div>
        <div style={{ flex: 1 }}>
          <AdvancedModeToggle compact />
        </div>
      </div>

      {}
      <BotsStatusPanel botsData={botsData} />

      {}
      <SpacePilotStatus
        webHidAvailable={webHidAvailable}
        spacePilotConnected={spacePilotConnected}
        spacePilotButtons={spacePilotButtons}
        onConnect={handleConnectSpacePilot}
      />

      {/* UNIFIED TAB NAVIGATION */}
      <div className="scroll-area" style={{
        flex: 1,
        overflow: 'auto',
        marginTop: '8px',
        minHeight: 0
      }}>
        <Tabs value={activeTab} onValueChange={setActiveTab}>
          <TabsList style={{
            width: '100%',
            background: 'rgba(255,255,255,0.08)',
            border: '1px solid rgba(255,255,255,0.15)',
            borderRadius: '4px',
            padding: '2px',
            marginBottom: '8px',
            display: 'grid',
            gridTemplateColumns: `repeat(${gridColumns}, 1fr)`,
            gap: '2px',
            height: 'auto',
            minHeight: 'auto'
          }}>
            {visibleTabs.map((tab) => {
              const IconComponent = tab.icon;
              const isAdvancedTab = tab.isAdvanced;
              const isPowerUserTab = tab.isPowerUserOnly;

              return (
                <TabsTrigger
                  key={tab.id}
                  value={tab.id}
                  title={tab.description}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    alignItems: 'center',
                    gap: '2px',
                    padding: '6px 4px',
                    fontSize: '9px',
                    fontWeight: '500',
                    color: isAdvancedTab ? 'rgba(168,85,247,0.9)' : 'rgba(255,255,255,0.7)',
                    border: isPowerUserTab
                      ? '1px solid rgba(251,191,36,0.3)'
                      : '0',
                    borderRadius: '3px',
                    background: isPowerUserTab
                      ? 'rgba(251,191,36,0.05)'
                      : 'transparent',
                    cursor: 'pointer',
                    height: '100%',
                    transition: 'all 0.2s',
                    position: 'relative'
                  }}
                >
                  {/* Power user indicator */}
                  {isPowerUserTab && (
                    <div style={{ position: 'absolute', top: '2px', right: '2px' }}>
                      <Star size={6} style={{ color: '#fbbf24', fill: '#fbbf24' }} />
                    </div>
                  )}

                  {/* Advanced mode indicator */}
                  {isAdvancedTab && !isPowerUserTab && (
                    <div style={{ position: 'absolute', top: '2px', right: '2px' }}>
                      <Lock size={6} style={{ color: '#a855f7' }} />
                    </div>
                  )}

                  {IconComponent && <IconComponent size={14} />}
                  <div style={{ textAlign: 'center', lineHeight: '1.1' }}>
                    {tab.buttonKey && (
                      <div style={{ opacity: 0.6, fontSize: '7px' }}>{tab.buttonKey}</div>
                    )}
                    <div style={{ fontSize: '9px' }}>{tab.label}</div>
                  </div>
                </TabsTrigger>
              );
            })}
          </TabsList>

          {/* Tab Content - Using unified settings content */}
          <div style={{
            background: 'rgba(0,0,0,0.2)',
            border: '1px solid rgba(255,255,255,0.1)',
            borderRadius: '4px',
            padding: '8px',
            maxHeight: '300px',
            overflowY: 'auto'
          }}>
            {UNIFIED_TABS.map(tab => (
              <TabsContent key={tab.id} value={tab.id}>
                {tab.id === 'solid' ? (
                  <SolidTabContent />
                ) : (
                  <UnifiedSettingsTabContent
                    sectionId={tab.id}
                    onError={(err) => logger.error('Settings error:', err)}
                    onSuccess={(msg) => logger.debug('Settings success:', msg)}
                  />
                )}
              </TabsContent>
            ))}
          </div>
        </Tabs>
      </div>

      {/* Footer with tab count info */}
      <div style={{
        marginTop: '8px',
        padding: '4px 8px',
        background: 'rgba(255,255,255,0.03)',
        borderRadius: '3px',
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center',
        fontSize: '8px',
        color: 'rgba(255,255,255,0.4)'
      }}>
        <span>{visibleTabs.length} tabs visible</span>
        {!advancedMode && (
          <span>+{UNIFIED_TABS.filter(t => t.isAdvanced).length} advanced hidden</span>
        )}
      </div>
    </div>
  );
};

// Exported component wrapped with ControlPanelProvider
export const IntegratedControlPanel: React.FC<ControlPanelProps> = (props) => {
  return (
    <ControlPanelProvider>
      <IntegratedControlPanelInner {...props} />
    </ControlPanelProvider>
  );
};

export default IntegratedControlPanel;
