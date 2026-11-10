//! Memory management, buffer resizing, and data upload/download operations.

use super::buffers::{Extent, GraphSizedBuffers};
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

/// Fingerprint of a CSR topology. Equal fingerprints with equal counts are treated as the same
/// graph by `initialize_graph`; a relabelled graph with an identical CSR is indistinguishable.
fn csr_fingerprint(row_offsets: &[i32], col_indices: &[i32]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    row_offsets.hash(&mut h);
    col_indices.hash(&mut h);
    h.finish()
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
        // New topology: anything computed from the old CSR no longer describes this graph.
        self.invalidate_graph_derived_state();
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
        // The CSR changed, so any captured rest lengths no longer line up with its slots.
        self.hold_overlay.invalidate();
        self.graph_fingerprint = Some(csr_fingerprint(row_offsets, col_indices));
        self.sssp_edge_weight_range = weights
            .iter()
            .copied()
            .filter(|w| w.is_finite())
            .fold(None, |range: Option<(f32, f32)>, w| {
                Some(range.map_or((w, w), |(lo, hi)| (lo.min(w), hi.max(w))))
            })
            .unwrap_or((0.0, 0.0));
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

    /// Fits the engine to a graph of `new_num_nodes` nodes and `new_num_edges` CSR slots.
    ///
    /// Every graph-sized buffer is registered in one table (`buffers.rs`); a growth replaces all
    /// of them at once with a fresh set from the same allocator `new` uses, so no buffer can be
    /// left at an old length. The fresh set is allocated in full before anything is replaced, so
    /// a failed allocation leaves the engine as it was. Positions and velocities are carried
    /// across; every other registered buffer returns to its default and the actor re-uploads
    /// it. A shrink keeps the allocation and only lowers the live counts.
    ///
    /// Any call is a graph change: everything indexed by the previous graph's nodes is
    /// discarded first (`invalidate_node_indexed_state`), because node indices may have moved
    /// even when capacity has not. A failed growth leaves buffers, counts and capacities as they
    /// were; only that derived state is lost.
    pub fn resize_buffers(&mut self, new_num_nodes: usize, new_num_edges: usize) -> Result<()> {
        self.invalidate_node_indexed_state();
        self.graph_fingerprint = None;

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

        // Read positions and velocities at the current allocation (which may exceed num_nodes
        // after an earlier 1.5x growth), to carry them into the new buffers.
        let copy_size = self.allocated_nodes;
        let mut carried: [Vec<f32>; 6] = std::array::from_fn(|_| vec![0.0f32; copy_size]);
        checked_copy_to(&self.pos_in_x, &mut carried[0], "pos_in_x")?;
        checked_copy_to(&self.pos_in_y, &mut carried[1], "pos_in_y")?;
        checked_copy_to(&self.pos_in_z, &mut carried[2], "pos_in_z")?;
        checked_copy_to(&self.vel_in_x, &mut carried[3], "vel_in_x")?;
        checked_copy_to(&self.vel_in_y, &mut carried[4], "vel_in_y")?;
        checked_copy_to(&self.vel_in_z, &mut carried[5], "vel_in_z")?;

        let mut fresh = GraphSizedBuffers::allocate(actual_new_nodes, actual_new_edges)?;

        // Carry positions and velocities into the fresh set *before* installing it, so a
        // failure here drops the fresh set and leaves the engine exactly as it was (counts,
        // capacities and buffers all old and mutually consistent).
        let new_capacity = Extent::Nodes.capacity(actual_new_nodes, actual_new_edges);
        let keep = copy_size.min(new_capacity);
        let [px, py, pz, vx, vy, vz] = &carried;
        #[cfg(test)]
        let inject_failure = std::mem::take(&mut self.inject_resize_carry_failure);
        for (i, (buf, data, name)) in [
            (&mut fresh.pos_in_x, px, "pos_in_x"),
            (&mut fresh.pos_in_y, py, "pos_in_y"),
            (&mut fresh.pos_in_z, pz, "pos_in_z"),
            (&mut fresh.pos_out_x, px, "pos_out_x"),
            (&mut fresh.pos_out_y, py, "pos_out_y"),
            (&mut fresh.pos_out_z, pz, "pos_out_z"),
            (&mut fresh.vel_in_x, vx, "vel_in_x"),
            (&mut fresh.vel_in_y, vy, "vel_in_y"),
            (&mut fresh.vel_in_z, vz, "vel_in_z"),
            (&mut fresh.vel_out_x, vx, "vel_out_x"),
            (&mut fresh.vel_out_y, vy, "vel_out_y"),
            (&mut fresh.vel_out_z, vz, "vel_out_z"),
        ]
        .into_iter()
        .enumerate()
        {
            #[cfg(test)]
            if inject_failure && i == 6 {
                return Err(anyhow!(
                    "carrying {name} across the resize: injected test failure"
                ));
            }
            let _ = i;
            buf.index(0..keep)
                .copy_from(&data[..keep])
                .map_err(|e| anyhow!("carrying {name} across the resize: {e}"))?;
        }

        // The force kernels hold the old edge_rest_snapshot address in `g_edge_rest_snapshot`;
        // unbind it before that buffer is freed. The next step binds the new one. The overlay's
        // rest lengths are per CSR slot, so a resize invalidates them.
        self.bind_edge_rest(0)?;
        self.install_graph_sized(fresh);
        self.hold_overlay.invalidate();

        // Node-indexed host state was discarded at the top (invalidate_node_indexed_state).
        self.aabb_num_blocks = self.aabb_block_results.len();

        self.num_nodes = new_num_nodes;
        self.num_edges = new_num_edges;
        self.allocated_nodes = new_capacity;
        self.allocated_edges = Extent::Edges.capacity(actual_new_nodes, actual_new_edges);
        self.total_memory_allocated = Self::calculate_memory_usage(
            self.allocated_nodes,
            self.allocated_edges,
            self.max_grid_cells,
        );

        info!(
            "Successfully resized GPU buffers to {}/{} allocated nodes/edges",
            self.allocated_nodes, self.allocated_edges
        );
        // The index is sized by node capacity, which just changed, so the old one must not be read
        // again. If it cannot be rebuilt, leave the engine with no constraints at all (index
        // unbound, none counted) rather than an index that no longer matches the buffers.
        if !self.constraints_host.is_empty() {
            let rebuilt = self.rebuild_constraint_index();
            if let Err(e) = rebuilt {
                self.num_constraints = 0;
                self.constraints_host.clear();
                if let Err(unbind) = self.bind_constraint_index(None) {
                    error!(
                        "could not unbind the constraint index after a failed rebuild: {unbind}"
                    );
                }
                return Err(
                    e.context("constraint index rebuild after resize failed; constraints dropped")
                );
            }
        }
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

        // Stage everything that can fail, then commit. The kernel-visible write is the last
        // fallible step; the assignments after it cannot fail. If anything returns `Err`, the
        // previous constraints, their index and the bound view are all still in place and agree
        // with each other.
        let staged_data = DeviceBuffer::from_slice(&constraints)?;
        let index =
            super::constraint_index::build_constraint_index(&constraints, self.allocated_nodes);
        self.log_constraint_index(constraints.len(), &index);
        self.bind_constraint_index(Some(&index))?;

        self.constraint_data = staged_data;
        self.num_constraints = constraints.len();
        self.constraints_host = constraints;
        self.constraint_uploads += 1;
        debug!(
            "Updated GPU constraints: {} active constraints with progressive activation support",
            self.num_constraints
        );
        Ok(())
    }

    /// Rebuild the per-node constraint index from `constraints_host` and bind it. Used when the
    /// node capacity changes; `set_constraints` builds its own from the incoming set.
    fn rebuild_constraint_index(&mut self) -> Result<()> {
        let index = super::constraint_index::build_constraint_index(
            &self.constraints_host,
            self.allocated_nodes,
        );
        self.log_constraint_index(self.constraints_host.len(), &index);
        self.bind_constraint_index(Some(&index))
    }

    fn log_constraint_index(
        &self,
        constraints: usize,
        index: &super::constraint_index::ConstraintIndex,
    ) {
        let (entries, longest, p99) = index.list_stats();
        info!(
            "constraint index: {} constraints -> {} entries over {} node slots; longest node list {} (p99 {})",
            constraints, entries, self.allocated_nodes, longest, p99
        );
    }

    /// Upload `index` and make it the one the force kernels read; `None` unbinds it (no
    /// constraints). The kernel-visible view is written in one 16-byte copy, after the new buffers
    /// exist and before the old ones are dropped, so there is no moment at which a bound pointer
    /// refers to freed memory, and no moment at which only one of the two is bound. On `Err`
    /// nothing has changed.
    pub(crate) fn bind_constraint_index(
        &mut self,
        index: Option<&super::constraint_index::ConstraintIndex>,
    ) -> Result<()> {
        use super::constraint_index::ConstraintIndexView;
        let Some(index) = index else {
            let empty_offsets = DeviceBuffer::from_slice(&[] as &[i32])?;
            let empty_ids = DeviceBuffer::from_slice(&[] as &[i32])?;
            self.write_module_global(c"g_constraint_index", &ConstraintIndexView::default())?;
            self.constraint_node_offsets = empty_offsets;
            self.constraint_node_ids = empty_ids;
            return Ok(());
        };
        let offsets = DeviceBuffer::from_slice(&index.offsets)?;
        let ids = DeviceBuffer::from_slice(&index.ids)?;
        let view = ConstraintIndexView {
            offsets: offsets.as_device_ptr().as_raw(),
            ids: ids.as_device_ptr().as_raw(),
        };
        self.write_module_global(c"g_constraint_index", &view)?;
        self.constraint_node_offsets = offsets;
        self.constraint_node_ids = ids;
        Ok(())
    }

    /// Point the force kernels' `g_edge_rest_snapshot` at `raw` (a device address; 0 = overlay
    /// off). Writes only when the address differs from the one already bound, so a steady hold
    /// costs nothing and a re-allocated buffer is picked up on the next step.
    pub(crate) fn bind_edge_rest(&self, raw: u64) -> Result<()> {
        use std::sync::atomic::Ordering::Relaxed;
        if self.edge_rest_bound.load(Relaxed) == raw {
            return Ok(());
        }
        self.write_module_global(c"g_edge_rest_snapshot", &raw)?;
        self.edge_rest_bound.store(raw, Relaxed);
        Ok(())
    }

    /// Write one `__device__` global of the force module and wait for the write to land.
    ///
    /// The step stream is NON_BLOCKING, so the synchronous host-to-device copy is not ordered
    /// against it by the legacy default stream; a context synchronise makes the write visible
    /// before the next launch. These writes happen when a constraint set or a hold changes, not
    /// per step. A PTX without the symbol is an error: there is no degraded mode.
    fn write_module_global<T: cust::memory::DeviceCopy>(
        &self,
        name: &std::ffi::CStr,
        value: &T,
    ) -> Result<()> {
        let mut global = self
            ._module
            .get_global::<T>(name)
            .map_err(|e| anyhow!("PTX has no {:?}: {}", name, e))?;
        global
            .copy_from(value)
            .map_err(|e| anyhow!("could not write {:?}: {}", name, e))?;
        cust::context::CurrentContext::synchronize()
            .map_err(|e| anyhow!("synchronise after writing {:?}: {}", name, e))
    }

    pub fn clear_constraints(&mut self) -> Result<()> {
        // Unbind first: if that fails nothing has changed. After it, "no constraints" is a
        // consistent state whatever happens to the stale device copy below.
        self.bind_constraint_index(None)?;
        self.num_constraints = 0;
        self.constraints_host.clear();

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
            // A sparse mask is shorter than the buffer's capacity: fill only its prefix (the
            // kernel reads `compute_mask_len` entries). A whole-buffer copy demanded equal
            // lengths, so every sparse mask failed before S1a's review round.
            self.compute_mask
                .index(0..filtered.len())
                .copy_from(&filtered[..])
                .map_err(|e| anyhow!("uploading compute_mask: {e}"))?;
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
        } else if self.graph_fingerprint != Some(csr_fingerprint(row_offsets, col_indices)) {
            // Same counts, different graph: node indices may now name other nodes.
            self.invalidate_node_indexed_state();
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

    /// The `d_sssp_dist` argument for the force kernels: the persisted SSSP distances when they
    /// were computed for the current graph and hold one entry per live node, otherwise null
    /// (the kernels then skip the SSSP rest-length term). The force kernels' signature is part
    /// of the Codescape ABI and carries no length, so the bound is enforced here: the distances
    /// are discarded on every graph change (`invalidate_graph_derived_state`), and the length
    /// check makes a stale buffer unreachable even if that invariant were broken.
    pub(crate) fn sssp_distances_for_force_pass(&self) -> cust::memory::DevicePointer<f32> {
        match &self.sssp_device_distances {
            Some(buf) if self.sssp_available && buf.len() >= self.num_nodes => buf.as_device_ptr(),
            _ => cust::memory::DevicePointer::null(),
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
