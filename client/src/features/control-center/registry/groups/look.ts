/**
 * Source group — Look & Materials (id `look`). registry/placement.ts (ADR-2189) decides which section shows each field.
 * Node/edge appearance, graph-type visuals, lighting, selection, bloom and gem
 * material — migrated from the legacy Graph + Effects tabs. Paths verbatim.
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  // Nodes
  { key: 'nodeColor', subgroup: 'Nodes', label: 'Fallback node colour', type: 'color', path: 'visualisation.graphs.knowledge.nodes.baseColor', description: 'Base color for nodes (used when colour scheme is "base")' },
  { key: 'colorScheme', subgroup: 'Nodes', label: 'Colour scheme', type: 'select', options: ['type', 'domain', 'base', 'community', 'cluster', 'centrality', 'sssp', 'cycles'], optionLabels: { type: 'Node type', domain: 'Domain', base: 'Fallback colour', community: 'Community', cluster: 'Cluster', centrality: 'Centrality', sssp: 'Graph distance (SSSP)', cycles: 'Cycles' }, path: 'visualisation.graphs.knowledge.nodes.colorScheme', description: 'How nodes are coloured: "type"/"domain"/"base" are semantic; "community" by Louvain partition, "cluster" by DBSCAN cluster, "centrality" by PageRank (blue→red ramp), "sssp" by graph distance, "cycles" lights nodes on a directed cycle (e.g. a subclass loop) and dims the rest. Analytic modes fall through to "type" for nodes the server left without that signal.' },
  { key: 'nodeSize', subgroup: 'Nodes', label: 'Node size', type: 'slider', min: 0.1, max: 1, step: 0.05, path: 'visualisation.graphs.knowledge.nodes.nodeSize', description: 'Global size gain (per-node magnitude comes from degree + content size)' },
  // Edges
  { key: 'edgeWidth', subgroup: 'Edges', label: 'Edge Thickness (close-up tubes)', type: 'slider', min: 0.02, max: 0.5, step: 0.01, path: 'visualisation.graphs.knowledge.edges.baseWidth', description: 'Cylinder radius of edges (1:1 — the slider value is the tube radius)' },
  { key: 'edgeOpacity', subgroup: 'Edges', label: 'Edge Opacity (all edges)', type: 'slider', min: 0, max: 0.3, step: 0.005, path: 'visualisation.graphs.knowledge.edges.opacity', description: 'Transparency of every edge: the overview lines and the close-up glass tubes. Dense graphs overlap many edges, so values above ~0.2 read as solid; the useful range lives at the bottom.' },
  // Graph-Type Visuals
  { key: 'kgEdgeColor', subgroup: 'Edges', label: 'Edge colour: knowledge graph mode', type: 'color', path: 'visualisation.graphTypeVisuals.knowledgeGraph.edgeColor', description: 'The one edge colour for knowledge-graph mode. Shows when edges are not coloured by relationship type' },
  { key: 'ontologyEdgeColor', subgroup: 'Edges', label: 'Edge colour: ontology mode', type: 'color', path: 'visualisation.graphTypeVisuals.ontology.edgeColor', description: 'The one edge colour for ontology mode. Shows when edges are not coloured by relationship type' },
  { key: 'agentEdgeColor', subgroup: 'Edges', label: 'Edge colour: agent mode', type: 'color', path: 'visualisation.graphTypeVisuals.agent.edgeColor', description: 'The one edge colour for agent mode (was the generic Edge colour). Leave unset to colour edges by relationship type' },
  // Lighting & Rendering
  { key: 'ambientLight', subgroup: 'Lighting & Rendering', label: 'Ambient Light', type: 'slider', min: 0, max: 2, step: 0.1, path: 'visualisation.rendering.ambientLightIntensity', description: 'Overall scene brightness', macro: 'luminosity' },
  { key: 'directionalLight', subgroup: 'Lighting & Rendering', label: 'Direct Light', type: 'slider', min: 0, max: 2, step: 0.1, path: 'visualisation.rendering.directionalLightIntensity', description: 'Directional light intensity' },
  { key: 'maxEdgesCeiling', subgroup: 'Lighting & Rendering', label: 'Edge memory budget', type: 'slider', min: 1024, max: 262144, step: 1024, path: 'visualisation.rendering.maxEdgesCeiling', description: 'Hard cap on dynamically-grown edge instance capacity (Phase 6)' },
  { key: 'softwareFallback', subgroup: 'Lighting & Rendering', label: 'Software WebGL Fallback', type: 'select', options: ['auto', 'force-on', 'force-off'], path: 'visualisation.rendering.softwareFallback', description: 'Behaviour on software-rendered WebGL contexts (SwiftShader/llvmpipe)' },
  // Selection
  { key: 'selectionHighlightColor', subgroup: 'Selection', label: 'Selection Color', type: 'color', path: 'visualisation.interaction.selectionHighlightColor', description: 'Edge color when node is selected' },
  // Bloom / Glow
  { key: 'glow', subgroup: 'Bloom / Glow', label: 'Emphasis glow', type: 'toggle', path: 'visualisation.glow.enabled', description: 'Enable bloom post-processing', },
  { key: 'glowIntensity', subgroup: 'Bloom / Glow', label: 'Glow Intensity', type: 'slider', min: 0, max: 1.5, step: 0.05, path: 'visualisation.glow.intensity', description: 'Brightness of bloom glow', macro: 'luminosity' },
  { key: 'glowRadius', subgroup: 'Bloom / Glow', label: 'Glow Radius', type: 'slider', min: 0, max: 1.0, step: 0.05, path: 'visualisation.glow.radius', description: 'Size of glow spread' },
  { key: 'glowThreshold', subgroup: 'Bloom / Glow', label: 'Glow Threshold', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.glow.threshold', description: 'Minimum brightness for glow' },
  // Gem Material
  { key: 'gemIor', subgroup: 'Gem Material', label: 'Gem IOR', type: 'slider', min: 1.0, max: 3.0, step: 0.01, path: 'visualisation.gemMaterial.ior', description: 'Index of refraction for gem nodes' },
  { key: 'gemTransmission', subgroup: 'Gem Material', label: 'Gem Transmission', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.gemMaterial.transmission', description: 'Light transmission through gems' },
  { key: 'gemClearcoat', subgroup: 'Gem Material', label: 'Gem Clearcoat', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.gemMaterial.clearcoat', description: 'Clearcoat intensity on gems' },
  { key: 'gemClearcoatRoughness', subgroup: 'Gem Material', label: 'Clearcoat Rough', type: 'slider', min: 0, max: 0.5, step: 0.01, path: 'visualisation.gemMaterial.clearcoatRoughness', description: 'Clearcoat roughness' },
  { key: 'gemEmissiveIntensity', subgroup: 'Gem Material', label: 'Gem Emissive', type: 'slider', min: 0, max: 2, step: 0.05, path: 'visualisation.gemMaterial.emissiveIntensity', description: 'Emissive glow intensity of gems' },
  { key: 'gemIridescence', subgroup: 'Gem Material', label: 'Gem Iridescence', type: 'slider', min: 0, max: 1, step: 0.05, path: 'visualisation.gemMaterial.iridescence', description: 'Rainbow sheen intensity' },
];

export const look: GroupData = {
  id: 'look',
  label: 'Look & Materials',
  description: 'Node and edge appearance, lighting, bloom, and physically-based gem materials.',
  loadPaths: [
    'visualisation.graphs.knowledge.nodes',
    'visualisation.graphs.knowledge.edges',
    'visualisation.graphTypeVisuals',
    'visualisation.rendering',
    'visualisation.glow',
    'visualisation.gemMaterial',
    'visualisation.interaction',
  ],
  fields,
};
