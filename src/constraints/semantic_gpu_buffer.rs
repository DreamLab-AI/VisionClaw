// Semantic GPU Buffer - CUDA-compatible constraint buffer for semantic physics
// Optimized data layout for GPU upload and processing

use super::semantic_physics_types::*;
use std::mem;

/// GPU-compatible representation of semantic physics constraint
/// Memory-aligned for efficient CUDA transfer
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct SemanticGPUConstraint {
    /// Constraint type ID
    /// 1 = Separation, 2 = HierarchicalAttraction, 3 = Alignment,
    /// 4 = BidirectionalEdge, 5 = Colocation, 6 = Containment
    pub constraint_type: i32,

    /// Priority (1-10, lower is higher priority)
    pub priority: i32,

    /// Node/class indices (up to 4)
    pub node_indices: [i32; 4],

    /// Primary parameters (distance, position, etc.)
    pub params: [f32; 4],

    /// Secondary parameters (strength, radius, etc.)
    pub params2: [f32; 4],

    /// Priority weight (precomputed)
    pub weight: f32,

    /// Axis for alignment (0=None, 1=X, 2=Y, 3=Z)
    pub axis: i32,

    /// Reserved for future use / alignment
    _padding: [f32; 2],
}

/// Constraint type IDs for GPU
pub mod gpu_semantic_types {
    pub const NONE: i32 = 0;
    pub const SEPARATION: i32 = 1;
    pub const HIERARCHICAL_ATTRACTION: i32 = 2;
    pub const ALIGNMENT: i32 = 3;
    pub const BIDIRECTIONAL_EDGE: i32 = 4;
    pub const COLOCATION: i32 = 5;
    pub const CONTAINMENT: i32 = 6;
}

impl Default for SemanticGPUConstraint {
    fn default() -> Self {
        Self {
            constraint_type: gpu_semantic_types::NONE,
            priority: 5,
            node_indices: [-1; 4],
            params: [0.0; 4],
            params2: [0.0; 4],
            weight: 1.0,
            axis: 0,
            _padding: [0.0; 2],
        }
    }
}

impl SemanticGPUConstraint {
    /// Create from semantic physics constraint with IRI to index mapping
    pub fn from_semantic(
        constraint: &SemanticPhysicsConstraint,
        iri_to_index: &std::collections::HashMap<String, i32>,
    ) -> Self {
        let mut gpu_constraint = Self {
            priority: constraint.priority() as i32,
            weight: constraint.priority_weight(),
            ..Default::default()
        };

        match constraint {
            SemanticPhysicsConstraint::Separation {
                class_a,
                class_b,
                min_distance,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::SEPARATION;
                gpu_constraint.node_indices[0] = *iri_to_index.get(class_a).unwrap_or(&-1);
                gpu_constraint.node_indices[1] = *iri_to_index.get(class_b).unwrap_or(&-1);
                gpu_constraint.params[0] = *min_distance;
                gpu_constraint.params[1] = *strength;
            }

            SemanticPhysicsConstraint::HierarchicalAttraction {
                child_class,
                parent_class,
                ideal_distance,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::HIERARCHICAL_ATTRACTION;
                gpu_constraint.node_indices[0] = *iri_to_index.get(child_class).unwrap_or(&-1);
                gpu_constraint.node_indices[1] = *iri_to_index.get(parent_class).unwrap_or(&-1);
                gpu_constraint.params[0] = *ideal_distance;
                gpu_constraint.params[1] = *strength;
            }

            SemanticPhysicsConstraint::Alignment {
                class_iri,
                axis,
                target_position,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::ALIGNMENT;
                gpu_constraint.node_indices[0] = *iri_to_index.get(class_iri).unwrap_or(&-1);
                gpu_constraint.params[0] = *target_position;
                gpu_constraint.params[1] = *strength;
                gpu_constraint.axis = match axis {
                    Axis::X => 1,
                    Axis::Y => 2,
                    Axis::Z => 3,
                };
            }

            SemanticPhysicsConstraint::BidirectionalEdge {
                class_a,
                class_b,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::BIDIRECTIONAL_EDGE;
                gpu_constraint.node_indices[0] = *iri_to_index.get(class_a).unwrap_or(&-1);
                gpu_constraint.node_indices[1] = *iri_to_index.get(class_b).unwrap_or(&-1);
                gpu_constraint.params[0] = *strength;
            }

            SemanticPhysicsConstraint::Colocation {
                class_a,
                class_b,
                target_distance,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::COLOCATION;
                gpu_constraint.node_indices[0] = *iri_to_index.get(class_a).unwrap_or(&-1);
                gpu_constraint.node_indices[1] = *iri_to_index.get(class_b).unwrap_or(&-1);
                gpu_constraint.params[0] = *target_distance;
                gpu_constraint.params[1] = *strength;
            }

            SemanticPhysicsConstraint::Containment {
                child_class,
                parent_class,
                radius,
                strength,
                ..
            } => {
                gpu_constraint.constraint_type = gpu_semantic_types::CONTAINMENT;
                gpu_constraint.node_indices[0] = *iri_to_index.get(child_class).unwrap_or(&-1);
                gpu_constraint.node_indices[1] = *iri_to_index.get(parent_class).unwrap_or(&-1);
                gpu_constraint.params[0] = *radius;
                gpu_constraint.params[1] = *strength;
            }
        }

        gpu_constraint
    }

    /// Check if constraint is valid (has valid node indices)
    pub fn is_valid(&self) -> bool {
        self.constraint_type != gpu_semantic_types::NONE && self.node_indices[0] >= 0
    }
}

/// GPU buffer for semantic physics constraints
pub struct SemanticGPUConstraintBuffer {
    /// Constraint data (CUDA-compatible)
    pub data: Vec<SemanticGPUConstraint>,

    /// Number of active constraints
    pub count: usize,

    /// Buffer capacity
    pub capacity: usize,

    /// IRI to index mapping
    pub iri_to_index: std::collections::HashMap<String, i32>,
}

impl SemanticGPUConstraintBuffer {
    /// Create new buffer with specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            count: 0,
            capacity,
            iri_to_index: std::collections::HashMap::new(),
        }
    }

    /// Register class IRI and get index
    pub fn register_class(&mut self, class_iri: &str) -> i32 {
        let next_index = self.iri_to_index.len() as i32;
        *self
            .iri_to_index
            .entry(class_iri.to_string())
            .or_insert(next_index)
    }

    /// Add semantic constraints to buffer
    pub fn add_constraints(
        &mut self,
        constraints: &[SemanticPhysicsConstraint],
    ) -> Result<(), String> {
        if self.count + constraints.len() > self.capacity {
            return Err(format!(
                "Buffer overflow: {} + {} > {}",
                self.count,
                constraints.len(),
                self.capacity
            ));
        }

        // Register all class IRIs first
        for constraint in constraints {
            for class_iri in constraint.involved_classes() {
                self.register_class(&class_iri);
            }
        }

        // Convert to GPU format
        for constraint in constraints {
            let gpu_constraint =
                SemanticGPUConstraint::from_semantic(constraint, &self.iri_to_index);

            if gpu_constraint.is_valid() {
                self.data.push(gpu_constraint);
                self.count += 1;
            }
        }

        Ok(())
    }

    /// Get raw pointer for CUDA upload
    pub fn as_ptr(&self) -> *const SemanticGPUConstraint {
        self.data.as_ptr()
    }

    /// Get buffer size in bytes
    pub fn size_bytes(&self) -> usize {
        self.count * mem::size_of::<SemanticGPUConstraint>()
    }

    /// Get number of constraints
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clear buffer
    pub fn clear(&mut self) {
        self.data.clear();
        self.count = 0;
    }

    /// Get constraint statistics
    pub fn get_stats(&self) -> SemanticConstraintStats {
        let mut stats = SemanticConstraintStats {
            total_constraints: self.count,
            ..Default::default()
        };

        for constraint in &self.data {
            match constraint.constraint_type {
                gpu_semantic_types::SEPARATION => stats.separation_count += 1,
                gpu_semantic_types::HIERARCHICAL_ATTRACTION => stats.hierarchical_count += 1,
                gpu_semantic_types::ALIGNMENT => stats.alignment_count += 1,
                gpu_semantic_types::BIDIRECTIONAL_EDGE => stats.bidirectional_count += 1,
                gpu_semantic_types::COLOCATION => stats.colocation_count += 1,
                gpu_semantic_types::CONTAINMENT => stats.containment_count += 1,
                _ => {}
            }

            stats.total_weight += constraint.weight;
            stats.avg_priority += constraint.priority as f32;
        }

        if self.count > 0 {
            stats.avg_priority /= self.count as f32;
        }

        stats
    }
}

/// Statistics for semantic constraints
#[derive(Debug, Clone, Default)]
pub struct SemanticConstraintStats {
    pub total_constraints: usize,
    pub separation_count: usize,
    pub hierarchical_count: usize,
    pub alignment_count: usize,
    pub bidirectional_count: usize,
    pub colocation_count: usize,
    pub containment_count: usize,
    pub total_weight: f32,
    pub avg_priority: f32,
}

impl SemanticConstraintStats {
    /// Print human-readable statistics
    pub fn print(&self) {
        log::info!("Semantic Constraint Statistics:");
        log::info!("  Total: {}", self.total_constraints);
        log::info!("  Separation: {}", self.separation_count);
        log::info!("  Hierarchical: {}", self.hierarchical_count);
        log::info!("  Alignment: {}", self.alignment_count);
        log::info!("  Bidirectional: {}", self.bidirectional_count);
        log::info!("  Colocation: {}", self.colocation_count);
        log::info!("  Containment: {}", self.containment_count);
        log::info!("  Total Weight: {:.2}", self.total_weight);
        log::info!("  Avg Priority: {:.1}", self.avg_priority);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_constraint_size_alignment() {
        let size = mem::size_of::<SemanticGPUConstraint>();
        // Should be 16-byte aligned for CUDA
        assert_eq!(size % 16, 0);
        println!("SemanticGPUConstraint size: {} bytes", size);
    }

    #[test]
    fn test_separation_constraint_conversion() {
        let mut buffer = SemanticGPUConstraintBuffer::new(10);

        let constraint = SemanticPhysicsConstraint::Separation {
            class_a: "ClassA".to_string(),
            class_b: "ClassB".to_string(),
            min_distance: 50.0,
            strength: 0.8,
            priority: 5,
        };

        buffer.add_constraints(&[constraint]).unwrap();

        assert_eq!(buffer.len(), 1);
        assert_eq!(
            buffer.data[0].constraint_type,
            gpu_semantic_types::SEPARATION
        );
        assert_eq!(buffer.data[0].params[0], 50.0);
        assert_eq!(buffer.data[0].params[1], 0.8);
    }

    #[test]
    fn test_alignment_constraint() {
        let mut buffer = SemanticGPUConstraintBuffer::new(10);

        let constraint = SemanticPhysicsConstraint::Alignment {
            class_iri: "ClassA".to_string(),
            axis: Axis::X,
            target_position: 100.0,
            strength: 0.7,
            priority: 3,
        };

        buffer.add_constraints(&[constraint]).unwrap();

        assert_eq!(
            buffer.data[0].constraint_type,
            gpu_semantic_types::ALIGNMENT
        );
        assert_eq!(buffer.data[0].axis, 1); // X = 1
        assert_eq!(buffer.data[0].params[0], 100.0);
    }

    #[test]
    fn test_iri_registration() {
        let mut buffer = SemanticGPUConstraintBuffer::new(10);

        let idx1 = buffer.register_class("ClassA");
        let idx2 = buffer.register_class("ClassB");
        let idx3 = buffer.register_class("ClassA"); // Should return same index

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 0); // Reused
    }

    #[test]
    fn test_buffer_stats() {
        let mut buffer = SemanticGPUConstraintBuffer::new(10);

        let constraints = vec![
            SemanticPhysicsConstraint::Separation {
                class_a: "A".to_string(),
                class_b: "B".to_string(),
                min_distance: 50.0,
                strength: 0.8,
                priority: 5,
            },
            SemanticPhysicsConstraint::HierarchicalAttraction {
                child_class: "C".to_string(),
                parent_class: "D".to_string(),
                ideal_distance: 30.0,
                strength: 0.6,
                priority: 5,
            },
            SemanticPhysicsConstraint::Alignment {
                class_iri: "E".to_string(),
                axis: Axis::Y,
                target_position: 0.0,
                strength: 0.5,
                priority: 7,
            },
        ];

        buffer.add_constraints(&constraints).unwrap();

        let stats = buffer.get_stats();
        assert_eq!(stats.total_constraints, 3);
        assert_eq!(stats.separation_count, 1);
        assert_eq!(stats.hierarchical_count, 1);
        assert_eq!(stats.alignment_count, 1);
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buffer = SemanticGPUConstraintBuffer::new(2);

        let constraints = vec![
            SemanticPhysicsConstraint::Separation {
                class_a: "A".to_string(),
                class_b: "B".to_string(),
                min_distance: 50.0,
                strength: 0.8,
                priority: 5,
            },
            SemanticPhysicsConstraint::Separation {
                class_a: "C".to_string(),
                class_b: "D".to_string(),
                min_distance: 50.0,
                strength: 0.8,
                priority: 5,
            },
            SemanticPhysicsConstraint::Separation {
                class_a: "E".to_string(),
                class_b: "F".to_string(),
                min_distance: 50.0,
                strength: 0.8,
                priority: 5,
            },
        ];

        let result = buffer.add_constraints(&constraints);
        assert!(result.is_err());
    }

    #[test]
    fn test_priority_weight_precomputed() {
        let mut buffer = SemanticGPUConstraintBuffer::new(10);

        let constraint = SemanticPhysicsConstraint::Separation {
            class_a: "A".to_string(),
            class_b: "B".to_string(),
            min_distance: 50.0,
            strength: 0.8,
            priority: 1, // Highest priority
        };

        buffer.add_constraints(&[constraint]).unwrap();

        // Priority 1 should have weight close to 1.0
        assert!((buffer.data[0].weight - 1.0).abs() < 0.001);
    }
}
