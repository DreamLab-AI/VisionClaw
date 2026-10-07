//! `visionclaw-gpu` — ADR-090 Phase 3 GPU/CUDA infrastructure crate.
//!
//! This crate owns:
//! - CUDA kernel source files (`cuda_sources/`)
//! - Pre-compiled PTX data (`ptx/`)
//! - PTX loader / runtime compilation utilities (`ptx_loader`)
//!
//! ## What lives here (Phase 3)
//! - CUDA `.cu` sources and pre-compiled `.ptx` binaries
//! - `ptx_loader`: runtime PTX acquisition, CUDA arch detection
//!
//! ## What is deferred to Phase 4
//! The GPU *actor* tree (`src/actors/gpu/` in webxr) could not be extracted in
//! Phase 3 because it depends on modules still inside the webxr monolith:
//! `crate::actors::messages`, `crate::gpu::*`, `crate::telemetry::*`,
//! `crate::utils::socket_flow_messages`, `crate::utils::unified_gpu_compute`.
//! Those cross-cutting dependencies must be resolved first.
//! See the Phase 3 implementation report for details.

/// ADR-070 CUDA integration hardening — host-side (CPU) logic for the
/// stability third criterion (D2.2), input-edge NaN guard (D2.3), and the
/// sparse compute mask (D3.1 / P2). Pure and GPU-free so it is unit-testable
/// without a device.
pub mod hardening;
pub mod ptx_loader;
/// Build-time PTX acceptance policy (ADR-2030) — pure, dependency-free logic
/// shared by `build.rs` and the library's test suite.
///
/// # Why this file is `include!`d by the build script
///
/// A build script cannot depend on the crate it builds, so build-time logic is
/// normally untestable and drifts from whatever the library believes. The
/// closeout for ADR-2030 had to *extract* the PTX phase into a separate probe to
/// test it at all, which proves the point: nothing guaranteed the probe still
/// matched the build script.
///
/// This module is therefore compiled twice from one source of truth —
/// `include!`d into `build.rs`, and declared as `pub mod ptx_policy` in the
/// library so the tests below run against the exact code the build executes. It
/// uses only `std`, so the include is free of dependency concerns.
///
/// # What the closeout found, and what this fixes
///
/// | Finding | Policy here |
/// |---|---|
/// | `nvcc` absent panicked *before* the fallback was consulted | [`NvccOutcome::LaunchFailed`] is distinct from [`NvccOutcome::CompilerFailed`], and both reach the fallback |
/// | A successful compiler writing `NOT PTX` passed the non-empty gate | [`validate_ptx`] checks directives and required symbols, not just length |
/// | The `.version` rewrite spliced a fixed 12-byte window, so `9.10` became `9.00` | [`rewrite_ptx_version`] parses the version token and rewrites by span |
/// | A downgrade warning was emitted even when nothing changed | [`VersionRewrite`] reports `Unchanged` distinctly from `Rewritten` |
/// | Nothing recorded which module was selected, or its content identity | [`PtxArtefact`] records source, original and rewritten digests |
///
/// Digests here are **not** cryptographic and are never used as a security
/// boundary: they are FNV-1a content tags for matching a built artefact to the
/// source it came from in a build manifest.
pub mod ptx_policy;
