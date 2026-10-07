//! GPU Memory Manager Test Suite
//!
//! Comprehensive tests for the unified GPU memory manager covering:
//! 1. Buffer allocation and deallocation
//! 2. Memory pool management (growth, limits, tracking)
//! 3. Error handling for OOM conditions
//! 4. Concurrent access patterns
//!
//! Target: src/gpu/memory_manager.rs (13 unwraps, 2 unsafe blocks)
//!
//! AISP 5.1 WAVE 2 - coverage-specialist task

#[cfg(all(test, feature = "gpu"))]
mod gpu_memory_manager_tests {
    use visionclaw_server::gpu::memory_manager::{BufferConfig, GpuMemoryManager};

    // ============================================================
    // SECTION 1: Buffer Allocation and Deallocation Tests
    // ============================================================

    /// Test basic manager creation with default settings
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_manager_creation_default() {
        let manager = GpuMemoryManager::new();
        assert!(
            manager.is_ok(),
            "Manager creation with defaults should succeed"
        );

        let manager = manager.unwrap();
        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 0, "New manager should have no buffers");
        assert_eq!(
            stats.total_allocated_bytes, 0,
            "New manager should have 0 allocated bytes"
        );
        assert_eq!(
            stats.allocation_count, 0,
            "New manager should have 0 allocations"
        );
    }

    /// Test manager creation with custom memory limit
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_manager_creation_with_limit() {
        let limit_bytes = 512 * 1024 * 1024; // 512MB
        let manager = GpuMemoryManager::with_limit(limit_bytes);
        assert!(
            manager.is_ok(),
            "Manager creation with custom limit should succeed"
        );
    }

    /// Test single buffer allocation
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_single_buffer_allocation() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        let result = manager.allocate::<f32>("test_buffer", 1000, config);

        assert!(result.is_ok(), "Buffer allocation should succeed");

        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 1, "Should have 1 buffer");
        assert_eq!(stats.allocation_count, 1, "Should have 1 allocation");
        assert!(stats.total_allocated_bytes >= 1000 * std::mem::size_of::<f32>());
    }

    /// Test multiple buffer allocations
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_multiple_buffer_allocations() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("buffer_a", 100, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("buffer_b", 200, config.clone())
            .unwrap();
        manager.allocate::<f32>("buffer_c", 300, config).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 3, "Should have 3 buffers");
        assert_eq!(stats.allocation_count, 3, "Should have 3 allocations");
    }

    /// Test duplicate buffer name handling (should skip, not error)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_duplicate_buffer_name_skipped() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("duplicate_name", 100, config.clone())
            .unwrap();

        // Second allocation with same name should be skipped (not error)
        let result = manager.allocate::<f32>("duplicate_name", 500, config);
        assert!(result.is_ok(), "Duplicate name should not error");

        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 1, "Should still have only 1 buffer");
    }

    /// Test buffer deallocation
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_buffer_deallocation() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("to_free", 1000, config).unwrap();

        let stats_before = manager.stats();
        assert_eq!(stats_before.buffer_count, 1);

        let result = manager.free("to_free");
        assert!(result.is_ok(), "Buffer deallocation should succeed");

        let stats_after = manager.stats();
        assert_eq!(
            stats_after.buffer_count, 0,
            "Buffer count should be 0 after free"
        );
    }

    /// Test freeing non-existent buffer returns error
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_free_nonexistent_buffer_error() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let result = manager.free("does_not_exist");
        assert!(result.is_err(), "Freeing non-existent buffer should error");
    }

    /// Test freeing multiple buffers in different orders
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_free_multiple_buffers_various_orders() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("buf1", 100, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("buf2", 200, config.clone())
            .unwrap();
        manager.allocate::<f32>("buf3", 300, config).unwrap();

        // Free middle buffer first
        manager.free("buf2").unwrap();
        assert_eq!(manager.stats().buffer_count, 2);

        // Free first buffer
        manager.free("buf1").unwrap();
        assert_eq!(manager.stats().buffer_count, 1);

        // Free last buffer
        manager.free("buf3").unwrap();
        assert_eq!(manager.stats().buffer_count, 0);
    }

    /// Test buffer access (get_buffer immutable)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_get_buffer_immutable() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("readable", 100, config).unwrap();

        let buffer = manager.get_buffer::<f32>("readable");
        assert!(
            buffer.is_ok(),
            "get_buffer should succeed for existing buffer"
        );
    }

    /// Test buffer access (get_buffer_mut mutable)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_get_buffer_mutable() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("writable", 100, config).unwrap();

        let buffer = manager.get_buffer_mut::<f32>("writable");
        assert!(
            buffer.is_ok(),
            "get_buffer_mut should succeed for existing buffer"
        );
    }

    /// Test accessing non-existent buffer returns error
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_get_nonexistent_buffer_error() {
        let manager = GpuMemoryManager::new().expect("Manager creation failed");

        let result = manager.get_buffer::<f32>("not_allocated");
        assert!(
            result.is_err(),
            "Accessing non-existent buffer should error"
        );
    }

    /// Test type mismatch when accessing buffer
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_buffer_type_mismatch_error() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("f32_buffer", 100, config).unwrap();

        // Try to access as i32 - should fail due to type mismatch
        let result = manager.get_buffer::<i32>("f32_buffer");
        assert!(result.is_err(), "Type mismatch should return error");
    }

    // ============================================================
    // SECTION 2: Memory Pool Management Tests
    // ============================================================

    /// Test BufferConfig default values
    #[test]
    fn test_buffer_config_defaults() {
        let config = BufferConfig::default();
        assert_eq!(
            config.bytes_per_element, 4,
            "Default should be f32 (4 bytes)"
        );
        assert_eq!(
            config.growth_factor, 1.5,
            "Default growth factor should be 1.5"
        );
        assert_eq!(
            config.min_size_bytes, 4096,
            "Default min size should be 4KB"
        );
        assert_eq!(
            config.max_size_bytes,
            1024 * 1024 * 1024,
            "Default max should be 1GB"
        );
        assert!(!config.enable_async, "Async should be disabled by default");
    }

    /// Test BufferConfig preset for positions
    #[test]
    fn test_buffer_config_for_positions() {
        let config = BufferConfig::for_positions();
        assert_eq!(
            config.bytes_per_element, 12,
            "Positions should be 12 bytes (f32x3)"
        );
        assert_eq!(
            config.growth_factor, 1.3,
            "Positions growth factor should be 1.3"
        );
        assert!(config.enable_async, "Positions should have async enabled");
    }

    /// Test BufferConfig preset for velocities
    #[test]
    fn test_buffer_config_for_velocities() {
        let config = BufferConfig::for_velocities();
        assert_eq!(
            config.bytes_per_element, 12,
            "Velocities should be 12 bytes (f32x3)"
        );
        assert!(config.enable_async, "Velocities should have async enabled");
    }

    /// Test BufferConfig preset for edges
    #[test]
    fn test_buffer_config_for_edges() {
        let config = BufferConfig::for_edges();
        assert_eq!(config.bytes_per_element, 32, "Edges should be 32 bytes");
        assert_eq!(
            config.growth_factor, 2.0,
            "Edges growth factor should be 2.0"
        );
        assert!(!config.enable_async, "Edges should not have async enabled");
    }

    /// Test BufferConfig preset for grid cells
    #[test]
    fn test_buffer_config_for_grid_cells() {
        let config = BufferConfig::for_grid_cells();
        assert_eq!(config.bytes_per_element, 8, "Grid cells should be 8 bytes");
        assert!(
            !config.enable_async,
            "Grid cells should not have async enabled"
        );
    }

    /// Test ensure_capacity when no resize needed
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_ensure_capacity_no_resize_needed() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("buffer", 1000, config).unwrap();

        // Request smaller capacity - should not resize
        let result = manager.ensure_capacity::<f32>("buffer", 500);
        assert!(
            result.is_ok(),
            "ensure_capacity with smaller size should succeed"
        );

        let stats = manager.stats();
        assert_eq!(stats.resize_count, 0, "No resize should occur");
    }

    /// Test ensure_capacity triggers resize
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_ensure_capacity_triggers_resize() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::for_positions();
        manager.allocate::<f32>("positions", 100, config).unwrap();

        // Request larger capacity - should trigger resize
        let result = manager.ensure_capacity::<f32>("positions", 500);
        assert!(result.is_ok(), "ensure_capacity should succeed");

        let stats = manager.stats();
        assert!(stats.resize_count > 0, "Resize count should increase");
    }

    /// Test ensure_capacity respects growth factor
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_ensure_capacity_growth_factor() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig {
            growth_factor: 2.0, // Double each time
            ..Default::default()
        };

        manager
            .allocate::<f32>("growing_buffer", 100, config)
            .unwrap();

        // Request 150 elements - with 2.0 growth, should get 200
        manager
            .ensure_capacity::<f32>("growing_buffer", 150)
            .unwrap();

        let stats = manager.stats();
        assert!(stats.resize_count > 0, "Resize should occur");
    }

    /// Test ensure_capacity on non-existent buffer errors
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_ensure_capacity_nonexistent_buffer_error() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let result = manager.ensure_capacity::<f32>("not_allocated", 100);
        assert!(
            result.is_err(),
            "ensure_capacity on non-existent buffer should error"
        );
    }

    /// Test total allocated tracking
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_total_allocated_tracking() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("buf1", 100, config.clone())
            .unwrap();
        manager.allocate::<f32>("buf2", 200, config).unwrap();

        let stats = manager.stats();
        let expected_min_bytes = (100 + 200) * std::mem::size_of::<f32>();
        assert!(
            stats.total_allocated_bytes >= expected_min_bytes,
            "Total allocated should be at least {} bytes, got {}",
            expected_min_bytes,
            stats.total_allocated_bytes
        );
    }

    /// Test peak allocated tracking
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_peak_allocated_tracking() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("buf1", 1000, config.clone())
            .unwrap();
        manager.allocate::<f32>("buf2", 2000, config).unwrap();

        let peak_after_alloc = manager.stats().peak_allocated_bytes;

        // Free one buffer
        manager.free("buf2").unwrap();

        let stats = manager.stats();
        // Peak should not decrease
        assert_eq!(
            stats.peak_allocated_bytes, peak_after_alloc,
            "Peak should not decrease after free"
        );
        // Current should decrease
        assert!(
            stats.total_allocated_bytes < peak_after_alloc,
            "Current allocation should decrease after free"
        );
    }

    /// Test leak detection when no leaks present
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_leak_detection_no_leaks() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager.allocate::<f32>("temp", 100, config).unwrap();
        manager.free("temp").unwrap();

        let leaks = manager.check_leaks();
        assert!(
            leaks.is_empty(),
            "No leaks should be detected when all buffers freed"
        );
    }

    /// Test leak detection catches unfree'd buffers
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_leak_detection_catches_leaks() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("leaked_buffer", 100, config)
            .unwrap();

        // Don't free it - should be detected as leak
        let leaks = manager.check_leaks();
        assert_eq!(leaks.len(), 1, "Should detect 1 leak");
        assert_eq!(leaks[0], "leaked_buffer", "Leaked buffer name should match");
    }

    /// Test multiple leaks detected
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_leak_detection_multiple_leaks() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        manager
            .allocate::<f32>("leak1", 100, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("leak2", 200, config.clone())
            .unwrap();
        manager.allocate::<f32>("leak3", 300, config).unwrap();

        let leaks = manager.check_leaks();
        assert_eq!(leaks.len(), 3, "Should detect 3 leaks");
    }

    /// Test statistics tracking across operations
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_statistics_comprehensive() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        // Initial state
        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 0);
        assert_eq!(stats.allocation_count, 0);
        assert_eq!(stats.resize_count, 0);

        // Allocate
        let config = BufferConfig::for_positions();
        manager
            .allocate::<f32>("buf1", 100, config.clone())
            .unwrap();
        manager.allocate::<f32>("buf2", 200, config).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 2);
        assert_eq!(stats.allocation_count, 2);

        // Resize
        manager.ensure_capacity::<f32>("buf1", 500).unwrap();

        let stats = manager.stats();
        assert!(stats.resize_count > 0, "Resize count should increase");

        // Free
        manager.free("buf1").unwrap();

        let stats = manager.stats();
        assert_eq!(
            stats.buffer_count, 1,
            "Buffer count should decrease after free"
        );
    }

    // ============================================================
    // SECTION 3: Error Handling for OOM Conditions
    // ============================================================

    /// Test memory limit enforcement on allocation
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_memory_limit_enforcement() {
        let small_limit = 4096; // 4KB limit
        let mut manager =
            GpuMemoryManager::with_limit(small_limit).expect("Manager creation failed");

        let config = BufferConfig::default();

        // Allocate within limit
        let result = manager.allocate::<f32>("small", 100, config.clone());
        assert!(
            result.is_ok(),
            "Small allocation within limit should succeed"
        );

        // Try to allocate beyond limit
        let result = manager.allocate::<f32>("huge", 1_000_000, config);
        assert!(result.is_err(), "Allocation exceeding limit should fail");
    }

    /// Test ensure_capacity respects max buffer size
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_ensure_capacity_max_size_enforced() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig {
            max_size_bytes: 4096, // Very small max
            ..Default::default()
        };

        manager
            .allocate::<f32>("limited_buffer", 100, config)
            .unwrap();

        // Request capacity that would exceed max_size_bytes
        let result = manager.ensure_capacity::<f32>("limited_buffer", 10_000_000);
        assert!(
            result.is_err(),
            "Resize exceeding max_size_bytes should fail"
        );
    }

    /// Test handling of very large allocation request
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_very_large_allocation_rejected() {
        let mut manager =
            GpuMemoryManager::with_limit(1024 * 1024).expect("Manager creation failed");

        let config = BufferConfig::default();
        // Request more than available GPU memory
        let result = manager.allocate::<f32>("impossible", usize::MAX / 1000, config);
        assert!(result.is_err(), "Impossibly large allocation should fail");
    }

    /// Test error handling for invalid operations sequence
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_invalid_operation_sequence_error_handling() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        // Try to get buffer before allocation
        let result = manager.get_buffer::<f32>("not_allocated");
        assert!(result.is_err(), "get_buffer before allocation should error");

        // Try to resize before allocation
        let result = manager.ensure_capacity::<f32>("not_allocated", 100);
        assert!(
            result.is_err(),
            "ensure_capacity before allocation should error"
        );

        // Try to start async download before allocation
        let result = manager.start_async_download::<f32>("not_allocated");
        assert!(
            result.is_err(),
            "start_async_download before allocation should error"
        );

        // Try to free before allocation
        let result = manager.free("not_allocated");
        assert!(result.is_err(), "free before allocation should error");
    }

    /// Test async transfer on non-async buffer fails
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_async_transfer_on_non_async_buffer_error() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig {
            enable_async: false,
            ..Default::default()
        };

        manager.allocate::<f32>("sync_only", 100, config).unwrap();

        let result = manager.start_async_download::<f32>("sync_only");
        assert!(
            result.is_err(),
            "Async download on non-async buffer should fail"
        );
    }

    /// Test wait_for_download without pending transfer fails
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_wait_for_download_no_pending_error() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig {
            enable_async: true,
            ..Default::default()
        };

        manager.allocate::<f32>("async_buf", 100, config).unwrap();

        // Don't start a download, just try to wait
        let result = manager.wait_for_download::<f32>("async_buf");
        assert!(
            result.is_err(),
            "wait_for_download without pending transfer should fail"
        );
    }

    // ============================================================
    // SECTION 4: Concurrent Access Patterns
    // ============================================================
    //
    // NOTE: GpuMemoryManager uses Box<dyn Any> for type-erased buffer storage,
    // which is not Send/Sync. Thread-sharing tests are performed using
    // sequential simulation instead of actual multi-threading.
    // The internal allocations HashMap uses Arc<Mutex<...>> for safe concurrent
    // tracking of memory statistics.

    /// Test rapid sequential allocations (simulates concurrent workload)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_rapid_sequential_allocations() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");
        let num_allocations = 20;

        for i in 0..num_allocations {
            let config = BufferConfig::default();
            let buffer_name = format!("rapid_alloc_{}", i);
            manager.allocate::<f32>(&buffer_name, 100, config).unwrap();
        }

        let stats = manager.stats();
        assert_eq!(
            stats.buffer_count, num_allocations,
            "All rapid allocations should succeed"
        );
    }

    /// Test interleaved alloc/free operations (simulates concurrent behavior)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_interleaved_alloc_free_operations() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();

        // Interleave allocations and frees
        manager
            .allocate::<f32>("buf_0", 100, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("buf_1", 100, config.clone())
            .unwrap();
        manager.free("buf_0").unwrap();
        manager
            .allocate::<f32>("buf_2", 100, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("buf_3", 100, config.clone())
            .unwrap();
        manager.free("buf_1").unwrap();
        manager.free("buf_2").unwrap();
        manager.allocate::<f32>("buf_4", 100, config).unwrap();
        manager.free("buf_3").unwrap();
        manager.free("buf_4").unwrap();

        let stats = manager.stats();
        assert_eq!(stats.buffer_count, 0, "All buffers should be freed");
        assert!(manager.check_leaks().is_empty(), "No leaks expected");
    }

    /// Test stats consistency during mixed operations
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_stats_consistency_during_operations() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        for i in 0..10 {
            let config = BufferConfig::default();
            manager
                .allocate::<f32>(&format!("stats_test_{}", i), 100, config)
                .unwrap();

            let stats = manager.stats();
            assert_eq!(
                stats.buffer_count,
                i + 1,
                "Buffer count should track allocations"
            );
            assert!(
                stats.total_allocated_bytes > 0,
                "Should have bytes allocated"
            );
            assert_eq!(
                stats.allocation_count,
                i + 1,
                "Allocation count should increment"
            );
        }

        // Free half
        for i in 0..5 {
            manager.free(&format!("stats_test_{}", i)).unwrap();
            let stats = manager.stats();
            assert_eq!(stats.buffer_count, 9 - i, "Buffer count should track frees");
        }
    }

    /// Test atomic counter behavior in allocation tracking
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_atomic_allocation_tracking() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::default();
        let buffer_size = 1000;
        let element_size = std::mem::size_of::<f32>();

        manager
            .allocate::<f32>("atomic_test_1", buffer_size, config.clone())
            .unwrap();
        manager
            .allocate::<f32>("atomic_test_2", buffer_size, config.clone())
            .unwrap();

        let stats = manager.stats();
        assert_eq!(stats.allocation_count, 2);
        assert!(stats.total_allocated_bytes >= 2 * buffer_size * element_size);
        assert!(stats.peak_allocated_bytes >= stats.total_allocated_bytes);
    }

    /// Test memory tracking across resize operations
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_memory_tracking_across_resizes() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::for_positions();
        manager.allocate::<f32>("resizable", 100, config).unwrap();

        let initial_stats = manager.stats();
        let initial_allocated = initial_stats.total_allocated_bytes;

        // Resize larger
        manager.ensure_capacity::<f32>("resizable", 1000).unwrap();

        let after_resize_stats = manager.stats();
        assert!(
            after_resize_stats.total_allocated_bytes > initial_allocated,
            "Total allocated should increase after resize"
        );
        assert!(
            after_resize_stats.resize_count > 0,
            "Resize count should increment"
        );
    }

    // ============================================================
    // SECTION 5: Async Transfer Tests
    // ============================================================

    /// Test basic async transfer workflow
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_async_transfer_basic() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig {
            enable_async: true,
            ..Default::default()
        };

        manager
            .allocate::<f32>("async_buffer", 100, config)
            .unwrap();

        // Start async download
        let result = manager.start_async_download::<f32>("async_buffer");
        assert!(result.is_ok(), "start_async_download should succeed");

        // Wait for completion
        let data = manager.wait_for_download::<f32>("async_buffer");
        assert!(data.is_ok(), "wait_for_download should succeed");

        let data = data.unwrap();
        assert_eq!(data.len(), 100, "Downloaded data should match buffer size");
    }

    /// Test double buffering (ping-pong pattern)
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_async_double_buffering() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        let config = BufferConfig::for_positions();
        manager.allocate::<f32>("positions", 100, config).unwrap();

        // First transfer
        manager.start_async_download::<f32>("positions").unwrap();
        let data1 = manager.wait_for_download::<f32>("positions").unwrap();

        // Second transfer (should use alternate buffer internally)
        manager.start_async_download::<f32>("positions").unwrap();
        let data2 = manager.wait_for_download::<f32>("positions").unwrap();

        assert_eq!(data1.len(), 100);
        assert_eq!(data2.len(), 100);

        let stats = manager.stats();
        assert_eq!(
            stats.async_transfer_count, 2,
            "Should record 2 async transfers"
        );
    }

    // ============================================================
    // SECTION 6: Full Lifecycle Tests
    // ============================================================

    /// Test complete buffer lifecycle: allocate -> access -> resize -> async -> free
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_complete_lifecycle() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        // 1. Allocate with async support
        let config = BufferConfig::for_positions();
        manager.allocate::<f32>("lifecycle", 100, config).unwrap();
        assert_eq!(manager.stats().buffer_count, 1);

        // 2. Access buffer (immutable)
        let _buffer = manager.get_buffer::<f32>("lifecycle").unwrap();

        // 3. Access buffer (mutable)
        let _buffer_mut = manager.get_buffer_mut::<f32>("lifecycle").unwrap();

        // 4. Resize larger
        manager.ensure_capacity::<f32>("lifecycle", 500).unwrap();
        assert!(manager.stats().resize_count > 0);

        // 5. Async transfer
        manager.start_async_download::<f32>("lifecycle").unwrap();
        let data = manager.wait_for_download::<f32>("lifecycle").unwrap();
        assert!(data.len() >= 500); // Should be at least requested size

        // 6. Free
        manager.free("lifecycle").unwrap();
        assert_eq!(manager.stats().buffer_count, 0);

        // 7. Verify no leaks
        let leaks = manager.check_leaks();
        assert!(leaks.is_empty(), "No leaks after proper lifecycle");
    }

    /// Test multiple buffers with different configs through full lifecycle
    #[test]
    #[ignore = "Requires CUDA device"]
    fn test_multiple_buffers_lifecycle() {
        let mut manager = GpuMemoryManager::new().expect("Manager creation failed");

        // Allocate different buffer types
        manager
            .allocate::<f32>("positions", 1000, BufferConfig::for_positions())
            .unwrap();
        manager
            .allocate::<f32>("velocities", 1000, BufferConfig::for_velocities())
            .unwrap();
        manager
            .allocate::<u8>("edges", 500, BufferConfig::for_edges())
            .unwrap();
        manager
            .allocate::<i32>("grid", 200, BufferConfig::for_grid_cells())
            .unwrap();

        assert_eq!(manager.stats().buffer_count, 4);

        // Resize some
        manager.ensure_capacity::<f32>("positions", 5000).unwrap();
        manager.ensure_capacity::<f32>("velocities", 5000).unwrap();

        // Async transfers on async-enabled buffers
        manager.start_async_download::<f32>("positions").unwrap();
        let _ = manager.wait_for_download::<f32>("positions").unwrap();

        manager.start_async_download::<f32>("velocities").unwrap();
        let _ = manager.wait_for_download::<f32>("velocities").unwrap();

        // Free all
        manager.free("positions").unwrap();
        manager.free("velocities").unwrap();
        manager.free("edges").unwrap();
        manager.free("grid").unwrap();

        assert_eq!(manager.stats().buffer_count, 0);
        assert!(manager.check_leaks().is_empty());
    }
}

// ============================================================
// Unit tests that don't require GPU (config validation, etc.)
// ============================================================
#[cfg(test)]
mod config_tests {
    use visionclaw_server::gpu::memory_manager::BufferConfig;

    #[test]
    fn test_default_config_values() {
        let config = BufferConfig::default();
        assert_eq!(config.bytes_per_element, 4);
        assert_eq!(config.growth_factor, 1.5);
        assert_eq!(config.max_size_bytes, 1024 * 1024 * 1024);
        assert_eq!(config.min_size_bytes, 4096);
        assert!(!config.enable_async);
    }

    #[test]
    fn test_positions_preset() {
        let config = BufferConfig::for_positions();
        assert_eq!(config.bytes_per_element, 12); // f32 * 3
        assert_eq!(config.growth_factor, 1.3);
        assert_eq!(config.max_size_bytes, 512 * 1024 * 1024);
        assert!(config.enable_async);
    }

    #[test]
    fn test_velocities_preset() {
        let config = BufferConfig::for_velocities();
        assert_eq!(config.bytes_per_element, 12); // f32 * 3
        assert_eq!(config.growth_factor, 1.3);
        assert!(config.enable_async);
    }

    #[test]
    fn test_edges_preset() {
        let config = BufferConfig::for_edges();
        assert_eq!(config.bytes_per_element, 32);
        assert_eq!(config.growth_factor, 2.0);
        assert_eq!(config.max_size_bytes, 2048 * 1024 * 1024);
        assert!(!config.enable_async);
    }

    #[test]
    fn test_grid_cells_preset() {
        let config = BufferConfig::for_grid_cells();
        assert_eq!(config.bytes_per_element, 8);
        assert_eq!(config.growth_factor, 1.5);
        assert_eq!(config.max_size_bytes, 256 * 1024 * 1024);
        assert!(!config.enable_async);
    }

    #[test]
    fn test_config_clone() {
        let config = BufferConfig::for_positions();
        let cloned = config.clone();
        assert_eq!(config.bytes_per_element, cloned.bytes_per_element);
        assert_eq!(config.growth_factor, cloned.growth_factor);
        assert_eq!(config.enable_async, cloned.enable_async);
    }
}
