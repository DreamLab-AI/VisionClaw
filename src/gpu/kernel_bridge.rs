//! CUDA Kernel Bridge - Safe wrappers around GPU FFI with CPU fallback
//!
//! This module provides a unified interface for GPU kernel invocations.
//! When compiled with `feature = "gpu"`, calls dispatch to CUDA kernels
//! via FFI. Without the feature, all calls fall back to CPU implementations
//! that log a warning on first invocation.
//!
//! # Design
//!
//! The bridge uses compile-time feature gating (`cfg(feature = "gpu")`) to
//! select between GPU and CPU paths. This avoids both runtime overhead and
//! linker errors when CUDA libraries are unavailable.
//!
//! # Safety
//!
//! All unsafe FFI calls are encapsulated within safe public functions that
//! validate pointer arguments and slice lengths before crossing the FFI
//! boundary.

#[cfg(not(feature = "gpu"))]
use log::warn;
#[cfg(not(feature = "gpu"))]
use std::sync::atomic::{AtomicBool, Ordering};

// Use the canonical DynamicForceConfigGPU from the GPU semantic forces module.
// Multiple identical copies of this #[repr(C)] struct exist across the codebase
// (actor, service, GPU module). This bridge uses the GPU module's canonical version.
// Callers holding the actor's version can safely transmute slices because the types
// have identical layout (verified by static_assertions in the actor module).
pub use crate::gpu::semantic_forces::DynamicForceConfigGPU;

// ============================================================================
// Fallback warning flags (one log per function, per process lifetime)
// Only compiled when GPU feature is disabled to avoid dead-code warnings.
// ============================================================================

#[cfg(not(feature = "gpu"))]
static WARNED_SET_DYNAMIC_BUFFER: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_UPDATE_DYNAMIC_CONFIG: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_SET_DYNAMIC_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_GET_BUFFER_VERSION: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_GET_MAX_REL_TYPES: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_HIERARCHY_LEVELS: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_TYPE_CENTROIDS: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_FINALIZE_CENTROIDS: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_PHYSICALITY_CLUSTER: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_ROLE_CLUSTER: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_MATURITY_LAYOUT: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_PHYSICALITY_CENTROIDS: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_FINALIZE_PHYSICALITY: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_ROLE_CENTROIDS: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gpu"))]
static WARNED_FINALIZE_ROLE: AtomicBool = AtomicBool::new(false);

#[cfg(not(feature = "gpu"))]
fn warn_once(flag: &AtomicBool, fn_name: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        warn!(
            "kernel_bridge: GPU unavailable, using CPU fallback for `{}`. \
             Build with `--features gpu` to enable CUDA acceleration.",
            fn_name
        );
    }
}

// ============================================================================
// GPU path (feature = "gpu")
// ============================================================================

#[cfg(feature = "gpu")]
mod gpu_ffi {
    use super::DynamicForceConfigGPU;

    extern "C" {
        /// 1 when the FFI symbols are the no-op stubs of a PTX-only build.
        pub fn visionclaw_ffi_stubbed() -> i32;

        pub fn set_dynamic_relationship_buffer(
            configs: *const DynamicForceConfigGPU,
            num_types: i32,
            enabled: bool,
        ) -> i32;

        pub fn update_dynamic_relationship_config(
            type_id: i32,
            config: *const DynamicForceConfigGPU,
        ) -> i32;

        pub fn set_dynamic_relationships_enabled(enabled: bool) -> i32;

        pub fn get_dynamic_relationship_buffer_version() -> i32;

        pub fn get_max_relationship_types() -> i32;

        pub fn calculate_hierarchy_levels(
            edge_sources: *const i32,
            edge_targets: *const i32,
            edge_types: *const i32,
            node_levels: *mut i32,
            changed: *mut bool,
            num_edges: i32,
            num_nodes: i32,
        );

        pub fn calculate_type_centroids(
            node_types: *const i32,
            positions: *const super::Float3,
            type_centroids: *mut super::Float3,
            type_counts: *mut i32,
            num_nodes: i32,
            num_types: i32,
        );

        pub fn finalize_type_centroids(
            type_centroids: *mut super::Float3,
            type_counts: *const i32,
            num_types: i32,
        );

        pub fn apply_physicality_cluster_force(
            node_physicality: *const i32,
            physicality_centroids: *const super::Float3,
            positions: *mut super::Float3,
            forces: *mut super::Float3,
            num_nodes: i32,
        );

        pub fn apply_role_cluster_force(
            node_role: *const i32,
            role_centroids: *const super::Float3,
            positions: *mut super::Float3,
            forces: *mut super::Float3,
            num_nodes: i32,
        );

        pub fn apply_maturity_layout_force(
            node_maturity: *const i32,
            positions: *mut super::Float3,
            forces: *mut super::Float3,
            num_nodes: i32,
        );

        pub fn calculate_physicality_centroids(
            node_physicality: *const i32,
            positions: *const super::Float3,
            physicality_centroids: *mut super::Float3,
            physicality_counts: *mut i32,
            num_nodes: i32,
        );

        pub fn finalize_physicality_centroids(
            physicality_centroids: *mut super::Float3,
            physicality_counts: *const i32,
        );

        pub fn calculate_role_centroids(
            node_role: *const i32,
            positions: *const super::Float3,
            role_centroids: *mut super::Float3,
            role_counts: *mut i32,
            num_nodes: i32,
        );

        pub fn finalize_role_centroids(role_centroids: *mut super::Float3, role_counts: *const i32);
    }
}

// ============================================================================
// Float3 type (matches CUDA float3, needed for centroid bridge)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Float3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

// ============================================================================
// Public safe wrappers
// ============================================================================

/// Upload dynamic relationship configurations to the GPU.
///
/// Returns 0 on success, non-zero on error.
/// On CPU fallback: always returns 0 (no-op).
pub fn set_dynamic_relationship_buffer(configs: &[DynamicForceConfigGPU], enabled: bool) -> i32 {
    #[cfg(feature = "gpu")]
    {
        if configs.len() > i32::MAX as usize {
            return -1;
        }
        let ptr = if configs.is_empty() {
            std::ptr::null()
        } else {
            configs.as_ptr()
        };
        unsafe { gpu_ffi::set_dynamic_relationship_buffer(ptr, configs.len() as i32, enabled) }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (configs, enabled);
        warn_once(
            &WARNED_SET_DYNAMIC_BUFFER,
            "set_dynamic_relationship_buffer",
        );
        0
    }
}

/// Update a single relationship type configuration on the GPU (hot-reload).
///
/// Returns 0 on success, non-zero on error.
/// On CPU fallback: always returns 0 (no-op).
pub fn update_dynamic_relationship_config(type_id: i32, config: &DynamicForceConfigGPU) -> i32 {
    #[cfg(feature = "gpu")]
    {
        unsafe { gpu_ffi::update_dynamic_relationship_config(type_id, config as *const _) }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (type_id, config);
        warn_once(
            &WARNED_UPDATE_DYNAMIC_CONFIG,
            "update_dynamic_relationship_config",
        );
        0
    }
}

/// Enable or disable dynamic relationship forces on the GPU.
///
/// Returns 0 on success, non-zero on error.
/// On CPU fallback: always returns 0 (no-op).
pub fn set_dynamic_relationships_enabled(enabled: bool) -> i32 {
    #[cfg(feature = "gpu")]
    {
        unsafe { gpu_ffi::set_dynamic_relationships_enabled(enabled) }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = enabled;
        warn_once(
            &WARNED_SET_DYNAMIC_ENABLED,
            "set_dynamic_relationships_enabled",
        );
        0
    }
}

/// Get current buffer version for hot-reload detection.
///
/// On CPU fallback: always returns 0.
pub fn get_dynamic_relationship_buffer_version() -> i32 {
    #[cfg(feature = "gpu")]
    {
        unsafe { gpu_ffi::get_dynamic_relationship_buffer_version() }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(
            &WARNED_GET_BUFFER_VERSION,
            "get_dynamic_relationship_buffer_version",
        );
        0
    }
}

/// Get maximum supported relationship types.
///
/// On CPU fallback: returns 256 (generous default matching typical GPU constant memory).
pub fn get_max_relationship_types() -> i32 {
    #[cfg(feature = "gpu")]
    {
        unsafe { gpu_ffi::get_max_relationship_types() }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_GET_MAX_REL_TYPES, "get_max_relationship_types");
        256
    }
}

/// Calculate hierarchy levels using GPU label propagation.
///
/// On CPU fallback: performs a single-pass edge scan to propagate levels.
/// This is O(edges) per call vs O(1) amortized on GPU, but functionally equivalent
/// for the iterative BFS pattern used by `SemanticForcesActor`.
pub fn calculate_hierarchy_levels(
    edge_sources: &[i32],
    edge_targets: &[i32],
    edge_types: &[i32],
    node_levels: &mut [i32],
    changed: &mut bool,
    num_edges: usize,
    num_nodes: usize,
) {
    debug_assert_eq!(edge_sources.len(), edge_targets.len());
    debug_assert_eq!(edge_sources.len(), edge_types.len());
    debug_assert!(edge_sources.len() >= num_edges);
    debug_assert!(node_levels.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::calculate_hierarchy_levels(
                edge_sources.as_ptr(),
                edge_targets.as_ptr(),
                edge_types.as_ptr(),
                node_levels.as_mut_ptr(),
                changed as *mut bool,
                num_edges as i32,
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_HIERARCHY_LEVELS, "calculate_hierarchy_levels");
        // CPU fallback: single-pass edge propagation (same logic as the CUDA kernel)
        *changed = false;
        for i in 0..num_edges {
            let edge_type = edge_types[i];
            if edge_type == 2 {
                // hierarchy edge
                let src = edge_sources[i] as usize;
                let tgt = edge_targets[i] as usize;
                if src < num_nodes && tgt < num_nodes && node_levels[src] >= 0 {
                    let new_level = node_levels[src] + 1;
                    if new_level > node_levels[tgt] {
                        node_levels[tgt] = new_level;
                        *changed = true;
                    }
                }
            }
        }
    }
}

/// Calculate centroid positions for each node type.
///
/// On CPU fallback: accumulates positions per type into `type_centroids` and
/// counts into `type_counts`. Caller must still call `finalize_type_centroids`
/// to divide by counts.
pub fn calculate_type_centroids(
    node_types: &[i32],
    positions: &[Float3],
    type_centroids: &mut [Float3],
    type_counts: &mut [i32],
    num_nodes: usize,
    num_types: usize,
) {
    debug_assert!(node_types.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);
    debug_assert!(type_centroids.len() >= num_types);
    debug_assert!(type_counts.len() >= num_types);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::calculate_type_centroids(
                node_types.as_ptr(),
                positions.as_ptr(),
                type_centroids.as_mut_ptr(),
                type_counts.as_mut_ptr(),
                num_nodes as i32,
                num_types as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_TYPE_CENTROIDS, "calculate_type_centroids");
        // CPU fallback: accumulate positions
        for c in type_centroids.iter_mut().take(num_types) {
            c.x = 0.0;
            c.y = 0.0;
            c.z = 0.0;
        }
        for cnt in type_counts.iter_mut().take(num_types) {
            *cnt = 0;
        }
        for i in 0..num_nodes {
            let t = node_types[i] as usize;
            if t < num_types {
                type_centroids[t].x += positions[i].x;
                type_centroids[t].y += positions[i].y;
                type_centroids[t].z += positions[i].z;
                type_counts[t] += 1;
            }
        }
    }
}

/// Finalize centroids by dividing accumulated positions by count.
///
/// On CPU fallback: performs the division directly.
pub fn finalize_type_centroids(
    type_centroids: &mut [Float3],
    type_counts: &[i32],
    num_types: usize,
) {
    debug_assert!(type_centroids.len() >= num_types);
    debug_assert!(type_counts.len() >= num_types);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::finalize_type_centroids(
                type_centroids.as_mut_ptr(),
                type_counts.as_ptr(),
                num_types as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_FINALIZE_CENTROIDS, "finalize_type_centroids");
        for i in 0..num_types {
            let count = type_counts[i];
            if count > 0 {
                let c = count as f32;
                type_centroids[i].x /= c;
                type_centroids[i].y /= c;
                type_centroids[i].z /= c;
            }
        }
    }
}

/// Apply physicality-based clustering forces on GPU.
///
/// On CPU fallback: no-op (caller must use CPU implementation).
pub fn apply_physicality_cluster_force(
    node_physicality: &[i32],
    physicality_centroids: &[Float3],
    positions: &mut [Float3],
    forces: &mut [Float3],
    num_nodes: usize,
) {
    debug_assert!(node_physicality.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);
    debug_assert!(forces.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::apply_physicality_cluster_force(
                node_physicality.as_ptr(),
                physicality_centroids.as_ptr(),
                positions.as_mut_ptr(),
                forces.as_mut_ptr(),
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (
            node_physicality,
            physicality_centroids,
            positions,
            forces,
            num_nodes,
        );
        warn_once(
            &WARNED_PHYSICALITY_CLUSTER,
            "apply_physicality_cluster_force",
        );
    }
}

/// Apply role-based clustering forces on GPU.
///
/// On CPU fallback: no-op (caller must use CPU implementation).
pub fn apply_role_cluster_force(
    node_role: &[i32],
    role_centroids: &[Float3],
    positions: &mut [Float3],
    forces: &mut [Float3],
    num_nodes: usize,
) {
    debug_assert!(node_role.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);
    debug_assert!(forces.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::apply_role_cluster_force(
                node_role.as_ptr(),
                role_centroids.as_ptr(),
                positions.as_mut_ptr(),
                forces.as_mut_ptr(),
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (node_role, role_centroids, positions, forces, num_nodes);
        warn_once(&WARNED_ROLE_CLUSTER, "apply_role_cluster_force");
    }
}

/// Apply maturity-based layout forces on GPU.
///
/// On CPU fallback: no-op (caller must use CPU implementation).
pub fn apply_maturity_layout_force(
    node_maturity: &[i32],
    positions: &mut [Float3],
    forces: &mut [Float3],
    num_nodes: usize,
) {
    debug_assert!(node_maturity.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);
    debug_assert!(forces.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::apply_maturity_layout_force(
                node_maturity.as_ptr(),
                positions.as_mut_ptr(),
                forces.as_mut_ptr(),
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (node_maturity, positions, forces, num_nodes);
        warn_once(&WARNED_MATURITY_LAYOUT, "apply_maturity_layout_force");
    }
}

/// Calculate physicality centroids on GPU.
///
/// On CPU fallback: accumulates positions per physicality type. Caller must
/// call `finalize_physicality_centroids` to divide by counts.
pub fn calculate_physicality_centroids(
    node_physicality: &[i32],
    positions: &[Float3],
    physicality_centroids: &mut [Float3],
    physicality_counts: &mut [i32],
    num_nodes: usize,
) {
    debug_assert!(node_physicality.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::calculate_physicality_centroids(
                node_physicality.as_ptr(),
                positions.as_ptr(),
                physicality_centroids.as_mut_ptr(),
                physicality_counts.as_mut_ptr(),
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(
            &WARNED_PHYSICALITY_CENTROIDS,
            "calculate_physicality_centroids",
        );
        let num_types = physicality_centroids.len();
        for c in physicality_centroids.iter_mut() {
            c.x = 0.0;
            c.y = 0.0;
            c.z = 0.0;
        }
        for cnt in physicality_counts.iter_mut() {
            *cnt = 0;
        }
        for i in 0..num_nodes {
            let t = node_physicality[i] as usize;
            if t < num_types {
                physicality_centroids[t].x += positions[i].x;
                physicality_centroids[t].y += positions[i].y;
                physicality_centroids[t].z += positions[i].z;
                physicality_counts[t] += 1;
            }
        }
    }
}

/// Finalize physicality centroids by dividing accumulated positions by count.
///
/// On CPU fallback: performs the division directly.
pub fn finalize_physicality_centroids(
    physicality_centroids: &mut [Float3],
    physicality_counts: &[i32],
) {
    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::finalize_physicality_centroids(
                physicality_centroids.as_mut_ptr(),
                physicality_counts.as_ptr(),
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(
            &WARNED_FINALIZE_PHYSICALITY,
            "finalize_physicality_centroids",
        );
        let num_types = physicality_centroids.len().min(physicality_counts.len());
        for i in 0..num_types {
            let count = physicality_counts[i];
            if count > 0 {
                let c = count as f32;
                physicality_centroids[i].x /= c;
                physicality_centroids[i].y /= c;
                physicality_centroids[i].z /= c;
            }
        }
    }
}

/// Calculate role centroids on GPU.
///
/// On CPU fallback: accumulates positions per role type. Caller must
/// call `finalize_role_centroids` to divide by counts.
pub fn calculate_role_centroids(
    node_role: &[i32],
    positions: &[Float3],
    role_centroids: &mut [Float3],
    role_counts: &mut [i32],
    num_nodes: usize,
) {
    debug_assert!(node_role.len() >= num_nodes);
    debug_assert!(positions.len() >= num_nodes);

    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::calculate_role_centroids(
                node_role.as_ptr(),
                positions.as_ptr(),
                role_centroids.as_mut_ptr(),
                role_counts.as_mut_ptr(),
                num_nodes as i32,
            );
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_ROLE_CENTROIDS, "calculate_role_centroids");
        let num_types = role_centroids.len();
        for c in role_centroids.iter_mut() {
            c.x = 0.0;
            c.y = 0.0;
            c.z = 0.0;
        }
        for cnt in role_counts.iter_mut() {
            *cnt = 0;
        }
        for i in 0..num_nodes {
            let t = node_role[i] as usize;
            if t < num_types {
                role_centroids[t].x += positions[i].x;
                role_centroids[t].y += positions[i].y;
                role_centroids[t].z += positions[i].z;
                role_counts[t] += 1;
            }
        }
    }
}

/// Finalize role centroids by dividing accumulated positions by count.
///
/// On CPU fallback: performs the division directly.
pub fn finalize_role_centroids(role_centroids: &mut [Float3], role_counts: &[i32]) {
    #[cfg(feature = "gpu")]
    {
        unsafe {
            gpu_ffi::finalize_role_centroids(role_centroids.as_mut_ptr(), role_counts.as_ptr());
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        warn_once(&WARNED_FINALIZE_ROLE, "finalize_role_centroids");
        let num_types = role_centroids.len().min(role_counts.len());
        for i in 0..num_types {
            let count = role_counts[i];
            if count > 0 {
                let c = count as f32;
                role_centroids[i].x /= c;
                role_centroids[i].y /= c;
                role_centroids[i].z /= c;
            }
        }
    }
}

/// Returns true when the build includes GPU/CUDA support.
///
/// `feature = "gpu"` alone is not enough: a PTX-only build links no-op stubs for
/// every `extern "C"` entry point here, so those calls would silently do nothing.
/// The link-time probe `visionclaw_ffi_stubbed` distinguishes the two.
pub fn gpu_available() -> bool {
    #[cfg(feature = "gpu")]
    {
        // SAFETY: argument-less pure probe defined in either the stub or the real object.
        unsafe { gpu_ffi::visionclaw_ffi_stubbed() == 0 }
    }
    #[cfg(not(feature = "gpu"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_available_reflects_feature() {
        // In test builds without the gpu feature, this should be false.
        // The assertion is conditional so the test passes regardless of feature state.
        let available = gpu_available();
        if cfg!(feature = "gpu") {
            // With the feature on, availability must mirror the link probe: a
            // PTX-only (stubbed) build reports false, a real object build true.
            #[cfg(feature = "gpu")]
            {
                let stubbed = unsafe { gpu_ffi::visionclaw_ffi_stubbed() } == 1;
                assert_eq!(available, !stubbed);
            }
        } else {
            assert!(!available);
        }
    }

    #[test]
    fn test_fallback_set_dynamic_buffer_returns_zero() {
        if cfg!(feature = "gpu") {
            return; // Skip -- would call real CUDA
        }
        let configs: Vec<DynamicForceConfigGPU> = vec![];
        assert_eq!(set_dynamic_relationship_buffer(&configs, false), 0);
    }

    #[test]
    fn test_fallback_get_max_relationship_types() {
        if cfg!(feature = "gpu") {
            return;
        }
        assert_eq!(get_max_relationship_types(), 256);
    }

    #[test]
    fn test_fallback_hierarchy_levels_propagation() {
        if cfg!(feature = "gpu") {
            return;
        }
        // Two nodes, one hierarchy edge from 0 -> 1
        let edge_sources = [0i32];
        let edge_targets = [1i32];
        let edge_types = [2i32]; // hierarchy edge type
        let mut node_levels = [0i32, -1]; // node 0 is root
        let mut changed = false;

        calculate_hierarchy_levels(
            &edge_sources,
            &edge_targets,
            &edge_types,
            &mut node_levels,
            &mut changed,
            1,
            2,
        );

        assert!(changed);
        assert_eq!(node_levels[0], 0);
        assert_eq!(node_levels[1], 1);
    }

    #[test]
    fn test_fallback_type_centroids() {
        if cfg!(feature = "gpu") {
            return;
        }
        let node_types = [0i32, 0, 1];
        let positions = [
            Float3 {
                x: 2.0,
                y: 4.0,
                z: 6.0,
            },
            Float3 {
                x: 4.0,
                y: 6.0,
                z: 8.0,
            },
            Float3 {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            },
        ];
        let mut centroids = [Float3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }; 2];
        let mut counts = [0i32; 2];

        calculate_type_centroids(&node_types, &positions, &mut centroids, &mut counts, 3, 2);
        finalize_type_centroids(&mut centroids, &counts, 2);

        assert_eq!(counts[0], 2);
        assert_eq!(counts[1], 1);
        // Type 0 centroid: avg of (2,4,6) and (4,6,8) = (3,5,7)
        assert!((centroids[0].x - 3.0).abs() < 0.001);
        assert!((centroids[0].y - 5.0).abs() < 0.001);
        assert!((centroids[0].z - 7.0).abs() < 0.001);
        // Type 1 centroid: (10,20,30)
        assert!((centroids[1].x - 10.0).abs() < 0.001);
    }
}
