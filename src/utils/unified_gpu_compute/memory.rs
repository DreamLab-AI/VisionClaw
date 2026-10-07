//! Memory management, buffer resizing, and data upload/download operations.

use super::construction::UnifiedGPUCompute;
use super::types::ComputeMode;
use crate::models::constraints::ConstraintData;
use crate::models::simulation_params::SimParams;
use anyhow::{anyhow, Result};
use cust::memory::{CopyDestination, DeviceBuffer};
use log::{debug, error, info, warn};

/// Safe wrapper for DeviceBuffer::copy_from that returns Err instead of
/// panicking on size mismatch (the underlying cust assert! poisons the mutex
/// and kills the actor).
#[inline]
fn checked_copy_from<T: cust::memory::DeviceCopy>(
    dest: &mut DeviceBuffer<T>,
    src: &[T],
    label: &str,
) -> Result<()> {
    if dest.len() != src.len() {
        let msg = format!(
            "copy_from size mismatch in {}: device buffer has {} elements, host slice has {} elements",
            label, dest.len(), src.len()
        );
        error!("[GPU SAFE_COPY] {}", msg);
        eprintln!("[GPU SAFE_COPY MISMATCH] {}", msg);
        return Err(anyhow!(msg));
    }
    dest.copy_from(src)
        .map_err(|e| anyhow!("copy_from CUDA error in {}: {}", label, e))
}

#[inline]
fn checked_copy_to<T: cust::memory::DeviceCopy>(
    src: &DeviceBuffer<T>,
    dest: &mut [T],
    label: &str,
) -> Result<()> {
    if src.len() != dest.len() {
        let msg = format!(
            "copy_to size mismatch in {}: device buffer has {} elements, host slice has {} elements",
            label, src.len(), dest.len()
        );
        error!("[GPU SAFE_COPY] {}", msg);
        eprintln!("[GPU SAFE_COPY MISMATCH] {}", msg);
        return Err(anyhow!(msg));
    }
    src.copy_to(dest)
        .map_err(|e| anyhow!("copy_to CUDA error in {}: {}", label, e))
}

impl UnifiedGPUCompute {
    pub fn upload_positions(&mut self, x: &[f32], y: &[f32], z: &[f32]) -> Result<()> {
        if x.len() != self.num_nodes || y.len() != self.num_nodes || z.len() != self.num_nodes {
            return Err(anyhow!(
                "Position array size mismatch: expected {} nodes, got x:{}, y:{}, z:{}",
                self.num_nodes,
                x.len(),
                y.len(),
                z.len()
            ));
        }

        if x.len() < self.allocated_nodes {
            let mut padded_x = x.to_vec();
            let mut padded_y = y.to_vec();
            let mut padded_z = z.to_vec();
            padded_x.resize(self.allocated_nodes, 0.0);
            padded_y.resize(self.allocated_nodes, 0.0);
            padded_z.resize(self.allocated_nodes, 0.0);
            checked_copy_from(&mut self.pos_in_x, &padded_x, "pos_in_x")?;
            checked_copy_from(&mut self.pos_in_y, &padded_y, "pos_in_y")?;
            checked_copy_from(&mut self.pos_in_z, &padded_z, "pos_in_z")?;
        } else {
            checked_copy_from(&mut self.pos_in_x, x, "pos_in_x")?;
            checked_copy_from(&mut self.pos_in_y, y, "pos_in_y")?;
            checked_copy_from(&mut self.pos_in_z, z, "pos_in_z")?;
        }
        Ok(())
    }

    /// Upload ontology class metadata for class-based physics
    /// Maps owl_class_iri to integer class IDs and sets class-specific force parameters
    pub fn upload_class_metadata(
        &mut self,
        class_ids: &[i32],
        class_charges: &[f32],
        class_masses: &[f32],
    ) -> Result<()> {
        if class_ids.len() != self.num_nodes {
            return Err(anyhow!(
                "Class ID array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_ids.len()
            ));
        }
        if class_charges.len() != self.num_nodes {
            return Err(anyhow!(
                "Class charge array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_charges.len()
            ));
        }
        if class_masses.len() != self.num_nodes {
            return Err(anyhow!(
                "Class mass array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                class_masses.len()
            ));
        }

        // Pad to allocated_nodes for overallocated device buffers
        let alloc = self.class_id.len();
        let mut padded_ids = class_ids.to_vec();
        let mut padded_charges = class_charges.to_vec();
        let mut padded_masses = class_masses.to_vec();
        padded_ids.resize(alloc, 0);
        padded_charges.resize(alloc, 1.0);
        padded_masses.resize(alloc, 1.0);

        checked_copy_from(&mut self.class_id, &padded_ids, "class_id")?;
        checked_copy_from(&mut self.class_charge, &padded_charges, "class_charge")?;
        checked_copy_from(&mut self.class_mass, &padded_masses, "class_mass")?;

        Ok(())
    }

    /// Upload per-population spring strength multipliers (Knowledge/Ontology/Agent).
    /// Value 1.0 == identity (current LinLog coefficient); higher == stronger springs
    /// for that node. Read in both spring paths of the force kernels.
    pub fn upload_spring_scale(&mut self, spring_scales: &[f32]) -> Result<()> {
        if spring_scales.len() != self.num_nodes {
            return Err(anyhow!(
                "Spring scale array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                spring_scales.len()
            ));
        }
        let alloc = self.spring_scale.len();
        let mut padded = spring_scales.to_vec();
        padded.resize(alloc, 1.0);
        checked_copy_from(&mut self.spring_scale, &padded, "spring_scale")?;
        Ok(())
    }

    /// Upload the per-node pinned mask (0 = free, non-0 = pinned). A pinned node is
    /// held at its current GPU position by the integrate kernel (integration
    /// skipped) while still exerting forces on neighbours. `flags[i]` corresponds
    /// to GPU node index `i`. The slice is padded to the allocated buffer length
    /// with 0 (free) so trailing over-allocated slots never read as pinned.
    pub fn upload_pinned_mask(&mut self, flags: &[i32]) -> Result<()> {
        if flags.len() != self.num_nodes {
            return Err(anyhow!(
                "Pinned mask size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                flags.len()
            ));
        }
        let alloc = self.pinned_mask.len();
        let mut padded = flags.to_vec();
        padded.resize(alloc, 0);
        checked_copy_from(&mut self.pinned_mask, &padded, "pinned_mask")?;
        Ok(())
    }

    /// Upload per-node DAG hierarchy ranks for the radial bias force (PHASE 2).
    /// `ranks[i]` is the rank of GPU node index `i`: 0 = root, deeper = larger,
    /// -1.0 = not in the hierarchy (no bias). Padded to the allocated buffer with
    /// -1.0 so trailing over-allocated slots never receive a bias.
    pub fn upload_node_rank(&mut self, ranks: &[f32]) -> Result<()> {
        if ranks.len() != self.num_nodes {
            return Err(anyhow!(
                "Node rank size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                ranks.len()
            ));
        }
        let alloc = self.node_rank.len();
        let mut padded = ranks.to_vec();
        padded.resize(alloc, -1.0);
        checked_copy_from(&mut self.node_rank, &padded, "node_rank")?;
        Ok(())
    }

    /// Upload per-node centered plane offsets for the stratified-plane bias
    /// (ADR-141 P2). NaN = not assigned to any plane (no bias). Padded to the
    /// allocated buffer with NaN so trailing over-allocated slots never receive
    /// a bias.
    pub fn upload_node_plane(&mut self, planes: &[f32]) -> Result<()> {
        if planes.len() != self.num_nodes {
            return Err(anyhow!(
                "Node plane size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                planes.len()
            ));
        }
        let alloc = self.node_plane.len();
        let mut padded = planes.to_vec();
        padded.resize(alloc, f32::NAN);
        checked_copy_from(&mut self.node_plane, &padded, "node_plane")?;
        Ok(())
    }

    pub fn upload_edges_csr(
        &mut self,
        row_offsets: &[i32],
        col_indices: &[i32],
        weights: &[f32],
    ) -> Result<()> {
        if row_offsets.len() != self.num_nodes + 1 {
            return Err(anyhow!(
                "Row offsets size mismatch: expected {} (num_nodes + 1), got {}",
                self.num_nodes + 1,
                row_offsets.len()
            ));
        }

        if col_indices.len() != weights.len() {
            return Err(anyhow!(
                "Edge arrays size mismatch: col_indices has {}, weights has {}",
                col_indices.len(),
                weights.len()
            ));
        }

        if col_indices.len() > self.allocated_edges {
            return Err(anyhow!(
                "Too many edges: trying to upload {}, but only {} allocated",
                col_indices.len(),
                self.allocated_edges
            ));
        }

        if row_offsets.len() <= self.allocated_nodes + 1 {
            let mut padded_row_offsets = row_offsets.to_vec();
            let last_val = *padded_row_offsets.last().unwrap_or(&0);
            padded_row_offsets.resize(self.allocated_nodes + 1, last_val);
            checked_copy_from(
                &mut self.edge_row_offsets,
                &padded_row_offsets,
                "edge_row_offsets",
            )?;
        } else {
            checked_copy_from(&mut self.edge_row_offsets, row_offsets, "edge_row_offsets")?;
        }

        if col_indices.len() < self.allocated_edges {
            let mut padded_col_indices = col_indices.to_vec();
            let mut padded_weights = weights.to_vec();
            padded_col_indices.resize(self.allocated_edges, 0);
            padded_weights.resize(self.allocated_edges, 0.0);
            checked_copy_from(
                &mut self.edge_col_indices,
                &padded_col_indices,
                "edge_col_indices",
            )?;
            checked_copy_from(&mut self.edge_weights, &padded_weights, "edge_weights")?;
        } else {
            checked_copy_from(&mut self.edge_col_indices, col_indices, "edge_col_indices")?;
            checked_copy_from(&mut self.edge_weights, weights, "edge_weights")?;
        }

        self.num_edges = col_indices.len();
        Ok(())
    }

    /// Download the CSR graph structure from GPU device memory.
    /// Returns (row_offsets, col_indices) where row_offsets has length num_nodes+1
    /// and col_indices has length num_edges.
    pub fn download_csr(&self) -> Result<(Vec<i32>, Vec<i32>)> {
        let mut row_offsets = vec![0i32; self.num_nodes + 1];
        let mut col_indices = vec![0i32; self.num_edges];
        checked_copy_to(&self.edge_row_offsets, &mut row_offsets, "edge_row_offsets")?;
        if self.num_edges > 0 {
            checked_copy_to(&self.edge_col_indices, &mut col_indices, "edge_col_indices")?;
        }
        Ok((row_offsets, col_indices))
    }

    pub fn download_positions(&self, x: &mut [f32], y: &mut [f32], z: &mut [f32]) -> Result<()> {
        // Device buffers may be overallocated (allocated_nodes > num_nodes).
        // Download the full buffer then truncate, or download exactly num_nodes.
        if x.len() == self.pos_in_x.len() {
            checked_copy_to(&self.pos_in_x, x, "pos_in_x")?;
            checked_copy_to(&self.pos_in_y, y, "pos_in_y")?;
            checked_copy_to(&self.pos_in_z, z, "pos_in_z")?;
        } else {
            // Download full allocated buffer then copy only num_nodes elements
            let mut full_x = vec![0.0f32; self.pos_in_x.len()];
            let mut full_y = vec![0.0f32; self.pos_in_y.len()];
            let mut full_z = vec![0.0f32; self.pos_in_z.len()];
            checked_copy_to(&self.pos_in_x, &mut full_x, "pos_in_x")?;
            checked_copy_to(&self.pos_in_y, &mut full_y, "pos_in_y")?;
            checked_copy_to(&self.pos_in_z, &mut full_z, "pos_in_z")?;
            let n = x.len().min(full_x.len());
            x[..n].copy_from_slice(&full_x[..n]);
            y[..n].copy_from_slice(&full_y[..n]);
            z[..n].copy_from_slice(&full_z[..n]);
        }
        Ok(())
    }

    pub fn download_velocities(&self, x: &mut [f32], y: &mut [f32], z: &mut [f32]) -> Result<()> {
        if x.len() == self.vel_in_x.len() {
            checked_copy_to(&self.vel_in_x, x, "vel_in_x")?;
            checked_copy_to(&self.vel_in_y, y, "vel_in_y")?;
            checked_copy_to(&self.vel_in_z, z, "vel_in_z")?;
        } else {
            let mut full_x = vec![0.0f32; self.vel_in_x.len()];
            let mut full_y = vec![0.0f32; self.vel_in_y.len()];
            let mut full_z = vec![0.0f32; self.vel_in_z.len()];
            checked_copy_to(&self.vel_in_x, &mut full_x, "vel_in_x")?;
            checked_copy_to(&self.vel_in_y, &mut full_y, "vel_in_y")?;
            checked_copy_to(&self.vel_in_z, &mut full_z, "vel_in_z")?;
            let n = x.len().min(full_x.len());
            x[..n].copy_from_slice(&full_x[..n]);
            y[..n].copy_from_slice(&full_y[..n]);
            z[..n].copy_from_slice(&full_z[..n]);
        }
        Ok(())
    }

    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.pos_in_x, &mut self.pos_out_x);
        std::mem::swap(&mut self.pos_in_y, &mut self.pos_out_y);
        std::mem::swap(&mut self.pos_in_z, &mut self.pos_out_z);
        std::mem::swap(&mut self.vel_in_x, &mut self.vel_out_x);
        std::mem::swap(&mut self.vel_in_y, &mut self.vel_out_y);
        std::mem::swap(&mut self.vel_in_z, &mut self.vel_out_z);
    }

    pub fn get_memory_metrics(&self) -> (usize, f32, usize) {
        let current_usage =
            Self::calculate_memory_usage(self.num_nodes, self.num_edges, self.max_grid_cells);
        let allocated_usage = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );
        let utilization = current_usage as f32 / allocated_usage as f32;
        (current_usage, utilization, self.resize_count)
    }

    pub fn get_grid_occupancy(&self, num_grid_cells: usize) -> f32 {
        if num_grid_cells == 0 {
            return 0.0;
        }
        let avg_nodes_per_cell = self.num_nodes as f32 / num_grid_cells as f32;

        let optimal_occupancy = 8.0;
        (avg_nodes_per_cell / optimal_occupancy).min(1.0)
    }

    pub fn resize_cell_buffers(&mut self, required_cells: usize) -> Result<()> {
        if required_cells <= self.max_grid_cells {
            return Ok(());
        }

        if required_cells > self.max_allowed_grid_cells {
            warn!(
                "Grid size {} exceeds maximum allowed {}, capping to maximum",
                required_cells, self.max_allowed_grid_cells
            );
            let capped_size = self.max_allowed_grid_cells;
            return self.resize_cell_buffers_internal(capped_size);
        }

        let new_size = ((required_cells as f32 * self.cell_buffer_growth_factor) as usize)
            .min(self.max_allowed_grid_cells);

        self.resize_cell_buffers_internal(new_size)
    }

    fn resize_cell_buffers_internal(&mut self, new_size: usize) -> Result<()> {
        info!(
            "Resizing cell buffers from {} to {} cells ({}x growth)",
            self.max_grid_cells, new_size, self.cell_buffer_growth_factor
        );

        // cell_start/cell_end are per-frame scratch: every physics step zeroes
        // them from zero_buffer and refills them with compute_cell_bounds_kernel,
        // so there is nothing to carry across a resize. Allocate fresh zeroed
        // buffers into temporaries first, then commit all bookkeeping together.
        //
        // The previous implementation copied the old contents back with a
        // length-checked copy that REQUIRES src.len() == dest.len(); on a real
        // growth (e.g. 32768 → 2097152) that check failed and the `?` aborted
        // mid-resize, leaving cell_start/cell_end grown but max_grid_cells and
        // zero_buffer stale at the old size. The next grid kernel then indexed
        // the grown buffer with the stale dimension → out-of-bounds write → a
        // sticky CUDA illegal-memory-access that poisoned the context and froze
        // the whole physics pipeline.
        let new_cell_start = DeviceBuffer::zeroed(new_size).map_err(|e| {
            anyhow!(
                "Failed to allocate cell_start buffer of size {}: {}",
                new_size,
                e
            )
        })?;
        let new_cell_end = DeviceBuffer::zeroed(new_size).map_err(|e| {
            anyhow!(
                "Failed to allocate cell_end buffer of size {}: {}",
                new_size,
                e
            )
        })?;
        self.cell_start = new_cell_start;
        self.cell_end = new_cell_end;

        let old_memory = self.total_memory_allocated;
        self.max_grid_cells = new_size;
        self.zero_buffer = vec![0i32; new_size];
        self.resize_count += 1;
        self.total_memory_allocated = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );

        let memory_delta = self.total_memory_allocated as i64 - old_memory as i64;
        info!(
            "Cell buffer resize complete. Memory change: {:+} bytes, Total: {} MB",
            memory_delta,
            self.total_memory_allocated / 1024 / 1024
        );

        if self.resize_count > 10 {
            warn!("High resize frequency detected ({} resizes). Consider increasing initial buffer size.",
                  self.resize_count);
        }

        Ok(())
    }

    pub fn resize_buffers(&mut self, new_num_nodes: usize, new_num_edges: usize) -> Result<()> {
        if new_num_nodes <= self.num_nodes && new_num_edges <= self.num_edges {
            self.num_nodes = new_num_nodes;
            self.num_edges = new_num_edges;
            return Ok(());
        }

        info!(
            "Resizing GPU buffers from {}/{} to {}/{} nodes/edges",
            self.num_nodes, self.num_edges, new_num_nodes, new_num_edges
        );

        let actual_new_nodes = ((new_num_nodes as f32 * 1.5) as usize).max(self.num_nodes);
        let actual_new_edges = ((new_num_edges as f32 * 1.5) as usize).max(self.num_edges);

        // Use allocated_nodes (not num_nodes) to match actual device buffer size,
        // which may be larger due to 1.5x overallocation from a previous resize.
        let copy_size = self.allocated_nodes;
        let mut pos_x_data = vec![0.0f32; copy_size];
        let mut pos_y_data = vec![0.0f32; copy_size];
        let mut pos_z_data = vec![0.0f32; copy_size];
        let mut vel_x_data = vec![0.0f32; copy_size];
        let mut vel_y_data = vec![0.0f32; copy_size];
        let mut vel_z_data = vec![0.0f32; copy_size];

        checked_copy_to(&self.pos_in_x, &mut pos_x_data, "pos_in_x")?;
        checked_copy_to(&self.pos_in_y, &mut pos_y_data, "pos_in_y")?;
        checked_copy_to(&self.pos_in_z, &mut pos_z_data, "pos_in_z")?;
        checked_copy_to(&self.vel_in_x, &mut vel_x_data, "vel_in_x")?;
        checked_copy_to(&self.vel_in_y, &mut vel_y_data, "vel_in_y")?;
        checked_copy_to(&self.vel_in_z, &mut vel_z_data, "vel_in_z")?;

        pos_x_data.resize(actual_new_nodes, 0.0);
        pos_y_data.resize(actual_new_nodes, 0.0);
        pos_z_data.resize(actual_new_nodes, 0.0);
        vel_x_data.resize(actual_new_nodes, 0.0);
        vel_y_data.resize(actual_new_nodes, 0.0);
        vel_z_data.resize(actual_new_nodes, 0.0);

        self.pos_in_x = DeviceBuffer::from_slice(&pos_x_data)?;
        self.pos_in_y = DeviceBuffer::from_slice(&pos_y_data)?;
        self.pos_in_z = DeviceBuffer::from_slice(&pos_z_data)?;
        self.vel_in_x = DeviceBuffer::from_slice(&vel_x_data)?;
        self.vel_in_y = DeviceBuffer::from_slice(&vel_y_data)?;
        self.vel_in_z = DeviceBuffer::from_slice(&vel_z_data)?;

        self.pos_out_x = DeviceBuffer::from_slice(&pos_x_data)?;
        self.pos_out_y = DeviceBuffer::from_slice(&pos_y_data)?;
        self.pos_out_z = DeviceBuffer::from_slice(&pos_z_data)?;
        self.vel_out_x = DeviceBuffer::from_slice(&vel_x_data)?;
        self.vel_out_y = DeviceBuffer::from_slice(&vel_y_data)?;
        self.vel_out_z = DeviceBuffer::from_slice(&vel_z_data)?;

        self.mass = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        self.node_graph_id = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.edge_row_offsets = DeviceBuffer::zeroed(actual_new_nodes + 1)?;
        self.edge_col_indices = DeviceBuffer::zeroed(actual_new_edges)?;
        self.edge_weights = DeviceBuffer::zeroed(actual_new_edges)?;
        self.force_x = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.force_y = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.force_z = DeviceBuffer::zeroed(actual_new_nodes)?;

        self.cell_keys = DeviceBuffer::zeroed(actual_new_nodes)?;
        let sorted_indices: Vec<i32> = (0..actual_new_nodes as i32).collect();
        self.sorted_node_indices = DeviceBuffer::from_slice(&sorted_indices)?;

        // Persistent Thrust grid-sort output buffers must track allocated_nodes.
        self.sort_keys_out = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.sort_values_out = DeviceBuffer::zeroed(actual_new_nodes)?;

        self.total_memory_allocated = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );

        // Class metadata buffers must be resized with positions to avoid
        // stale CUDA device pointers after the position buffers are reallocated.
        self.class_id = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.class_charge = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        self.class_mass = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        self.spring_scale = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        // Pinned mask resized with positions; a resize (node add/remove) drops all
        // pins — the actor re-uploads the mask on the next step via its dirty flag.
        self.pinned_mask = DeviceBuffer::zeroed(actual_new_nodes)?;
        // DAG rank buffer resized with positions; reset to -1 (unranked) — the
        // actor recomputes and re-uploads ranks after a graph upload.
        self.node_rank = DeviceBuffer::from_slice(&vec![-1.0f32; actual_new_nodes])?;
        // Stratified-plane buffer resized with positions; reset to NaN (unassigned)
        // — the actor recomputes and re-uploads planes after a graph upload.
        self.node_plane = DeviceBuffer::from_slice(&vec![f32::NAN; actual_new_nodes])?;

        // Degree weight buffer must be resized with positions
        self.degree_weight = DeviceBuffer::from_slice(&vec![1.0f32; actual_new_nodes])?;
        self.degree_weights_available = false;

        // FA2 adaptive speed: prev_force buffers reset to zero on resize
        self.prev_force_x = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.prev_force_y = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.prev_force_z = DeviceBuffer::zeroed(actual_new_nodes)?;

        // ADR-070 D2.2 / D3.1: constraint-force telemetry + sparse compute mask.
        // The mask is invalidated on resize (node indices may have moved); it is
        // rebuilt from the persona/filter on the next set_compute_mask call.
        self.node_constraint_force = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.compute_mask = DeviceBuffer::zeroed(actual_new_nodes.max(1))?;
        self.compute_mask_len = 0;

        self.cluster_assignments = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.distances_to_centroid = DeviceBuffer::zeroed(actual_new_nodes)?;

        // Louvain cohesion buffers track node count; labels are invalidated on
        // resize so the next force step re-runs detection before applying cohesion.
        self.community_centroids_x = DeviceBuffer::zeroed(actual_new_nodes.max(1))?;
        self.community_centroids_y = DeviceBuffer::zeroed(actual_new_nodes.max(1))?;
        self.community_centroids_z = DeviceBuffer::zeroed(actual_new_nodes.max(1))?;
        self.community_sizes = DeviceBuffer::zeroed(actual_new_nodes.max(1))?;
        self.community_count_active = 0;
        self.last_cohesion_refresh_iter = 0;

        let new_num_blocks = actual_new_nodes.div_ceil(256);
        self.partial_inertia = DeviceBuffer::zeroed(new_num_blocks)?;
        self.min_distances = DeviceBuffer::zeroed(actual_new_nodes)?;

        self.lof_scores = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.local_densities = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.zscore_values = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.feature_values = DeviceBuffer::zeroed(actual_new_nodes)?;
        self.partial_sums = DeviceBuffer::zeroed(new_num_blocks)?;
        self.partial_sq_sums = DeviceBuffer::zeroed(new_num_blocks)?;

        self.aabb_num_blocks = new_num_blocks;
        self.aabb_block_results = DeviceBuffer::zeroed(new_num_blocks)?;
        self.partial_kinetic_energy = DeviceBuffer::zeroed(new_num_blocks)?;
        self.node_degrees = DeviceBuffer::zeroed(actual_new_nodes)?;

        self.num_nodes = new_num_nodes;
        self.num_edges = new_num_edges;
        self.allocated_nodes = actual_new_nodes;
        self.allocated_edges = actual_new_edges;

        info!(
            "Successfully resized GPU buffers to {}/{} allocated nodes/edges",
            actual_new_nodes, actual_new_edges
        );
        Ok(())
    }

    pub fn set_params(&mut self, params: SimParams) -> Result<()> {
        info!(
            "Setting SimParams - spring_k: {:.4}, repel_k: {:.2}, damping: {:.3}, dt: {:.3}",
            params.spring_k, params.repel_k, params.damping, params.dt
        );

        self.params = params;

        info!("SimParams successfully updated");
        Ok(())
    }

    pub fn set_mode(&mut self, _mode: ComputeMode) {}

    pub fn set_constraints(&mut self, mut constraints: Vec<ConstraintData>) -> Result<()> {
        let current_iteration = self.iteration;
        for constraint in &mut constraints {
            if constraint.activation_frame == 0 {
                constraint.activation_frame = current_iteration;
                debug!(
                    "Setting activation frame {} for constraint type {}",
                    current_iteration, constraint.kind
                );
            }
        }

        if constraints.len() > self.constraint_data.len() {
            info!(
                "Resizing constraint buffer from {} to {} with progressive activation",
                self.constraint_data.len(),
                constraints.len()
            );

            let new_constraint_buffer = DeviceBuffer::from_slice(&constraints)?;
            self.constraint_data = new_constraint_buffer;
        } else if !constraints.is_empty() {
            let constraint_len = self.constraint_data.len();
            let copy_len = constraints.len().min(constraint_len);
            checked_copy_from(
                &mut self.constraint_data,
                &constraints[..copy_len],
                "constraint_data",
            )?;
        }

        self.num_constraints = constraints.len();
        debug!(
            "Updated GPU constraints: {} active constraints with progressive activation support",
            self.num_constraints
        );
        Ok(())
    }

    pub fn clear_constraints(&mut self) -> Result<()> {
        self.num_constraints = 0;

        let empty_constraints = vec![ConstraintData::default(); self.constraint_data.len()];
        checked_copy_from(
            &mut self.constraint_data,
            &empty_constraints,
            "constraint_data",
        )?;

        Ok(())
    }

    /// ADR-070 D3.1 (P2) — bind a sparse compute mask (Epic E.4 persona masking).
    ///
    /// `mask` is the compacted, ascending list of node indices the force pass
    /// should evaluate this and subsequent ticks — built host-side by
    /// [`visionclaw_gpu::hardening::build_compute_mask_with_neighbors`] so it
    /// already includes the 1-hop neighbours of every visible node (the ADR
    /// §Risks force-coherence mitigation). Indices at or beyond `allocated_nodes`
    /// are dropped defensively. Passing an empty mask is equivalent to
    /// [`clear_compute_mask`](Self::clear_compute_mask): the force pass reverts to
    /// evaluating every node.
    pub fn set_compute_mask(&mut self, mask: &[i32]) -> Result<()> {
        let filtered: Vec<i32> = mask
            .iter()
            .copied()
            .filter(|&ix| ix >= 0 && (ix as usize) < self.allocated_nodes)
            .collect();

        if filtered.is_empty() {
            self.compute_mask_len = 0;
            return Ok(());
        }

        if filtered.len() > self.compute_mask.len() {
            // Capacity is allocated_nodes, so a well-formed mask never exceeds it;
            // grow defensively rather than truncate silently.
            self.compute_mask = DeviceBuffer::from_slice(&filtered)?;
        } else {
            checked_copy_from(&mut self.compute_mask, &filtered, "compute_mask")?;
        }
        self.compute_mask_len = filtered.len();
        debug!(
            "ADR-070 D3.1: bound sparse compute mask with {} node(s) (of {} allocated)",
            self.compute_mask_len, self.allocated_nodes
        );
        Ok(())
    }

    /// ADR-070 D3.1 — clear any active compute mask; the force pass returns to
    /// evaluating every node.
    pub fn clear_compute_mask(&mut self) {
        self.compute_mask_len = 0;
    }

    /// Whether a sparse compute mask is currently active.
    pub fn compute_mask_active(&self) -> bool {
        self.compute_mask_len > 0
    }

    /// Upload constraints to GPU losslessly.
    ///
    /// ADR-098 D3: the previous implementation round-tripped each constraint
    /// through a 7-float flat array, silently dropping `node_idx[1..3]`, `count`
    /// and `activation_frame`. Pairwise SEPARATION / DISTANCE constraints (which
    /// need both endpoints in `node_idx[0..1]`) and the progressive-activation
    /// ramp (which reads `activation_frame`) were corrupted by that path. This
    /// now delegates to `set_constraints`, which preserves the full
    /// `ConstraintData` struct and stamps activation frames.
    pub fn upload_constraints(
        &mut self,
        constraints: &[crate::models::constraints::ConstraintData],
    ) -> Result<()> {
        if constraints.is_empty() {
            return self.clear_constraints();
        }
        self.set_constraints(constraints.to_vec())
    }

    /// Upload pre-computed degree weights for degree-weighted gravity.
    /// `weights` should contain `log(1 + degree)` for each node.
    /// Isolated nodes (degree 0) should have weight 0.0.
    pub fn upload_degree_weights(&mut self, weights: &[f32]) -> Result<()> {
        if weights.len() != self.num_nodes {
            return Err(anyhow!(
                "Degree weight array size mismatch: expected {} nodes, got {}",
                self.num_nodes,
                weights.len()
            ));
        }

        let alloc = self.degree_weight.len();
        if weights.len() < alloc {
            let mut padded = weights.to_vec();
            padded.resize(alloc, 0.0);
            checked_copy_from(&mut self.degree_weight, &padded, "degree_weight")?;
        } else {
            checked_copy_from(&mut self.degree_weight, weights, "degree_weight")?;
        }
        self.degree_weights_available = true;

        let isolated_count = weights.iter().filter(|&&w| w < 1e-6).count();
        info!(
            "Uploaded degree weights: {} nodes ({} isolated, {} connected)",
            weights.len(),
            isolated_count,
            weights.len() - isolated_count
        );
        Ok(())
    }

    /// Upload a CSR graph and its initial `[x, y, z]` positions, resizing the
    /// device buffers first if the node or edge count changed.
    pub fn initialize_graph(
        &mut self,
        row_offsets: &[i32],
        col_indices: &[i32],
        edge_weights: &[f32],
        positions: [&[f32]; 3],
        num_nodes: usize,
        num_edges: usize,
    ) -> Result<()> {
        if num_nodes != self.num_nodes || num_edges != self.num_edges {
            self.resize_buffers(num_nodes, num_edges)?;
        }

        self.upload_edges_csr(row_offsets, col_indices, edge_weights)?;

        let [positions_x, positions_y, positions_z] = positions;
        self.upload_positions(positions_x, positions_y, positions_z)?;

        info!(
            "Graph initialized with {} nodes and {} edges",
            num_nodes, num_edges
        );
        Ok(())
    }

    pub fn update_positions_only(
        &mut self,
        positions_x: &[f32],
        positions_y: &[f32],
        positions_z: &[f32],
    ) -> Result<()> {
        self.upload_positions(positions_x, positions_y, positions_z)?;
        Ok(())
    }

    /// Get the active number of nodes in the GPU compute context.
    ///
    /// Returns `self.num_nodes` (the live graph size), NOT the position buffer
    /// length. Buffers are over-allocated with headroom, so `pos_in_x.len()`
    /// reports the allocation capacity, not the active count — analytics
    /// (PageRank, connected components) that processed `pos_in_x.len()` were
    /// running kernels over phantom padding nodes whose CSR/edge data is
    /// undefined, wasting work and producing garbage component counts.
    pub fn get_num_nodes(&self) -> usize {
        self.num_nodes
    }

    /// Returns the raw device pointer to the persisted SSSP distance buffer,
    /// or a null pointer if no SSSP has been computed yet.
    ///
    /// This pointer is suitable for passing as `d_sssp_dist` to CUDA kernels.
    pub fn get_sssp_device_ptr(&self) -> cust::memory::DevicePointer<f32> {
        match &self.sssp_device_distances {
            Some(buf) => buf.as_device_ptr(),
            None => cust::memory::DevicePointer::null(),
        }
    }

    /// Toggle the SSSP spring-adjust feature at runtime.
    ///
    /// When enabled and SSSP distances are available, the force kernel will use
    /// graph-theoretic distances to modulate spring rest lengths.
    pub fn enable_sssp_spring_adjust(&mut self, enabled: bool) {
        self.sssp_spring_adjust_enabled = enabled;
    }

    /// Returns whether the SSSP spring-adjust feature is currently enabled.
    pub fn is_sssp_spring_adjust_enabled(&self) -> bool {
        self.sssp_spring_adjust_enabled
    }
}
