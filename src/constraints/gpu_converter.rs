// GPU Converter - Convert Physics Constraints to CUDA Format
// Week 3 Deliverable: CUDA-Compatible Data Structures

use super::physics_constraint::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ConstraintData {
    pub kind: i32,

    pub count: i32,

    pub node_idx: [i32; 4],

    pub params: [f32; 4],

    pub params2: [f32; 4],

    pub weight: f32,

    pub activation_frame: i32,

    _padding: [f32; 4],
}

pub mod gpu_constraint_kind {
    pub const NONE: i32 = 0;
    pub const SEPARATION: i32 = 1;
    pub const CLUSTERING: i32 = 2;
    pub const COLOCATION: i32 = 3;
    pub const BOUNDARY: i32 = 4;
    pub const HIERARCHICAL_LAYER: i32 = 5;
    pub const CONTAINMENT: i32 = 6;
}

impl Default for ConstraintData {
    fn default() -> Self {
        Self {
            kind: gpu_constraint_kind::NONE,
            count: 0,
            node_idx: [-1; 4],
            params: [0.0; 4],
            params2: [0.0; 4],
            weight: 1.0,
            activation_frame: -1,
            _padding: [0.0; 4],
        }
    }
}

pub fn to_gpu_constraint_data(constraint: &PhysicsConstraint) -> ConstraintData {
    let mut data = ConstraintData {
        count: constraint.nodes.len().min(4) as i32,
        ..Default::default()
    };
    for (i, &node_id) in constraint.nodes.iter().take(4).enumerate() {
        data.node_idx[i] = node_id as i32;
    }

    data.weight = constraint.priority_weight();

    data.activation_frame = constraint.activation_frame.unwrap_or(-1);

    match &constraint.constraint_type {
        PhysicsConstraintType::Separation {
            min_distance,
            strength,
        } => {
            data.kind = gpu_constraint_kind::SEPARATION;
            data.params[0] = *min_distance;
            data.params[1] = *strength;
        }

        PhysicsConstraintType::Clustering {
            ideal_distance,
            stiffness,
        } => {
            data.kind = gpu_constraint_kind::CLUSTERING;
            data.params[0] = *ideal_distance;
            data.params[1] = *stiffness;
        }

        PhysicsConstraintType::Colocation {
            target_distance,
            strength,
        } => {
            data.kind = gpu_constraint_kind::COLOCATION;
            data.params[0] = *target_distance;
            data.params[1] = *strength;
        }

        PhysicsConstraintType::Boundary { bounds, strength } => {
            data.kind = gpu_constraint_kind::BOUNDARY;
            data.params[0] = bounds[0];
            data.params[1] = bounds[1];
            data.params[2] = bounds[2];
            data.params[3] = bounds[3];
            data.params2[0] = bounds[4];
            data.params2[1] = bounds[5];
            data.params2[2] = *strength;
        }

        PhysicsConstraintType::HierarchicalLayer { z_level, strength } => {
            data.kind = gpu_constraint_kind::HIERARCHICAL_LAYER;
            data.params[0] = *z_level;
            data.params[1] = *strength;
        }

        PhysicsConstraintType::Containment {
            parent_node,
            radius,
            strength,
        } => {
            data.kind = gpu_constraint_kind::CONTAINMENT;
            data.params[0] = *parent_node as f32;
            data.params[1] = *radius;
            data.params[2] = *strength;
        }
    }

    data
}

pub fn to_gpu_constraint_batch(constraints: &[PhysicsConstraint]) -> Vec<ConstraintData> {
    constraints.iter().map(to_gpu_constraint_data).collect()
}

/// Buffer for GPU constraint data with reusable allocation
///
/// This buffer pre-allocates memory to avoid repeated allocations during
/// batch conversion operations. Use `convert_batch` for efficient conversion
/// that reuses the internal buffer.
pub struct GPUConstraintBuffer {
    /// The constraint data storage
    pub data: Vec<ConstraintData>,

    /// Number of active constraints in the buffer
    pub count: usize,

    /// Maximum capacity of the buffer
    pub capacity: usize,

    /// Reusable buffer for batch conversion operations to avoid allocation per batch
    conversion_buffer: Vec<ConstraintData>,
}

impl GPUConstraintBuffer {
    /// Create a new GPU constraint buffer with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            count: 0,
            capacity,
            conversion_buffer: Vec::with_capacity(capacity),
        }
    }

    /// Add constraints to the buffer, converting them to GPU format
    pub fn add_constraints(&mut self, constraints: &[PhysicsConstraint]) -> Result<(), String> {
        if self.count + constraints.len() > self.capacity {
            return Err(format!(
                "Buffer overflow: {} + {} > {}",
                self.count,
                constraints.len(),
                self.capacity
            ));
        }

        let gpu_data = to_gpu_constraint_batch(constraints);
        self.data.extend(gpu_data);
        self.count += constraints.len();

        Ok(())
    }

    /// Convert a batch of constraints using the reusable internal buffer
    ///
    /// This method avoids allocation per batch by reusing `conversion_buffer`.
    /// The returned slice is valid until the next call to `convert_batch`.
    pub fn convert_batch(&mut self, batch: &[PhysicsConstraint]) -> &[ConstraintData] {
        self.conversion_buffer.clear();
        self.conversion_buffer
            .extend(batch.iter().map(to_gpu_constraint_data));
        &self.conversion_buffer
    }

    /// Clear the buffer for reuse
    pub fn clear(&mut self) {
        self.data.clear();
        self.count = 0;
    }

    pub fn as_ptr(&self) -> *const ConstraintData {
        self.data.as_ptr()
    }

    pub fn size_bytes(&self) -> usize {
        self.count * std::mem::size_of::<ConstraintData>()
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

#[derive(Debug, Clone)]
pub struct ConstraintStats {
    pub total_constraints: usize,
    pub separation_count: usize,
    pub clustering_count: usize,
    pub colocation_count: usize,
    pub boundary_count: usize,
    pub hierarchical_count: usize,
    pub containment_count: usize,
    pub user_defined_count: usize,
    pub progressive_count: usize,
    pub total_weight: f32,
}

impl ConstraintStats {
    pub fn from_buffer(buffer: &GPUConstraintBuffer) -> Self {
        let mut stats = Self {
            total_constraints: buffer.count,
            separation_count: 0,
            clustering_count: 0,
            colocation_count: 0,
            boundary_count: 0,
            hierarchical_count: 0,
            containment_count: 0,
            user_defined_count: 0,
            progressive_count: 0,
            total_weight: 0.0,
        };

        for constraint_data in &buffer.data {
            match constraint_data.kind {
                gpu_constraint_kind::SEPARATION => stats.separation_count += 1,
                gpu_constraint_kind::CLUSTERING => stats.clustering_count += 1,
                gpu_constraint_kind::COLOCATION => stats.colocation_count += 1,
                gpu_constraint_kind::BOUNDARY => stats.boundary_count += 1,
                gpu_constraint_kind::HIERARCHICAL_LAYER => stats.hierarchical_count += 1,
                gpu_constraint_kind::CONTAINMENT => stats.containment_count += 1,
                _ => {}
            }

            stats.total_weight += constraint_data.weight;

            if constraint_data.activation_frame >= 0 {
                stats.progressive_count += 1;
            }

            if (constraint_data.weight - 1.0).abs() < 0.001 {
                stats.user_defined_count += 1;
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_separation_constraint_conversion() {
        let constraint = PhysicsConstraint::separation(vec![1, 2], 35.0, 0.8, 5);
        let gpu_data = to_gpu_constraint_data(&constraint);

        assert_eq!(gpu_data.kind, gpu_constraint_kind::SEPARATION);
        assert_eq!(gpu_data.count, 2);
        assert_eq!(gpu_data.node_idx[0], 1);
        assert_eq!(gpu_data.node_idx[1], 2);
        assert_eq!(gpu_data.node_idx[2], -1);
        assert_eq!(gpu_data.params[0], 35.0);
        assert_eq!(gpu_data.params[1], 0.8);
        assert!(gpu_data.weight > 0.0);
    }

    #[test]
    fn test_clustering_constraint_conversion() {
        let constraint = PhysicsConstraint::clustering(vec![10, 20], 20.0, 0.6, 3);
        let gpu_data = to_gpu_constraint_data(&constraint);

        assert_eq!(gpu_data.kind, gpu_constraint_kind::CLUSTERING);
        assert_eq!(gpu_data.count, 2);
        assert_eq!(gpu_data.params[0], 20.0);
        assert_eq!(gpu_data.params[1], 0.6);
    }

    #[test]
    fn test_boundary_constraint_conversion() {
        let bounds = [-20.0, 20.0, -20.0, 20.0, -20.0, 20.0];
        let constraint = PhysicsConstraint::boundary(vec![1], bounds, 0.7, 5);
        let gpu_data = to_gpu_constraint_data(&constraint);

        assert_eq!(gpu_data.kind, gpu_constraint_kind::BOUNDARY);
        assert_eq!(gpu_data.params[0], -20.0);
        assert_eq!(gpu_data.params[1], 20.0);
        assert_eq!(gpu_data.params[2], -20.0);
        assert_eq!(gpu_data.params[3], 20.0);
        assert_eq!(gpu_data.params2[0], -20.0);
        assert_eq!(gpu_data.params2[1], 20.0);
        assert_eq!(gpu_data.params2[2], 0.7);
    }

    #[test]
    fn test_hierarchical_layer_conversion() {
        let constraint = PhysicsConstraint::hierarchical_layer(vec![1, 2, 3], 100.0, 0.7, 5);
        let gpu_data = to_gpu_constraint_data(&constraint);

        assert_eq!(gpu_data.kind, gpu_constraint_kind::HIERARCHICAL_LAYER);
        assert_eq!(gpu_data.count, 3);
        assert_eq!(gpu_data.params[0], 100.0);
        assert_eq!(gpu_data.params[1], 0.7);
    }

    #[test]
    fn test_containment_conversion() {
        let constraint = PhysicsConstraint::containment(vec![1, 2], 100, 50.0, 0.8, 5);
        let gpu_data = to_gpu_constraint_data(&constraint);

        assert_eq!(gpu_data.kind, gpu_constraint_kind::CONTAINMENT);
        assert_eq!(gpu_data.params[0], 100.0);
        assert_eq!(gpu_data.params[1], 50.0);
        assert_eq!(gpu_data.params[2], 0.8);
    }

    #[test]
    fn test_activation_frame() {
        let constraint =
            PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 5).with_activation_frame(60);

        let gpu_data = to_gpu_constraint_data(&constraint);
        assert_eq!(gpu_data.activation_frame, 60);
    }

    #[test]
    fn test_priority_weight() {
        let c1 = PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 1);
        let c2 = PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 10);

        let gpu1 = to_gpu_constraint_data(&c1);
        let gpu2 = to_gpu_constraint_data(&c2);

        assert!(gpu1.weight > gpu2.weight);
        assert!((gpu1.weight - 1.0).abs() < 0.001);
        assert!((gpu2.weight - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_batch_conversion() {
        let constraints = vec![
            PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 5),
            PhysicsConstraint::clustering(vec![2, 3], 20.0, 0.6, 3),
            PhysicsConstraint::colocation(vec![3, 4], 2.0, 0.9, 1),
        ];

        let gpu_batch = to_gpu_constraint_batch(&constraints);
        assert_eq!(gpu_batch.len(), 3);
        assert_eq!(gpu_batch[0].kind, gpu_constraint_kind::SEPARATION);
        assert_eq!(gpu_batch[1].kind, gpu_constraint_kind::CLUSTERING);
        assert_eq!(gpu_batch[2].kind, gpu_constraint_kind::COLOCATION);
    }

    #[test]
    fn test_gpu_buffer() {
        let mut buffer = GPUConstraintBuffer::new(100);

        let constraints = vec![
            PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 5),
            PhysicsConstraint::clustering(vec![2, 3], 20.0, 0.6, 3),
        ];

        assert!(buffer.add_constraints(&constraints).is_ok());
        assert_eq!(buffer.len(), 2);
        assert!(!buffer.is_empty());

        let size = buffer.size_bytes();
        assert_eq!(size, 2 * std::mem::size_of::<ConstraintData>());

        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buffer = GPUConstraintBuffer::new(2);

        let constraints = vec![
            PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 5),
            PhysicsConstraint::clustering(vec![2, 3], 20.0, 0.6, 3),
            PhysicsConstraint::colocation(vec![3, 4], 2.0, 0.9, 1),
        ];

        assert!(buffer.add_constraints(&constraints).is_err());
    }

    #[test]
    fn test_constraint_stats() {
        let mut buffer = GPUConstraintBuffer::new(100);

        let constraints = vec![
            PhysicsConstraint::separation(vec![1, 2], 10.0, 0.5, 5),
            PhysicsConstraint::separation(vec![2, 3], 15.0, 0.6, 5),
            PhysicsConstraint::clustering(vec![3, 4], 20.0, 0.6, 3),
            PhysicsConstraint::colocation(vec![4, 5], 2.0, 0.9, 1).mark_user_defined(),
        ];

        buffer.add_constraints(&constraints).unwrap();

        let stats = ConstraintStats::from_buffer(&buffer);
        assert_eq!(stats.total_constraints, 4);
        assert_eq!(stats.separation_count, 2);
        assert_eq!(stats.clustering_count, 1);
        assert_eq!(stats.colocation_count, 1);
        assert_eq!(stats.user_defined_count, 1);
    }

    #[test]
    fn test_constraint_data_size() {
        let size = std::mem::size_of::<ConstraintData>();

        assert_eq!(size % 16, 0);
    }
}
