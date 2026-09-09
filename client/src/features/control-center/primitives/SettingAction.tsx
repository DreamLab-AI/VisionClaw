/**
 * SettingAction — action buttons (reset_layout, refresh_graph, toggle-webgpu,
 * run_clustering). design-spec.md §1.1, §1.5, §1.9.
 *
 * Every call site is ported verbatim from UnifiedSettingsTabContent.handleAction
 * so writes/side-effects hit the exact same endpoints and reload semantics as
 * the legacy panel.
 */

import React, { useCallback, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import type { RegistryField } from '../registry/types';
import { webSocketService } from '../../../store/websocketStore';
import { isWebGPURenderer, setForceWebGLOverride } from '../../../rendering/rendererFactory';
import { toggleDemo, isDemoRunning } from '../../bots/agentDemoMode';
import { createLogger } from '../../../utils/loggerConfig';
import { cn } from '../../../utils/classNameUtils';

const logger = createLogger('SettingAction');

export interface SettingActionProps {
  field: RegistryField;
  testId: string;
  /** Run Grouping's transient localKey values (method, numClusters, eps, minSamples, resolution). */
  localValues?: Record<string, unknown>;
  disabled?: boolean;
  onSuccess?: (message: string) => void;
  onError?: (message: string) => void;
}

const BUTTON_STYLE: Record<string, string> = {
  reset_layout: 'bg-amber-600 hover:bg-amber-500 shadow-amber-600/30',
  run_clustering: 'bg-violet-600 hover:bg-violet-500 shadow-violet-600/30',
  'toggle-webgpu': '', // computed dynamically below (active/inactive)
  'toggle-demo': '',   // computed dynamically below (active/inactive)
  refresh_graph: 'bg-primary hover:bg-primary/90 shadow-primary/30',
};

export const SettingAction: React.FC<SettingActionProps> = ({
  field,
  testId,
  localValues,
  disabled,
  onSuccess,
  onError,
}) => {
  const [running, setRunning] = useState(false);
  const [demoActive, setDemoActive] = useState(() => isDemoRunning());
  const isRunClustering = field.action === 'run_clustering';
  const isWebGPUToggle = field.action === 'toggle-webgpu';
  const isDemoToggle = field.action === 'toggle-demo';
  const webgpuActive = isWebGPUToggle ? isWebGPURenderer : false;

  const handleAction = useCallback(async () => {
    if (field.action === 'refresh_graph') {
      webSocketService.forceRefreshFilter();
      onSuccess?.('Graph refresh triggered - applying current filter settings');
      return;
    }

    if (field.action === 'reset_layout') {
      try {
        const { default: axios } = await import('axios');
        await axios.post('/api/settings/physics/reset-layout');
        onSuccess?.('Layout reset — positions re-randomized with safe physics defaults');
      } catch (err) {
        const message = (err as { response?: { data?: { error?: string } } })?.response?.data?.error;
        onError?.(message || 'Failed to reset layout');
      }
      return;
    }

    if (field.action === 'toggle-webgpu') {
      setForceWebGLOverride(isWebGPURenderer);
      window.location.reload();
      return;
    }

    if (field.action === 'toggle-demo') {
      await toggleDemo();
      setDemoActive(isDemoRunning());
      onSuccess?.(isDemoRunning() ? 'Demo agents injected — 6 synthetic agents cycling work→idle' : 'Demo agents removed');
      return;
    }

    if (field.action === 'run_clustering') {
      if (running) return;
      setRunning(true);
      try {
        const method = (localValues?.method as string) ?? 'communities';
        const params: Record<string, number> = {};
        if (method === 'kmeans') {
          params.numClusters = (localValues?.numClusters as number) ?? 8;
        } else if (method === 'dbscan') {
          params.eps = (localValues?.eps as number) ?? 5.0;
          params.minSamples = (localValues?.minSamples as number) ?? 3;
        } else {
          params.resolution = (localValues?.resolution as number) ?? 1.0;
        }
        const { default: axios } = await import('axios');
        await axios.post('/api/analytics/clustering/run', { method, params });
        onSuccess?.(`Grouping started (${method}) — colours and hulls update when the GPU run completes`);
      } catch (err) {
        const status = (err as { response?: { status?: number } })?.response?.status;
        const message = (err as { response?: { data?: { error?: string } } })?.response?.data?.error;
        onError?.(status === 401 ? 'Sign in to run grouping (authentication required)' : message || 'Failed to start grouping');
      } finally {
        setRunning(false);
      }
      return;
    }

    logger.warn('Unknown action:', field.action);
  }, [field.action, localValues, onError, onSuccess, running]);

  const label = isWebGPUToggle
    ? webgpuActive
      ? 'WebGPU Active — Click for WebGL'
      : 'WebGL Active — Click for WebGPU'
    : isDemoToggle
      ? demoActive
        ? 'Demo Running — Click to Stop'
        : 'Start Agent Demo'
      : isRunClustering && running
        ? 'Running…'
        : field.label;

  const colorClass = isWebGPUToggle
    ? webgpuActive
      ? 'bg-emerald-600 hover:bg-emerald-500 shadow-emerald-600/30'
      : 'bg-gray-600 hover:bg-gray-500 shadow-gray-600/30'
    : isDemoToggle
      ? demoActive
        ? 'bg-emerald-600 hover:bg-emerald-500 shadow-emerald-600/30'
        : 'bg-cyan-600 hover:bg-cyan-500 shadow-cyan-600/30'
      : BUTTON_STYLE[field.action ?? ''] ?? 'bg-primary hover:bg-primary/90 shadow-primary/30';

  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        id={testId}
        data-testid={testId}
        onClick={handleAction}
        disabled={disabled || (isRunClustering && running)}
        aria-label={label}
        aria-busy={isRunClustering && running}
        className={cn(
          'w-full flex items-center justify-center gap-2 py-2 px-4 rounded text-white text-xs font-semibold shadow transition-transform hover:-translate-y-px disabled:opacity-70 disabled:cursor-wait',
          colorClass
        )}
      >
        <RefreshCw size={13} className={isRunClustering && running ? 'animate-spin' : undefined} />
        {label}
      </button>
      {field.description && <p className="cc-helper-text text-center">{field.description}</p>}
    </div>
  );
};

SettingAction.displayName = 'SettingAction';

export default SettingAction;
