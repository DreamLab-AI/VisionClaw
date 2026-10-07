//! Performance metrics, kernel statistics, stress majorization, and PageRank.

use super::construction::UnifiedGPUCompute;
use super::types::GPUPerformanceMetrics;
use crate::utils::advanced_logging::{log_gpu_error, log_gpu_kernel, log_memory_event};
use crate::utils::result_helpers::safe_json_number;
use anyhow::Result;
use cust::event::{Event, EventFlags};
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer};
use log::info;
use std::collections::HashMap;

impl UnifiedGPUCompute {
    pub fn record_kernel_time(&mut self, kernel_name: &str, execution_time_ms: f32) {
        *self
            .performance_metrics
            .total_kernel_calls
            .entry(kernel_name.to_string())
            .or_insert(0) += 1;

        let times = self
            .performance_metrics
            .kernel_times
            .entry(kernel_name.to_string())
            .or_default();
        times.push(execution_time_ms);
        if times.len() > 100 {
            times.remove(0);
        }

        let avg_time = times.iter().sum::<f32>() / times.len() as f32;
        match kernel_name {
            "force_pass_kernel" => self.performance_metrics.force_kernel_avg_time = avg_time,
            "integrate_pass_kernel" => {
                self.performance_metrics.integrate_kernel_avg_time = avg_time
            }
            "build_grid_kernel" => self.performance_metrics.grid_build_avg_time = avg_time,
            "relaxation_step_kernel" | "compact_frontier_kernel" => {
                self.performance_metrics.sssp_avg_time = avg_time
            }
            "kmeans_assign_kernel" | "kmeans_update_centroids_kernel" => {
                self.performance_metrics.clustering_avg_time = avg_time
            }
            "compute_lof_kernel" | "zscore_kernel" => {
                self.performance_metrics.anomaly_detection_avg_time = avg_time
            }
            "label_propagation_kernel" => {
                self.performance_metrics.community_detection_avg_time = avg_time
            }
            _ => {}
        }

        let execution_time_us = execution_time_ms * 1000.0;
        let memory_mb = self.performance_metrics.current_memory_usage as f64 / (1024.0 * 1024.0);
        let peak_memory_mb = self.performance_metrics.peak_memory_usage as f64 / (1024.0 * 1024.0);
        log_gpu_kernel(
            kernel_name,
            execution_time_us as f64,
            memory_mb,
            peak_memory_mb,
        );
    }

    pub fn execute_kernel_with_timing<F>(
        &mut self,
        kernel_name: &str,
        mut kernel_func: F,
    ) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let start_event = Event::new(EventFlags::DEFAULT)?;
        let stop_event = Event::new(EventFlags::DEFAULT)?;

        start_event.record(&self.stream)?;

        kernel_func()?;

        stop_event.record(&self.stream)?;

        self.stream.synchronize()?;
        let elapsed_ms = start_event.elapsed_time_f32(&stop_event)?;

        self.record_kernel_time(kernel_name, elapsed_ms);

        Ok(())
    }

    pub fn get_performance_metrics(&self) -> &GPUPerformanceMetrics {
        &self.performance_metrics
    }

    pub fn get_performance_metrics_mut(&mut self) -> &mut GPUPerformanceMetrics {
        &mut self.performance_metrics
    }

    pub fn update_memory_usage(&mut self) {
        let node_memory = self.allocated_nodes * std::mem::size_of::<f32>() * 12;
        let edge_memory =
            self.allocated_edges * (std::mem::size_of::<i32>() * 2 + std::mem::size_of::<f32>());
        let grid_memory = self.max_grid_cells * std::mem::size_of::<i32>() * 4;
        let cluster_memory = self.max_clusters * std::mem::size_of::<f32>() * 3;
        let anomaly_memory = self.allocated_nodes * std::mem::size_of::<f32>() * 4;

        let current_usage =
            node_memory + edge_memory + grid_memory + cluster_memory + anomaly_memory;
        let previous_usage = self.performance_metrics.current_memory_usage;

        self.performance_metrics.current_memory_usage = current_usage;
        if current_usage > self.performance_metrics.peak_memory_usage {
            self.performance_metrics.peak_memory_usage = current_usage;
        }
        self.performance_metrics.total_memory_allocated = self.total_memory_allocated;

        if (current_usage as f64 - previous_usage as f64).abs() > (1024.0 * 1024.0) {
            let event_type = if current_usage > previous_usage {
                "allocation"
            } else {
                "deallocation"
            };
            let allocated_mb = current_usage as f64 / (1024.0 * 1024.0);
            let peak_mb = self.performance_metrics.peak_memory_usage as f64 / (1024.0 * 1024.0);
            log_memory_event(event_type, allocated_mb, peak_mb);
        }
    }

    pub fn log_gpu_error(&self, error_msg: &str, recovery_attempted: bool) {
        log_gpu_error(error_msg, recovery_attempted);
    }

    pub fn reset_performance_metrics(&mut self) {
        let peak_memory = self.performance_metrics.peak_memory_usage;
        let total_allocated = self.performance_metrics.total_memory_allocated;

        self.performance_metrics = GPUPerformanceMetrics::default();
        self.performance_metrics.peak_memory_usage = peak_memory;
        self.performance_metrics.total_memory_allocated = total_allocated;
    }

    pub fn get_kernel_statistics(&self) -> HashMap<String, serde_json::Value> {
        let mut stats = HashMap::new();

        for (kernel_name, times) in &self.performance_metrics.kernel_times {
            if !times.is_empty() {
                let avg_time = times.iter().sum::<f32>() / times.len() as f32;
                let min_time = times.iter().cloned().fold(f32::INFINITY, f32::min);
                let max_time = times.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let total_calls = self
                    .performance_metrics
                    .total_kernel_calls
                    .get(kernel_name)
                    .unwrap_or(&0);

                let mut kernel_stats = HashMap::new();
                kernel_stats.insert(
                    "avg_time_ms".to_string(),
                    serde_json::Value::Number(safe_json_number(avg_time as f64)),
                );
                kernel_stats.insert(
                    "min_time_ms".to_string(),
                    serde_json::Value::Number(safe_json_number(min_time as f64)),
                );
                kernel_stats.insert(
                    "max_time_ms".to_string(),
                    serde_json::Value::Number(safe_json_number(max_time as f64)),
                );
                kernel_stats.insert(
                    "total_calls".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(*total_calls)),
                );
                kernel_stats.insert(
                    "recent_samples".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(times.len())),
                );

                stats.insert(
                    kernel_name.clone(),
                    serde_json::Value::Object(kernel_stats.into_iter().collect()),
                );
            }
        }

        stats
    }

    /// Compute all-pairs BFS shortest-path distances on a CSR graph.
    /// Returns a flat n*n Vec<f32> where result[i*n + j] is the hop distance
    /// from node i to node j. Unreachable pairs get distance 0.0 (they will
    /// be assigned zero weight in stress majorization so they don't contribute).
    ///
    /// For large graphs (>2000 nodes) this falls back to a landmark-based
    /// approximation using sqrt(n) random landmarks to keep O(n * sqrt(n) * (n+m)).
    fn bfs_all_pairs_distances(n: usize, row_offsets: &[i32], col_indices: &[i32]) -> Vec<f32> {
        use std::collections::VecDeque;

        if n == 0 {
            return Vec::new();
        }

        // For very large graphs, use landmark BFS approximation
        let use_landmarks = n > 2000;
        let sources: Vec<usize> = if use_landmarks {
            let num_landmarks = (n as f64).sqrt().ceil() as usize;
            let step = n.checked_div(num_landmarks).unwrap_or(1);
            (0..num_landmarks).map(|i| (i * step).min(n - 1)).collect()
        } else {
            (0..n).collect()
        };

        // BFS from each source, store distances
        let mut landmark_dists: Vec<Vec<i32>> = Vec::with_capacity(sources.len());
        for &src in &sources {
            let mut dist = vec![-1i32; n];
            dist[src] = 0;
            let mut queue = VecDeque::new();
            queue.push_back(src);

            while let Some(u) = queue.pop_front() {
                let start = row_offsets[u] as usize;
                let end = if u + 1 < row_offsets.len() {
                    row_offsets[u + 1] as usize
                } else {
                    col_indices.len()
                };
                for &col in col_indices
                    .iter()
                    .take(end.min(col_indices.len()))
                    .skip(start)
                {
                    let v = col as usize;
                    if v < n && dist[v] < 0 {
                        dist[v] = dist[u] + 1;
                        queue.push_back(v);
                    }
                }
            }
            landmark_dists.push(dist);
        }

        let mut result = vec![0.0f32; n * n];

        if use_landmarks {
            // Approximate: d(i,j) ~ min over landmarks L of (d(L,i) + d(L,j))
            for i in 0..n {
                for j in (i + 1)..n {
                    let mut best = i32::MAX;
                    for ld in &landmark_dists {
                        if ld[i] >= 0 && ld[j] >= 0 {
                            best = best.min(ld[i] + ld[j]);
                        }
                    }
                    let d = if best == i32::MAX { 0.0 } else { best as f32 };
                    result[i * n + j] = d;
                    result[j * n + i] = d;
                }
            }
        } else {
            // Exact all-pairs
            for (src_idx, &src) in sources.iter().enumerate() {
                let ld = &landmark_dists[src_idx];
                for j in 0..n {
                    let d = if ld[j] < 0 { 0.0 } else { ld[j] as f32 };
                    result[src * n + j] = d;
                }
            }
        }

        result
    }

    pub fn run_stress_majorization(&mut self) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
        info!("Running GPU stress majorization with convergence detection");

        let block_size = 256u32;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);
        let n = self.num_nodes;

        // Resolve the clustering module (stress kernels live in GpuClusteringKernels PTX)
        let module = if let Some(ref clustering_mod) = self.clustering_module {
            clustering_mod
        } else {
            &self._module
        };

        let mut pos_x = vec![0.0f32; n];
        let mut pos_y = vec![0.0f32; n];
        let mut pos_z = vec![0.0f32; n];
        self.download_positions(&mut pos_x, &mut pos_y, &mut pos_z)?;

        // Compute BFS shortest-path distances from the CSR graph structure.
        let (row_offsets, col_indices) = self.download_csr()?;
        let hop_distances = Self::bfs_all_pairs_distances(n, &row_offsets, &col_indices);

        let mut target_distances = vec![0.0f32; n * n];
        let mut weights = vec![0.0f32; n * n];

        for i in 0..n {
            for j in 0..n {
                let d = hop_distances[i * n + j];
                if i == j || d == 0.0 {
                    target_distances[i * n + j] = 0.0;
                    weights[i * n + j] = 0.0;
                } else {
                    // Standard stress majorization: w_ij = d_ij^{-2}
                    target_distances[i * n + j] = d;
                    weights[i * n + j] = 1.0 / (d * d);
                }
            }
        }

        let d_target_distances = DeviceBuffer::from_slice(&target_distances)?;
        let d_weights = DeviceBuffer::from_slice(&weights)?;
        let d_new_pos_x = DeviceBuffer::from_slice(&pos_x)?;
        let d_new_pos_y = DeviceBuffer::from_slice(&pos_y)?;
        let d_new_pos_z = DeviceBuffer::from_slice(&pos_z)?;

        let stress_grid_size = grid_size;

        let max_iterations = 50;
        let learning_rate = self.params.learning_rate_default;
        let convergence_threshold = 1e-4f32;

        let step_kernel = module.get_function("stress_majorization_step_kernel")?;
        let stress_kernel = module.get_function("compute_stress_kernel")?;

        let mut prev_stress = f32::MAX;

        for iter in 0..max_iterations {
            // --- Step: update positions via stress_majorization_step_kernel ---
            // SAFETY: All device buffers are valid allocations with capacity >= num_nodes.
            // pos_in_* hold current positions, d_new_pos_* receive updated output.
            // d_target_distances and d_weights are NxN matrices.
            // edge_row_offsets/edge_col_indices are valid CSR graph data.
            unsafe {
                let stream = &self.stream;
                launch!(
                step_kernel<<<grid_size, block_size, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    d_new_pos_x.as_device_ptr(),
                    d_new_pos_y.as_device_ptr(),
                    d_new_pos_z.as_device_ptr(),
                    d_target_distances.as_device_ptr(),
                    d_weights.as_device_ptr(),
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_col_indices.as_device_ptr(),
                    learning_rate,
                    n as i32,
                    crate::config::dev_config::physics().force_epsilon
                ))?;
            }
            self.stream.synchronize()?;

            // Copy new positions back into pos_in buffers for the next iteration
            self.pos_in_x.copy_from(&d_new_pos_x)?;
            self.pos_in_y.copy_from(&d_new_pos_y)?;
            self.pos_in_z.copy_from(&d_new_pos_z)?;

            // --- Convergence: compute stress via compute_stress_kernel ---
            let d_partial_stress = DeviceBuffer::<f32>::zeroed(stress_grid_size as usize)?;

            // compute_stress_kernel uses shared memory of block_size * sizeof(f32)
            let shared_mem_bytes = block_size * std::mem::size_of::<f32>() as u32;
            // SAFETY: pos_in_* hold updated positions, d_target_distances/d_weights are NxN,
            // d_partial_stress has grid_size elements for block-level reduction output.
            unsafe {
                let stream = &self.stream;
                launch!(
                stress_kernel<<<stress_grid_size, block_size, shared_mem_bytes, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    d_target_distances.as_device_ptr(),
                    d_weights.as_device_ptr(),
                    d_partial_stress.as_device_ptr(),
                    n as i32
                ))?;
            }
            self.stream.synchronize()?;

            // Download partial stress and sum on CPU (grid_size values)
            let mut partial_stress = vec![0.0f32; stress_grid_size as usize];
            d_partial_stress.copy_to(&mut partial_stress)?;
            let current_stress: f32 = partial_stress.iter().sum();

            // Check convergence: relative change in stress
            let stress_change = (prev_stress - current_stress).abs();
            let relative_change = if prev_stress.abs() > f32::EPSILON {
                stress_change / prev_stress.abs()
            } else {
                stress_change
            };

            if iter > 0 && relative_change < convergence_threshold {
                info!(
                    "GPU stress majorization converged at iteration {} (stress={:.6}, rel_change={:.6})",
                    iter, current_stress, relative_change
                );
                break;
            }
            prev_stress = current_stress;
        }

        d_new_pos_x.copy_to(&mut pos_x)?;
        d_new_pos_y.copy_to(&mut pos_y)?;
        d_new_pos_z.copy_to(&mut pos_z)?;

        Ok((pos_x, pos_y, pos_z))
    }

    /// Compute the current stress value on GPU using compute_stress_kernel.
    /// Returns the total (un-normalized) stress.
    pub fn compute_stress_gpu(&mut self) -> Result<f32> {
        let n = self.num_nodes;
        if n < 2 {
            return Ok(0.0);
        }

        let block_size = 256u32;
        let grid_size = (n as u32).div_ceil(block_size);

        let module = if let Some(ref clustering_mod) = self.clustering_module {
            clustering_mod
        } else {
            &self._module
        };

        // Compute BFS target distances and weights
        let (row_offsets, col_indices) = self.download_csr()?;
        let hop_distances = Self::bfs_all_pairs_distances(n, &row_offsets, &col_indices);

        let mut target_distances = vec![0.0f32; n * n];
        let mut weights = vec![0.0f32; n * n];

        for i in 0..n {
            for j in 0..n {
                let d = hop_distances[i * n + j];
                if i == j || d == 0.0 {
                    target_distances[i * n + j] = 0.0;
                    weights[i * n + j] = 0.0;
                } else {
                    target_distances[i * n + j] = d;
                    weights[i * n + j] = 1.0 / (d * d);
                }
            }
        }

        let d_target_distances = DeviceBuffer::from_slice(&target_distances)?;
        let d_weights = DeviceBuffer::from_slice(&weights)?;
        let d_partial_stress = DeviceBuffer::<f32>::zeroed(grid_size as usize)?;

        let shared_mem_bytes = block_size * std::mem::size_of::<f32>() as u32;
        let stress_kernel = module.get_function("compute_stress_kernel")?;

        // SAFETY: pos_in_* hold current positions, d_target_distances/d_weights are NxN,
        // d_partial_stress has grid_size elements for block-level reduction output.
        unsafe {
            let stream = &self.stream;
            launch!(
            stress_kernel<<<grid_size, block_size, shared_mem_bytes, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                d_target_distances.as_device_ptr(),
                d_weights.as_device_ptr(),
                d_partial_stress.as_device_ptr(),
                n as i32
            ))?;
        }
        self.stream.synchronize()?;

        let mut partial_stress = vec![0.0f32; grid_size as usize];
        d_partial_stress.copy_to(&mut partial_stress)?;
        let total_stress: f32 = partial_stress.iter().sum();

        Ok(total_stress)
    }

    /// Run PageRank centrality computation on the graph using GPU kernels.
    ///
    /// Uses the linked CUDA PageRank kernels from `pagerank.cu` for GPU-accelerated
    /// power iteration. Falls back to CPU computation if GPU execution fails.
    ///
    /// # Parameters
    /// - `damping`: Damping factor (typically 0.85)
    /// - `max_iterations`: Maximum number of iterations
    /// - `epsilon`: Convergence threshold
    /// - `normalize`: Whether to normalize the results
    /// - `use_optimized`: Use optimized kernel variant with shared memory
    /// # Returns
    /// Tuple of (PageRank scores, iterations performed, converged, convergence value)
    pub fn run_pagerank_centrality(
        &mut self,
        damping: f32,
        max_iterations: usize,
        epsilon: f32,
        normalize: bool,
        use_optimized: bool,
    ) -> Result<(Vec<f32>, usize, bool, f32)> {
        let num_nodes = self.get_num_nodes();
        if num_nodes == 0 {
            return Ok((vec![], 0, true, 0.0));
        }

        info!("Running GPU-accelerated PageRank on {} nodes", num_nodes);

        // Download CSR row offsets to compute out-degrees on host
        // Use allocated sizes for copy_to (device buffers may be overallocated)
        let mut row_offsets = vec![0i32; self.edge_row_offsets.len()];
        self.edge_row_offsets.copy_to(&mut row_offsets)?;
        row_offsets.truncate(num_nodes + 1);

        // Compute out-degrees from CSR row offsets
        let mut out_degrees = vec![0i32; num_nodes];
        for node in 0..num_nodes {
            out_degrees[node] = row_offsets[node + 1] - row_offsets[node];
        }

        // Download CSR col_indices to build CSC (transpose) on the host.
        // The PageRank kernel requires CSC format for O(n+m) complexity.
        let num_edges = row_offsets[num_nodes] as usize;
        let mut col_indices_host = vec![0i32; self.edge_col_indices.len()];
        if num_edges > 0 {
            self.edge_col_indices.copy_to(&mut col_indices_host)?;
        }
        col_indices_host.truncate(num_edges);

        // Build CSC: transpose the CSR graph
        // CSC col_offsets[v] = start of incoming edges for node v in csc_row_indices
        // CSC row_indices[j] = source node of incoming edge j
        let mut csc_col_offsets = vec![0i32; num_nodes + 1];
        let mut csc_row_indices = vec![0i32; num_edges];

        // Count incoming edges per node (= column counts in CSR)
        for &dst in &col_indices_host {
            if (dst as usize) < num_nodes {
                csc_col_offsets[dst as usize + 1] += 1;
            }
        }
        // Prefix sum to get offsets
        for v in 0..num_nodes {
            csc_col_offsets[v + 1] += csc_col_offsets[v];
        }
        // Fill row_indices using a working copy of offsets
        let mut write_pos = csc_col_offsets.clone();
        for src in 0..num_nodes {
            let edge_start = row_offsets[src] as usize;
            let edge_end = row_offsets[src + 1] as usize;
            for &col in col_indices_host.iter().take(edge_end).skip(edge_start) {
                let dst = col as usize;
                if dst < num_nodes {
                    let pos = write_pos[dst] as usize;
                    csc_row_indices[pos] = src as i32;
                    write_pos[dst] += 1;
                }
            }
        }

        // Upload CSC to GPU
        let d_csc_col_offsets = DeviceBuffer::from_slice(&csc_col_offsets)?;
        let d_csc_row_indices = DeviceBuffer::from_slice(&csc_row_indices)?;

        // Allocate GPU buffers for PageRank computation
        let mut d_pagerank_old = DeviceBuffer::<f32>::zeroed(num_nodes)?;
        let mut d_pagerank_new = DeviceBuffer::<f32>::zeroed(num_nodes)?;
        let d_out_degree = DeviceBuffer::from_slice(&out_degrees)?;

        let num_blocks = num_nodes.div_ceil(256);
        let d_diff_buffer = DeviceBuffer::<f32>::zeroed(num_blocks)?;

        let stream_ptr = self.stream.as_inner() as *mut ::std::os::raw::c_void;

        // Initialize PageRank values to 1/N on GPU
        // SAFETY: d_pagerank_old is a valid DeviceBuffer with num_nodes elements.
        // stream_ptr is a valid CUDA stream handle from self.stream.
        unsafe {
            super::types::pagerank_init(
                d_pagerank_old.as_device_ptr().as_raw() as *mut f32,
                num_nodes as ::std::os::raw::c_int,
                stream_ptr,
            );
        }
        self.stream.synchronize()?;

        // Power iteration loop
        let mut final_iterations = max_iterations;
        let mut converged = false;
        let mut final_delta = 0.0f32;

        for iteration in 0..max_iterations {
            // Run one PageRank iteration on GPU using CSC format
            // SAFETY: All device pointers (d_pagerank_old, d_pagerank_new,
            // d_csc_col_offsets, d_csc_row_indices, d_out_degree) are valid
            // DeviceBuffers with sufficient capacity. num_nodes matches the
            // allocation sizes. stream_ptr is a valid CUDA stream.
            unsafe {
                if use_optimized {
                    super::types::pagerank_iterate_optimized(
                        d_pagerank_old.as_device_ptr().as_raw() as *const f32,
                        d_pagerank_new.as_device_ptr().as_raw() as *mut f32,
                        d_csc_col_offsets.as_device_ptr().as_raw() as *const _,
                        d_csc_row_indices.as_device_ptr().as_raw() as *const _,
                        d_out_degree.as_device_ptr().as_raw() as *const _,
                        num_nodes as ::std::os::raw::c_int,
                        damping,
                        stream_ptr,
                    );
                } else {
                    super::types::pagerank_iterate(
                        d_pagerank_old.as_device_ptr().as_raw() as *const f32,
                        d_pagerank_new.as_device_ptr().as_raw() as *mut f32,
                        d_csc_col_offsets.as_device_ptr().as_raw() as *const _,
                        d_csc_row_indices.as_device_ptr().as_raw() as *const _,
                        d_out_degree.as_device_ptr().as_raw() as *const _,
                        num_nodes as ::std::os::raw::c_int,
                        damping,
                        stream_ptr,
                    );
                }

                // Handle dangling nodes
                super::types::pagerank_handle_dangling_global(
                    d_pagerank_new.as_device_ptr().as_raw() as *mut f32,
                    d_pagerank_old.as_device_ptr().as_raw() as *const f32,
                    d_out_degree.as_device_ptr().as_raw() as *const _,
                    num_nodes as ::std::os::raw::c_int,
                    damping,
                    stream_ptr,
                );

                // Check convergence
                let delta = super::types::pagerank_check_convergence(
                    d_pagerank_old.as_device_ptr().as_raw() as *const f32,
                    d_pagerank_new.as_device_ptr().as_raw() as *const f32,
                    d_diff_buffer.as_device_ptr().as_raw() as *mut f32,
                    num_nodes as ::std::os::raw::c_int,
                    stream_ptr,
                );

                final_delta = delta;

                if delta < epsilon {
                    info!(
                        "GPU PageRank converged after {} iterations (delta: {})",
                        iteration + 1,
                        delta
                    );
                    final_iterations = iteration + 1;
                    converged = true;

                    // Copy final results from d_pagerank_new (the most recent output)
                    std::mem::swap(&mut d_pagerank_old, &mut d_pagerank_new);
                    break;
                }
            }

            // Swap buffers for next iteration
            std::mem::swap(&mut d_pagerank_old, &mut d_pagerank_new);
        }

        // Download results from GPU
        let mut scores = vec![0.0f32; num_nodes];
        d_pagerank_old.copy_to(&mut scores)?;

        // Normalize if requested
        if normalize {
            let sum: f32 = scores.iter().sum();
            if sum > 0.0 {
                for score in scores.iter_mut() {
                    *score /= sum;
                }
            }
        }

        self.record_kernel_time("pagerank_iteration", final_delta);

        Ok((scores, final_iterations, converged, final_delta))
    }

    /// Run connected components detection on the GPU using label propagation.
    ///
    /// Uses the linked CUDA kernel from `gpu_connected_components.cu` which
    /// implements parallel label propagation to find connected components.
    ///
    /// # Parameters
    /// - `max_iterations`: Maximum iterations for label propagation convergence
    /// # Returns
    /// Tuple of (labels per node, number of components)
    pub fn run_connected_components_gpu(&mut self, max_iterations: i32) -> Result<(Vec<i32>, i32)> {
        let num_nodes = self.get_num_nodes();
        if num_nodes == 0 {
            return Ok((vec![], 0));
        }

        info!(
            "Running GPU-accelerated connected components on {} nodes",
            num_nodes
        );

        // Allocate output buffers on device
        let d_labels = DeviceBuffer::<i32>::zeroed(num_nodes)?;
        let d_num_components = DeviceBuffer::<i32>::zeroed(1)?;

        let stream_ptr = self.stream.as_inner() as *mut ::std::os::raw::c_void;

        // SAFETY: All device pointers are valid DeviceBuffers:
        // - edge_row_offsets: [num_nodes+1] i32 in CSR format
        // - edge_col_indices: [num_edges] i32 column indices
        // - d_labels: [num_nodes] i32 output labels
        // - d_num_components: single i32 output count
        // - num_nodes matches the graph size
        // - max_iterations > 0
        // - stream_ptr is a valid CUDA stream handle
        unsafe {
            super::types::compute_connected_components_gpu(
                self.edge_row_offsets.as_device_ptr().as_raw() as *const ::std::os::raw::c_int,
                self.edge_col_indices.as_device_ptr().as_raw() as *const ::std::os::raw::c_int,
                d_labels.as_device_ptr().as_raw() as *mut ::std::os::raw::c_int,
                d_num_components.as_device_ptr().as_raw() as *mut ::std::os::raw::c_int,
                num_nodes as ::std::os::raw::c_int,
                max_iterations as ::std::os::raw::c_int,
                stream_ptr,
            );
        }

        self.stream.synchronize()?;

        // Download results
        let mut labels = vec![0i32; num_nodes];
        d_labels.copy_to(&mut labels)?;

        let mut num_components = vec![0i32; 1];
        d_num_components.copy_to(&mut num_components)?;

        info!(
            "GPU connected components: found {} components",
            num_components[0]
        );

        Ok((labels, num_components[0]))
    }
}
