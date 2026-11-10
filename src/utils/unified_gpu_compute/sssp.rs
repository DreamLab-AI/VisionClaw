//! Single-source shortest paths (SSSP) on the engine's CSR.

use super::construction::UnifiedGPUCompute;
use anyhow::{anyhow, Result};
use cust::launch;
use cust::memory::{AsyncCopyDestination, DeviceBuffer};

/// Buffers SSSP reads or writes, checked against the live graph before any launch.
const SSSP_BUFFERS: &[&str] = &[
    "dist",
    "current_frontier",
    "next_frontier_flags",
    "edge_row_offsets",
    "edge_col_indices",
    "edge_weights",
];

const BLOCK: u32 = 256;

impl UnifiedGPUCompute {
    /// Shortest-path distances from `source_idx` to every live node; unreachable nodes are
    /// `f32::INFINITY`. Edge lengths are the CSR `edge_weights`, which must be non-negative.
    ///
    /// * `delta`: `Some(d)` with `0 < d < ∞` runs delta-stepping with bucket width `d`: the
    ///   frontier relaxation settles every node closer than the current bucket bound, then the
    ///   bound moves to the bucket holding the nearest unsettled node. Any other value runs the
    ///   plain frontier relaxation (Bellman-Ford over an active frontier). Both return the same
    ///   distances; `delta` only changes the order of work.
    ///
    /// On success the distances are also kept on the device for the force pass's SSSP
    /// rest-length term; they are discarded on the next graph change.
    ///
    /// # Errors
    ///
    /// The source is out of range, a registered buffer is shorter than the graph needs, a CUDA
    /// call fails, or the relaxation does not converge (a negative edge weight forms a negative
    /// cycle, since the CSR holds both directions of every edge). No distances are kept then.
    pub fn run_sssp(&mut self, source_idx: usize, delta: Option<f32>) -> Result<Vec<f32>> {
        if source_idx >= self.num_nodes {
            return Err(anyhow!("source node {} out of range", source_idx));
        }
        self.sssp_available = false;
        self.sssp_device_distances = None;
        self.require_graph_capacity(SSSP_BUFFERS)?;

        let bucket_width = delta.filter(|d| *d > 0.0 && d.is_finite());
        let result = self.relax_from(source_idx, bucket_width);

        match result {
            Ok((distances, kept)) => {
                self.sssp_device_distances = Some(kept);
                self.sssp_available = true;
                log::info!("SSSP computation successful from source {}", source_idx);
                Ok(distances)
            }
            Err(e) => {
                log::error!("SSSP computation failed: {e:#}. State invalidated.");
                Err(e)
            }
        }
    }

    /// The relaxation itself. Every copy, memset and launch is issued on the SSSP stream and the
    /// host reads only after synchronising that stream: the stream is `NON_BLOCKING`, so the
    /// synchronous default-stream copies used before S1a were not ordered against its kernels.
    ///
    /// Returns the distances and a device copy of them for the force pass.
    fn relax_from(
        &self,
        source: usize,
        bucket_width: Option<f32>,
    ) -> Result<(Vec<f32>, DeviceBuffer<f32>)> {
        let n = self.num_nodes;
        let stream = self.sssp_stream.as_ref().unwrap_or(&self.stream);
        let relax = self._module.get_function("relaxation_step_kernel")?;
        let compact = self._module.get_function("compact_frontier_kernel")?;
        let mut counter = DeviceBuffer::from_slice(&[0i32])?;
        let mut count = [0i32];

        let mut distances = vec![f32::INFINITY; n];
        distances[source] = 0.0;
        let seed = [source as i32];
        // SAFETY: each destination is a sub-slice of a registered buffer that
        // `require_graph_capacity` proved holds at least `n` elements; the host sources live
        // until the stream is synchronised below.
        unsafe {
            self.dist
                .index(0..n)
                .async_copy_from(&distances[..], stream)?;
            self.current_frontier
                .index(0..1)
                .async_copy_from(&seed[..], stream)?;
        }
        let mut frontier_len = 1usize;

        // Nodes closer than `bound` are settled once the inner loop drains.
        let mut bound = bucket_width.unwrap_or(f32::INFINITY);
        let max_rounds = 10 * n.max(1);
        let mut rounds = 0usize;
        loop {
            while frontier_len > 0 {
                rounds += 1;
                if rounds > max_rounds {
                    return Err(anyhow!(
                        "SSSP did not converge within {max_rounds} relaxation rounds; edge \
                         weights must be non-negative (in the symmetric CSR a negative weight \
                         is a negative cycle)"
                    ));
                }
                let grid = (frontier_len as u32).div_ceil(BLOCK);
                let compact_grid = (n as u32).div_ceil(BLOCK);
                // SAFETY: `dist`, `next_frontier_flags` and `current_frontier` hold at least
                // `n` elements and the CSR buffers at least `num_nodes + 1` / `num_edges`
                // (`require_graph_capacity`); the frontier holds `frontier_len <= n` valid node
                // indices written by the compaction or the seeding; the relaxation kernel
                // guards `t < frontier_size` and the compaction `idx < num_nodes`.
                unsafe {
                    self.next_frontier_flags
                        .index(0..n)
                        .set_32_async(0, stream)?;
                    counter.set_32_async(0, stream)?;
                    launch!(relax<<<grid, BLOCK, 0, stream>>>(
                        self.dist.as_device_ptr(),
                        self.current_frontier.as_device_ptr(),
                        frontier_len as i32,
                        self.edge_row_offsets.as_device_ptr(),
                        self.edge_col_indices.as_device_ptr(),
                        self.edge_weights.as_device_ptr(),
                        self.next_frontier_flags.as_device_ptr(),
                        bound,
                        n as i32
                    ))?;
                    launch!(compact<<<compact_grid, BLOCK, 0, stream>>>(
                        self.next_frontier_flags.as_device_ptr(),
                        self.current_frontier.as_device_ptr(),
                        counter.as_device_ptr(),
                        n as i32
                    ))?;
                    counter.async_copy_to(&mut count[..], stream)?;
                }
                stream.synchronize()?;
                frontier_len = count[0] as usize;
            }

            let Some(width) = bucket_width else {
                break;
            };
            // The bucket below `bound` is settled. Distances at or beyond it were recorded by
            // the relaxation but not expanded; continue from the bucket holding the nearest of
            // them, skipping empty buckets, or stop when none is left.
            let mut snapshot = vec![0.0f32; n];
            // SAFETY: as above; `snapshot` lives until the synchronise.
            unsafe {
                self.dist
                    .index(0..n)
                    .async_copy_to(&mut snapshot[..], stream)?;
            }
            stream.synchronize()?;
            let nearest = snapshot
                .iter()
                .copied()
                .filter(|d| d.is_finite() && *d >= bound)
                .fold(f32::INFINITY, f32::min);
            if !nearest.is_finite() {
                break;
            }
            let lower = bound;
            bound = ((nearest / width).floor() + 1.0) * width;
            if bound <= nearest {
                bound = nearest + width;
            }
            let frontier: Vec<i32> = snapshot
                .iter()
                .enumerate()
                .filter(|(_, d)| **d >= lower && **d < bound)
                .map(|(i, _)| i as i32)
                .collect();
            // SAFETY: `frontier.len() <= n`; the vector lives until the next synchronise.
            unsafe {
                self.current_frontier
                    .index(0..frontier.len())
                    .async_copy_from(&frontier[..], stream)?;
            }
            frontier_len = frontier.len();
        }

        // SAFETY: `kept` is fully overwritten by the device-to-device copy before anything reads
        // it; the copies are ordered on the stream and complete at the synchronise.
        let kept = unsafe {
            let mut kept = DeviceBuffer::<f32>::uninitialized(n)?;
            kept.async_copy_from(&self.dist.index(0..n), stream)?;
            self.dist
                .index(0..n)
                .async_copy_to(&mut distances[..], stream)?;
            kept
        };
        stream.synchronize()?;
        Ok((distances, kept))
    }

    // `run_sssp_batch` removed (S1a): it had no callers and copied whole-buffer lengths into
    // `dist`, the same equal-length panic `run_sssp` had.
    //
    // `run_apsp_gpu` removed (ADR-031 D8 / NFR-7, superseded by ADR-2054): the
    // dense [n][n] approximate APSP matrix is O(n^2) memory and is forbidden
    // on the analytics path. The backing `approximate_apsp_kernel` has been
    // deleted from gpu_landmark_apsp.cu entirely (it was already `#if 0`'d
    // out of compilation); the caller (ShortestPathActor::handle<ComputeAPSP>)
    // fails closed rather than materialising the matrix on CPU.
}
