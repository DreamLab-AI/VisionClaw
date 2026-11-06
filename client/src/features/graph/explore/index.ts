/**
 * Explore lane public API (ADR-2183, ADR-2184, ADR-2185), for the menus lane to mount.
 *
 * Components: ExplorePanel, GraphLegend (mounting either feeds the node table;
 * GraphLegend also installs the colorBy / sizeBy encoder).
 * Functions: clearAllFilters(), useActiveFilterCount(), setHoveredNodeEmphasis().
 */
export { ExplorePanel } from './ExplorePanel';
export { GraphLegend, LegendView } from './GraphLegend';
export { clearAllFilters, exploreStore } from './exploreStore';
export { useActiveFilterCount, useExploreEncoding, useExploreTableSource } from './useExplore';
export { setHoveredNodeEmphasis, SOURCE as HIGHLIGHT_SOURCES } from './filterRegistry';
export { toArrowTable } from './nodeTable';
