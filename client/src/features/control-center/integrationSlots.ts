/**
 * Integration slots — the one file that connects the menus (ADR-2189) to the
 * components the render and explore lanes deliver. Every dock or panel control
 * that depends on another lane imports from here, so a lane's API change is a
 * change to this file only.
 *
 * | Export               | Lane       | Source                                             |
 * |----------------------|------------|----------------------------------------------------|
 * | requestFit(target?)  | gv-render  | graph/hooks/useCameraAutoFit (ADR-2180)            |
 * | OverviewEdgeNotice   | gv-render  | graph/overview/OverviewEdgeNotice (ADR-2181)       |
 * | ExplorePanel         | gv-explore | graph/explore (ADR-2185)                           |
 * | GraphLegend          | gv-explore | graph/explore (ADR-2184; installs the encoder, so  |
 * |                      |            | it is mounted even while the legend is hidden)     |
 * | clearAllFilters      | gv-explore | graph/explore: clears filter:explore:* only        |
 * | useActiveFilterCount | gv-explore | graph/explore: counts every filter:* source        |
 * | HIDE_FILTERED_PATH   | gv-explore | graph/hooks/useGraphFiltering (ADR-2189 §5)        |
 *
 * Rule: never clear filters with highlightMask.clearAll(); that also wipes
 * the selection and hover. Use clearAllFilters().
 */
export { requestFit } from '../graph/hooks/useCameraAutoFit';
export type { FitTarget } from '../graph/utils/sceneFitBounds';
export { OverviewEdgeNotice } from '../graph/overview/OverviewEdgeNotice';
export { ExplorePanel, GraphLegend, clearAllFilters, useActiveFilterCount } from '../graph/explore';
export { HIDE_FILTERED_SETTING_PATH as HIDE_FILTERED_PATH } from '../graph/hooks/useGraphFiltering';
