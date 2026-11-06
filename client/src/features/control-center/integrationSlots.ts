/**
 * Integration slots — the ONE file that connects the menus (ADR-2189) to the
 * components the render and explore lanes deliver. Integrating a lane is a
 * change to this file only: replace a fallback below with a re-export.
 *
 * Rule: no slot may produce a dead control. Each export is either a working
 * fallback (today's behaviour) or `null`, and the dock HIDES the control whose
 * slot is `null`.
 *
 * | Slot                   | Lane       | Until the lane lands                                   |
 * |------------------------|------------|--------------------------------------------------------|
 * | requestFit(target)     | gv-render  | today's whole-graph fit (CAMERA_FIT_EVENT)             |
 * | FIT_TARGETS_SUPPORTED  | gv-render  | false: the Fit button offers no scope menu             |
 * | useEdgeCapacityNotice  | gv-render  | returns null: no notice                                |
 * | ExplorePanel           | gv-explore | null: the Explore button opens Settings › Explore      |
 * | GraphLegend            | gv-explore | null: the Legend button is hidden                      |
 * | clearAllFilters        | gv-explore | null: no "clear filters" affordance                    |
 * | useActiveFilterCount   | gv-explore | returns 0: no badge                                    |
 * | HIDE_FILTERED_PATH     | gv-explore | null: Explore › Filters shows no "Hide filtered" switch |
 */
import type React from 'react';
import { CAMERA_FIT_EVENT } from '../graph/hooks/useCameraAutoFit';

/** ADR-2180 fit targets; the same values as `visualisation.graphView.fitTarget`. */
export type FitTarget = 'knowledge' | 'graphs' | 'scene';

/**
 * Frame the camera on `target` (ADR-2180).
 * Fallback: today's fit, which frames the node bounding box; the target rides in
 * the event detail so a listener that understands it can use it.
 */
export function requestFit(target: FitTarget): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(CAMERA_FIT_EVENT, { detail: { target } }));
}

/** True once `requestFit` honours its target, which enables the Fit scope menu. */
export const FIT_TARGETS_SUPPORTED = false;

/** ADR-2181: edges dropped because the overview edge capacity was reached. */
export interface EdgeCapacityNotice {
  /** edges drawn */
  shown: number;
  /** edges in the graph */
  total: number;
}

/** Null when every edge is drawn (or until gv-render supplies the hook). */
export function useEdgeCapacityNotice(): EdgeCapacityNotice | null {
  return null;
}

/** ADR-2185 explore panel (search, timeline, histograms, category bars, filters). */
export const ExplorePanel: React.ComponentType<{ onClose?: () => void }> | null = null;

/** ADR-2184 legend for the active colour and size encodings. */
export const GraphLegend: React.ComponentType | null = null;

/** ADR-2185: clear every highlight-mask filter clause. */
export const clearAllFilters: (() => void) | null = null;

/** ADR-2185: number of active filter clauses (badge on the Explore button). */
export function useActiveFilterCount(): number {
  return 0;
}

/** ADR-2189 §5: path of the "Hide filtered nodes" boolean, owned by gv-explore. */
export const HIDE_FILTERED_PATH: string | null = null;
