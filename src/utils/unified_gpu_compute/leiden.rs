//! GPU Leiden community detection (Traag–Waltman–van Eck, 2019).
//!
//! Strict upgrade over the Louvain path (`community.rs`). It reuses the SAME GPU
//! kernels — `louvain_local_pass_kernel` for the local-move phase and
//! `louvain_aggregate_edges_kernel` for contraction — and inserts the defining
//! Leiden step Louvain lacks: a REFINEMENT phase between local-move and
//! aggregation.
//!
//! Per level:
//!   1. Local move (seeded). Identical kernel to Louvain. Level 0 seeds singletons;
//!      deeper levels seed each super-node with the UNREFINED community it came
//!      from (Leiden's hallmark — the aggregate is partitioned by the previous
//!      community, not by singletons).
//!   2. Refinement. Within each community found by the local move, every node
//!      starts as its own sub-community and is greedily merged into the neighbour
//!      sub-community of highest positive modularity gain — but ONLY along an
//!      existing edge inside that community. Two consequences:
//!        * sub-communities are always internally CONNECTED (a merge can never
//!          cross a missing edge) → fixes Louvain's disconnected-community defect,
//!          which would otherwise place a cohesion centroid in empty space between
//!          two disjoint sub-blobs;
//!        * a fragment with no internal edge to the rest of its community stays a
//!          singleton sub-community → loosely-bound communities split under the
//!          resolution penalty γ, easing the modularity resolution limit.
//!   3. Aggregate on the REFINED sub-communities (each becomes one super-node);
//!      the next level's local move is seeded from the unrefined communities.
//!
//! Refinement and dense aggregation run on the host, mirroring the existing
//! Louvain path's host loops, so NO new CUDA kernels are required.

use super::clustering::safe_download;
use super::community::modularity_csr;
use super::construction::UnifiedGPUCompute;
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::launch;
use cust::memory::{CopyDestination, DeviceBuffer};
use log::info;
use std::collections::HashMap;

/// Relabel raw ids to dense contiguous `[0, k)` preserving first-seen order.
fn compact(raw: &[i32]) -> (Vec<i32>, usize) {
    let mut remap: HashMap<i32, i32> = HashMap::new();
    let mut dense = vec![0i32; raw.len()];
    for (i, &c) in raw.iter().enumerate() {
        let next = remap.len() as i32;
        dense[i] = *remap.entry(c).or_insert(next);
    }
    let k = remap.len();
    (dense, k)
}

/// Relabel + per-community sizes (final-partition form).
fn compact_with_sizes(raw: &[i32]) -> (Vec<i32>, Vec<i32>) {
    let mut remap: HashMap<i32, i32> = HashMap::new();
    let mut sizes: Vec<i32> = Vec::new();
    let mut dense = vec![0i32; raw.len()];
    for (i, &c) in raw.iter().enumerate() {
        let next = remap.len() as i32;
        let id = *remap.entry(c).or_insert(next);
        if id as usize >= sizes.len() {
            sizes.push(0);
        }
        sizes[id as usize] += 1;
        dense[i] = id;
    }
    (dense, sizes)
}

/// Leiden refinement of a single level.
///
/// `p` is the (dense) community per current-level node from the local move.
/// Returns `(refined_dense, num_refined, p_of_refined)` where `refined_dense[v]`
/// is the connected sub-community of node `v` and `p_of_refined[r]` is the
/// unrefined community that sub-community `r` belongs to (used to seed the next
/// level so super-nodes from one community start together).
///
/// Single agglomerative pass: only PRISTINE singletons act as merge sources (a
/// node that has already absorbed others, or been absorbed, is skipped), which
/// keeps every gain evaluation a valid singleton-into-community move and bounds
/// the merge chains to depth one. Connectivity is guaranteed because a source is
/// only ever merged into a sub-community it shares an edge with.
fn leiden_refine(
    off: &[i32],
    idx: &[i32],
    wt: &[f32],
    nw: &[f32],
    p: &[i32],
    num_comm: usize,
    total_weight: f32,
    resolution: f32,
) -> (Vec<i32>, usize, Vec<i32>) {
    let n = p.len();
    let m2 = 2.0 * total_weight as f64; // 2m

    // refined[v] points at its sub-community root (root r has refined[r]==r).
    let mut refined: Vec<i32> = (0..n as i32).collect();
    // sigma_tot of each sub-community, indexed by root node id.
    let mut sub_degree: Vec<f64> = nw.iter().map(|&x| x as f64).collect();
    // grown[r] = some node has been merged into root r → r is no longer eligible
    // as a merge SOURCE (its accumulated edges would invalidate the singleton gain).
    let mut grown = vec![false; n];

    // Group node ids by their community for boundary-restricted refinement.
    let mut by_comm: Vec<Vec<i32>> = vec![Vec::new(); num_comm];
    for (v, &c) in p.iter().enumerate() {
        by_comm[c as usize].push(v as i32);
    }

    for nodes in &by_comm {
        for &vv in nodes {
            let v = vv as usize;
            // Eligible source = pristine singleton (root, nothing merged in).
            if refined[v] != v as i32 || grown[v] {
                continue;
            }
            let s = off[v] as usize;
            let e = off[v + 1] as usize;
            let cv = p[v];

            // E(v, T): edge weight from v to each neighbour sub-community T, counting
            // only neighbours inside the SAME community. `refined[u]` is always a root.
            let mut e_vt: HashMap<i32, f64> = HashMap::new();
            for ei in s..e {
                let u = idx[ei] as usize;
                if p[u] != cv {
                    continue;
                }
                let t = refined[u];
                if t == v as i32 {
                    continue;
                }
                *e_vt.entry(t).or_insert(0.0) += wt[ei] as f64;
            }

            // Pick T maximizing  E(v,T) - γ·k_v·σ_T/(2m), require strictly positive.
            let kv = nw[v] as f64;
            let mut best_t = -1i32;
            let mut best_gain = 0.0f64;
            for (&t, &evt) in e_vt.iter() {
                let gain = evt - resolution as f64 * kv * sub_degree[t as usize] / m2;
                if gain > best_gain {
                    best_gain = gain;
                    best_t = t;
                }
            }

            if best_t >= 0 {
                refined[v] = best_t;
                sub_degree[best_t as usize] += kv;
                sub_degree[v] = 0.0;
                grown[best_t as usize] = true;
            }
        }
    }

    // Resolve roots (chains are depth ≤ 1 by construction) and compact.
    let mut remap: HashMap<i32, i32> = HashMap::new();
    let mut p_of_refined: Vec<i32> = Vec::new();
    let mut dense = vec![0i32; n];
    for v in 0..n {
        let mut r = v as i32;
        while refined[r as usize] != r {
            r = refined[r as usize];
        }
        let next = remap.len() as i32;
        let id = *remap.entry(r).or_insert(next);
        if id as usize >= p_of_refined.len() {
            p_of_refined.push(p[r as usize]);
        }
        dense[v] = id;
    }
    (dense, remap.len(), p_of_refined)
}

impl UnifiedGPUCompute {
    /// GPU Leiden community detection. Same return shape as
    /// `run_louvain_community_detection`:
    /// `(labels, num_communities, modularity, iterations, sizes, converged)`.
    /// Labels are dense contiguous in `[0, num_communities)`.
    pub fn run_leiden_community_detection(
        &mut self,
        max_iterations: u32,
        resolution: f32,
        _seed: u32,
    ) -> Result<(Vec<i32>, usize, f32, u32, Vec<i32>, bool)> {
        let _ctx = Context::new(self.device)
            .map_err(|e| anyhow!("Failed to set CUDA context for Leiden: {}", e))?;

        info!(
            "Running GPU Leiden community detection ({} nodes)",
            self.num_nodes
        );

        if self.num_nodes == 0 {
            return Ok((Vec::new(), 0, 0.0, 0, Vec::new(), true));
        }

        let block_size: u32 = 256;
        let stream = &self.stream;

        let clust_mod = self
            .clustering_module
            .as_ref()
            .ok_or(anyhow!("Clustering PTX module not loaded"))?;
        let louvain_kernel = clust_mod.get_function("louvain_local_pass_kernel")?;
        let aggregate_kernel = clust_mod.get_function("louvain_aggregate_edges_kernel")?;
        let compute_degrees_kernel = self._module.get_function("compute_node_degrees_kernel")?;

        // Weighted node degrees (k_i) of the ORIGINAL graph; total_weight (m) is
        // invariant across contraction levels.
        let grid0 = (self.num_nodes as u32).div_ceil(block_size);
        unsafe {
            launch!(
                compute_degrees_kernel<<<grid0, block_size, 0, stream>>>(
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

        if total_weight <= 0.0 {
            let labels: Vec<i32> = (0..self.num_nodes as i32).collect();
            let sizes = vec![1i32; self.num_nodes];
            return Ok((labels, self.num_nodes, 0.0, 0, sizes, true));
        }

        let n0 = self.num_nodes;
        let offsets_host0 = safe_download(&self.edge_row_offsets, n0 + 1)?;
        let nnz0 = offsets_host0[n0] as usize;
        let indices_host0 = safe_download(&self.edge_col_indices, nnz0)?;
        let weights_host0 = safe_download(&self.edge_weights, nnz0)?;

        // Current-level CSR kept on host (refinement + seeding need it directly).
        let mut off = offsets_host0.clone();
        let mut idx = indices_host0.clone();
        let mut wt = weights_host0.clone();
        let mut nw = degrees_host0.clone();
        let mut cur_n = n0;

        // Device mirror of the current-level CSR for the kernels.
        let mut cur_offsets = DeviceBuffer::from_slice(&off)?;
        let mut cur_indices = DeviceBuffer::from_slice(&idx)?;
        let mut cur_weights = DeviceBuffer::from_slice(&wt)?;
        let mut cur_node_weights = DeviceBuffer::from_slice(&nw)?;

        // orig node -> current-level super-node id.
        let mut orig_to_super: Vec<i32> = (0..n0 as i32).collect();
        // Seed communities for the current level's local move (level 0 = singletons).
        let mut seed_comm: Vec<i32> = (0..n0 as i32).collect();

        let mut best_labels: Vec<i32> = orig_to_super.clone();
        let mut best_modularity = f32::NEG_INFINITY;
        let mut total_iterations = 0u32;
        let mut any_converged = false;

        const MAX_LEVELS: usize = 20;
        const MAX_AGG_BYTES: u64 = 512 * 1024 * 1024;

        for level in 0..MAX_LEVELS {
            let grid = (cur_n as u32).div_ceil(block_size);

            // --- Phase 1: local move, seeded. Community weights derived from seed. ---
            let mut comm_host = seed_comm.clone();
            let mut cw_host = vec![0f32; cur_n];
            for i in 0..cur_n {
                cw_host[seed_comm[i] as usize] += nw[i];
            }

            let d_comm = DeviceBuffer::from_slice(&comm_host)?;
            let mut d_comm_in = DeviceBuffer::from_slice(&comm_host)?;
            let mut d_cw_snapshot = DeviceBuffer::from_slice(&cw_host)?;
            let mut d_cw_next = DeviceBuffer::from_slice(&cw_host)?;
            let mut d_improve = DeviceBuffer::from_slice(&[false])?;

            let mut level_converged = false;
            let mut quiet = 0u32;
            for iteration in 0..max_iterations {
                total_iterations += 1;
                // Freeze the start-of-pass partition so every thread's gain reads a
                // consistent assignment (the D1 race fix from the Louvain path).
                comm_host = safe_download(&d_comm, cur_n)?;
                d_comm_in.copy_from(&comm_host)?;
                d_cw_snapshot.copy_from(&cw_host)?;
                d_cw_next.copy_from(&cw_host)?;
                d_improve.copy_from(&[false])?;

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

            // Dense community partition P over the current-level super-nodes.
            let raw = safe_download(&d_comm, cur_n)?;
            let (p_dense, num_comm) = compact(&raw);

            // Candidate final labels = P community of every original node.
            let composed: Vec<i32> = orig_to_super.iter().map(|&s| p_dense[s as usize]).collect();
            let (composed_dense, _) = compact(&composed);
            let modularity = modularity_csr(
                &composed_dense,
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
                best_labels = composed_dense.clone();
            }

            // No structure beyond singletons at this level → done.
            if num_comm == cur_n {
                break;
            }
            // Past level 0, stop once the global partition stops improving.
            if level > 0 && !improved_level {
                break;
            }

            // --- Phase 2: refinement into connected sub-communities. ---
            let (refined_dense, num_refined, p_of_refined) = leiden_refine(
                &off,
                &idx,
                &wt,
                &nw,
                &p_dense,
                num_comm,
                total_weight,
                resolution,
            );

            // Refinement produced no coarsening (every node its own sub-community):
            // the aggregate would not shrink → record best and stop.
            if num_refined >= cur_n {
                break;
            }

            // NFR-7: bound the transient dense (num_refined x num_refined) buffer.
            let agg_bytes = (num_refined as u64) * (num_refined as u64) * 4;
            if agg_bytes > MAX_AGG_BYTES {
                log::warn!(
                    "Leiden: level {} has {} refined communities ({} MB dense agg) — stopping (NFR-7 cap)",
                    level,
                    num_refined,
                    agg_bytes / (1024 * 1024)
                );
                break;
            }

            // --- Phase 3: aggregate on the REFINED sub-communities. ---
            let d_node_dense = DeviceBuffer::from_slice(&refined_dense)?;
            let agg_len = num_refined * num_refined;
            let d_agg: DeviceBuffer<f32> = DeviceBuffer::zeroed(agg_len)?;
            unsafe {
                launch!(
                    aggregate_kernel<<<grid, block_size, 0, stream>>>(
                        cur_weights.as_device_ptr(),
                        cur_indices.as_device_ptr(),
                        cur_offsets.as_device_ptr(),
                        d_node_dense.as_device_ptr(),
                        d_agg.as_device_ptr(),
                        cur_n as i32,
                        num_refined as i32
                    )
                )?;
            }
            self.stream.synchronize()?;
            let agg_host = safe_download(&d_agg, agg_len)?;

            // Dense -> CSR (drop zero entries); new node weights = row sums.
            let mut new_off = vec![0i32; num_refined + 1];
            let mut new_idx: Vec<i32> = Vec::new();
            let mut new_wt: Vec<f32> = Vec::new();
            let mut new_nw = vec![0f32; num_refined];
            for r in 0..num_refined {
                let mut row_sum = 0.0f32;
                for c in 0..num_refined {
                    let w = agg_host[r * num_refined + c];
                    if w != 0.0 {
                        new_idx.push(c as i32);
                        new_wt.push(w);
                        row_sum += w;
                    }
                }
                new_nw[r] = row_sum;
                new_off[r + 1] = new_idx.len() as i32;
            }

            // Advance the level: super-nodes are the refined sub-communities,
            // seeded by their unrefined community (Leiden seeding).
            off = new_off;
            idx = new_idx;
            wt = new_wt;
            nw = new_nw;
            cur_offsets = DeviceBuffer::from_slice(&off)?;
            cur_indices = DeviceBuffer::from_slice(&idx)?;
            cur_weights = DeviceBuffer::from_slice(&wt)?;
            cur_node_weights = DeviceBuffer::from_slice(&nw)?;

            for s in orig_to_super.iter_mut() {
                *s = refined_dense[*s as usize];
            }
            seed_comm = p_of_refined; // dense in [0, num_comm)
            cur_n = num_refined;
        }

        let (labels, sizes) = compact_with_sizes(&best_labels);
        let num_communities = sizes.len();

        info!(
            "GPU Leiden (multi-level): {} communities, modularity={:.4}, iterations={}, converged={}",
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
}
