/**
 * GlassDock — bottom-center dock shell that hosts the macro row + collapse pill.
 * design-spec.md §2, §6.1. Collapsed/expanded state is owned by the consumer
 * (WP2's useControlCenterUI) — this component is purely presentational and
 * takes `collapsed`/`onToggleCollapsed` as props.
 */

import React from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { cn } from '../../../utils/classNameUtils';
import { GlassPanel } from './GlassPanel';
import { Tooltip, TooltipProvider } from '../../design-system/components/Tooltip';
import '../styles/control-center.css';

export interface GlassDockProps {
  /** Whether the dock is collapsed to a single pill (Cmd/Ctrl+. hero mode). */
  collapsed: boolean;
  onToggleCollapsed: () => void;
  children: React.ReactNode;
  className?: string;
  /** id of the dock body, referenced by the collapse toggle's aria-controls */
  id?: string;
}

export const GlassDock: React.FC<GlassDockProps> = ({
  collapsed,
  onToggleCollapsed,
  children,
  className,
  id = 'control-center-dock',
}) => {
  const toggleLabel = collapsed ? 'Expand control dock' : 'Collapse control dock';

  return (
    // Shared tooltip context for the dock so the collapse toggle — and any
    // dock control rendered as a child — can surface a styled cc-glass tooltip
    // on hover/focus without each mounting its own provider.
    <TooltipProvider delayDuration={300}>
      <div
        className={cn('cc-dock-transition', 'fixed bottom-6 left-1/2 -translate-x-1/2 z-40 flex flex-col items-center gap-1', className)}
        data-testid="control-center-dock"
      >
        <GlassPanel
          dockRadius
          animated
          id={id}
          role="toolbar"
          aria-label="Control dock"
          data-scene-occluder={collapsed ? undefined : ''}
          aria-hidden={collapsed}
          className={cn(
            'flex items-center gap-3 px-4 py-2 transition-[opacity,transform]',
            collapsed && 'pointer-events-none opacity-0 scale-95 translate-y-2 absolute'
          )}
        >
          {children}
        </GlassPanel>

        <Tooltip content={toggleLabel}>
          <button
            type="button"
            data-testid="control-center-dock-collapse-toggle"
            aria-expanded={!collapsed}
            aria-controls={id}
            aria-label={toggleLabel}
            onClick={onToggleCollapsed}
            className={cn(
              'cc-glass cc-dock-transition flex items-center justify-center h-7 w-7 rounded-full',
              'text-muted-foreground hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'
            )}
          >
            {collapsed ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>
        </Tooltip>
      </div>
    </TooltipProvider>
  );
};

GlassDock.displayName = 'GlassDock';

export default GlassDock;
