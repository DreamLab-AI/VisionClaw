/**
 * Source group — Graph View (id `graphView`, 20 fields). ADR-2189 places its fields
 * across Explore › Camera, Encode, and Look (registry/placement.ts).
 *
 * The ADR-2179..2186 overview contract under `visualisation.graphView.*`: fit
 * scope, screen-space overview edges, sprite / gem LOD, colour/size encoding with
 * grey-out, and the label budget. Every path routes to the opaque `visual` bucket
 * (serverBucketFor → 'visual'); there is no Rust type.
 *
 * `overviewEdges.capacity` ('auto' | number) is a select of named budgets. A
 * select writes strings, and `normaliseEdgeCapacity` accepts a numeric string and
 * reads any other string as 'auto', so the stored value stays 'auto' | number.
 *
 * Option lists mirror `features/graph/explore/tableSchema.ts` (not imported, to
 * keep this module dependency-free for the ts-node manifest emitter).
 */
import type { GroupData, RegistryField } from '../types';

const G = 'visualisation.graphView.';

/** 'scheme' (today's look under Node Colour Scheme), then the columns a colour encoding accepts. */
export const COLOR_BY_OPTIONS = [
  'scheme', 'domain', 'source_domain', 'type', 'maturity', 'ontology_type', 'owl_type', 'cluster', 'community',
  'centrality', 'anomaly', 'quality_score', 'wikilink_count', 'file_size', 'change_count',
  'last_modified', 'last_commit',
];

/** Numeric node-table columns a size encoding accepts; 'none' keeps today's sizing. */
export const SIZE_BY_OPTIONS = [
  'none', 'centrality', 'anomaly', 'quality_score', 'wikilink_count', 'file_size', 'change_count',
];

const fields: RegistryField[] = [
  // ADR-2180
  { key: 'fitTarget', subgroup: 'Camera', label: 'Fit View Target', type: 'select', options: ['knowledge', 'graphs', 'scene'], optionLabels: { knowledge: 'Knowledge body', graphs: 'Both graphs', scene: 'Whole scene' }, path: `${G}fitTarget`, description: 'What Fit View frames: the knowledge body, both graphs, or the whole scene including the memory cloud' },

  // ADR-2181
  { key: 'overviewEdgesEnabled', subgroup: 'Overview Edges', label: 'Overview Edges', type: 'toggle', path: `${G}overviewEdges.enabled`, description: 'Draw every edge as a screen-space line at overview zoom' },
  { key: 'overviewEdgeWidth', subgroup: 'Overview Edges', label: 'Edge Width (px)', type: 'slider', min: 0.5, max: 4, step: 0.5, path: `${G}overviewEdges.widthPx`, description: 'Base on-screen width of an overview edge' },
  { key: 'overviewEdgeMaxWidth', subgroup: 'Overview Edges', label: 'Max Edge Width (px)', type: 'slider', min: 1, max: 8, step: 0.5, path: `${G}overviewEdges.maxWidthPx`, description: 'Width of the heaviest edge when width follows weight' },
  { key: 'overviewEdgeFadeNear', subgroup: 'Overview Edges', label: 'Fade Start (px)', type: 'slider', min: 0, max: 300, step: 5, path: `${G}overviewEdges.fadeNearPx`, description: 'Edges shorter than this on screen are fully opaque' },
  { key: 'overviewEdgeFadeFar', subgroup: 'Overview Edges', label: 'Fade End (px)', type: 'slider', min: 0, max: 600, step: 5, path: `${G}overviewEdges.fadeFarPx`, description: 'Edges longer than this on screen fade to the minimum opacity' },
  { key: 'overviewEdgeMinOpacity', subgroup: 'Overview Edges', label: 'Long-Edge Fade Floor', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}overviewEdges.minOpacity`, description: 'How far the longest on-screen edges fade, as a fraction of Edge Opacity (1 = no fade; edges between Fade Start and Fade End ramp down to it). It does not change short edges: use Edge Opacity for that.' },
  { key: 'glassAtCloseZoom', subgroup: 'Overview Edges', label: 'Glass Edges Up Close', type: 'toggle', path: `${G}overviewEdges.glassAtCloseZoom`, description: 'Draw glass tubes for selected, hovered and close-up edges' },
  // ADR-2181 D4. The desktop twin of the headset's per-volume edge limit (Edges ±): when
  // the budget is short the heaviest edges are kept and the canvas shows "N of M drawn".
  { key: 'overviewEdgeBudget', subgroup: 'Overview Edges', label: 'Edge Budget', type: 'select', options: ['auto', '5000', '10000', '20000', '40000', '80000'], optionLabels: { auto: 'All edges', '5000': '5,000', '10000': '10,000', '20000': '20,000 (headset)', '40000': '40,000', '80000': '80,000' }, path: `${G}overviewEdges.capacity`, description: 'How many overview edges to draw at most. Below the total, the heaviest edges are kept and the canvas shows how many were dropped. 20,000 matches the headset.' },

  // ADR-2182
  { key: 'spriteSize', subgroup: 'Overview Nodes', label: 'Node Dot Size (px)', type: 'slider', min: 1, max: 16, step: 0.5, path: `${G}nodes.spriteSizePx`, description: 'On-screen size of a node dot at overview zoom' },
  { key: 'gemLodDistance', subgroup: 'Overview Nodes', label: 'Gem Distance', type: 'slider', min: 0, max: 1000, step: 10, path: `${G}nodes.gemLodDistance`, description: 'Camera distance inside which nodes draw as gems' },
  { key: 'bloomScope', subgroup: 'Overview Nodes', label: 'Glow scope', type: 'select', options: ['emphasis', 'scene', 'off'], optionLabels: { emphasis: 'Emphasised nodes only', scene: 'Whole scene', off: 'Off' }, path: `${G}nodes.bloomScope`, description: 'Glow only selected, hovered and search-hit nodes, the whole scene, or nothing' },

  // ADR-2183 / ADR-2184
  { key: 'colorBy', subgroup: 'Explore', label: 'Colour By', type: 'select', options: COLOR_BY_OPTIONS, path: `${G}explore.colorBy`, description: "'scheme' keeps the node colour scheme; a column colours by that attribute" },
  { key: 'sizeBy', subgroup: 'Explore', label: 'Size By', type: 'select', options: SIZE_BY_OPTIONS, path: `${G}explore.sizeBy`, description: 'Numeric node attribute that sets node size' },
  { key: 'showLegend', subgroup: 'Explore', label: 'Show Legend', type: 'toggle', path: `${G}explore.showLegend`, description: 'Show the legend for the active colour and size encodings' },
  { key: 'greyoutOpacity', subgroup: 'Explore', label: 'Filtered Edge Opacity', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}explore.greyoutOpacity`, description: 'Opacity of edges touching a filtered-out node' },
  // ADR-2189 §5 / ADR-2183: filters grey out by default; this keeps the old removal for those who want it.
  { key: 'hideFiltered', subgroup: 'Explore', label: 'Hide filtered nodes', type: 'toggle', path: `${G}explore.hideFiltered`, description: 'Remove filtered-out nodes from the view instead of greying them out' },
  { key: 'greyoutMix', subgroup: 'Explore', label: 'Filtered Node Fade', type: 'slider', min: 0, max: 1, step: 0.05, path: `${G}explore.greyoutMix`, description: 'How far filtered-out nodes fade toward the background' },

  // ADR-2186
  { key: 'labelBudget', subgroup: 'Labels', label: 'Overview Labels', type: 'slider', min: 0, max: 200, step: 5, path: `${G}labels.budget`, description: 'Most important nodes labelled at overview zoom', macro: 'focus' },
  { key: 'labelDynamicBudget', subgroup: 'Labels', label: 'Viewport Labels', type: 'slider', min: 0, max: 200, step: 5, path: `${G}labels.dynamicBudget`, description: 'Important nodes labelled within the current view' },
  { key: 'labelSelectedBudget', subgroup: 'Labels', label: 'Selection Labels', type: 'slider', min: 0, max: 500, step: 10, path: `${G}labels.selectedBudget`, description: 'Labels for the selection and its neighbourhood' },
  { key: 'labelFontPx', subgroup: 'Labels', label: 'Label Size (px)', type: 'slider', min: 8, max: 32, step: 1, path: `${G}labels.fontPx`, description: 'On-screen label font size' },
];

export const graphView: GroupData = {
  id: 'graphView',
  label: 'Graph View',
  description: 'Overview-first graph view: fit target, overview edges and nodes, colour/size encoding and the label budget.',
  loadPaths: ['visualisation.graphView'],
  fields,
};
