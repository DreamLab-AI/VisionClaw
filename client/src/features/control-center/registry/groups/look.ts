/**
 * Group 2 — Look & Materials (id `look`, hotkey 2, 29 fields).
 * Node/edge appearance, graph-type visuals, lighting, selection, bloom and gem
 * material — migrated from the legacy Graph + Effects tabs. Paths verbatim.
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  // Nodes
  { key: 'nodeColor', subgroup: 'Nodes', label: 'Node Color', type: 'color', path: 'visualisation.graphs.knowledge.nodes.baseColor', description: 'Base color for nodes (used when colour scheme is "base")' },
  { key: 'colorScheme', subgroup: 'Nodes', label: 'Node colour by', type: 'select', options: ['type', 'domain', 'base', 'community', 'cluster', 'centrality', 'sssp', 'cycles'], path: 'visualisation.graphs.knowledge.nodes.colorScheme', description: 'How nodes are coloured: "type"/"domain"/"base" are semantic; "community" by Louvain partition, "cluster" by DBSCAN cluster, "centrality" by PageRank (blue→red ramp), "sssp" by graph distance, "cycles" lights nodes on a directed cycle (e.g. a subclass loop) and dims the rest. Analytic modes fall through to "type" for nodes the server left without that signal.' },
  { key: 'sizeScheme', subgroup: 'Nodes', label: 'Node size by', type: 'select', options: ['degree', 'fileSize', 'hybrid'], path: 'visualisation.graphs.knowledge.nodes.sizeScheme', description: 'How nodes are sized: "degree" by connection count, "fileSize" by content byte-size, "hybrid" combines both' },
  { key: 'nodeSize', subgroup: 'Nodes', label: 'Node Size', type: 'slider', min: 0.1, max: 1, step: 0.05, path: 'visualisation.graphs.knowledge.nodes.nodeSize', description: 'Global size gain (per-node magnitude comes from degree + content size)' },
  { key: 'perNodeGlow', subgroup: 'Nodes', label: 'Per-node glow (authority/degree)', type: 'toggle', path: 'visualisation.graphs.knowledge.nodes.perNodeGlow', description: 'When on, per-node emissive (from the metadata texture) drives glow; when off, nodes use a uniform glow' },
  { key: 'enableMetadataShape', subgroup: 'Nodes', label: 'Metadata Shape', type: 'toggle', path: 'visualisation.graphs.knowledge.nodes.enableMetadataShape', description: 'Shape based on metadata' },
  // Edges
  { key: 'edgeColor', subgroup: 'Edges', label: 'Edge Color', type: 'color', path: 'visualisation.graphs.knowledge.edges.color', description: 'Base color for edges' },
  { key: 'edgeWidth', subgroup: 'Edges', label: 'Edge Thickness', type: 'slider', min: 0.02, max: 0.5, step: 0.01, path: 'visualisation.graphs.knowledge.edges.baseWidth', description: 'Cylinder radius of edges (1:1 — the slider value is the tube radius)' },
  { key: 'edgeOpacity', subgroup: 'Edges', label: 'Edge Opacity', type: 'slider', min: 0, max: 0.3, step: 0.005, path: 'visualisation.graphs.knowledge.edges.opacity', description: 'Per-edge alpha. Dense graphs overlap many edges, so values above ~0.2 read as solid — the useful range lives at the bottom.' },
  { key: 'colorByType', subgroup: 'Edges', label: 'Colour edges by relationship type', type: 'toggle', path: 'visualisation.graphs.knowledge.edges.colorByType', description: 'Colour each edge by its relationship type (11 edge types) instead of the single base colour above' },
  { key: 'widthByWeight', subgroup: 'Edges', label: 'Edge width by weight', type: 'toggle', path: 'visualisation.graphs.knowledge.edges.widthByWeight', description: 'Scale edge width by edge weight instead of using a uniform base width' },
  // Graph-Type Visuals
  { key: 'kgEdgeColor', subgroup: 'Graph-Type Visuals', label: 'KG Edge Color', type: 'color', path: 'visualisation.graphTypeVisuals.knowledgeGraph.edgeColor', description: 'Edge color for knowledge graph mode' },
  { key: 'ontologyEdgeColor', subgroup: 'Graph-Type Visuals', label: 'Ontology Edge Color', type: 'color', path: 'visualisation.graphTypeVisuals.ontology.edgeColor', description: 'Edge color for ontology mode' },
  { key: 'ringTintByClass', subgroup: 'Graph-Type Visuals', label: 'Tint ontology rings by class', type: 'toggle', path: 'visualisation.graphTypeVisuals.ontology.ringTintByClass', description: 'Tint each ontology node\'s orbital rings by its class instead of a uniform ring colour' },
  // Lighting & Rendering
  { key: 'ambientLight', subgroup: 'Lighting & Rendering', label: 'Ambient Light', type: 'slider', min: 0, max: 2, step: 0.1, path: 'visualisation.rendering.ambientLightIntensity', description: 'Overall scene brightness', macro: 'luminosity' },
  { key: 'directionalLight', subgroup: 'Lighting & Rendering', label: 'Direct Light', type: 'slider', min: 0, max: 2, step: 0.1, path: 'visualisation.rendering.directionalLightIntensity', description: 'Directional light intensity' },
  { key: 'maxEdgesCeiling', subgroup: 'Lighting & Rendering', label: 'Max Edges Ceiling', type: 'slider', min: 1024, max: 262144, step: 1024, path: 'visualisation.rendering.maxEdgesCeiling', description: 'Hard cap on dynamically-grown edge instance capacity (Phase 6)' },
  { key: 'softwareFallback', subgroup: 'Lighting & Rendering', label: 'Software WebGL Fallback', type: 'select', options: ['auto', 'force-on', 'force-off'], path: 'visualisation.rendering.softwareFallback', description: 'Behaviour on software-rendered WebGL contexts (SwiftShader/llvmpipe)' },
  // Selection
  { key: 'selectionHighlightColor', subgroup: 'Selection', label: 'Selection Color', type: 'color', path: 'visualisation.interaction.selectionHighlightColor', description: 'Edge color when node is selected' },
  // Bloom / Glow
  { key: 'glow', subgroup: 'Bloom / Glow', label: 'Bloom Glow', type: 'toggle', path: 'visualisation.glow.enabled', description: 'Enable bloom post-processing', macro: 'luminosity' },
  { key: 'glowIntensity', subgroup: 'Bloom / Glow', label: 'Glow Intensity', type: 'slider', min: 0, max: 1.5, step: 0.05, path: 'visualisation.glow.intensity', description: 'Brightness of bloom glow', macro: 'luminosity' },
  { key: 'glowRadius', subgroup: 'Bloom / Glow', label: 'Glow Radius', type: 'slider', min: 0, max: 1.0, step: 0.05, path: 'visualisation.glow.radius', description: 'Size of glow spread' },
  { key: 'glowThreshold', subgroup: 'Bloom / Glow', label: 'Glow Threshold', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.glow.threshold', description: 'Minimum brightness for glow' },
  // Gem Material
  { key: 'gemIor', subgroup: 'Gem Material', label: 'Gem IOR', type: 'slider', min: 1.0, max: 3.0, step: 0.01, path: 'visualisation.gemMaterial.ior', description: 'Index of refraction for gem nodes' },
  { key: 'gemTransmission', subgroup: 'Gem Material', label: 'Gem Transmission', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.gemMaterial.transmission', description: 'Light transmission through gems' },
  { key: 'gemClearcoat', subgroup: 'Gem Material', label: 'Gem Clearcoat', type: 'slider', min: 0, max: 1, step: 0.01, path: 'visualisation.gemMaterial.clearcoat', description: 'Clearcoat intensity on gems' },
  { key: 'gemClearcoatRoughness', subgroup: 'Gem Material', label: 'Clearcoat Rough', type: 'slider', min: 0, max: 0.5, step: 0.01, path: 'visualisation.gemMaterial.clearcoatRoughness', description: 'Clearcoat roughness' },
  { key: 'gemEmissiveIntensity', subgroup: 'Gem Material', label: 'Gem Emissive', type: 'slider', min: 0, max: 2, step: 0.05, path: 'visualisation.gemMaterial.emissiveIntensity', description: 'Emissive glow intensity of gems', macro: 'luminosity' },
  { key: 'gemIridescence', subgroup: 'Gem Material', label: 'Gem Iridescence', type: 'slider', min: 0, max: 1, step: 0.05, path: 'visualisation.gemMaterial.iridescence', description: 'Rainbow sheen intensity' },
];

export const look: GroupData = {
  id: 'look',
  label: 'Look & Materials',
  description: 'Node and edge appearance, lighting, bloom, and physically-based gem materials.',
  hotkey: '2',
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
