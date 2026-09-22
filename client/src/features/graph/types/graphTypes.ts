

export type GraphType = 'knowledge' | 'visionclaw';

/**
 * The knowledge-graph type value is `knowledge` (ADR-2041); the server has
 * rejected the retired `logseq` since ADR-2115. The only remaining receive-side
 * mapping is for persisted browser settings (`migrateGraphSettingsKey`,
 * `toVisualKey`).
 */

export interface GraphNode {
  id: string;
  label: string;
  position: {
    x: number;
    y: number;
    z: number;
  };
  metadata?: Record<string, any>;
  graphType?: GraphType;
  owlClassIri?: string;  // Ontology class IRI for semantic identity
  nodeType?: string;     // Visual node type for rendering
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
  weight?: number;
  metadata?: Record<string, any>;
  graphType?: GraphType; 
}

export interface TypedGraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
  graphType: GraphType;
  lastUpdate?: number;
}

// Message types for graph updates
export interface GraphUpdateMessage {
  type: 'node-update' | 'edge-update' | 'position-update' | 'bulk-update';
  graphType: GraphType;
  data: any;
  timestamp: number;
}

// Physics settings per graph type
export interface GraphPhysicsConfig {
  
  spring_k: number; 
  repel_k: number; 
  max_velocity: number; 
  damping: number; 
  
  
  rest_length: number; 
  repulsion_cutoff: number; 
  repulsion_softening_epsilon: number; 
  center_gravity_k: number; 
  grid_cell_size: number; 
  warmup_iterations: number; 
  cooling_rate: number; 
  feature_flags: number; 
  
  
  boundary_extreme_multiplier: number; 
  boundary_extreme_force_multiplier: number; 
  boundary_velocity_damping: number; 
  max_force: number; 
  seed: number; 
  iteration: number; 
  
  
  springStrength?: number;
  updateThreshold?: number;
  nodeRepulsion?: number;
  linkDistance?: number;
  gravityStrength?: number;
}

export interface GraphTypeConfig {
  knowledge: {
    physics: GraphPhysicsConfig;
    rendering: {
      nodeSize: number;
      edgeWidth: number;
      labelSize: number;
    };
  };
  visionclaw: {
    physics: GraphPhysicsConfig;
    rendering: {
      agentSize: number;
      connectionWidth: number;
      healthIndicator: boolean;
    };
  };
}

// Default configurations
export const DEFAULT_GRAPH_CONFIG: GraphTypeConfig = {
  knowledge: {
    physics: {
      
      spring_k: 0.2,
      repel_k: 1.0,
      max_velocity: 5.0,
      damping: 0.95,
      
      
      rest_length: 50.0,
      repulsion_cutoff: 50.0,
      repulsion_softening_epsilon: 0.0001,
      center_gravity_k: 0.0,
      grid_cell_size: 50.0,
      warmup_iterations: 100,
      cooling_rate: 0.001,
      feature_flags: 7,
      
      
      boundary_extreme_multiplier: 2.0,
      boundary_extreme_force_multiplier: 5.0,
      boundary_velocity_damping: 0.5,
      max_force: 100,
      seed: 42,
      iteration: 0,
      
      
      springStrength: 0.2,
      updateThreshold: 0.05,
      nodeRepulsion: 10,
      linkDistance: 30
    },
    rendering: {
      nodeSize: 5,
      edgeWidth: 1,
      labelSize: 1.2
    }
  },
  visionclaw: {
    physics: {
      
      spring_k: 0.3,
      repel_k: 2.0,
      max_velocity: 10.0,
      damping: 0.95,
      
      
      rest_length: 50.0,
      repulsion_cutoff: 50.0,
      repulsion_softening_epsilon: 0.0001,
      center_gravity_k: 0.1,
      grid_cell_size: 50.0,
      warmup_iterations: 100,
      cooling_rate: 0.001,
      feature_flags: 7,
      
      
      boundary_extreme_multiplier: 2.5,
      boundary_extreme_force_multiplier: 6.0,
      boundary_velocity_damping: 0.6,
      max_force: 120,
      seed: 42,
      iteration: 0,
      
      
      springStrength: 0.3,
      updateThreshold: 0.1,
      nodeRepulsion: 15,
      linkDistance: 20,
      gravityStrength: 0.1
    },
    rendering: {
      agentSize: 8,
      connectionWidth: 2,
      healthIndicator: true
    }
  }
};