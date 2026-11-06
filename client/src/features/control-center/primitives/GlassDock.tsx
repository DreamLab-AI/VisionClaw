/**
 * GlassDock — bottom-center dock shell that hosts the macro row + collapse pill.
 * design-spec.md §2, §6.1. Collapsed/expanded state is owned by the consumer
 * (WP2's useControlCenterUI) — this component is purely presentational and
 * takes `collapsed`/`onToggleCollapsed` as props.
 */

import React, { useLayoutEffect, useRef } from 'react';
import { observeResize } from './observeResize';
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

/**
 * Publish the dock's clearance (viewport bottom → dock top, plus a gap) as the
 * `--cc-dock-clearance` CSS variable, so the settings panel and the Ask box end
 * just above the dock at any viewport size or dock height (ADR-2189 §4).
 */
function useDockClearance(ref: React.RefObject<HTMLElement | null>, collapsed: boolean): void {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof window === 'undefined') return;
    const root = document.documentElement;
    const publish = () => {
      const top = el.getBoundingClientRect().top;
      const clearance = Math.max(0, Math.round(window.innerHeight - top + 8));
      root.style.setProperty('--cc-dock-clearance', `${clearance}px`);
    };
    publish();
    return observeResize(el, publish);
  }, [ref, collapsed]);
}

export const GlassDock: React.FC<GlassDockProps> = ({
  collapsed,
  onToggleCollapsed,
  children,
  className,
  id = 'control-center-dock',
}) => {
  const toggleLabel = collapsed ? 'Expand control dock' : 'Collapse control dock';
  const frameRef = useRef<HTMLDivElement>(null);
  useDockClearance(frameRef, collapsed);

  return (
    // Shared tooltip context for the dock so the collapse toggle — and any
    // dock control rendered as a child — can surface a styled cc-glass tooltip
    // on hover/focus without each mounting its own provider.
    <TooltipProvider delayDuration={300}>
      <div
        ref={frameRef}
        className={cn('cc-dock-frame cc-dock-transition', className)}
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
          // A collapsed dock is invisible: `inert` also takes its controls out of
          // the tab order and the accessibility tree (aria-hidden alone does not).
          inert={collapsed}
          className={cn(
            'cc-dock-body flex items-center gap-3 px-4 py-2 transition-[opacity,transform]',
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
