/**
 * StatusSurface — the unified system-status surface for the control centre.
 *
 * Replaces the old free-floating top-right StatusCluster pill. It anchors in the
 * bottom badge cluster alongside the Agents and KPI badges (mirroring their
 * idiom exactly), so status now reads as part of the ONE control interface
 * rather than a detached HUD element.
 *
 * At rest it is a slim glass chip — health dot (websocket lifecycle) + agent
 * count + a SpacePilot dot that appears once a device is connected. It carries
 * NO polling of its own while collapsed: the heavy telemetry (constraint stats,
 * inferred-edges refresh, the motion sampler) lives in StatusFlyout, which mounts
 * only on expand. The chip's inputs are all event-driven store/service reads.
 *
 * Everything is read directly from stores/services — nothing is threaded in as
 * props — so ControlCenter no longer has to relay status/SpacePilot state.
 */

import React, { useState } from 'react';
import { Activity, Bot } from 'lucide-react';
import { voiceSupportIssues } from './voiceSupport';
import { useBotsDataOptional } from '../../bots/contexts/BotsDataContext';
import { useWebSocketStatus } from './useConnectionTelemetry';
import { useSpacePilot } from './useSpacePilot';
import { StatusFlyout } from './StatusFlyout';
import { useSettingsSyncStatus, describeUnsaved } from '../../../store/settingsSyncStatus';

function dotColor(status: 'connected' | 'connecting' | 'disconnected'): string {
  if (status === 'connected') return '#22c55e';
  if (status === 'connecting') return '#f59e0b';
  return '#ef4444';
}

export const StatusSurface: React.FC = () => {
  // Pure click-toggle (the KpiPanel / Agents-badge idiom). A focus-to-expand
  // handler would fight the button's own click — focus fires first on click,
  // toggling open, then the click toggles it straight back shut. The chip is a
  // real <button>, so Enter/Space activate it and keyboard operation is intact.
  const [expanded, setExpanded] = useState(false);

  const websocketStatus = useWebSocketStatus();
  const spacePilot = useSpacePilot();
  const agentCount = useBotsDataOptional()?.botsData?.nodeCount ?? 0;
  const [voiceIssues] = useState(voiceSupportIssues);
  // Settings changes the server does not have (failed, sync off, or made before load):
  // shown here so a lost change is never only a debug log line.
  const unsaved = useSettingsSyncStatus((s) => s.unsaved);
  const unsavedCount = Object.keys(unsaved).length;
  const label = [
    voiceIssues.length ? 'System status (voice unavailable)' : 'System status',
    unsavedCount ? `${unsavedCount} settings not saved on the server` : '',
  ].filter(Boolean).join('; ');

  return (
    <div className="relative flex flex-col items-end gap-2" data-corner-item="status">
      {expanded && (
        <StatusFlyout
          websocketStatus={websocketStatus}
          spacePilot={spacePilot}
          onClose={() => setExpanded(false)}
        />
      )}

      <button
        type="button"
        data-testid="status-surface"
        aria-label={label}
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        className="cc-glass flex items-center gap-2 px-3 py-1.5 rounded-full text-xs text-foreground hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ background: dotColor(websocketStatus) }}
          aria-hidden="true"
        />
        <Activity size={13} aria-hidden="true" />
        <span className="cc-value-readout flex items-center gap-1">
          <Bot size={11} aria-hidden="true" />
          {agentCount.toLocaleString()}
        </span>
        {voiceIssues.length > 0 && (
          <span
            data-testid="status-voice-dot"
            className="inline-block h-1.5 w-1.5 rounded-full"
            style={{ background: '#f59e0b' }}
            role="img"
            aria-label="Voice unavailable"
            title={`Voice unavailable: ${voiceIssues.join('; ')}`}
          />
        )}
        {unsavedCount > 0 && (
          <span
            data-testid="status-unsaved-settings"
            className="cc-value-readout rounded-full px-1.5 text-[10px] leading-4"
            style={{ background: '#f59e0b33', color: '#f59e0b' }}
            title={`Settings not saved on the server: ${describeUnsaved(unsaved)}`}
          >
            {unsavedCount} unsaved
          </span>
        )}
        {spacePilot.connected && (
          <span
            className="inline-block h-1.5 w-1.5 rounded-full"
            style={{ background: '#22c55e' }}
            role="img"
            aria-label="SpacePilot connected"
            title="SpacePilot connected"
          />
        )}
      </button>
    </div>
  );
};

StatusSurface.displayName = 'StatusSurface';

export default StatusSurface;
