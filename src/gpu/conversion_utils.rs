//! GPU Conversion Utilities
//!
//! Provides type-safe conversion utilities for GPU data transfer operations,
//! eliminating duplicate conversion code across GPU modules.
//!
//! This module consolidates:
//! - Position/vector tuple ↔ Vec<f32> conversions
//! - Node data serialization for GPU buffers
//! - Buffer size validation
//! - Type-safe GPU format conversions

use crate::utils::gpu_safety::GPUSafetyError;
use std::fmt::Debug;

/// Conversion error type for GPU data transformations
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    #[error("Invalid buffer size: expected {expected}, got {actual}")]
    InvalidBufferSize { expected: usize, actual: usize },

    #[error("Buffer size not divisible by {stride}: length is {length}")]
    InvalidStride { stride: usize, length: usize },

    #[error("Position index {index} out of bounds for buffer with {max_nodes} nodes")]
    IndexOutOfBounds { index: usize, max_nodes: usize },

    #[error("Invalid data: {reason}")]
    InvalidData { reason: String },

    #[error("GPU safety error: {0}")]
    SafetyError(#[from] GPUSafetyError),
}

pub type Result<T> = std::result::Result<T, ConversionError>;

// ============================================================================
// Position/Vector Conversions (3D and 4D)
// ============================================================================

/// Convert 3D position tuples to flat GPU buffer format
/// # Example
/// ```rust,ignore
/// let positions = vec![(1.0, 2.0, 3.0), (4.0, 5.0, 6.0)];
/// let buffer = positions_to_gpu(&positions);
/// assert_eq!(buffer, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
/// ```
pub fn positions_to_gpu(positions: &[(f32, f32, f32)]) -> Vec<f32> {
    positions
        .iter()
        .flat_map(|(x, y, z)| vec![*x, *y, *z])
        .collect()
}

/// Convert flat GPU buffer to 3D position tuples
/// # Errors
/// Returns error if buffer length is not divisible by 3
pub fn gpu_to_positions(buffer: &[f32]) -> Result<Vec<(f32, f32, f32)>> {
    if !buffer.len().is_multiple_of(3) {
        return Err(ConversionError::InvalidStride {
            stride: 3,
            length: buffer.len(),
        });
    }

    Ok(buffer
        .chunks(3)
        .map(|chunk| (chunk[0], chunk[1], chunk[2]))
        .collect())
}

/// Convert 4D position tuples (x, y, z, w) to flat GPU buffer format
/// Used for homogeneous coordinates or Vec4 types
pub fn positions_4d_to_gpu(positions: &[(f32, f32, f32, f32)]) -> Vec<f32> {
    positions
        .iter()
        .flat_map(|(x, y, z, w)| vec![*x, *y, *z, *w])
        .collect()
}

/// Convert flat GPU buffer to 4D position tuples
/// # Errors
/// Returns error if buffer length is not divisible by 4
pub fn gpu_to_positions_4d(buffer: &[f32]) -> Result<Vec<(f32, f32, f32, f32)>> {
    if !buffer.len().is_multiple_of(4) {
        return Err(ConversionError::InvalidStride {
            stride: 4,
            length: buffer.len(),
        });
    }

    Ok(buffer
        .chunks(4)
        .map(|chunk| (chunk[0], chunk[1], chunk[2], chunk[3]))
        .collect())
}

// ============================================================================
// Generic Buffer Conversions
// ============================================================================

/// Convert generic slice to GPU buffer format (f32)
/// Handles type conversion for common numeric types
pub fn to_gpu_buffer<T: Into<f32> + Copy>(data: &[T]) -> Vec<f32> {
    data.iter().map(|&v| v.into()).collect()
}

/// Convert GPU buffer back to specified type
/// # Errors
/// Returns error if conversion fails or data is invalid
pub fn from_gpu_buffer<T: TryFrom<f32> + Debug>(buffer: &[f32]) -> Result<Vec<T>>
where
    <T as TryFrom<f32>>::Error: Debug,
{
    buffer
        .iter()
        .map(|&v| {
            T::try_from(v).map_err(|e| ConversionError::InvalidData {
                reason: format!("Failed to convert {:?}: {:?}", v, e),
            })
        })
        .collect()
}

// ============================================================================
// Buffer Validation
// ============================================================================

/// Validate buffer size matches expected element count and stride
/// # Arguments
/// * `buffer` - The buffer to validate
/// * `expected_elements` - Expected number of elements
/// * `stride` - Number of values per element (e.g., 3 for Vec3, 4 for Vec4)
pub fn validate_buffer_size(buffer: &[f32], expected_elements: usize, stride: usize) -> Result<()> {
    let expected_size = expected_elements * stride;

    if buffer.len() != expected_size {
        return Err(ConversionError::InvalidBufferSize {
            expected: expected_size,
            actual: buffer.len(),
        });
    }

    Ok(())
}

/// Validate buffer can be divided into chunks of given stride
pub fn validate_buffer_stride(buffer: &[f32], stride: usize) -> Result<()> {
    if !buffer.len().is_multiple_of(stride) {
        return Err(ConversionError::InvalidStride {
            stride,
            length: buffer.len(),
        });
    }

    Ok(())
}

/// Get element count from buffer with given stride
pub fn get_element_count(buffer: &[f32], stride: usize) -> Result<usize> {
    validate_buffer_stride(buffer, stride)?;
    Ok(buffer.len() / stride)
}

// ============================================================================
// Node Data Serialization
// ============================================================================

/// GPU node representation - compact format for GPU transfer
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GpuNode {
    pub position: [f32; 4], // x, y, z, w
    pub velocity: [f32; 4], // vx, vy, vz, vw
    pub color: [f32; 4],    // r, g, b, a
    pub importance: f32,
}

impl GpuNode {
    /// Total size in f32 elements (4 + 4 + 4 + 1 = 13)
    pub const STRIDE: usize = 13;

    /// Create new GPU node with validation
    pub fn new(
        position: [f32; 4],
        velocity: [f32; 4],
        color: [f32; 4],
        importance: f32,
    ) -> Result<Self> {
        // Validate all values are finite
        for &val in position
            .iter()
            .chain(velocity.iter())
            .chain(color.iter())
            .chain(std::iter::once(&importance))
        {
            if !val.is_finite() {
                return Err(ConversionError::InvalidData {
                    reason: format!("Non-finite value in node data: {}", val),
                });
            }
        }

        Ok(Self {
            position,
            velocity,
            color,
            importance,
        })
    }

    /// Convert node to flat buffer format
    pub fn to_buffer(&self) -> Vec<f32> {
        let mut buffer = Vec::with_capacity(Self::STRIDE);
        buffer.extend_from_slice(&self.position);
        buffer.extend_from_slice(&self.velocity);
        buffer.extend_from_slice(&self.color);
        buffer.push(self.importance);
        buffer
    }

    /// Create node from buffer at given offset
    pub fn from_buffer(buffer: &[f32], offset: usize) -> Result<Self> {
        if offset + Self::STRIDE > buffer.len() {
            return Err(ConversionError::IndexOutOfBounds {
                index: offset,
                max_nodes: buffer.len() / Self::STRIDE,
            });
        }

        let slice = &buffer[offset..offset + Self::STRIDE];

        Ok(Self {
            position: [slice[0], slice[1], slice[2], slice[3]],
            velocity: [slice[4], slice[5], slice[6], slice[7]],
            color: [slice[8], slice[9], slice[10], slice[11]],
            importance: slice[12],
        })
    }
}

/// Convert multiple nodes to interleaved GPU buffer
pub fn nodes_to_gpu_buffer(nodes: &[GpuNode]) -> Vec<f32> {
    nodes.iter().flat_map(|node| node.to_buffer()).collect()
}

/// Convert GPU buffer to multiple nodes
pub fn gpu_buffer_to_nodes(buffer: &[f32]) -> Result<Vec<GpuNode>> {
    validate_buffer_stride(buffer, GpuNode::STRIDE)?;

    (0..buffer.len())
        .step_by(GpuNode::STRIDE)
        .map(|offset| GpuNode::from_buffer(buffer, offset))
        .collect()
}

// ============================================================================
// Specialized Conversions for Render Data
// ============================================================================

/// Validate render data buffers have consistent sizes
/// Ensures positions, colors are Vec4 aligned and importance matches node count
pub fn validate_render_data(
    positions: &[f32],
    colors: &[f32],
    importance: &[f32],
) -> Result<usize> {
    // Validate positions are Vec4 (stride 4)
    validate_buffer_stride(positions, 4)?;

    // Validate colors are Vec4 (stride 4)
    validate_buffer_stride(colors, 4)?;

    let node_count = positions.len() / 4;

    // Ensure colors match position count
    if colors.len() / 4 != node_count {
        return Err(ConversionError::InvalidBufferSize {
            expected: node_count * 4,
            actual: colors.len(),
        });
    }

    // Ensure importance matches node count
    if importance.len() != node_count {
        return Err(ConversionError::InvalidBufferSize {
            expected: node_count,
            actual: importance.len(),
        });
    }

    Ok(node_count)
}

/// Extract single node's position from Vec4 buffer
pub fn extract_position_vec4(buffer: &[f32], node_index: usize) -> Result<[f32; 4]> {
    let offset = node_index * 4;

    if offset + 4 > buffer.len() {
        return Err(ConversionError::IndexOutOfBounds {
            index: node_index,
            max_nodes: buffer.len() / 4,
        });
    }

    Ok([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ])
}

/// Extract single node's position as 3D tuple (ignoring w component)
pub fn extract_position_3d(buffer: &[f32], node_index: usize) -> Result<(f32, f32, f32)> {
    let vec4 = extract_position_vec4(buffer, node_index)?;
    Ok((vec4[0], vec4[1], vec4[2]))
}

// ============================================================================
// Memory Layout Helpers
// ============================================================================

/// Calculate required buffer size for given element count and stride
pub fn calculate_buffer_size(element_count: usize, stride: usize) -> usize {
    element_count * stride
}

/// Calculate memory footprint in bytes for buffer
pub fn calculate_memory_footprint(buffer: &[f32]) -> usize {
    std::mem::size_of_val(buffer)
}

/// Allocate zeroed GPU buffer with given capacity
pub fn allocate_gpu_buffer(element_count: usize, stride: usize) -> Vec<f32> {
    vec![0.0; calculate_buffer_size(element_count, stride)]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positions_to_gpu() {
        let positions = vec![(1.0, 2.0, 3.0), (4.0, 5.0, 6.0)];
        let buffer = positions_to_gpu(&positions);
        assert_eq!(buffer, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_gpu_to_positions() {
        let buffer = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let positions = gpu_to_positions(&buffer).unwrap();
        assert_eq!(positions, vec![(1.0, 2.0, 3.0), (4.0, 5.0, 6.0)]);
    }

    #[test]
    fn test_gpu_to_positions_invalid_stride() {
        let buffer = vec![1.0, 2.0, 3.0, 4.0]; // Not divisible by 3
        assert!(gpu_to_positions(&buffer).is_err());
    }

    #[test]
    fn test_positions_4d_conversions() {
        let positions = vec![(1.0, 2.0, 3.0, 1.0), (4.0, 5.0, 6.0, 1.0)];
        let buffer = positions_4d_to_gpu(&positions);
        assert_eq!(buffer.len(), 8);

        let recovered = gpu_to_positions_4d(&buffer).unwrap();
        assert_eq!(recovered, positions);
    }

    #[test]
    fn test_validate_buffer_size() {
        let buffer = vec![0.0; 12]; // 3 Vec4 elements
        assert!(validate_buffer_size(&buffer, 3, 4).is_ok());
        assert!(validate_buffer_size(&buffer, 4, 4).is_err());
    }

    #[test]
    fn test_validate_buffer_stride() {
        let buffer = vec![0.0; 12];
        assert!(validate_buffer_stride(&buffer, 3).is_ok());
        assert!(validate_buffer_stride(&buffer, 4).is_ok());
        assert!(validate_buffer_stride(&buffer, 5).is_err());
    }

    #[test]
    fn test_get_element_count() {
        let buffer = vec![0.0; 12];
        assert_eq!(get_element_count(&buffer, 3).unwrap(), 4);
        assert_eq!(get_element_count(&buffer, 4).unwrap(), 3);
    }

    #[test]
    fn test_gpu_node_conversion() {
        let node = GpuNode::new(
            [1.0, 2.0, 3.0, 1.0],
            [0.1, 0.2, 0.3, 0.0],
            [1.0, 0.0, 0.0, 1.0],
            0.5,
        )
        .unwrap();

        let buffer = node.to_buffer();
        assert_eq!(buffer.len(), GpuNode::STRIDE);

        let recovered = GpuNode::from_buffer(&buffer, 0).unwrap();
        assert_eq!(recovered.position, node.position);
        assert_eq!(recovered.importance, node.importance);
    }

    #[test]
    fn test_nodes_to_gpu_buffer() {
        let nodes = vec![
            GpuNode::new([1.0, 2.0, 3.0, 1.0], [0.0; 4], [1.0, 0.0, 0.0, 1.0], 0.5).unwrap(),
            GpuNode::new([4.0, 5.0, 6.0, 1.0], [0.0; 4], [0.0, 1.0, 0.0, 1.0], 0.8).unwrap(),
        ];

        let buffer = nodes_to_gpu_buffer(&nodes);
        assert_eq!(buffer.len(), 2 * GpuNode::STRIDE);

        let recovered = gpu_buffer_to_nodes(&buffer).unwrap();
        assert_eq!(recovered.len(), 2);
    }

    #[test]
    fn test_validate_render_data() {
        let positions = vec![0.0; 16]; // 4 Vec4 positions
        let colors = vec![0.0; 16]; // 4 Vec4 colors
        let importance = vec![0.0; 4]; // 4 importance values

        let node_count = validate_render_data(&positions, &colors, &importance).unwrap();
        assert_eq!(node_count, 4);
    }

    #[test]
    fn test_validate_render_data_mismatch() {
        let positions = vec![0.0; 16]; // 4 nodes
        let colors = vec![0.0; 12]; // 3 nodes - MISMATCH
        let importance = vec![0.0; 4];

        assert!(validate_render_data(&positions, &colors, &importance).is_err());
    }

    #[test]
    fn test_extract_position_vec4() {
        let buffer = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let pos = extract_position_vec4(&buffer, 1).unwrap();
        assert_eq!(pos, [5.0, 6.0, 7.0, 8.0]);
    }

    #[test]
    fn test_extract_position_3d() {
        let buffer = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let pos = extract_position_3d(&buffer, 0).unwrap();
        assert_eq!(pos, (1.0, 2.0, 3.0));
    }

    #[test]
    fn test_calculate_buffer_size() {
        assert_eq!(calculate_buffer_size(10, 4), 40);
        assert_eq!(calculate_buffer_size(100, 3), 300);
    }

    #[test]
    fn test_calculate_memory_footprint() {
        let buffer = vec![0.0; 100];
        assert_eq!(calculate_memory_footprint(&buffer), 400); // 100 * 4 bytes
    }

    #[test]
    fn test_allocate_gpu_buffer() {
        let buffer = allocate_gpu_buffer(10, 4);
        assert_eq!(buffer.len(), 40);
        assert!(buffer.iter().all(|&v| v == 0.0));
    }
}
