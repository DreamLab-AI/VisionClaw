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

/// Most delta-stepping buckets one run processes. Each bucket costs one device-to-host copy of
/// the distance array and a host scan, both under the engine lock, so a tiny `delta` on a large
/// graph would otherwise cost O(buckets x n). Past this many buckets the run finishes with the
/// plain frontier relaxation, which returns the same distances. At 10,000 nodes the cap bounds
/// the bucket traffic at 64 x 40 kB = 2.5 MB per run.
pub const SSSP_MAX_DELTA_BUCKETS: usize = 64;

/// Smallest accepted `delta`, as a fraction of the graph's largest edge weight. A narrower
/// bucket than 1/4096 of the heaviest edge only multiplies buckets without changing the result.
pub const SSSP_MIN_DELTA_FRACTION: f32 = 1.0 / 4096.0;

/// Prefix of every `delta` rejection, so callers can report it as a client error (HTTP 400).
pub const INVALID_SSSP_DELTA: &str = "invalid SSSP delta";

/// Checks a requested bucket width before any graph is consulted: it must be a finite number
/// greater than zero. The weight-relative minimum is checked by [`UnifiedGPUCompute::run_sssp`].
///
/// # Errors
///
/// A message starting with [`INVALID_SSSP_DELTA`].
pub fn check_sssp_delta(delta: f32) -> std::result::Result<(), String> {
    if delta.is_finite() && delta > 0.0 {
        Ok(())
    } else {
        Err(format!(
            "{INVALID_SSSP_DELTA}: {delta}; it must be a finite number greater than zero"
        ))
    }
}

impl UnifiedGPUCompute {
    /// Shortest-path distances from `source_idx` to every live node; unreachable nodes are
    /// `f32::INFINITY`. Edge lengths are the CSR `edge_weights`, which must be non-negative.
    ///
    /// * `delta`: `None` runs the plain frontier relaxation (Bellman-Ford over an active
    ///   frontier). `Some(d)` runs delta-stepping with bucket width `d`: the frontier relaxation
    ///   settles every node closer than the current bucket bound, then the bound moves to the
    ///   bucket holding the nearest unsettled node. `d` must be finite, positive and at least
    ///   [`SSSP_MIN_DELTA_FRACTION`] of the largest edge weight. After
    ///   [`SSSP_MAX_DELTA_BUCKETS`] buckets, or if the bound can no longer advance in f32, the
    ///   run finishes with the plain relaxation. Every mode returns the same distances.
    ///
    /// On success the distances are also kept on the device for the force pass's SSSP
    /// rest-length term; they are discarded on the next graph change.
    ///
    /// # Errors
    ///
    /// The source is out of range; `delta` is invalid (the message starts with
    /// [`INVALID_SSSP_DELTA`]); an edge weight is negative (in the symmetric CSR that is a
    /// negative cycle); a registered buffer is shorter than the graph needs; a CUDA call fails;
    /// or the relaxation does not converge. Invalid arguments leave any kept distances alone;
    /// every other failure discards them.
    pub fn run_sssp(&mut self, source_idx: usize, delta: Option<f32>) -> Result<Vec<f32>> {
        if source_idx >= self.num_nodes {
            return Err(anyhow!("source node {} out of range", source_idx));
        }
        if let Some(d) = delta {
            check_sssp_delta(d).map_err(|e| anyhow!(e))?;
            let floor = self.sssp_edge_weight_range.1 * SSSP_MIN_DELTA_FRACTION;
            if d < floor {
                return Err(anyhow!(
                    "{INVALID_SSSP_DELTA}: {d} is below {floor}, 1/4096 of the largest edge \
                     weight ({}); use a wider bucket or omit delta",
                    self.sssp_edge_weight_range.1
                ));
            }
        }
        if self.sssp_edge_weight_range.0 < 0.0 {
            return Err(anyhow!(
                "SSSP needs non-negative edge weights; the smallest is {} (in the symmetric CSR \
                 a negative weight is a negative cycle)",
                self.sssp_edge_weight_range.0
            ));
        }
        self.sssp_available = false;
        self.sssp_device_distances = None;
        self.require_graph_capacity(SSSP_BUFFERS)?;

        let result = self.relax_from(source_idx, delta);

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
    pub(super) fn relax_from(
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

        // Nodes closer than `bound` are settled once the inner loop drains. `width` becomes
        // `None` when the run falls back to the plain relaxation.
        let mut width = bucket_width;
        let mut bound = width.unwrap_or(f32::INFINITY);
        // Within one drain of the frontier, a shortest path gains at least one hop per round,
        // so more than n + 1 rounds means the weights are not what `run_sssp` checked.
        let max_rounds_per_drain = n + 1;
        let mut buckets = 0usize;
        // Re-seeded frontier for the next bucket. Declared outside the loop so it outlives its
        // asynchronous upload, which completes at the inner loop's next synchronise.
        let mut frontier: Vec<i32> = Vec::new();
        // Termination: every pass either ends the loop, or (bucketed) strictly raises `bound`
        // and counts a bucket, or (fallback) clears `width`, after which the next pass ends
        // the loop. So there are at most SSSP_MAX_DELTA_BUCKETS + 2 passes; the check below
        // turns any violation of that argument into an error rather than a spin under the lock.
        for pass in 0.. {
            if pass > SSSP_MAX_DELTA_BUCKETS + 2 {
                return Err(anyhow!(
                    "SSSP exceeded {} bucket passes; the bucket loop failed to terminate",
                    SSSP_MAX_DELTA_BUCKETS + 2
                ));
            }
            let mut rounds = 0usize;
            while frontier_len > 0 {
                rounds += 1;
                if rounds > max_rounds_per_drain {
                    return Err(anyhow!(
                        "SSSP did not converge within {max_rounds_per_drain} relaxation rounds"
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

            let Some(w) = width else {
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
            buckets += 1;
            let lower = bound;
            let mut next = ((nearest / w).floor() + 1.0) * w;
            if next.partial_cmp(&nearest) != Some(std::cmp::Ordering::Greater) {
                next = nearest + w;
            }
            let advances = next.partial_cmp(&nearest) == Some(std::cmp::Ordering::Greater);
            if advances && buckets < SSSP_MAX_DELTA_BUCKETS {
                bound = next;
            } else {
                // Either `w` is below half the f32 spacing at `nearest` (the bound cannot move)
                // or the bucket budget is spent: finish with the plain relaxation from every
                // tentatively labelled, unsettled node.
                log::debug!(
                    "SSSP: delta-stepping falls back to plain relaxation after {buckets} \
                     buckets (bound {bound}, nearest {nearest}, delta {w})"
                );
                width = None;
                bound = f32::INFINITY;
            }
            frontier.clear();
            frontier.extend(
                snapshot
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.is_finite() && **d >= lower && **d < bound)
                    .map(|(i, _)| i as i32),
            );
            // SAFETY: `frontier.len() <= n`; the vector is not touched again until after the
            // inner loop's next synchronise, by which time the upload has completed.
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
