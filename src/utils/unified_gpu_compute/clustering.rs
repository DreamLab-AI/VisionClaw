//! Clustering algorithms: K-means, LOF anomaly detection, Z-score anomaly detection.

use super::construction::UnifiedGPUCompute;
use super::types::int3;
use super::types::{GpuKMeansMetricsOutput, GpuKMeansOutput};
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer};
use log::info;

/// Safe download: allocate buffer matching device buffer size, copy, then truncate
/// to the requested logical size. Avoids panics from overallocated device buffers.
pub(crate) fn safe_download<T: Default + Clone + cust::memory::DeviceCopy>(
    device_buf: &DeviceBuffer<T>,
    logical_size: usize,
) -> Result<Vec<T>> {
    let mut full = vec![T::default(); device_buf.len()];
    device_buf.copy_to(&mut full)?;
    full.truncate(logical_size);
    Ok(full)
}

/// Safe upload: pad host slice to match device buffer size if needed.
/// Avoids panics when device buffers are overallocated (e.g. 1.5x growth).
pub(crate) fn safe_upload<T: Default + Clone + cust::memory::DeviceCopy>(
    device_buf: &mut DeviceBuffer<T>,
    data: &[T],
) -> Result<()> {
    if data.len() == device_buf.len() {
        device_buf.copy_from(data)?;
    } else {
        let mut padded = data.to_vec();
        padded.resize(device_buf.len(), T::default());
        device_buf.copy_from(&padded)?;
    }
    Ok(())
}

impl UnifiedGPUCompute {
    pub fn run_kmeans(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<GpuKMeansOutput> {
        // Make CUDA context current for this thread (required when called from spawn_blocking)
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for k-means: {}", e))?;

        if num_clusters > self.max_clusters {
            return Err(anyhow!(
                "Too many clusters requested: {} > {}",
                num_clusters,
                self.max_clusters
            ));
        }

        let module = if let Some(ref clustering_mod) = self.clustering_module {
            clustering_mod
        } else {
            &self._module
        };

        let block_size = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);

        for centroid in 0..num_clusters {
            let init_kernel = module.get_function("init_centroids_kernel")?;
            let shared_memory_size = block_size * 4;
            let stream = &self.stream;

            unsafe {
                launch!(
                    init_kernel<<<num_clusters as u32, block_size, shared_memory_size, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.min_distances.as_device_ptr(),
                    self.selected_nodes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32,
                    centroid as i32,
                    seed
                ))?;
            }
            self.stream.synchronize()?;
        }

        let mut prev_inertia = f32::INFINITY;
        let mut final_inertia = 0.0f32;

        for _iteration in 0..max_iterations {
            let assign_kernel = self._module.get_function("assign_clusters_kernel")?;
            let stream = &self.stream;
            unsafe {
                launch!(
                    assign_kernel<<<grid_size, block_size, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.distances_to_centroid.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            let update_kernel = self._module.get_function("update_centroids_kernel")?;
            let centroid_shared_memory = block_size * (3 * 4 + 4);
            let stream = &self.stream;
            unsafe {
                launch!(
                    update_kernel<<<num_clusters as u32, block_size, centroid_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            let inertia_kernel = self._module.get_function("compute_inertia_kernel")?;
            let inertia_shared_memory = block_size * 4;
            let stream = &self.stream;
            unsafe {
                launch!(
                    inertia_kernel<<<grid_size, block_size, inertia_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.partial_inertia.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;

            let partial_inertias = safe_download(&self.partial_inertia, grid_size as usize)?;

            let current_inertia: f32 = partial_inertias.iter().sum();
            final_inertia = current_inertia;

            if (prev_inertia - current_inertia).abs() < tolerance {
                info!(
                    "K-means converged at iteration {} with inertia {:.4}",
                    _iteration, current_inertia
                );
                break;
            }

            prev_inertia = current_inertia;
        }

        let assignments = safe_download(&self.cluster_assignments, self.num_nodes)?;

        let centroids_x = safe_download(&self.centroids_x, num_clusters)?;
        let centroids_y = safe_download(&self.centroids_y, num_clusters)?;
        let centroids_z = safe_download(&self.centroids_z, num_clusters)?;

        let centroids: Vec<(f32, f32, f32)> = centroids_x
            .into_iter()
            .zip(centroids_y)
            .zip(centroids_z)
            .map(|((x, y), z)| (x, y, z))
            .collect();

        Ok((assignments, centroids, final_inertia))
    }

    pub fn run_kmeans_clustering_with_metrics(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<GpuKMeansMetricsOutput> {
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for k-means: {}", e))?;

        if num_clusters > self.max_clusters {
            return Err(anyhow!(
                "Too many clusters requested: {} > {}",
                num_clusters,
                self.max_clusters
            ));
        }

        let block_size = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);

        for centroid in 0..num_clusters {
            let init_kernel = self._module.get_function("init_centroids_kernel")?;
            let shared_memory_size = block_size * 4;
            let stream = &self.stream;

            unsafe {
                launch!(
                    init_kernel<<<num_clusters as u32, block_size, shared_memory_size, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.min_distances.as_device_ptr(),
                    self.selected_nodes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32,
                    centroid as i32,
                    seed
                ))?;
            }
            self.stream.synchronize()?;
        }

        let mut prev_inertia = f32::INFINITY;
        let mut final_inertia = 0.0f32;
        let mut converged = false;
        let mut actual_iterations = 0u32;

        for iteration in 0..max_iterations {
            actual_iterations = iteration + 1;

            let assign_kernel = self._module.get_function("assign_clusters_kernel")?;
            let stream = &self.stream;
            unsafe {
                launch!(
                    assign_kernel<<<grid_size, block_size, 0, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.distances_to_centroid.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            let update_kernel = self._module.get_function("update_centroids_kernel")?;
            let centroid_shared_memory = block_size * (3 * 4 + 4);
            let stream = &self.stream;
            unsafe {
                launch!(
                    update_kernel<<<num_clusters as u32, block_size, centroid_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    num_clusters as i32
                ))?;
            }

            let inertia_kernel = self._module.get_function("compute_inertia_kernel")?;
            let inertia_shared_memory = block_size * 4;
            let stream = &self.stream;
            unsafe {
                launch!(
                    inertia_kernel<<<grid_size, block_size, inertia_shared_memory, stream>>>(
                    self.pos_in_x.as_device_ptr(),
                    self.pos_in_y.as_device_ptr(),
                    self.pos_in_z.as_device_ptr(),
                    self.centroids_x.as_device_ptr(),
                    self.centroids_y.as_device_ptr(),
                    self.centroids_z.as_device_ptr(),
                    self.cluster_assignments.as_device_ptr(),
                    self.partial_inertia.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;

            let partial_inertias = safe_download(&self.partial_inertia, grid_size as usize)?;

            let current_inertia: f32 = partial_inertias.iter().sum();
            final_inertia = current_inertia;

            if (prev_inertia - current_inertia).abs() < tolerance {
                info!(
                    "K-means converged at iteration {} with inertia {:.4}",
                    iteration, current_inertia
                );
                converged = true;
                break;
            }

            prev_inertia = current_inertia;
        }

        let assignments = safe_download(&self.cluster_assignments, self.num_nodes)?;

        let centroids_x = safe_download(&self.centroids_x, num_clusters)?;
        let centroids_y = safe_download(&self.centroids_y, num_clusters)?;
        let centroids_z = safe_download(&self.centroids_z, num_clusters)?;

        let centroids: Vec<(f32, f32, f32)> = centroids_x
            .into_iter()
            .zip(centroids_y)
            .zip(centroids_z)
            .map(|((x, y), z)| (x, y, z))
            .collect();

        Ok((
            assignments,
            centroids,
            final_inertia,
            actual_iterations,
            converged,
        ))
    }

    pub fn run_lof_anomaly_detection(
        &mut self,
        k_neighbors: i32,
        radius: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        let block_size = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);

        let grid_dims = int3 {
            x: 32,
            y: 32,
            z: 32,
        };

        let lof_kernel = self._module.get_function("compute_lof_kernel")?;
        let stream = &self.stream;
        // SAFETY: LOF anomaly detection kernel launch is safe because:
        // 1. pos_in_* buffers contain valid position data
        // 2. sorted_node_indices, cell_start, cell_end, cell_keys are from spatial grid
        // 3. lof_scores and local_densities are output buffers with capacity >= num_nodes
        // 4. grid_dims contains valid grid dimensions for spatial partitioning
        // 5. k_neighbors and radius are validated algorithm parameters
        unsafe {
            launch!(
                lof_kernel<<<grid_size, block_size, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                self.sorted_node_indices.as_device_ptr(),
                self.cell_start.as_device_ptr(),
                self.cell_end.as_device_ptr(),
                self.cell_keys.as_device_ptr(),
                grid_dims,
                self.lof_scores.as_device_ptr(),
                self.local_densities.as_device_ptr(),
                self.num_nodes as i32,
                k_neighbors,
                radius,
                crate::config::dev_config::physics().world_bounds_min,
                crate::config::dev_config::physics().world_bounds_max,
                crate::config::dev_config::physics().cell_size_lod,
                crate::config::dev_config::physics().k_neighbors_max as i32
            ))?;
        }

        self.stream.synchronize()?;

        let lof_scores = safe_download(&self.lof_scores, self.num_nodes)?;
        let local_densities = safe_download(&self.local_densities, self.num_nodes)?;

        Ok((lof_scores, local_densities))
    }

    pub fn run_zscore_anomaly_detection(&mut self, feature_data: &[f32]) -> Result<Vec<f32>> {
        if feature_data.len() != self.num_nodes {
            return Err(anyhow!(
                "Feature data size {} doesn't match number of nodes {}",
                feature_data.len(),
                self.num_nodes
            ));
        }

        // Pad to allocated_nodes for overallocated device buffer
        if feature_data.len() < self.feature_values.len() {
            let mut padded = feature_data.to_vec();
            padded.resize(self.feature_values.len(), 0.0);
            self.feature_values.copy_from(&padded)?;
        } else {
            self.feature_values.copy_from(feature_data)?;
        }

        let block_size = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);

        let stats_kernel = self._module.get_function("compute_feature_stats_kernel")?;
        let stats_shared_memory = block_size * 2 * 4;
        let stream = &self.stream;
        // SAFETY: Feature statistics kernel launch is safe because:
        // 1. feature_values was just populated from feature_data via copy_from()
        // 2. partial_sums and partial_sq_sums are output buffers with capacity >= grid_size
        // 3. shared_memory size (2 floats per thread) fits within GPU limits
        // 4. This is a parallel reduction computing sum and sum-of-squares
        unsafe {
            launch!(
                stats_kernel<<<grid_size, block_size, stats_shared_memory, stream>>>(
                self.feature_values.as_device_ptr(),
                self.partial_sums.as_device_ptr(),
                self.partial_sq_sums.as_device_ptr(),
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        let partial_sums = safe_download(&self.partial_sums, grid_size as usize)?;
        let partial_sq_sums = safe_download(&self.partial_sq_sums, grid_size as usize)?;

        let total_sum: f32 = partial_sums.iter().sum();
        let total_sq_sum: f32 = partial_sq_sums.iter().sum();

        let mean = total_sum / self.num_nodes as f32;
        let variance = (total_sq_sum / self.num_nodes as f32) - (mean * mean);
        let std_dev = variance.sqrt();

        let zscore_kernel = self._module.get_function("compute_zscore_kernel")?;
        let stream = &self.stream;
        // SAFETY: Z-score computation kernel launch is safe because:
        // 1. feature_values contains the input feature data
        // 2. zscore_values is the output buffer with capacity >= num_nodes
        // 3. mean and std_dev are computed from the stats kernel reduction
        // 4. The kernel performs element-wise (value - mean) / std_dev
        unsafe {
            launch!(
                zscore_kernel<<<grid_size, block_size, 0, stream>>>(
                self.feature_values.as_device_ptr(),
                self.zscore_values.as_device_ptr(),
                mean,
                std_dev,
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        let zscore_values = safe_download(&self.zscore_values, self.num_nodes)?;

        Ok(zscore_values)
    }

    pub fn run_kmeans_clustering(
        &mut self,
        num_clusters: usize,
        max_iterations: u32,
        tolerance: f32,
        seed: u32,
    ) -> Result<GpuKMeansOutput> {
        self.run_kmeans(num_clusters, max_iterations, tolerance, seed)
    }

    pub fn run_anomaly_detection_lof(
        &mut self,
        k_neighbors: i32,
        radius: f32,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        self.run_lof_anomaly_detection(k_neighbors, radius)
    }
}
