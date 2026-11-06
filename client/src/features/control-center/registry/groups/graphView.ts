/**
 * Group 12 — Graph View (id `graphView`, hotkey 12, 19 fields).
 *
 * The ADR-2179..2186 overview contract under `visualisation.graphView.*`: fit
 * scope, screen-space overview edges, sprite / gem LOD, colour/size encoding with
 * grey-out, and the label budget. Every path routes to the opaque `visual` bucket
 * (serverBucketFor → 'visual'); there is no Rust type.
 *
 * Provisional home: the menu lane rehomes these fields during its IA redesign.
 * `overviewEdges.capacity` ('auto' | number) is deliberately NOT a control here:
 * no registry control writes a number-or-'auto' (a select writes strings), so it
 * stays a stored setting read through `resolveOverviewEdgeCapacity`.
 *
 * Option lists mirror `features/graph/explore/tableSchema.ts` (not imported, to
 * keep this module dependency-free for the ts-node manifest emitter).
 */
import type { GroupData, RegistryField } from '../types';

const G = 'visualisation.graphView.';

/** Categorical and numeric node-table columns a colour encoding accepts. */
export const COLOR_BY_OPTIONS = [
  'domain', 'source_domain', 'type', 'maturity', 'ontology_type', 'owl_type', 'cluster', 'community',
  'centrality', 'anomaly', 'quality_score', 'wikilink_count', 'file_size', 'change_count',
  'last_modified', 'last_commit',
];

/** Numeric node-table columns a size encoding accepts; 'none' keeps today's sizing. */
export const SIZE_BY_OPTIONS = [
  'none', 'centrality', 'anomaly', 'quality_score', 'wikilink_count', 'file_size', 'change_count',
];

const fields: RegistryField[] = [
  // ADR-2180
  { key: 'fitTarget', subgroup: 'Camera', label: 'Fit View Target', type: 'select', options: ['knowledge', 'graphs', 'scene'], path: `${G}fitTarget`, description: 'What Fit View frames: the knowledge body, both graphs, or the whole scene including the memory cloud' },

  // ADR-2181
  { key: 'overviewEdgesEnabled', subgroup: 'Overview Edges', label: 'Overview Edges', type: 'toggle', path: `${G}overviewEdges.enabled`, description: 'Draw every edge as a screen-space line at overview zoom' },
  { key: 'overviewEdgeWidth', subgroup: 'Overview Edges', label: 'Edge Width (px)', type: 'slider', min: 0.5, max: 4, step: 0.5, path: `${G}overviewEdges.widthPx`, description: 'Base on-screen width of an overview edge' },
  { key: 'overviewEdgeMaxWidth', subgroup: 'Overview Edges', label: 'Max Edge Width (px)', type: 'slider', min: 1, max: 8, step: 0.5, path: `${G}overviewEdges.maxWidthPx`, description: 'Width of the heaviest edge when width follows weight' },
  { key: 'overviewEdgeFadeNear', subgroup: 'Overview Edges', label: 'Fade Start (px)', type: 'slider', min: 0, max: 300, step: 5, path: `${G}overviewEdges.fadeNearPx`, description: 'Edges shorter than this on screen are fully opaque' },
  { key: 'overviewEdgeFadeFar', subgroup: 'Overview Edges', label: 'Fade End (px)', type: 'slider', min: 0, max: 600, step: 5, path: `${G}overviewEdges.fadeFarPx`, description: 'Edges longer than this on screen fade to the minimum opacity' },
  { key: 'overviewEdgeMinOpacity', subgroup: 'Overview Edges', label: 'Long-Edge Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}overviewEdges.minOpacity`, description: 'Opacity of the longest on-screen edges' },
  { key: 'glassAtCloseZoom', subgroup: 'Overview Edges', label: 'Glass Edges Up Close', type: 'toggle', path: `${G}overviewEdges.glassAtCloseZoom`, description: 'Draw glass tubes for selected, hovered and close-up edges' },

  // ADR-2182
  { key: 'spriteSize', subgroup: 'Overview Nodes', label: 'Node Dot Size (px)', type: 'slider', min: 1, max: 16, step: 0.5, path: `${G}nodes.spriteSizePx`, description: 'On-screen size of a node dot at overview zoom' },
  { key: 'gemLodDistance', subgroup: 'Overview Nodes', label: 'Gem Distance', type: 'slider', min: 0, max: 1000, step: 10, path: `${G}nodes.gemLodDistance`, description: 'Camera distance inside which nodes draw as gems' },
  { key: 'bloomScope', subgroup: 'Overview Nodes', label: 'Glow', type: 'select', options: ['emphasis', 'scene', 'off'], path: `${G}nodes.bloomScope`, description: 'Glow only selected, hovered and search-hit nodes, the whole scene, or nothing' },

  // ADR-2183 / ADR-2184
  { key: 'colorBy', subgroup: 'Explore', label: 'Colour By', type: 'select', options: COLOR_BY_OPTIONS, path: `${G}explore.colorBy`, description: 'Node attribute that sets node colour' },
  { key: 'sizeBy', subgroup: 'Explore', label: 'Size By', type: 'select', options: SIZE_BY_OPTIONS, path: `${G}explore.sizeBy`, description: 'Numeric node attribute that sets node size' },
  { key: 'showLegend', subgroup: 'Explore', label: 'Show Legend', type: 'toggle', path: `${G}explore.showLegend`, description: 'Show the legend for the active colour and size encodings' },
  { key: 'greyoutOpacity', subgroup: 'Explore', label: 'Filtered Edge Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}explore.greyoutOpacity`, description: 'Opacity of edges touching a filtered-out node' },
  { key: 'greyoutMix', subgroup: 'Explore', label: 'Filtered Node Fade', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}explore.greyoutMix`, description: 'How far filtered-out nodes fade toward the background' },

  // ADR-2186
  { key: 'labelBudget', subgroup: 'Labels', label: 'Overview Labels', type: 'slider', min: 0, max: 200, step: 5, path: `${G}labels.budget`, description: 'Most important nodes labelled at overview zoom' },
  { key: 'labelSelectedBudget', subgroup: 'Labels', label: 'Selection Labels', type: 'slider', min: 0, max: 500, step: 10, path: `${G}labels.selectedBudget`, description: 'Labels for the selection and its neighbourhood' },
  { key: 'labelFontPx', subgroup: 'Labels', label: 'Label Size (px)', type: 'slider', min: 8, max: 32, step: 1, path: `${G}labels.fontPx`, description: 'On-screen label font size' },
];

export const graphView: GroupData = {
  id: 'graphView',
  label: 'Graph View',
  description: 'Overview-first graph view: fit target, overview edges and nodes, colour/size encoding and the label budget.',
  hotkey: '12',
  loadPaths: ['visualisation.graphView'],
  fields,
};
