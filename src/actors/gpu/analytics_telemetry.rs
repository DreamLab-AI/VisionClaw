//! GPU-path-execution telemetry for the analytics kernels (task #74).
//!
//! ADR-031 zero-fallback intent: an analytics kernel that silently runs on the CPU
//! instead of the GPU is a correctness/performance regression that must not pass
//! unnoticed. Every analytics dispatch in `src/actors/gpu/` records *which path it
//! actually executed on* through [`record_execution`]. The record does three things:
//!
//! 1. **Log** — a structured line at `info` (GPU, the expected path) or `warn`
//!    (CPU fallback, the gated condition).
//! 2. **Counter** — process-global atomic counters per kernel and per path, exposed
//!    through [`snapshot`] (surfaced by the `/analytics/gpu-metrics` route and the
//!    GPU analytics WebSocket).
//! 3. **Result field** — callers attach [`ExecutionPath`] to their result/message so
//!    downstream consumers (and the wire/HTTP layers) can see the path per run.
//!
//! This module owns no kernel math. It only observes dispatch outcomes.

use std::sync::atomic::{AtomicU64, Ordering};

/// The compute path an analytics kernel actually ran on for a single dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPath {
    /// Executed on the GPU — the expected, non-gated path.
    Gpu,
    /// Fell back to a CPU implementation after a GPU failure. This is the gated
    /// condition under the zero-fallback intent: it is recorded as a `warn` and
    /// increments the per-kernel fallback counter.
    CpuFallback,
}

impl ExecutionPath {
    /// `true` when the dispatch used the GPU (the expected path).
    pub fn is_gpu(self) -> bool {
        matches!(self, ExecutionPath::Gpu)
    }

    /// `true` when the dispatch silently dropped to a CPU fallback (gated).
    pub fn is_fallback(self) -> bool {
        matches!(self, ExecutionPath::CpuFallback)
    }

    /// Stable label used in logs, counters, and serialized result fields.
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionPath::Gpu => "gpu",
            ExecutionPath::CpuFallback => "cpu_fallback",
        }
    }
}

/// The analytics kernels whose dispatch path is tracked (Louvain/PageRank/DBSCAN/
/// LOF/SSSP and the connectivity helpers that share the GPU analytics pipeline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsKernel {
    Louvain,
    LabelPropagation,
    Kmeans,
    Dbscan,
    Pagerank,
    Lof,
    Sssp,
    Apsp,
    ConnectedComponents,
    Leiden,
}

impl AnalyticsKernel {
    /// Stable label used in logs and counter keys.
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyticsKernel::Louvain => "louvain",
            AnalyticsKernel::LabelPropagation => "label_propagation",
            AnalyticsKernel::Kmeans => "kmeans",
            AnalyticsKernel::Dbscan => "dbscan",
            AnalyticsKernel::Pagerank => "pagerank",
            AnalyticsKernel::Lof => "lof",
            AnalyticsKernel::Sssp => "sssp",
            AnalyticsKernel::Apsp => "apsp",
            AnalyticsKernel::ConnectedComponents => "connected_components",
            AnalyticsKernel::Leiden => "leiden",
        }
    }

    /// Index into the fixed per-kernel counter arrays.
    fn idx(self) -> usize {
        match self {
            AnalyticsKernel::Louvain => 0,
            AnalyticsKernel::LabelPropagation => 1,
            AnalyticsKernel::Kmeans => 2,
            AnalyticsKernel::Dbscan => 3,
            AnalyticsKernel::Pagerank => 4,
            AnalyticsKernel::Lof => 5,
            AnalyticsKernel::Sssp => 6,
            AnalyticsKernel::Apsp => 7,
            AnalyticsKernel::ConnectedComponents => 8,
            AnalyticsKernel::Leiden => 9,
        }
    }
}

const KERNEL_COUNT: usize = 10;

/// Process-global per-kernel GPU-run counters. Index via [`AnalyticsKernel::idx`].
static GPU_RUNS: [AtomicU64; KERNEL_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Process-global per-kernel CPU-fallback counters. Index via [`AnalyticsKernel::idx`].
static CPU_FALLBACK_RUNS: [AtomicU64; KERNEL_COUNT] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Record the path an analytics kernel executed on and emit the gated log line.
///
/// Returns the same [`ExecutionPath`] so callers can fluently attach it to a result:
/// `let path = record_execution(AnalyticsKernel::Sssp, ExecutionPath::Gpu);`.
///
/// A `CpuFallback` is logged at `warn` and increments the fallback counter — it is
/// the zero-fallback gated condition. A `Gpu` run is logged at `info` (debug-level
/// noise is avoided since analytics dispatches are coarse-grained).
pub fn record_execution(kernel: AnalyticsKernel, path: ExecutionPath) -> ExecutionPath {
    let i = kernel.idx();
    match path {
        ExecutionPath::Gpu => {
            GPU_RUNS[i].fetch_add(1, Ordering::Relaxed);
            log::info!(
                "analytics-gpu-telemetry: kernel={} path=gpu (gpu_runs={}, cpu_fallback_runs={})",
                kernel.as_str(),
                GPU_RUNS[i].load(Ordering::Relaxed),
                CPU_FALLBACK_RUNS[i].load(Ordering::Relaxed),
            );
        }
        ExecutionPath::CpuFallback => {
            CPU_FALLBACK_RUNS[i].fetch_add(1, Ordering::Relaxed);
            // Zero-fallback gate: a silent CPU fallback is a gated regression. Surface
            // it at warn so it is never missed in logs or the metrics snapshot.
            log::warn!(
                "analytics-gpu-telemetry: ZERO-FALLBACK GATE — kernel={} path=cpu_fallback \
                 (the GPU path failed and a CPU implementation ran instead); \
                 cpu_fallback_runs={} gpu_runs={}",
                kernel.as_str(),
                CPU_FALLBACK_RUNS[i].load(Ordering::Relaxed),
                GPU_RUNS[i].load(Ordering::Relaxed),
            );
        }
    }
    path
}

/// Per-kernel execution-path counts for one kernel.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct KernelExecutionCounts {
    pub kernel: &'static str,
    pub gpu_runs: u64,
    pub cpu_fallback_runs: u64,
}

/// Read the current process-global execution-path counters for every analytics
/// kernel. Used by the `/analytics/gpu-metrics` route and the analytics WebSocket
/// so the GPU-vs-fallback split is observable, not just logged.
pub fn snapshot() -> Vec<KernelExecutionCounts> {
    const KERNELS: [AnalyticsKernel; KERNEL_COUNT] = [
        AnalyticsKernel::Louvain,
        AnalyticsKernel::LabelPropagation,
        AnalyticsKernel::Kmeans,
        AnalyticsKernel::Dbscan,
        AnalyticsKernel::Pagerank,
        AnalyticsKernel::Lof,
        AnalyticsKernel::Sssp,
        AnalyticsKernel::Apsp,
        AnalyticsKernel::ConnectedComponents,
        AnalyticsKernel::Leiden,
    ];
    KERNELS
        .iter()
        .map(|k| {
            let i = k.idx();
            KernelExecutionCounts {
                kernel: k.as_str(),
                gpu_runs: GPU_RUNS[i].load(Ordering::Relaxed),
                cpu_fallback_runs: CPU_FALLBACK_RUNS[i].load(Ordering::Relaxed),
            }
        })
        .collect()
}

/// Total CPU-fallback count across all analytics kernels. A non-zero value means the
/// zero-fallback intent has been violated at least once this process lifetime.
pub fn total_cpu_fallbacks() -> u64 {
    CPU_FALLBACK_RUNS
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_run_records_as_gpu_and_increments_counter() {
        let before = snapshot()
            .into_iter()
            .find(|c| c.kernel == "pagerank")
            .map(|c| c.gpu_runs)
            .unwrap();
        let path = record_execution(AnalyticsKernel::Pagerank, ExecutionPath::Gpu);
        assert!(path.is_gpu());
        assert!(!path.is_fallback());
        let after = snapshot()
            .into_iter()
            .find(|c| c.kernel == "pagerank")
            .map(|c| c.gpu_runs)
            .unwrap();
        assert_eq!(
            after,
            before + 1,
            "GPU run must increment the gpu_runs counter"
        );
    }

    #[test]
    fn fallback_records_as_gated_and_increments_fallback_counter() {
        let before = total_cpu_fallbacks();
        let path = record_execution(
            AnalyticsKernel::ConnectedComponents,
            ExecutionPath::CpuFallback,
        );
        assert!(path.is_fallback());
        assert!(!path.is_gpu());
        assert!(
            total_cpu_fallbacks() > before,
            "CPU fallback must increment the gated fallback counter"
        );
        assert_eq!(path.as_str(), "cpu_fallback");
    }

    #[test]
    fn every_kernel_has_a_distinct_index() {
        use std::collections::HashSet;
        let kernels = [
            AnalyticsKernel::Louvain,
            AnalyticsKernel::LabelPropagation,
            AnalyticsKernel::Kmeans,
            AnalyticsKernel::Dbscan,
            AnalyticsKernel::Pagerank,
            AnalyticsKernel::Lof,
            AnalyticsKernel::Sssp,
            AnalyticsKernel::Apsp,
            AnalyticsKernel::ConnectedComponents,
            AnalyticsKernel::Leiden,
        ];
        let indices: HashSet<usize> = kernels.iter().map(|k| k.idx()).collect();
        assert_eq!(indices.len(), KERNEL_COUNT, "kernel indices must be unique");
        assert!(indices.iter().all(|&i| i < KERNEL_COUNT));
    }
}
