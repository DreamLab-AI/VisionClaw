/**
 * Source group — Filtering & Quality (id `quality`). registry/placement.ts (ADR-2189) decides which section shows each field.
 * Merges node-type visibility (Graph tab), node filtering + GPU quality gates
 * (Quality tab), and the transient Run Grouping + Cluster Hulls (Analytics tab).
 * Run Grouping fields are localKey (transient task inputs), not settings paths;
 * their showWhen rules are preserved verbatim.
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  // Node Types
  { key: 'showKnowledge', subgroup: 'Node Types', label: 'Knowledge Nodes', type: 'toggle', path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.knowledge', description: 'Show knowledge graph nodes' },
  { key: 'showOntology', subgroup: 'Node Types', label: 'Ontology Nodes', type: 'toggle', path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.ontology', description: 'Show ontology nodes' },
  { key: 'showAgents', subgroup: 'Node Types', label: 'Agent Nodes', type: 'toggle', path: 'visualisation.graphs.knowledge.nodes.nodeTypeVisibility.agent', description: 'Show agent nodes' },
  // Node Filtering
  { key: 'filterEnabled', subgroup: 'Node Filtering', label: 'Filter on server (hides nodes for every viewer)', type: 'toggle', path: 'nodeFilter.enabled', description: 'Apply these filters on the server: non-matching nodes are removed from the graph every viewer loads. Highlight-only filtering arrives with the Explore panel (ADR-2183).' },
  { key: 'includeLinkedPages', subgroup: 'Node Filtering', label: 'Include Linked Pages', type: 'toggle', path: 'nodeFilter.includeLinkedPages', description: 'Show wikilink stub nodes (32K linked_page nodes). Disable for highest-quality view showing only fully-authored pages.' },
  { key: 'filterByQuality', subgroup: 'Node Filtering', label: 'Filter by Quality', type: 'toggle', path: 'nodeFilter.filterByQuality', description: 'Use quality score for filtering' },
  { key: 'qualityThreshold', subgroup: 'Node Filtering', label: 'Quality Threshold', type: 'slider', min: 0, max: 1, step: 0.05, path: 'nodeFilter.qualityThreshold', description: 'Minimum quality score (0-1)' },
  { key: 'filterByAuthority', subgroup: 'Node Filtering', label: 'Filter by Authority', type: 'toggle', path: 'nodeFilter.filterByAuthority', description: 'Use authority score for filtering' },
  { key: 'authorityThreshold', subgroup: 'Node Filtering', label: 'Authority Threshold', type: 'slider', min: 0, max: 1, step: 0.05, path: 'nodeFilter.authorityThreshold', description: 'Minimum authority score (0-1)' },
  { key: 'filterMode', subgroup: 'Node Filtering', label: 'Filter Mode', type: 'select', options: ['or', 'and'], path: 'nodeFilter.filterMode', description: 'How to combine filters (and = both, or = either)' },
  { key: 'minMaturity', subgroup: 'Node Filtering', label: 'Min Ontology Maturity', type: 'select', options: ['off', 'draft', 'developing', 'emerging', 'growing', 'established', 'mature'], path: 'nodeFilter.minMaturity', description: 'Hide ontology nodes below this OWL maturity tier (the real per-node quality signal). Knowledge pages are unaffected. Applied independently of the quality/authority filter mode.' },
  { key: 'minConnections', subgroup: 'Node Filtering', label: 'Min Connections (degree)', type: 'slider', min: 0, max: 20, step: 1, path: 'nodeFilter.minConnections', description: 'Hide nodes whose graph degree is below this (0 = off). Suppresses orphan/low-degree spray.' },
  { key: 'refreshGraph', subgroup: 'Node Filtering', label: 'Refresh Graph', type: 'action-button', action: 'refresh_graph', description: 'Apply filter changes and reload graph' },
  // GPU & Quality Gates
  { key: 'autoAdjust', subgroup: 'GPU & Quality Gates', label: 'Auto-Adjust Quality', type: 'toggle', path: 'qualityGates.autoAdjust', description: 'Automatic quality scaling' },
  { key: 'minFpsThreshold', subgroup: 'GPU & Quality Gates', label: 'Min FPS Threshold', type: 'slider', min: 15, max: 60, step: 5, path: 'qualityGates.minFpsThreshold', description: 'Minimum acceptable FPS' },
  // step-grid: min 0 (not 1000) so the 500000 default lands exactly on the grid (100*5000) and stays reachable — see defect-3.
  { key: 'ruvectorEnabled', subgroup: 'GPU & Quality Gates', label: 'RuVector Integration', type: 'toggle', path: 'qualityGates.ruvectorEnabled', description: 'HNSW similarity search' },
  // Cluster Visualisation
  { key: 'showClusters', subgroup: 'Cluster Visualisation', label: 'Show Clusters', type: 'toggle', path: 'qualityGates.showClusters', description: 'Color-coded node groups' },
  { key: 'showAnomalies', subgroup: 'Cluster Visualisation', label: 'Show Anomalies', type: 'toggle', path: 'qualityGates.showAnomalies', description: 'Highlight outliers' },
  { key: 'showCommunities', subgroup: 'Cluster Visualisation', label: 'Show Communities', type: 'toggle', path: 'qualityGates.showCommunities', description: 'Louvain communities' },
  // Run Grouping (transient — localKey, not a settings path)
  // `default` seeds SettingsPanel's transient local map so the Method select shows a
  // real value (not just a placeholder) and its showWhen-dependent sliders render — see defect-1.
  { key: 'groupingMethod', subgroup: 'Run Grouping', label: 'Method', type: 'select', localKey: 'method', default: 'communities', options: ['communities', 'kmeans', 'dbscan'], description: 'How to group nodes: "communities" = topological community detection (Leiden) coloured by partition; "kmeans" = spatial K-means into a fixed number of clusters; "dbscan" = density-based spatial clustering (auto cluster count, marks outliers).' },
  { key: 'groupingNumClusters', subgroup: 'Run Grouping', label: 'Cluster Count (K)', type: 'slider', localKey: 'numClusters', default: 8, min: 2, max: 50, step: 1, showWhen: { localKey: 'method', equals: 'kmeans' }, description: 'Number of spatial clusters for K-means (K).' },
  { key: 'groupingEps', subgroup: 'Run Grouping', label: 'Neighbourhood (eps)', type: 'slider', localKey: 'eps', default: 5, min: 0.1, max: 10, step: 0.1, showWhen: { localKey: 'method', equals: 'dbscan' }, description: 'DBSCAN neighbourhood radius — larger merges more nodes into each cluster (backend range 0.1–10).' },
  { key: 'groupingMinSamples', subgroup: 'Run Grouping', label: 'Min Points', type: 'slider', localKey: 'minSamples', default: 3, min: 1, max: 50, step: 1, showWhen: { localKey: 'method', equals: 'dbscan' }, description: 'DBSCAN minimum points to form a dense region — higher leaves more nodes as outliers.' },
  { key: 'groupingResolution', subgroup: 'Run Grouping', label: 'Resolution', type: 'slider', localKey: 'resolution', default: 1, min: 0.1, max: 5, step: 0.1, showWhen: { localKey: 'method', equals: 'communities' }, description: 'Community-detection resolution — higher yields more, smaller communities (backend range 0.1–10). Independent of the Physics-tab cohesion resolution.' },
  { key: 'runGrouping', subgroup: 'Run Grouping', label: 'Run Grouping', type: 'action-button', action: 'run_clustering', description: 'Run the selected grouping on the GPU. Colours and hulls update when the task completes (a few seconds). Requires sign-in.' },
  // Cluster Hulls
  { key: 'clusterHulls', subgroup: 'Cluster Hulls', label: 'Cluster Hulls', type: 'toggle', path: 'visualisation.clusterHulls.enabled', description: 'Draw a translucent convex hull around each server-provided cluster (DBSCAN) or community (Louvain) group' },
  { key: 'clusterHullOpacity', subgroup: 'Cluster Hulls', label: 'Hull Opacity', type: 'slider', min: 0, max: 0.5, step: 0.01, path: 'visualisation.clusterHulls.opacity', description: 'Translucency of cluster hull volumes (default 0.25)' },
  { key: 'clusterHullMax', subgroup: 'Cluster Hulls', label: 'Max Hulls', type: 'slider', min: 1, max: 64, step: 1, path: 'visualisation.clusterHulls.maxHulls', description: 'Cap on the number of hulls drawn — the N largest groups are kept so dense graphs stay legible (default 32)' },
  { key: 'clusterHullCommunityFallback', subgroup: 'Cluster Hulls', label: 'Community Hull Fallback', type: 'toggle', path: 'visualisation.clusterHulls.communityFallback', description: 'When the server provides no DBSCAN clusters, draw hulls around Louvain communities. Off by default — communities optimise modularity not spatial locality, so their hulls overlap; the cleaner community signal is "Node colour by → community".' },
  { key: 'clusterHullSpatialFallback', subgroup: 'Cluster Hulls', label: 'Spatial Hull Fallback', type: 'toggle', path: 'visualisation.clusterHulls.spatialFallback', description: 'When the server provides no cluster or community structure, fabricate hulls from spatial proximity instead of showing none' },
];

export const quality: GroupData = {
  id: 'quality',
  label: 'Filtering & Quality',
  description: 'Node-type visibility, quality/authority filters, GPU quality gates, grouping and hulls.',
  loadPaths: [
    'visualisation.graphs.knowledge.nodes.nodeTypeVisibility',
    'nodeFilter',
    'qualityGates',
    'visualisation.clusterHulls',
  ],
  fields,
};
