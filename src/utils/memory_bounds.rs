//! Memory Bounds Checking Utilities
//!
//! Provides comprehensive memory bounds checking and validation for GPU operations.
//! Includes overflow protection, alignment validation, and safe memory access patterns.

use log::debug;
use std::collections:::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct MemoryBounds {
    pub base_address: usize,
    pub size: usize,
    pub element_size: usize,
    pub element_count: usize,
    pub alignment: usize,
    pub is_readonly: bool,
    pub name: String,
}

impl MemoryBounds {
    pub fn new(name: String, size: usize, element_size: usize, alignment: usize) -> Self {
        let element_count = if element_size > 0 {
            size / element_size
        } else {
            0
        };

        Self {
            base_address: 0, 
            size,
            element_size,
            element_count,
            alignment,
            is_readonly: false,
            name,
        }
    }

    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.is_readonly = readonly;
        self
    }

    pub fn with_base_address(mut self, address: usize) -> Self {
        self.base_address = address;
        self
    }

    
    pub fn is_element_in_bounds(&self, element_index: usize) -> bool {
        element_index < self.element_count
    }

    
    pub fn is_byte_in_bounds(&self, byte_offset: usize) -> bool {
        byte_offset < self.size
    }

    
    pub fn element_to_byte_offset(&self, element_index: usize) -> Option<usize> {
        if self.is_element_in_bounds(element_index) {
            Some(element_index * self.element_size)
        } else {
            None
        }
    }

    
    pub fn is_range_valid(&self, start_element: usize, element_count: usize) -> bool {
        if start_element >= self.element_count {
            return false;
        }

        start_element.saturating_add(element_count) <= self.element_count
    }

    
    pub fn is_properly_aligned(&self, address: usize) -> bool {
        address % self.alignment == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryBoundsError {
    #[error("Element index {index} out of bounds for buffer '{name}' (size: {size} elements)")]
    ElementOutOfBounds {
        index: usize,
        size: usize,
        name: String,
    },

    #[error("Byte offset {offset} out of bounds for buffer '{name}' (size: {size} bytes)")]
    ByteOutOfBounds {
        offset: usize,
        size: usize,
        name: String,
    },

    #[error(
        "Memory range [{start}..{end}] out of bounds for buffer '{name}' (size: {size} elements)"
    )]
    RangeOutOfBounds {
        start: usize,
        end: usize,
        size: usize,
        name: String,
    },

    #[error("Unaligned memory access at address {address:#x} for buffer '{name}' (required alignment: {alignment})")]
    UnalignedAccess {
        address: usize,
        alignment: usize,
        name: String,
    },

    #[error("Attempt to write to readonly buffer '{name}'")]
    WriteToReadonly { name: String },

    #[error("Buffer '{name}' not found in bounds registry")]
    BufferNotFound { name: String },

    #[error("Integer overflow in bounds calculation for buffer '{name}': {operation}")]
    IntegerOverflow { name: String, operation: String },

    #[error("Invalid element size {element_size} for buffer '{name}' (must be > 0)")]
    InvalidElementSize { element_size: usize, name: String },
}

#[derive(Debug)]
pub struct MemoryBoundsRegistry {
    bounds: HashMap<String, MemoryBounds>,
    total_allocated: usize,
    max_allocation_size: usize,
}

impl MemoryBoundsRegistry {
    pub fn new(max_allocation_size: usize) -> Self {
        Self {
            bounds: HashMap::new(),
            total_allocated: 0,
            max_allocation_size,
        }
    }

    
    pub fn register_allocation(&mut self, bounds: MemoryBounds) -> Result<(), MemoryBoundsError> {
        
        if self.total_allocated.saturating_add(bounds.size) > self.max_allocation_size {
            return Err(MemoryBoundsError::IntegerOverflow {
                name: bounds.name.clone(),
                operation: format!(
                    "total allocation {} + {} > {}",
                    self.total_allocated, bounds.size, self.max_allocation_size
                ),
            });
        }

        
        if bounds.element_size == 0 {
            return Err(MemoryBoundsError::InvalidElementSize {
                element_size: bounds.element_size,
                name: bounds.name.clone(),
            });
        }

        self.total_allocated += bounds.size;
        debug!(
            "Registered memory allocation: {} ({} bytes, {} elements)",
            bounds.name, bounds.size, bounds.element_count
        );

        self.bounds.insert(bounds.name.clone(), bounds);
        Ok(())
    }

    
    pub fn unregister_allocation(&mut self, name: &str) -> Result<(), MemoryBoundsError> {
        if let Some(bounds) = self.bounds.remove(name) {
            self.total_allocated = self.total_allocated.saturating_sub(bounds.size);
            debug!(
                "Unregistered memory allocation: {} ({} bytes)",
                name, bounds.size
            );
            Ok(())
        } else {
            Err(MemoryBoundsError::BufferNotFound {
                name: name.to_string(),
            })
        }
    }

    
    pub fn get_bounds(&self, name: &str) -> Result<&MemoryBounds, MemoryBoundsError> {
        self.bounds
            .get(name)
            .ok_or_else(|| MemoryBoundsError::BufferNotFound {
                name: name.to_string(),
            })
    }

    
    pub fn check_element_access(
        &self,
        buffer_name: &str,
        element_index: usize,
        is_write: bool,
    ) -> Result<(), MemoryBoundsError> {
        let bounds = self.get_bounds(buffer_name)?;

        if is_write && bounds.is_readonly {
            return Err(MemoryBoundsError::WriteToReadonly {
                name: buffer_name.to_string(),
            });
        }

        if !bounds.is_element_in_bounds(element_index) {
            return Err(MemoryBoundsError::ElementOutOfBounds {
                index: element_index,
                size: bounds.element_count,
                name: buffer_name.to_string(),
            });
        }

        Ok(())
    }

    
    pub fn check_byte_access(
        &self,
        buffer_name: &str,
        byte_offset: usize,
        is_write: bool,
    ) -> Result<(), MemoryBoundsError> {
        let bounds = self.get_bounds(buffer_name)?;

        if is_write && bounds.is_readonly {
            return Err(MemoryBoundsError::WriteToReadonly {
                name: buffer_name.to_string(),
            });
        }

        if !bounds.is_byte_in_bounds(byte_offset) {
            return Err(MemoryBoundsError::ByteOutOfBounds {
                offset: byte_offset,
                size: bounds.size,
                name: buffer_name.to_string(),
            });
        }

        Ok(())
    }

    
    pub fn check_range_access(
        &self,
        buffer_name: &str,
        start_element: usize,
        element_count: usize,
        is_write: bool,
    ) -> Result<(), MemoryBoundsError> {
        let bounds = self.get_bounds(buffer_name)?;

        if is_write && bounds.is_readonly {
            return Err(MemoryBoundsError::WriteToReadonly {
                name: buffer_name.to_string(),
            });
        }

        if !bounds.is_range_valid(start_element, element_count) {
            let end = start_element.saturating_add(element_count);
            return Err(MemoryBoundsError::RangeOutOfBounds {
                start: start_element,
                end,
                size: bounds.element_count,
                name: buffer_name.to_string(),
            });
        }

        Ok(())
    }

    
    pub fn check_alignment(
        &self,
        buffer_name: &str,
        address: usize,
    ) -> Result<(), MemoryBoundsError> {
        let bounds = self.get_bounds(buffer_name)?;

        if !bounds.is_properly_aligned(address) {
            return Err(MemoryBoundsError::UnalignedAccess {
                address,
                alignment: bounds.alignment,
                name: buffer_name.to_string(),
            });
        }

        Ok(())
    }

    
    pub fn total_allocated(&self) -> usize {
        self.total_allocated
    }

    
    pub fn allocation_count(&self) -> usize {
        self.bounds.len()
    }

    
    pub fn get_usage_report(&self) -> MemoryUsageReport {
        let mut largest_allocation = 0;
        let mut buffer_types = HashMap::new();

        for bounds in self.bounds.values() {
            largest_allocation = largest_allocation.max(bounds.size);

            let buffer_type = if bounds.name.contains("node") {
                "nodes"
            } else if bounds.name.contains("edge") {
                "edges"
            } else if bounds.name.contains("constraint") {
                "constraints"
            } else {
                "other"
            };

            *buffer_types.entry(buffer_type.to_string()).or_insert(0) += bounds.size;
        }

        MemoryUsageReport {
            total_allocated: self.total_allocated,
            allocation_count: self.bounds.len(),
            largest_allocation,
            buffer_types,
            max_allocation_size: self.max_allocation_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryUsageReport {
    pub total_allocated: usize,
    pub allocation_count: usize,
    pub largest_allocation: usize,
    pub buffer_types: HashMap<String, usize>,
    pub max_allocation_size: usize,
}

impl MemoryUsageReport {
    pub fn usage_percentage(&self) -> f64 {
        if self.max_allocation_size > 0 {
            (self.total_allocated as f64 / self.max_allocation_size as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn format_report(&self) -> String {
        let mut report = String::new();
        report.push_str(&format!("Memory Usage Report:\n"));
        report.push_str(&format!(
            "  Total Allocated: {} bytes ({:.1}% of limit)\n",
            self.total_allocated,
            self.usage_percentage()
        ));
        report.push_str(&format!(
            "  Active Allocations: {}\n",
            self.allocation_count
        ));
        report.push_str(&format!(
            "  Largest Allocation: {} bytes\n",
            self.largest_allocation
        ));
        report.push_str("  By Buffer Type:\n");

        for (buffer_type, size) in &self.buffer_types {
            let percentage = if self.total_allocated > 0 {
                (*size as f64 / self.total_allocated as f64) * 100.0
            } else {
                0.0
            };
            report.push_str(&format!(
                "    {}: {} bytes ({:.1}%)\n",
                buffer_type, size, percentage
            ));
        }

        report
    }
}

pub struct ThreadSafeMemoryBoundsChecker {
    registry: Arc<Mutex<MemoryBoundsRegistry>>,
}

impl ThreadSafeMemoryBoundsChecker {
    pub fn new(max_allocation_size: usize) -> Self {
        Self {
            registry: Arc::new(Mutex::new(MemoryBoundsRegistry::new(max_allocation_size))),
        }
    }

    pub fn register_allocation(&self, bounds: MemoryBounds) -> Result<(), MemoryBoundsError> {
        self.registry
            .lock()
            .map_err(|_| MemoryBoundsError::IntegerOverflow {
                name: bounds.name.clone(),
                operation: "failed to acquire registry lock".to_string(),
            })?
            .register_allocation(bounds)
    }

    pub fn unregister_allocation(&self, name: &str) -> Result<(), MemoryBoundsError> {
        self.registry
            .lock()
            .map_err(|_| MemoryBoundsError::BufferNotFound {
                name: name.to_string(),
            })?
            .unregister_allocation(name)
    }

    pub fn check_element_access(
        &self,
        buffer_name: &str,
        element_index: usize,
        is_write: bool,
    ) -> Result<(), MemoryBoundsError> {
        self.registry
            .lock()
            .map_err(|_| MemoryBoundsError::BufferNotFound {
                name: buffer_name.to_string(),
            })?
            .check_element_access(buffer_name, element_index, is_write)
    }

    pub fn check_range_access(
        &self,
        buffer_name: &str,
        start_element: usize,
        element_count: usize,
        is_write: bool,
    ) -> Result<(), MemoryBoundsError> {
        self.registry
            .lock()
            .map_err(|_| MemoryBoundsError::BufferNotFound {
                name: buffer_name.to_string(),
            })?
            .check_range_access(buffer_name, start_element, element_count, is_write)
    }

    pub fn get_usage_report(&self) -> Option<MemoryUsageReport> {
        self.registry
            .lock()
            .ok()
            .map(|registry| registry.get_usage_report())
    }
}

pub struct SafeArrayAccess<T> {
    data: Vec<T>,
    bounds_checker: Option<Arc<ThreadSafeMemoryBoundsChecker>>,
    buffer_name: String,
}

impl<T: Clone> SafeArrayAccess<T> {
    pub fn new(data: Vec<T>, buffer_name: String) -> Self {
        Self {
            data,
            bounds_checker: None,
            buffer_name,
        }
    }

    pub fn with_bounds_checker(mut self, checker: Arc<ThreadSafeMemoryBoundsChecker>) -> Self {
        self.bounds_checker = Some(checker);
        self
    }

    pub fn get(&self, index: usize) -> Result<&T, MemoryBoundsError> {
        if let Some(checker) = &self.bounds_checker {
            checker.check_element_access(&self.buffer_name, index, false)?;
        }

        self.data
            .get(index)
            .ok_or_else(|| MemoryBoundsError::ElementOutOfBounds {
                index,
                size: self.data.len(),
                name: self.buffer_name.clone(),
            })
    }

    pub fn get_mut(&mut self, index: usize) -> Result<&mut T, MemoryBoundsError> {
        if let Some(checker) = &self.bounds_checker {
            checker.check_element_access(&self.buffer_name, index, true)?;
        }

        let len = self.data.len();
        self.data
            .get_mut(index)
            .ok_or_else(|| MemoryBoundsError::ElementOutOfBounds {
                index,
                size: len,
                name: self.buffer_name.clone(),
            })
    }

    pub fn slice(&self, start: usize, count: usize) -> Result<&[T], MemoryBoundsError> {
        if let Some(checker) = &self.bounds_checker {
            checker.check_range_access(&self.buffer_name, start, count, false)?;
        }

        let end = start.saturating_add(count);
        if end > self.data.len() {
            return Err(MemoryBoundsError::RangeOutOfBounds {
                start,
                end,
                size: self.data.len(),
                name: self.buffer_name.clone(),
            });
        }

        Ok(&self.data[start..end])
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_bounds_validation() {
        let bounds = MemoryBounds::new("test_buffer".to_string(), 1000, 4, 4);

        assert!(bounds.is_element_in_bounds(0));
        assert!(bounds.is_element_in_bounds(249)); 
        assert!(!bounds.is_element_in_bounds(250));

        assert!(bounds.is_byte_in_bounds(0));
        assert!(bounds.is_byte_in_bounds(999));
        assert!(!bounds.is_byte_in_bounds(1000));

        assert!(bounds.is_range_valid(0, 100));
        assert!(bounds.is_range_valid(200, 50));
        assert!(!bounds.is_range_valid(200, 100)); 
    }

    #[test]
    fn test_bounds_registry() {
        let mut registry = MemoryBoundsRegistry::new(10000);

        let bounds = MemoryBounds::new("test".to_string(), 1000, 4, 4);
        assert!(registry.register_allocation(bounds).is_ok());

        assert!(registry.check_element_access("test", 100, false).is_ok());
        assert!(registry.check_element_access("test", 300, false).is_err());

        assert!(registry.unregister_allocation("test").is_ok());
        assert!(registry.check_element_access("test", 100, false).is_err());
    }

    #[test]
    fn test_safe_array_access() {
        let data = vec![1, 2, 3, 4, 5];
        let mut safe_array = SafeArrayAccess::new(data, "test_array".to_string());

        assert_eq!(*safe_array.get(0).unwrap(), 1);
        assert_eq!(*safe_array.get(4).unwrap(), 5);
        assert!(safe_array.get(5).is_err());

        *safe_array.get_mut(0).unwrap() = 10;
        assert_eq!(*safe_array.get(0).unwrap(), 10);

        let slice = safe_array.slice(1, 3).unwrap();
        assert_eq!(slice, &[2, 3, 4]);

        assert!(safe_array.slice(3, 5).is_err()); 
    }

    #[test]
    fn test_alignment_checking() {
        let bounds = MemoryBounds::new("aligned_buffer".to_string(), 1000, 4, 16);

        assert!(bounds.is_properly_aligned(0));
        assert!(bounds.is_properly_aligned(16));
        assert!(bounds.is_properly_aligned(32));
        assert!(!bounds.is_properly_aligned(1));
        assert!(!bounds.is_properly_aligned(15));
    }
}

