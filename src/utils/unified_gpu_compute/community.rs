//! Community detection algorithms: Label Propagation, Louvain, DBSCAN.

use super::clustering::{safe_download, safe_upload};
use super::construction::UnifiedGPUCompute;
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer};
use log::info;

/// Newman modularity Q of a partition over a weighted CSR graph, computed on the
/// host. This is the single correct modularity measure for both the
/// label-propagation and Louvain paths — it uses the GLOBAL null model
/// (Σ_c (Σtot_c / 2m)²), not the broken per-edge expected-weight subtraction the
/// old `compute_modularity_kernel` used (that kernel reached only ~0.07 on a
/// graph whose true modularity is ~0.48).
///
///   Q = Σ_c [ intra_c / 2m − (Σtot_c / 2m)² ]
///
/// `labels` are dense community ids in [0, num_comm); `degrees[i]` is the
/// weighted degree k_i; `total_weight` is m = Σ k_i / 2. CSR entries store both
/// directions of every undirected edge, so `intra_c` (the directed intra sum)
/// already counts each internal edge twice — matching the 2m denominator.
pub(super) fn modularity_csr(
    labels: &[i32],
    offsets: &[i32],
    indices: &[i32],
    weights: &[f32],
    degrees: &[f32],
    total_weight: f32,
) -> f32 {
    let m = total_weight as f64;
    if m <= 0.0 {
        return 0.0;
    }
    let n = labels.len();
    let num_comm = labels.iter().copied().max().unwrap_or(-1) + 1;
    if num_comm <= 0 {
        return 0.0;
    }
    let mut intra = vec![0.0f64; num_comm as usize];
    let mut sigtot = vec![0.0f64; num_comm as usize];

    for i in 0..n {
        let ci = labels[i];
        if ci < 0 {
            continue;
        }
        let ci = ci as usize;
        sigtot[ci] += degrees[i] as f64;
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;
        for e in start..end {
            let j = indices[e] as usize;
            if labels[j] == labels[i] {
                intra[ci] += weights[e] as f64;
            }
        }
    }

    let two_m = 2.0 * m;
    let mut q = 0.0f64;
    for c in 0..num_comm as usize {
        let frac = sigtot[c] / two_m;
        q += intra[c] / two_m - frac * frac;
    }
    q as f32
}

impl UnifiedGPUCompute {
    pub fn run_community_detection(
        &mut self,
        max_iterations: u32,
        seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for community detection: {}", e))?;

        let block_size = 256;
        let grid_size = self.num_nodes.div_ceil(block_size);
        let stream = &self.stream;

        // Label-propagation kernels (init_random_states, init_labels,
        // compute_node_degrees, propagate_labels_*, check_convergence,
        // compute_modularity, count_community_sizes, relabel_communities) are
        // compiled into the main unified PTX module, not the clustering module.
        let unified_mod = &self._module;

        let init_random_kernel = unified_mod.get_function("init_random_states_kernel")?;
        // SAFETY: Random state initialization kernel is safe because:
        // 1. rand_states buffer is allocated for num_nodes curandState elements
        // 2. Each thread initializes its own random state using seed + thread_id
        // 3. curandState is repr(C) and can be safely written from GPU
        unsafe {
            launch!(
                init_random_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.rand_states.as_device_ptr().as_raw(),
                    self.num_nodes as i32,
                    seed
                )
            )?;
        }

        // SAFETY: Label initialization kernel is safe because:
        // 1. labels_current is a valid DeviceBuffer with capacity >= num_nodes
        // 2. Each thread writes its own index as the initial community label
        let init_labels_kernel = unified_mod.get_function("init_labels_kernel")?;
        unsafe {
            launch!(
                init_labels_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.labels_current.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }

        // SAFETY: Node degree computation kernel is safe because:
        // 1. edge_row_offsets and edge_weights are valid CSR graph data
        // 2. node_degrees is an output buffer with capacity >= num_nodes
        // 3. The kernel reads CSR offsets to compute weighted degree per node
        let compute_degrees_kernel = unified_mod.get_function("compute_node_degrees_kernel")?;
        unsafe {
            launch!(
                compute_degrees_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.node_degrees.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }

        self.stream.synchronize()?;
        let node_degrees_host = safe_download(&self.node_degrees, self.num_nodes)?;
        let total_weight: f32 = node_degrees_host.iter().sum::<f32>() / 2.0;

        let mut iterations = 0;
        let mut converged = false;

        // Single canonical community LPA kernel. It uses ZERO shared memory
        // (votes are tallied over each node's own neighbour list), so the launch
        // is shmem=0 regardless of max_labels — the old shared-histogram defect
        // (clamped size silently dropping high labels) is gone. The redundant
        // in-place async kernel was deleted, so this path is sync-only.
        let propagate_kernel = unified_mod.get_function("propagate_labels_sync_kernel")?;
        let check_convergence_kernel = unified_mod.get_function("check_convergence_kernel")?;

        for iter in 0..max_iterations {
            iterations = iter + 1;

            let convergence_flag_host = vec![1i32];
            self.convergence_flag.copy_from(&convergence_flag_host)?;

            // SAFETY: synchronous label propagation:
            // 1. labels_current = current labels (read-only), labels_next = output
            // 2. edge_* are valid CSR; label_counts is an ignored legacy arg
            // 3. shmem=0 (kernel tallies over the node's own neighbour list)
            // 4. rand_states provides deterministic per-node tie-breaking
            unsafe {
                launch!(
                    propagate_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                        self.labels_current.as_device_ptr(),
                        self.labels_next.as_device_ptr(),
                        self.edge_row_offsets.as_device_ptr(),
                        self.edge_col_indices.as_device_ptr(),
                        self.edge_weights.as_device_ptr(),
                        self.label_counts.as_device_ptr(),
                        self.num_nodes as i32,
                        self.max_labels as i32,
                        self.rand_states.as_device_ptr().as_raw()
                    )
                )?;
            }

            // SAFETY: convergence check compares labels_current vs labels_next
            // element-wise and clears the flag if any label differs.
            unsafe {
                launch!(
                    check_convergence_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                        self.labels_current.as_device_ptr(),
                        self.labels_next.as_device_ptr(),
                        self.convergence_flag.as_device_ptr(),
                        self.num_nodes as i32
                    )
                )?;
            }

            // Swap buffers for next iteration (host-side handle swap; already-
            // launched kernels captured their device pointers at launch time).
            std::mem::swap(&mut self.labels_current, &mut self.labels_next);

            self.stream.synchronize()?;
            let mut convergence_flag_host = vec![0i32];
            self.convergence_flag.copy_to(&mut convergence_flag_host)?;
            if convergence_flag_host[0] == 1 {
                converged = true;
                break;
            }
        }

        // Modularity (host CSR computation — correct Newman Q with the global
        // sigma_tot^2 null model). Computed on the final partition before
        // relabeling; relabeling only compacts ids so Q is invariant under it.
        self.stream.synchronize()?;
        let labels_for_q = safe_download(&self.labels_current, self.num_nodes)?;
        let off_h = safe_download(&self.edge_row_offsets, self.num_nodes + 1)?;
        let nnz = off_h[self.num_nodes] as usize;
        let idx_h = safe_download(&self.edge_col_indices, nnz)?;
        let w_h = safe_download(&self.edge_weights, nnz)?;
        let modularity = modularity_csr(
            &labels_for_q,
            &off_h,
            &idx_h,
            &w_h,
            &node_degrees_host,
            total_weight,
        );

        let zero_communities = vec![0i32; self.max_labels];
        safe_upload(&mut self.community_sizes, &zero_communities)?;

        let count_communities_kernel = unified_mod.get_function("count_community_sizes_kernel")?;
        // SAFETY: Community size counting kernel is safe because:
        // 1. labels_current contains valid community labels (0 to max_labels-1)
        // 2. community_sizes was zeroed before this kernel and has capacity >= max_labels
        // 3. The kernel uses atomic increments to count nodes per community
        unsafe {
            launch!(
                count_communities_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                    self.labels_current.as_device_ptr(),
                    self.community_sizes.as_device_ptr(),
                    self.num_nodes as i32,
                    self.max_labels as i32
                )
            )?;
        }

        self.stream.synchronize()?;

        let mut labels = safe_download(&self.labels_current, self.num_nodes)?;
        let community_sizes_host = safe_download(&self.community_sizes, self.max_labels)?;

        let mut label_map = vec![-1i32; self.max_labels];
        let mut compact_community_sizes = Vec::new();
        let mut num_communities = 0;

        for (i, &size) in community_sizes_host.iter().enumerate() {
            if size > 0 {
                label_map[i] = num_communities as i32;
                compact_community_sizes.push(size);
                num_communities += 1;
            }
        }

        if num_communities < self.max_labels {
            safe_upload(&mut self.label_mapping, &label_map)?;

            let relabel_kernel = unified_mod.get_function("relabel_communities_kernel")?;
            // SAFETY: Relabeling kernel is safe because:
            // 1. labels_current contains valid labels that index into label_mapping
            // 2. label_mapping was just populated with compact indices (0 to num_communities-1)
            // 3. The kernel reads label_mapping[labels_current[i]] for each node
            // 4. Entries with -1 in label_mapping indicate unused labels (should not occur)
            unsafe {
                launch!(
                    relabel_kernel<<<grid_size as u32, block_size as u32, 0, stream>>>(
                        self.labels_current.as_device_ptr(),
                        self.label_mapping.as_device_ptr(),
                        self.num_nodes as i32
                    )
                )?;
            }

            self.stream.synchronize()?;
            labels = safe_download(&self.labels_current, self.num_nodes)?;
        }

        Ok((
            labels,
            num_communities,
            modularity,
            iterations,
            compact_community_sizes,
            converged,
        ))
    }

    pub fn run_community_detection_label_propagation(
        &mut self,
        max_iterations: u32,
        seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        self.run_community_detection(max_iterations, seed)
    }

    /// REAL GPU Louvain community detection.
    ///
    /// Parallel local-move modularity optimisation over the CSR graph. Uses the
    /// same proven CSR buffers and GPU modularity kernel as the label-propagation
    /// path. node_weights are weighted degrees (k_i); community_weights track the
    /// total degree of each community (sigma_tot) and are kept consistent via
    /// atomic updates inside `louvain_local_pass_kernel`.
    pub fn run_louvain_community_detection(
        &mut self,
        max_iterations: u32,
        resolution: f32,
        _seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for Louvain: {}", e))?;

        info!(
            "Running REAL Louvain community detection on GPU ({} nodes)",
            self.num_nodes
        );

        if self.num_nodes == 0 {
            return Ok((Vec::new(), 0, 0.0, 0, Vec::new(), true));
        }

        let block_size: u32 = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);
        let stream = &self.stream;

        // louvain_local_pass_kernel, louvain_aggregate_edges_kernel live in the
        // clustering PTX module; compute_node_degrees_kernel lives in the main
        // unified PTX module. Modularity is computed on the host (modularity_csr).
        let clust_mod = self
            .clustering_module
            .as_ref()
            .ok_or(anyhow!("Clustering PTX module not loaded"))?;
        let unified_mod = &self._module;

        let louvain_kernel = clust_mod.get_function("louvain_local_pass_kernel")?;
        let aggregate_kernel = clust_mod.get_function("louvain_aggregate_edges_kernel")?;
        let compute_degrees_kernel = unified_mod.get_function("compute_node_degrees_kernel")?;

        // 1. Weighted node degrees (k_i) of the ORIGINAL graph -> self.node_degrees.
        //    total_weight (m) is invariant across contraction levels, so it is
        //    computed once and reused for every level's gain + modularity.
        // SAFETY: edge_row_offsets/edge_weights are valid CSR buffers; node_degrees
        // is an output buffer with capacity >= num_nodes.
        unsafe {
            launch!(
                compute_degrees_kernel<<<grid_size, block_size, 0, stream>>>(
                    self.edge_row_offsets.as_device_ptr(),
                    self.edge_weights.as_device_ptr(),
                    self.node_degrees.as_device_ptr(),
                    self.num_nodes as i32
                )
            )?;
        }
        self.stream.synchronize()?;

        let degrees_host0 = safe_download(&self.node_degrees, self.num_nodes)?;
        let total_weight: f32 = degrees_host0.iter().sum::<f32>() / 2.0;

        // No edges: every node is its own singleton community.
        if total_weight <= 0.0 {
            let labels: Vec<i32> = (0..self.num_nodes as i32).collect();
            let sizes = vec![1i32; self.num_nodes];
            return Ok((labels, self.num_nodes, 0.0, 0, sizes, true));
        }

        // Build owned level-0 CSR device buffers (copied from self) so every
        // contraction level is handled uniformly. nnz = offsets[n].
        let n0 = self.num_nodes;
        let offsets_host0 = safe_download(&self.edge_row_offsets, n0 + 1)?;
        let nnz0 = offsets_host0[n0] as usize;
        let indices_host0 = safe_download(&self.edge_col_indices, nnz0)?;
        let weights_host0 = safe_download(&self.edge_weights, nnz0)?;

        let mut cur_offsets = DeviceBuffer::from_slice(&offsets_host0)?;
        let mut cur_indices = DeviceBuffer::from_slice(&indices_host0)?;
        let mut cur_weights = DeviceBuffer::from_slice(&weights_host0)?;
        let mut cur_node_weights = DeviceBuffer::from_slice(&degrees_host0)?;
        let mut cur_n = n0;

        // orig_to_super[orig_node] = super-node id at the current level (the
        // composition of every level's relabeling).
        let mut orig_to_super: Vec<i32> = (0..n0 as i32).collect();

        let mut best_labels = orig_to_super.clone();
        let mut best_modularity = f32::NEG_INFINITY;
        let mut total_iterations = 0u32;
        let mut any_converged = false;

        // Bound the transient dense (num_comm x num_comm) aggregation buffer
        // (NFR-7): if a level fails to coarsen enough, stop rather than allocate
        // an O(n^2) matrix.
        const MAX_LEVELS: usize = 20;
        const MAX_AGG_BYTES: u64 = 512 * 1024 * 1024;

        for level in 0..MAX_LEVELS {
            let grid = (cur_n as u32).div_ceil(block_size);

            // Level init: each node its own community; community weight = degree.
            let mut comm_host: Vec<i32> = (0..cur_n as i32).collect();
            let d_comm = DeviceBuffer::from_slice(&comm_host)?;
            // Frozen start-of-pass community snapshot (read-only kernel input). The
            // kernel reads `d_comm_in` and writes `d_comm`; the host copies
            // d_comm -> d_comm_in before each pass so every thread's gain sees the
            // same consistent partition (the D1 race fix).
            let mut d_comm_in = DeviceBuffer::from_slice(&comm_host)?;
            let mut cw_host = safe_download(&cur_node_weights, cur_n)?;
            let mut d_cw_snapshot = DeviceBuffer::from_slice(&cw_host)?;
            let mut d_cw_next = DeviceBuffer::from_slice(&cw_host)?;
            let mut d_improve = DeviceBuffer::from_slice(&[false])?;

            // The kernel alternates move direction by iteration parity, so a
            // single no-improvement pass may just mean "no moves in this
            // direction". Declare convergence after two consecutive quiet passes.
            let mut level_converged = false;
            let mut quiet = 0u32;
            for iteration in 0..max_iterations {
                total_iterations += 1;
                // Freeze the start-of-pass partition: snapshot d_comm -> d_comm_in
                // so the kernel's gain reads a consistent partition even as other
                // threads write their moves into d_comm during this pass.
                comm_host = safe_download(&d_comm, cur_n)?;
                d_comm_in.copy_from(&comm_host)?;
                // Double-buffer sigma_tot: snapshot is read-only this pass, next
                // accumulates deltas. Both seed = current aggregate (cw_host).
                d_cw_snapshot.copy_from(&cw_host)?;
                d_cw_next.copy_from(&cw_host)?;
                d_improve.copy_from(&[false])?;

                // SAFETY: arg order matches louvain_local_pass_kernel:
                // (edge_weights, edge_indices, edge_offsets, node_communities_in,
                //  node_communities_out, node_weights, community_weights_snapshot,
                //  community_weights_next, improvement_flag, num_nodes,
                //  total_weight, resolution, iteration).
                unsafe {
                    launch!(
                        louvain_kernel<<<grid, block_size, 0, stream>>>(
                            cur_weights.as_device_ptr(),
                            cur_indices.as_device_ptr(),
                            cur_offsets.as_device_ptr(),
                            d_comm_in.as_device_ptr(),
                            d_comm.as_device_ptr(),
                            cur_node_weights.as_device_ptr(),
                            d_cw_snapshot.as_device_ptr(),
                            d_cw_next.as_device_ptr(),
                            d_improve.as_device_ptr(),
                            cur_n as i32,
                            total_weight,
                            resolution,
                            iteration as i32
                        )
                    )?;
                }
                self.stream.synchronize()?;

                // d_cw_next now holds the updated aggregate for the next pass.
                cw_host = safe_download(&d_cw_next, cur_n)?;

                let mut improved = vec![false];
                d_improve.copy_to(&mut improved)?;
                if improved[0] {
                    quiet = 0;
                } else {
                    quiet += 1;
                    if quiet >= 2 {
                        level_converged = true;
                        break;
                    }
                }
            }

            // Compact this level's raw community ids -> dense [0, num_comm).
            let raw = safe_download(&d_comm, cur_n)?;
            let mut remap: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
            let mut dense_of_level_node = vec![0i32; cur_n];
            for (i, &c) in raw.iter().enumerate() {
                let next = remap.len() as i32;
                dense_of_level_node[i] = *remap.entry(c).or_insert(next);
            }
            let num_comm = remap.len();

            // Compose: original node -> dense super id at this level.
            for s in orig_to_super.iter_mut() {
                *s = dense_of_level_node[*s as usize];
            }

            // Modularity of the composed labels on the ORIGINAL graph (the only
            // meaningful quality measure across levels). Computed on the host from
            // the already-resident level-0 CSR — correct Newman Q with the global
            // sigma_tot^2 null model, zero extra GPU work.
            let modularity = modularity_csr(
                &orig_to_super,
                &offsets_host0,
                &indices_host0,
                &weights_host0,
                &degrees_host0,
                total_weight,
            );

            if level_converged {
                any_converged = true;
            }

            let improved_level = modularity > best_modularity + 1e-6;
            if modularity > best_modularity {
                best_modularity = modularity;
                best_labels = orig_to_super.clone();
            }

            // No merges this level: contraction would be a no-op. Done.
            if num_comm == cur_n {
                break;
            }
            // Past level 0, stop once modularity stops improving (local optimum).
            if level > 0 && !improved_level {
                break;
            }

            // NFR-7 guard on the transient dense aggregation buffer.
            let agg_bytes = (num_comm as u64) * (num_comm as u64) * 4;
            if agg_bytes > MAX_AGG_BYTES {
                log::warn!(
                    "Louvain: level {} has {} communities ({} MB dense agg) — stopping coarsening (NFR-7 cap)",
                    level, num_comm, agg_bytes / (1024 * 1024)
                );
                break;
            }

            // Contract: scatter every original-edge weight into the dense
            // (community x community) adjacency, then compact to CSR for the
            // next level. The dense buffer is transient (freed at end of scope).
            let d_node_dense = DeviceBuffer::from_slice(&dense_of_level_node)?;
            let agg_len = num_comm * num_comm;
            let d_agg: DeviceBuffer<f32> = DeviceBuffer::zeroed(agg_len)?;
            // SAFETY: arg order matches louvain_aggregate_edges_kernel:
            // (edge_weights, edge_indices, edge_offsets, node_dense_community,
            //  agg, num_nodes, num_comm). agg is zeroed and sized num_comm^2.
            unsafe {
                launch!(
                    aggregate_kernel<<<grid, block_size, 0, stream>>>(
                        cur_weights.as_device_ptr(),
                        cur_indices.as_device_ptr(),
                        cur_offsets.as_device_ptr(),
                        d_node_dense.as_device_ptr(),
                        d_agg.as_device_ptr(),
                        cur_n as i32,
                        num_comm as i32
                    )
                )?;
            }
            self.stream.synchronize()?;
            let agg_host = safe_download(&d_agg, agg_len)?;

            // Dense -> CSR (drop zero entries); new node weights = row sums.
            let mut new_offsets = vec![0i32; num_comm + 1];
            let mut new_indices: Vec<i32> = Vec::new();
            let mut new_weights: Vec<f32> = Vec::new();
            let mut new_node_weights = vec![0f32; num_comm];
            for r in 0..num_comm {
                let mut row_sum = 0.0f32;
                for c in 0..num_comm {
                    let w = agg_host[r * num_comm + c];
                    if w != 0.0 {
                        new_indices.push(c as i32);
                        new_weights.push(w);
                        row_sum += w;
                    }
                }
                new_node_weights[r] = row_sum;
                new_offsets[r + 1] = new_indices.len() as i32;
            }

            cur_offsets = DeviceBuffer::from_slice(&new_offsets)?;
            cur_indices = DeviceBuffer::from_slice(&new_indices)?;
            cur_weights = DeviceBuffer::from_slice(&new_weights)?;
            cur_node_weights = DeviceBuffer::from_slice(&new_node_weights)?;
            cur_n = num_comm;
        }

        // Finalise from the best-modularity labeling: recompact to contiguous ids.
        let mut remap: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
        let mut sizes: Vec<i32> = Vec::new();
        let mut labels = vec![0i32; n0];
        for (i, &c) in best_labels.iter().enumerate() {
            let next = remap.len() as i32;
            let id = *remap.entry(c).or_insert(next);
            if (id as usize) >= sizes.len() {
                sizes.push(0);
            }
            sizes[id as usize] += 1;
            labels[i] = id;
        }
        let num_communities = sizes.len();

        info!(
            "GPU Louvain (multi-level): {} communities, modularity={:.4}, iterations={}, converged={}",
            num_communities, best_modularity, total_iterations, any_converged
        );

        Ok((
            labels,
            num_communities,
            best_modularity,
            total_iterations,
            sizes,
            any_converged,
        ))
    }

    pub fn run_dbscan_clustering(&mut self, eps: f32, min_pts: i32) -> Result<Vec<i32>> {
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for DBSCAN: {}", e))?;

        info!("Running REAL DBSCAN clustering on GPU");

        let block_size = 256;
        let grid_size = (self.num_nodes as u32).div_ceil(block_size);

        let mut labels = vec![0i32; self.num_nodes];
        let neighbor_counts = vec![0i32; self.num_nodes];
        let max_neighbors = 64;
        let neighbors = vec![0i32; self.num_nodes * max_neighbors];
        let neighbor_offsets = (0..self.num_nodes)
            .map(|i| (i * max_neighbors) as i32)
            .collect::<Vec<i32>>();

        let d_labels = DeviceBuffer::from_slice(&labels)?;
        let d_neighbors = DeviceBuffer::from_slice(&neighbors)?;
        let d_neighbor_counts = DeviceBuffer::from_slice(&neighbor_counts)?;
        let d_neighbor_offsets = DeviceBuffer::from_slice(&neighbor_offsets)?;

        let clustering_mod = self
            .clustering_module
            .as_ref()
            .ok_or(anyhow!("Clustering PTX module not loaded"))?;
        let find_neighbors_kernel = clustering_mod.get_function("dbscan_find_neighbors_kernel")?;

        // SAFETY: DBSCAN neighbor finding kernel launch is safe because:
        // 1. pos_in_* contain valid position data for num_nodes nodes
        // 2. d_neighbors is sized for num_nodes * max_neighbors indices
        // 3. d_neighbor_counts stores count per node (capacity >= num_nodes)
        // 4. d_neighbor_offsets stores offsets into d_neighbors for each node
        // 5. The kernel finds all points within eps distance using brute-force search
        unsafe {
            let stream = &self.stream;
            launch!(
            find_neighbors_kernel<<<grid_size, block_size, 0, stream>>>(
                self.pos_in_x.as_device_ptr(),
                self.pos_in_y.as_device_ptr(),
                self.pos_in_z.as_device_ptr(),
                d_neighbors.as_device_ptr(),
                d_neighbor_counts.as_device_ptr(),
                d_neighbor_offsets.as_device_ptr(),
                eps,
                self.num_nodes as i32,
                max_neighbors as i32
            ))?;
        }

        self.stream.synchronize()?;

        let mark_core_kernel = clustering_mod.get_function("dbscan_mark_core_points_kernel")?;

        // SAFETY: DBSCAN core point marking kernel is safe because:
        // 1. d_neighbor_counts contains neighbor counts from previous kernel
        // 2. d_labels is the output buffer for cluster labels (capacity >= num_nodes)
        // 3. min_pts is the threshold for core point classification
        // 4. The kernel marks nodes with >= min_pts neighbors as core points
        unsafe {
            let stream = &self.stream;
            launch!(
            mark_core_kernel<<<grid_size, block_size, 0, stream>>>(
                d_neighbor_counts.as_device_ptr(),
                d_labels.as_device_ptr(),
                min_pts,
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        // Phase 3: Propagate cluster labels until convergence
        let propagate_kernel = clustering_mod.get_function("dbscan_propagate_labels_kernel")?;

        let mut changed = vec![0i32; 1];
        let mut d_changed = DeviceBuffer::from_slice(&changed)?;

        const MAX_ITERATIONS: usize = 100;
        for _iter in 0..MAX_ITERATIONS {
            // Reset changed flag
            changed[0] = 0;
            d_changed.copy_from(&changed)?;

            // SAFETY: DBSCAN label propagation kernel is safe because:
            // 1. d_neighbors contains valid neighbor indices from find_neighbors
            // 2. d_neighbor_counts and d_neighbor_offsets provide bounds for neighbor access
            // 3. d_labels contains current cluster labels (read and written atomically)
            // 4. d_changed is a single-element flag set if any label changed
            // 5. The kernel propagates labels from core points to border points
            unsafe {
                let stream = &self.stream;
                launch!(
                propagate_kernel<<<grid_size, block_size, 0, stream>>>(
                    d_neighbors.as_device_ptr(),
                    d_neighbor_counts.as_device_ptr(),
                    d_neighbor_offsets.as_device_ptr(),
                    d_labels.as_device_ptr(),
                    d_changed.as_device_ptr(),
                    self.num_nodes as i32
                ))?;
            }

            self.stream.synchronize()?;
            d_changed.copy_to(&mut changed)?;

            if changed[0] == 0 {
                break;
            }
        }

        // Phase 4: Finalize noise points
        let finalize_kernel = clustering_mod.get_function("dbscan_finalize_noise_kernel")?;

        // SAFETY: DBSCAN finalization kernel is safe because:
        // 1. d_labels contains cluster labels from propagation phase
        // 2. The kernel marks unlabeled points (label == 0) as noise (-1)
        // 3. This is the final pass that produces the output cluster assignments
        unsafe {
            let stream = &self.stream;
            launch!(
            finalize_kernel<<<grid_size, block_size, 0, stream>>>(
                d_labels.as_device_ptr(),
                self.num_nodes as i32
            ))?;
        }

        self.stream.synchronize()?;

        // Copy final labels back to host
        d_labels.copy_to(&mut labels)?;

        // Compact to 1-based contiguous cluster ids with 0 = noise/unclustered
        // (ADR-031 invariant I-6: cluster_id is 1-based, 0 == unclustered). The
        // GPU labels are sparse core-point indices (>=0), with -1 for noise and
        // -2 for any still-unvisited point — both map to 0.
        let mut remap: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
        for l in labels.iter_mut() {
            if *l < 0 {
                *l = 0;
            } else {
                let next = remap.len() as i32 + 1;
                *l = *remap.entry(*l).or_insert(next);
            }
        }

        Ok(labels)
    }

    /// Run Louvain community detection and write the resulting community labels
    /// into `cluster_assignments`, so the force-loop `cluster_cohesion_kernel`
    /// pulls each node toward its COMMUNITY's centroid (topology-driven cohesion)
    /// rather than K-means' spatial centroids (which are tautological — they
    /// re-derive structure from the layout they are meant to shape).
    ///
    /// Sets `community_count_active` to the number of distinct communities, which
    /// the per-frame centroid reduction and cohesion kernel use as `num_clusters`
    /// (both guard `label < num_clusters`). Returns the community count.
    ///
    /// No-op (returns 0, count cleared) when the graph has no edges — Louvain over
    /// an edgeless graph yields all-singleton communities, for which cohesion is
    /// meaningless; the caller skips the cohesion pass in that case.
    pub fn refresh_community_cohesion_labels(&mut self) -> Result<usize> {
        if self.num_nodes == 0 || self.num_edges == 0 {
            self.community_count_active = 0;
            return Ok(0);
        }

        let iterations = self.clustering_iterations.max(1);
        let resolution = self.clustering_resolution;
        // Leiden is the default discrete detector (connected-community guarantee);
        // "louvain" remains selectable as the un-refined variant. Both drive the
        // same cohesion path — only the partition quality differs.
        let (labels, num_communities, modularity, _iters, _sizes, _converged) =
            if self.clustering_algorithm.eq_ignore_ascii_case("louvain") {
                self.run_louvain_community_detection(iterations, resolution, 0)?
            } else {
                self.run_leiden_community_detection(iterations, resolution, 0)?
            };

        // Singleton-only or empty partition: no meaningful community structure to
        // attract toward. Clear the active count so the force loop skips cohesion.
        if num_communities <= 1 || labels.len() != self.num_nodes {
            self.community_count_active = 0;
            return Ok(0);
        }

        // Labels are dense contiguous in [0, num_communities); upload directly as
        // the cohesion cluster_assignments (i32, one per node).
        safe_upload(&mut self.cluster_assignments, &labels)?;
        self.community_count_active = num_communities;

        info!(
            "[CohesionLouvain] {} communities drive cohesion (modularity={:.4})",
            num_communities, modularity
        );
        Ok(num_communities)
    }

    /// Set the live community-cohesion detector parameters at runtime.
    ///
    /// The cohesion force supports only the discrete community detectors
    /// (`leiden` default, `louvain` optional); any other value falls back to
    /// `leiden`. Clearing `community_count_active` forces the next physics step
    /// to re-run detection, so a resolution/algorithm change takes effect within
    /// one frame instead of waiting out the refresh cadence.
    pub fn set_community_detector(&mut self, algorithm: &str, resolution: f32, iterations: u32) {
        let algo = if algorithm.eq_ignore_ascii_case("louvain") {
            "louvain"
        } else {
            "leiden"
        };
        let resolution = resolution.clamp(0.1, 10.0);
        let iterations = iterations.clamp(1, 1000);

        let unchanged = self.clustering_algorithm.eq_ignore_ascii_case(algo)
            && (self.clustering_resolution - resolution).abs() <= f32::EPSILON
            && self.clustering_iterations == iterations;
        if unchanged {
            return;
        }

        self.clustering_algorithm = algo.to_string();
        self.clustering_resolution = resolution;
        self.clustering_iterations = iterations;
        // Force re-detection on the next cohesion pass.
        self.community_count_active = 0;

        info!(
            "[CohesionDetector] params updated: algorithm={}, resolution={:.3}, iterations={}",
            algo, resolution, iterations
        );
    }
}

#[cfg(test)]
mod tests {
    //! Known-answer + oracle cross-check tests for `modularity_csr` (ADR-031 D1).
    //!
    //! `modularity_csr` is module-private and `mod community` is private, so the
    //! correctness suite for the Newman-Q measure that backs the modularity gate
    //! lives here rather than in `tests/`. The seven golden values below are
    //! hand-derived (see the per-test comments); the final test cross-checks the
    //! CSR implementation against an independent host oracle on every fixture and
    //! partition so the two cannot drift.
    use super::modularity_csr;

    /// Build a symmetric (both-direction) unit-weight CSR from undirected edges,
    /// matching the layout `modularity_csr` consumes from the live graph.
    /// Returns `(offsets, indices, weights, degrees, total_weight)`.
    fn build_csr(
        n: usize,
        edges: &[(usize, usize)],
    ) -> (Vec<i32>, Vec<i32>, Vec<f32>, Vec<f32>, f32) {
        let mut adj: Vec<Vec<i32>> = vec![Vec::new(); n];
        for &(u, v) in edges {
            adj[u].push(v as i32);
            adj[v].push(u as i32);
        }
        let mut offsets = Vec::with_capacity(n + 1);
        let mut indices = Vec::new();
        offsets.push(0i32);
        for a in &adj {
            indices.extend_from_slice(a);
            offsets.push(indices.len() as i32);
        }
        let weights = vec![1.0f32; indices.len()];
        let degrees: Vec<f32> = adj.iter().map(|a| a.len() as f32).collect();
        let total_weight = edges.len() as f32; // m = Σk_i / 2
        (offsets, indices, weights, degrees, total_weight)
    }

    /// Independent host oracle: Q = (internal_edges / m) − Σ_c (Σtot_c / 2m)².
    /// Counts each undirected internal edge ONCE (vs the CSR directed double
    /// count) — a genuinely different code path, so agreement is meaningful.
    fn oracle_q(n: usize, edges: &[(usize, usize)], labels: &[i32]) -> f64 {
        let m = edges.len() as f64;
        if m <= 0.0 {
            return 0.0;
        }
        let internal = edges
            .iter()
            .filter(|&&(u, v)| labels[u] == labels[v])
            .count() as f64;
        let num_comm = (labels.iter().copied().max().unwrap_or(-1) + 1).max(0) as usize;
        let mut deg = vec![0.0f64; n];
        for &(u, v) in edges {
            deg[u] += 1.0;
            deg[v] += 1.0;
        }
        let mut sigtot = vec![0.0f64; num_comm];
        for i in 0..n {
            if labels[i] >= 0 {
                sigtot[labels[i] as usize] += deg[i];
            }
        }
        let two_m = 2.0 * m;
        let null: f64 = sigtot.iter().map(|&s| (s / two_m) * (s / two_m)).sum();
        internal / m - null
    }

    fn q_of(n: usize, edges: &[(usize, usize)], labels: &[i32]) -> f32 {
        let (off, idx, w, deg, m) = build_csr(n, edges);
        modularity_csr(labels, &off, &idx, &w, &deg, m)
    }

    const EPS: f32 = 1e-5;

    const TRIANGLE: &[(usize, usize)] = &[(0, 1), (1, 2), (0, 2)];
    const K4: &[(usize, usize)] = &[(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    // Two K3s joined by a single bridge (node 2 — node 3).
    const BARBELL_K3: &[(usize, usize)] = &[(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (2, 3)];
    // Two disjoint K3s, no bridge.
    const TWO_K3: &[(usize, usize)] = &[(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5)];
    // Two K4s joined by a single bridge (node 3 — node 4).
    const TWO_K4_BRIDGE: &[(usize, usize)] = &[
        (0, 1),
        (0, 2),
        (0, 3),
        (1, 2),
        (1, 3),
        (2, 3),
        (4, 5),
        (4, 6),
        (4, 7),
        (5, 6),
        (5, 7),
        (6, 7),
        (3, 4),
    ];

    #[test]
    fn triangle_single_community_is_zero() {
        // All mass internal, no null-model residual: Q = 1 − 1 = 0.
        assert!((q_of(3, TRIANGLE, &[0, 0, 0]) - 0.0).abs() < EPS);
    }

    #[test]
    fn triangle_all_singletons_is_minus_one_third() {
        // No internal edges; Q = 3·(−(2/6)²) = −1/3.
        assert!((q_of(3, TRIANGLE, &[0, 1, 2]) - (-1.0 / 3.0)).abs() < EPS);
    }

    #[test]
    fn two_disjoint_k3_is_half() {
        // Each clique a community: 2·(6/12 − (6/12)²) = 0.5.
        assert!((q_of(6, TWO_K3, &[0, 0, 0, 1, 1, 1]) - 0.5).abs() < EPS);
    }

    #[test]
    fn barbell_k3_two_communities_is_five_fourteenths() {
        // m=7; 2·(6/14 − (7/14)²) = 5/14 ≈ 0.357143.
        let q = q_of(6, BARBELL_K3, &[0, 0, 0, 1, 1, 1]);
        assert!((q - 5.0 / 14.0).abs() < EPS, "got {q}");
    }

    #[test]
    fn k4_single_community_is_zero() {
        assert!((q_of(4, K4, &[0, 0, 0, 0]) - 0.0).abs() < EPS);
    }

    #[test]
    fn k4_all_singletons_is_minus_quarter() {
        // 4·(−(3/12)²) = −0.25.
        assert!((q_of(4, K4, &[0, 1, 2, 3]) - (-0.25)).abs() < EPS);
    }

    #[test]
    fn two_k4_bridge_two_communities_is_eleven_twentysixths() {
        // m=13; 2·(12/26 − (13/26)²) = 11/26 ≈ 0.423077.
        let q = q_of(8, TWO_K4_BRIDGE, &[0, 0, 0, 0, 1, 1, 1, 1]);
        assert!((q - 11.0 / 26.0).abs() < EPS, "got {q}");
    }

    #[test]
    fn empty_or_degenerate_is_zero() {
        // m == 0 and the all-negative-label case both short-circuit to 0.
        assert_eq!(q_of(0, &[], &[]), 0.0);
        assert_eq!(q_of(3, TRIANGLE, &[-1, -1, -1]), 0.0);
    }

    #[test]
    fn matches_independent_oracle_on_all_fixtures_and_partitions() {
        let cases: &[(usize, &[(usize, usize)], Vec<i32>)] = &[
            (3, TRIANGLE, vec![0, 0, 0]),
            (3, TRIANGLE, vec![0, 1, 2]),
            (3, TRIANGLE, vec![0, 0, 1]),
            (6, TWO_K3, vec![0, 0, 0, 1, 1, 1]),
            (6, TWO_K3, vec![0, 0, 0, 0, 0, 0]),
            (6, BARBELL_K3, vec![0, 0, 0, 1, 1, 1]),
            (6, BARBELL_K3, vec![0, 0, 0, 0, 0, 0]),
            (6, BARBELL_K3, vec![0, 1, 2, 3, 4, 5]),
            (4, K4, vec![0, 0, 0, 0]),
            (4, K4, vec![0, 1, 2, 3]),
            (4, K4, vec![0, 0, 1, 1]),
            (8, TWO_K4_BRIDGE, vec![0, 0, 0, 0, 1, 1, 1, 1]),
            (8, TWO_K4_BRIDGE, vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ];
        for (n, edges, labels) in cases {
            let csr = q_of(*n, edges, labels) as f64;
            let oracle = oracle_q(*n, edges, labels);
            // q_of computes in f32; 1/3-valued fixtures round at ~1e-8, so the
            // tolerance must be f32-scale, not f64-scale.
            assert!(
                (csr - oracle).abs() < 1e-6,
                "CSR {csr} vs oracle {oracle} for labels {labels:?}"
            );
        }
    }
}
