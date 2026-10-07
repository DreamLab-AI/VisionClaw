/**
 * Group 1 — Motion & Forces (id `motion`, hotkey 1, 48 fields).
 * All fields migrated verbatim from the legacy Physics tab. Path strings are the
 * FROZEN backend contract — transcribed byte-identically from unifiedSettingsConfig.ts.
 * `qualityGates.*` semantic fields route to the quality-gates endpoint (not physics).
 */
import type { GroupData, RegistryField } from '../types';

const P = 'visualisation.graphs.knowledge.physics.';

const fields: RegistryField[] = [
  // Core Forces
  { key: 'springK', subgroup: 'Core Forces', label: 'Spring Strength', type: 'slider', min: 0, max: 100, step: 0.5, path: `${P}springK`, description: 'Edge spring constant for Hooke mode (default 12). In the default LinLog mode the per-population multipliers below govern spring strength.' },
  { key: 'springKKnowledge', subgroup: 'Core Forces', label: 'Spring: Knowledge', type: 'slider', min: 0, max: 10, step: 0.1, path: `${P}springKKnowledge`, description: 'Spring strength multiplier for knowledge-graph nodes — live in both LinLog and Hooke modes (default 1.0 = baseline).' },
  { key: 'springKOntology', subgroup: 'Core Forces', label: 'Spring: Ontology', type: 'slider', min: 0, max: 10, step: 0.1, path: `${P}springKOntology`, description: 'Spring strength multiplier for ontology (OWL) nodes (default 1.0 = baseline).' },
  { key: 'springKAgent', subgroup: 'Core Forces', label: 'Spring: Agent', type: 'slider', min: 0, max: 10, step: 0.1, path: `${P}springKAgent`, description: 'Spring strength multiplier for agent nodes (default 1.0 = baseline).' },
  { key: 'repelK', subgroup: 'Core Forces', label: 'Repulsion', type: 'slider', min: 0, max: 500, step: 10, path: `${P}repelK`, description: 'Node repulsion constant (default 120, backend caps at 500)', macro: 'density' },
  { key: 'restLength', subgroup: 'Core Forces', label: 'Node Spacing', type: 'slider', min: 1, max: 200, step: 1, path: `${P}restLength`, description: 'Spring rest length — small = dense, large = spread (default 50)', macro: 'density' },
  { key: 'centerGravityK', subgroup: 'Core Forces', label: 'Center Gravity', type: 'slider', min: 0, max: 1.0, step: 0.01, path: `${P}centerGravityK`, description: 'Uniform pull of every node toward the world origin — higher values pack the whole graph tighter around the centre (default 0.2). Distinct from Community Cohesion (Constraints), which pulls toward per-community centroids.', macro: 'density' },
  { key: 'gravity', subgroup: 'Core Forces', label: 'Gravity', type: 'slider', min: 0, max: 0.01, step: 0.0001, path: `${P}gravity`, description: 'Center-pull force — affects how loosely-connected nodes drift (default 0.002)' },
  { key: 'maxForce', subgroup: 'Core Forces', label: 'Max Force', type: 'slider', min: 1, max: 2000, step: 5, path: `${P}maxForce`, description: 'Maximum force per node (default 150)' },
  { key: 'maxVelocity', subgroup: 'Core Forces', label: 'Max Velocity', type: 'slider', min: 1, max: 500, step: 1, path: `${P}maxVelocity`, description: 'Maximum node speed (default 100)' },
  // Simulation
  { key: 'enabled', subgroup: 'Simulation', label: 'Physics Enabled', type: 'toggle', path: `${P}enabled`, description: 'Enable physics simulation' },
  { key: 'resetLayout', subgroup: 'Simulation', label: 'Reset Layout', type: 'action-button', action: 'reset_layout', description: 'Re-randomize all positions and reset physics to safe defaults — use when the graph has exploded or become unresponsive' },
  { key: 'autoBalance', subgroup: 'Simulation', label: 'Auto Balance', type: 'toggle', path: `${P}autoBalance`, description: 'Adaptive force balancing' },
  { key: 'dt', subgroup: 'Simulation', label: 'Time Step', type: 'slider', min: 0.001, max: 0.1, step: 0.001, path: `${P}dt`, description: 'Simulation time step (default 0.016)' },
  { key: 'iterations', subgroup: 'Simulation', label: 'Iterations', type: 'slider', min: 0, max: 2000, step: 10, path: `${P}iterations`, description: 'Solver iterations per frame — more = finer resolution (default 50)' },
  { key: 'warmupIterations', subgroup: 'Simulation', label: 'Warmup Iterations', type: 'slider', min: 0, max: 500, step: 10, path: `${P}warmupIterations`, description: 'Initial stabilization iterations (default 100)' },
  { key: 'coolingRate', subgroup: 'Simulation', label: 'Cooling Rate', type: 'slider', min: 0, max: 0.01, step: 0.0005, path: `${P}coolingRate`, description: 'Simulated annealing rate (default 0.001)' },
  { key: 'globalSpeed', subgroup: 'Simulation', label: 'Global Speed', type: 'slider', min: 0, max: 5, step: 0.01, path: `${P}globalSpeed`, description: 'FA2 base integration speed (default 0.4)', macro: 'motion' },
  { key: 'damping', subgroup: 'Simulation', label: 'Damping', type: 'slider', min: 0.01, max: 1.0, step: 0.01, path: `${P}damping`, description: 'Velocity damping — lower = more energy, higher = faster settle (default 0.9)', macro: 'motion' },
  // Repulsion & Spacing
  { key: 'maxRepulsionDist', subgroup: 'Repulsion & Spacing', label: 'Max Repulsion Dist', type: 'slider', min: 10, max: 800, step: 10, path: `${P}maxRepulsionDist`, description: 'Maximum repulsion range — larger affects more distant nodes (default 400, sized to the ~400-unit graph envelope)' },
  { key: 'separationRadius', subgroup: 'Repulsion & Spacing', label: 'Separation Radius', type: 'slider', min: 0, max: 50, step: 0.1, path: `${P}separationRadius`, description: 'Minimum node separation — tiny for dense, large for spacing (default ~2.12)' },
  { key: 'gridCellSize', subgroup: 'Repulsion & Spacing', label: 'Grid Cell Size', type: 'slider', min: 1, max: 200, step: 1, path: `${P}gridCellSize`, description: 'Spatial grid cell size — larger for spread-out graphs (default 50)' },
  { key: 'repulsionSofteningEpsilon', subgroup: 'Repulsion & Spacing', label: 'Repulsion Epsilon', type: 'slider', min: 0, max: 0.01, step: 0.0001, path: `${P}repulsionSofteningEpsilon`, description: 'Softening for close nodes (default 0.0001)' },
  // Bounds
  { key: 'enableBounds', subgroup: 'Bounds', label: 'Enable Bounds', type: 'toggle', path: `${P}enableBounds`, description: 'Constrain nodes to a bounding box' },
  { key: 'boundsSize', subgroup: 'Bounds', label: 'Bounds Size', type: 'slider', min: 100, max: 2000, step: 50, path: `${P}boundsSize`, description: 'Half-extent of the soft bounding cube per axis — the graph settles within ~this radius (default 400)' },
  { key: 'boundaryDamping', subgroup: 'Bounds', label: 'Boundary Damping', type: 'slider', min: 0, max: 1.0, step: 0.01, path: `${P}boundaryDamping`, description: 'Velocity damping when nodes approach boundary (default 0.95)' },
  // Layout Forces
  { key: 'linLogMode', subgroup: 'Layout Forces', label: 'LinLog Mode', type: 'toggle', path: `${P}linLogMode`, description: 'Logarithmic attraction (modularity-preserving) vs linear Hooke springs' },
  { key: 'scalingRatio', subgroup: 'Layout Forces', label: 'FA2 Scaling Ratio', type: 'slider', min: 0.5, max: 100, step: 0.5, path: `${P}scalingRatio`, description: 'ForceAtlas2 repulsion scaling — higher spreads degree-heavy nodes further (default 10)' },
  { key: 'adaptiveSpeed', subgroup: 'Layout Forces', label: 'Adaptive Speed', type: 'toggle', path: `${P}adaptiveSpeed`, description: 'Per-node adaptive convergence speed (reduces oscillation)' },
  { key: 'ssspAlpha', subgroup: 'Layout Forces', label: 'SSSP Alpha', type: 'slider', min: 0, max: 5, step: 0.1, path: `${P}ssspAlpha`, description: 'Single-source shortest-path force weighting (default 1.5)' },
  { key: 'graphSeparationX', subgroup: 'Layout Forces', label: 'Graph Separation', type: 'slider', min: 0, max: 400, step: 25, path: `${P}graphSeparationX`, description: 'Separation between the knowledge and ontology graphs — the depth gap between the two facing discs (default 0 = merged/overlapping, ~250 = clearly separated). Use with Disc Flatten to make them face one another.' },
  { key: 'axisCompressionZ', subgroup: 'Layout Forces', label: 'Z axis compression (1 = fully 3D)', type: 'slider', min: 0.05, max: 1.0, step: 0.05, path: `${P}axisCompressionZ`, description: 'Continuous Z-axis scale (default 1.0 = fully 3D). Lower values compress the graph toward the z=0 plane for disc-style layouts; 0.05 is nearly flat. Agents stay 3D as bridges.' },
  { key: 'enableDualDiscLayout', subgroup: 'Layout Forces', label: 'Dual-disc layout', type: 'toggle', path: `${P}enableDualDiscLayout`, description: 'Arrange the knowledge and ontology graphs as two facing discs across the separation gap (default off). Combine with Z axis compression and Graph Separation.' },
  { key: 'dagBiasK', subgroup: 'Layout Forces', label: 'Hierarchy shell force', type: 'slider', min: 0, max: 2, step: 0.05, path: `${P}dagBiasK`, description: 'Strength of the DAG radial rank-bias force that arranges hierarchy levels into concentric shells (default 0 = off).' },
  { key: 'dagLevelDistance', subgroup: 'Layout Forces', label: 'Hierarchy shell spacing', type: 'slider', min: 10, max: 200, step: 10, path: `${P}dagLevelDistance`, description: 'Radial distance between successive DAG hierarchy levels/shells (default 60).' },
  { key: 'planeBiasK', subgroup: 'Layout Forces', label: 'Plane Bias (type strata)', type: 'slider', min: 0, max: 2, step: 0.05, path: `${P}planeBiasK`, description: 'Strength of the plane spring force that stratifies nodes into parallel planes by type (default 0 = off).' },
  { key: 'planeSpacing', subgroup: 'Layout Forces', label: 'Plane Spacing', type: 'slider', min: 10, max: 200, step: 10, path: `${P}planeSpacing`, description: 'World-space gap between successive type planes (default 60).' },
  { key: 'layerBiasK', subgroup: 'Layout Forces', label: 'Sugiyama layer force', type: 'slider', min: 0, max: 2, step: 0.05, path: `${P}layerBiasK`, description: 'Strength of the Sugiyama Y-by-rank layer spring — pulls each hierarchy rank to its own horizontal layer for a top-down layered layout (default 0 = off; also auto-primed by the Hierarchical layout mode).' },
  { key: 'layerSpacing', subgroup: 'Layout Forces', label: 'Sugiyama layer spacing', type: 'slider', min: 10, max: 200, step: 10, path: `${P}layerSpacing`, description: 'Vertical gap between successive Sugiyama hierarchy layers (default 60).' },
  // Constraints
  { key: 'constraintRampFrames', subgroup: 'Constraints', label: 'Constraint Ramp', type: 'slider', min: 0, max: 300, step: 5, path: `${P}constraintRampFrames`, description: 'Frames over which ontology constraints ramp up after a change (default 60)' },
  { key: 'constraintMaxForcePerNode', subgroup: 'Constraints', label: 'Constraint Max Force', type: 'slider', min: 1, max: 2000, step: 5, path: `${P}constraintMaxForcePerNode`, description: 'Per-node cap on ontology constraint forces (default 50)' },
  { key: 'clusterStrength', subgroup: 'Constraints', label: 'Community Cohesion', type: 'slider', min: 0, max: 0.02, step: 0.0005, path: `${P}clusterStrength`, description: 'Strength of the community-cohesion force — pulls each node toward the centroid of its GPU-detected community. defaults to 0 (off) — raise it to pull communities together; the backend gates on >0.0001 and clamps at 0.02. The partition itself is recomputed on the GPU using the method + resolution below.' },
  { key: 'clusteringAlgorithm', subgroup: 'Constraints', label: 'Community Method', type: 'select', options: ['leiden', 'louvain'], path: `${P}clusteringAlgorithm`, description: 'Detector used to partition the graph into communities for the cohesion force. Leiden (default) yields well-connected communities; Louvain is faster but can produce disconnected ones. Applied live on the GPU.' },
  { key: 'clusteringResolution', subgroup: 'Constraints', label: 'Community Resolution', type: 'slider', min: 0.1, max: 5.0, step: 0.1, path: `${P}clusteringResolution`, description: 'Resolution of the GPU community detection — higher splits the graph into more, smaller communities; lower merges into fewer, larger ones (default 1.0). Re-runs the detector live; the cohesion force then pulls toward the new centroids.' },
  { key: 'temperature', subgroup: 'Constraints', label: 'Temperature', type: 'slider', min: 0, max: 1, step: 0.05, path: `${P}temperature`, description: 'Simulation temperature (energy) — higher = more movement (default 0, backend caps at 1.0)', macro: 'motion' },
  // Semantic & Layout Forces (route to quality-gates / semantic endpoints, not physics)
  { key: 'layoutMode', subgroup: 'Semantic & Layout Forces', label: 'Layout Mode', type: 'select', options: ['force-directed', 'dag-topdown', 'dag-leftright', 'dag-radial', 'type-clustering'], path: 'qualityGates.layoutMode', description: 'Graph layout (quality-gate vocabulary, validated server-side; layoutApi maps it to the engine LayoutMode) — force-directed uses spring/repulsion; dag-topdown/dag-leftright apply Sugiyama layering; dag-radial arranges rings; type-clustering groups by node type' },
  { key: 'semanticForces', subgroup: 'Semantic & Layout Forces', label: 'Semantic Layout Forces', type: 'toggle', path: 'qualityGates.semanticForces', description: 'Enable DAG hierarchy layout and type-based clustering forces' },
  { key: 'dagLevelAttraction', subgroup: 'Semantic & Layout Forces', label: 'DAG Level Attraction', type: 'slider', min: 0, max: 2.0, step: 0.05, path: 'qualityGates.dagLevelAttraction', description: 'How strongly nodes pull toward their hierarchy level' },
  { key: 'dagSiblingRepulsion', subgroup: 'Semantic & Layout Forces', label: 'DAG Sibling Repulsion', type: 'slider', min: 0, max: 2.0, step: 0.05, path: 'qualityGates.dagSiblingRepulsion', description: 'How strongly same-level nodes spread apart' },
  { key: 'typeClusterAttraction', subgroup: 'Semantic & Layout Forces', label: 'Type Cluster Attraction', type: 'slider', min: 0, max: 2.0, step: 0.05, path: 'qualityGates.typeClusterAttraction', description: 'How strongly same-type nodes group together' },
  { key: 'typeClusterRadius', subgroup: 'Semantic & Layout Forces', label: 'Type Cluster Radius', type: 'slider', min: 10, max: 500, step: 10, path: 'qualityGates.typeClusterRadius', description: 'Target radius for type-based cluster zones' },
  // Smooth Movement (client-side tweening)
  { key: 'tweeningEnabled', subgroup: 'Smooth Movement', label: 'Smooth Node Movement', type: 'toggle', path: 'visualisation.graphs.knowledge.tweening.enabled', description: 'Smoothly animate nodes toward server positions instead of snapping instantly' },
  { key: 'tweeningLerpBase', subgroup: 'Smooth Movement', label: 'Node Animation Speed', type: 'slider', min: 0.0001, max: 0.15, step: 0.001, path: 'visualisation.graphs.knowledge.tweening.lerpBase', description: 'How quickly nodes reach their target positions (lower = faster, higher = smoother)' },
  { key: 'tweeningMaxDivergence', subgroup: 'Smooth Movement', label: 'Maximum Node Jump', type: 'slider', min: 1, max: 100, step: 1, path: 'visualisation.graphs.knowledge.tweening.maxDivergence', description: 'Distance threshold above which nodes snap instantly instead of animating' },
  { key: 'tweeningSnapThreshold', subgroup: 'Smooth Movement', label: 'Snap Distance', type: 'slider', min: 0.01, max: 1.0, step: 0.01, path: 'visualisation.graphs.knowledge.tweening.snapThreshold', description: 'Distance below which nodes snap to their target (sub-pixel precision)' },
];

export const motion: GroupData = {
  id: 'motion',
  label: 'Motion & Forces',
  description: 'Force-directed layout: springs, repulsion, gravity, bounds, and semantic layout modes.',
  hotkey: '1',
  loadPaths: ['visualisation.graphs.knowledge.physics', 'qualityGates', 'visualisation.graphs.knowledge.tweening'],
  fields,
};
