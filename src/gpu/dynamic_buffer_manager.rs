//! # DEPRECATED: Use `crate::gpu::memory_manager` instead
//!
//! This module is deprecated in favor of the unified `GpuMemoryManager`.
//! The new manager provides:
//! - All functionality from this module (dynamic resizing, configs)
//! - Memory leak detection
//! - Async transfers with double buffering
//! - Better error handling and testing
//!
//! **Migration Guide**: See `/home/devuser/workspace/project/docs/gpu_memory_consolidation_analysis.md`
//!
//! This module will be removed in a future release.

#![deprecated(
    since = "0.1.0",
    note = "Use crate::gpu::memory_manager::GpuMemoryManager instead"
)]

//! Dynamic Buffer Manager for GPU Operations (LEGACY)
//!
//! Provides dynamic allocation and resizing of GPU buffers to handle
//! variable graph sizes without hardcoded limits.

use std::collections:::HashMap;
use std::ffi:::c_void;
use std::sync::Arc;
use log::{info, warn, debug};
use crate::utils::cuda_error_handling::{CudaErrorHandler, CudaMemoryGuard};

#[derive(Debug, Clone)]
pub struct BufferConfig {
    
    pub bytes_per_node: usize,
    
    pub growth_factor: f32,
    
    pub max_size_bytes: usize,
    
    pub min_size_bytes: usize,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            bytes_per_node: 64, 
            growth_factor: 1.5,
            max_size_bytes: 1024 * 1024 * 1024, 
            min_size_bytes: 1024, 
        }
    }
}

impl BufferConfig {
    pub fn for_positions() -> Self {
        Self {
            bytes_per_node: 12, 
            growth_factor: 1.3,
            max_size_bytes: 512 * 1024 * 1024, 
            min_size_bytes: 4096, 
        }
    }

    pub fn for_velocities() -> Self {
        Self {
            bytes_per_node: 12, 
            growth_factor: 1.3,
            max_size_bytes: 512 * 1024 * 1024,
            min_size_bytes: 4096,
        }
    }

    pub fn for_forces() -> Self {
        Self {
            bytes_per_node: 12, 
            growth_factor: 1.3,
            max_size_bytes: 512 * 1024 * 1024,
            min_size_bytes: 4096,
        }
    }

    pub fn for_edges() -> Self {
        Self {
            bytes_per_node: 32, 
            growth_factor: 2.0,
            max_size_bytes: 2048 * 1024 * 1024, 
            min_size_bytes: 8192,
        }
    }

    pub fn for_grid_cells() -> Self {
        Self {
            bytes_per_node: 8, 
            growth_factor: 1.5,
            max_size_bytes: 256 * 1024 * 1024, 
            min_size_bytes: 2048,
        }
    }
}

pub struct DynamicGpuBuffer {
    name: String,
    config: BufferConfig,
    current_buffer: Option<Arc<CudaMemoryGuard>>,
    current_capacity: usize,
    current_size: usize,
    error_handler: Arc<CudaErrorHandler>,
}

impl DynamicGpuBuffer {
    pub fn new(name: String, config: BufferConfig, error_handler: Arc<CudaErrorHandler>) -> Self {
        Self {
            name,
            config,
            current_buffer: None,
            current_capacity: 0,
            current_size: 0,
            error_handler,
        }
    }

    
    pub fn ensure_capacity(&mut self, required_elements: usize) -> Result<(), Box<dyn std::error::Error>> {
        let required_bytes = required_elements * self.config.bytes_per_node;

        if required_bytes <= self.current_capacity {
            debug!("Buffer {} already has sufficient capacity: {} bytes", self.name, self.current_capacity);
            return Ok(());
        }

        
        let mut new_capacity = if self.current_capacity == 0 {
            self.config.min_size_bytes.max(required_bytes)
        } else {
            let grown_size = (self.current_capacity as f32 * self.config.growth_factor) as usize;
            grown_size.max(required_bytes)
        };

        
        new_capacity = new_capacity.min(self.config.max_size_bytes);

        if required_bytes > new_capacity {
            return Err(format!("Required size {} exceeds maximum buffer size {} for {}",
                              required_bytes, new_capacity, self.name).into());
        }

        info!("Resizing buffer {} from {} bytes to {} bytes", self.name, self.current_capacity, new_capacity);

        
        let new_buffer = Arc::new(CudaMemoryGuard::new(
            new_capacity,
            format!("{}_dynamic", self.name),
            self.error_handler.clone()
        )?);

        
        if let Some(old_buffer) = &self.current_buffer {
            if self.current_size > 0 {
                debug!("Copying {} bytes from old buffer to new buffer", self.current_size);
                unsafe {
                    let result = cudaMemcpy(
                        new_buffer.as_ptr(),
                        old_buffer.as_ptr(),
                        self.current_size,
                        cudaMemcpyDeviceToDevice
                    );
                    if result != 0 {
                        return Err(format!("Failed to copy buffer data during resize: error code {}", result).into());
                    }
                }
                self.error_handler.check_error(&format!("resize_copy_{}", self.name))?;
            }
        }

        
        self.current_buffer = Some(new_buffer);
        self.current_capacity = new_capacity;

        info!("Successfully resized buffer {} to {} bytes", self.name, new_capacity);
        Ok(())
    }

    
    pub unsafe fn as_ptr(&self) -> *mut c_void {
        if let Some(buffer) = &self.current_buffer {
            buffer.as_ptr()
        } else {
            std::ptr::null_mut()
        }
    }

    
    pub fn capacity_bytes(&self) -> usize {
        self.current_capacity
    }

    
    pub fn size_bytes(&self) -> usize {
        self.current_size
    }

    
    pub fn set_size(&mut self, size_bytes: usize) {
        self.current_size = size_bytes.min(self.current_capacity);
    }

    
    pub fn is_allocated(&self) -> bool {
        self.current_buffer.is_some()
    }

    
    pub fn get_stats(&self) -> BufferStats {
        BufferStats {
            name: self.name.clone(),
            capacity_bytes: self.current_capacity,
            used_bytes: self.current_size,
            utilization: if self.current_capacity > 0 {
                self.current_size as f32 / self.current_capacity as f32
            } else {
                0.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct BufferStats {
    pub name: String,
    pub capacity_bytes: usize,
    pub used_bytes: usize,
    pub utilization: f32,
}

pub struct DynamicBufferManager {
    buffers: HashMap<String, DynamicGpuBuffer>,
    error_handler: Arc<CudaErrorHandler>,
    total_allocated: usize,
    max_total_allocation: usize,
}

impl DynamicBufferManager {
    pub fn new(error_handler: Arc<CudaErrorHandler>) -> Self {
        Self {
            buffers: HashMap::new(),
            error_handler,
            total_allocated: 0,
            max_total_allocation: 6 * 1024 * 1024 * 1024, 
        }
    }

    
    pub fn get_or_create_buffer(&mut self, name: &str, config: BufferConfig) -> &mut DynamicGpuBuffer {
        if !self.buffers.contains_key(name) {
            let buffer = DynamicGpuBuffer::new(
                name.to_string(),
                config,
                self.error_handler.clone()
            );
            self.buffers.insert(name.to_string(), buffer);
        }
        self.buffers.get_mut(name).unwrap()
    }

    
    pub fn resize_cell_buffers(&mut self, num_nodes: usize) -> Result<(), Box<dyn std::error::Error>> {
        info!("Resizing cell buffers for {} nodes", num_nodes);

        
        let grid_side_length = ((num_nodes as f64).powf(1.0/3.0).ceil() as usize).max(8);
        let total_cells = grid_side_length * grid_side_length * grid_side_length;

        info!("Grid dimensions: {}x{}x{} = {} cells", grid_side_length, grid_side_length, grid_side_length, total_cells);

        
        let pos_config = BufferConfig::for_positions();
        self.get_or_create_buffer("pos_x", pos_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("pos_y", pos_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("pos_z", pos_config.clone()).ensure_capacity(num_nodes)?;

        
        let vel_config = BufferConfig::for_velocities();
        self.get_or_create_buffer("vel_x", vel_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("vel_y", vel_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("vel_z", vel_config.clone()).ensure_capacity(num_nodes)?;

        
        let force_config = BufferConfig::for_forces();
        self.get_or_create_buffer("force_x", force_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("force_y", force_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("force_z", force_config.clone()).ensure_capacity(num_nodes)?;

        
        let cell_config = BufferConfig::for_grid_cells();
        self.get_or_create_buffer("cell_keys", cell_config.clone()).ensure_capacity(num_nodes)?;
        self.get_or_create_buffer("cell_start", cell_config.clone()).ensure_capacity(total_cells)?;
        self.get_or_create_buffer("cell_end", cell_config.clone()).ensure_capacity(total_cells)?;
        self.get_or_create_buffer("sorted_indices", cell_config.clone()).ensure_capacity(num_nodes)?;

        
        self.update_total_allocation();

        info!("Successfully resized all cell buffers for {} nodes, {} cells", num_nodes, total_cells);
        Ok(())
    }

    
    pub fn get_all_stats(&self) -> Vec<BufferStats> {
        self.buffers.values().map(|buffer| buffer.get_stats()).collect()
    }

    
    pub fn get_total_allocation(&self) -> usize {
        self.total_allocated
    }

    
    fn update_total_allocation(&mut self) {
        self.total_allocated = self.buffers.values()
            .map(|buffer| buffer.capacity_bytes())
            .sum();

        if self.total_allocated > self.max_total_allocation {
            warn!("Total GPU allocation {} exceeds maximum {}",
                  self.total_allocated, self.max_total_allocation);
        }

        debug!("Total GPU allocation: {} bytes across {} buffers",
               self.total_allocated, self.buffers.len());
    }

    
    pub fn can_allocate(&self, additional_bytes: usize) -> bool {
        self.total_allocated + additional_bytes <= self.max_total_allocation
    }

    
    pub fn cleanup_unused_buffers(&mut self) {
        let initial_count = self.buffers.len();

        
        self.buffers.retain(|name, buffer| {
            let stats = buffer.get_stats();
            if stats.utilization == 0.0 && stats.capacity_bytes > 0 {
                info!("Cleaning up unused buffer: {}", name);
                false
            } else {
                true
            }
        });

        let cleaned_count = initial_count - self.buffers.len();
        if cleaned_count > 0 {
            info!("Cleaned up {} unused buffers", cleaned_count);
            self.update_total_allocation();
        }
    }
}

// External CUDA function declarations
extern "C" {
    fn cudaMemcpy(dst: *mut c_void, src: *const c_void, count: usize, kind: i32) -> i32;
}

const cudaMemcpyDeviceToDevice: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cuda_error_handling:::get_global_cuda_error_handler;

    #[test]
    fn test_buffer_config_creation() {
        let config = BufferConfig::for_positions();
        assert_eq!(config.bytes_per_node, 12);
        assert!(config.growth_factor > 1.0);
    }

    #[test]
    fn test_dynamic_buffer_manager() {
        let handler = get_global_cuda_error_handler();
        let mut manager = DynamicBufferManager::new(handler);

        
        let config = BufferConfig::default();
        let buffer = manager.get_or_create_buffer("test_buffer", config);
        assert_eq!(buffer.name, "test_buffer");
        assert!(!buffer.is_allocated());
    }

    #[test]
    fn test_buffer_stats() {
        let handler = get_global_cuda_error_handler();
        let config = BufferConfig::default();
        let buffer = DynamicGpuBuffer::new("test".to_string(), config, handler);

        let stats = buffer.get_stats();
        assert_eq!(stats.name, "test");
        assert_eq!(stats.capacity_bytes, 0);
        assert_eq!(stats.utilization, 0.0);
    }
}