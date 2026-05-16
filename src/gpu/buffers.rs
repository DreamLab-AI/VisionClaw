//! `PhysicsGpuBuffers` — consolidated ownership of all per-node GPU buffers
//! used by the force simulation pipeline.
//!
//! Implements ADR-01 D1 (single struct owns every per-node buffer) and D3
//! (atomic resize policy). The legacy `UnifiedGPUCompute` struct in
//! `src/utils/unified_gpu_compute/construction.rs` still owns the production
//! path; this module is gated behind the `physics-v2` Cargo feature and is
//! intended to land iteratively per WORKTREE-PLAN §2.
//!
//! Design invariants:
//!
//! 1. Capacity changes go through exactly one method: [`PhysicsGpuBuffers::resize`].
//!    No external code may resize an individual buffer.
//! 2. `resize` is transactional. Either every device buffer is rebound at the
//!    new capacity, or none of them are and the previous capacity is preserved.
//! 3. The caller treats `Err` from `resize` as a fatal GPU error that triggers
//!    a supervisor restart (ADR-01 D4). The struct itself does not retry.
//! 4. Capacity growth follows D3: `max(node_count, current_capacity * 2)`,
//!    seeded with `ceil_to_power_of_two(min(node_count, 16384))`.
//! 5. The `class_id` buffer here is a per-node *domain-cluster* integer
//!    (matching `src/actors/gpu/gpu_resource_actor.rs`), NOT a wire-protocol
//!    flag-bit mask. Wire flag bits are encoded into node IDs at the broadcast
//!    layer (Phase 2).

#![cfg(feature = "physics-v2")]

use anyhow::{anyhow, Context, Result};
use cust::memory::{CopyDestination, DeviceBuffer};
use cust_core::DeviceCopy;

/// Maximum velocity magnitude enforced by the `numerical_safety` kernel
/// (ADR-01 D8). Velocities above this magnitude are clamped and counted.
pub const MAX_VELOCITY: f32 = 100.0;

/// Initial-capacity ceiling. `new()` seeds capacity to the smaller of the
/// node count and this value, rounded up to a power of two. Growth past this
/// value is permitted and only emits a warning.
pub const INITIAL_CAPACITY_CEILING: usize = 16_384;

/// Axis-aligned bounding box mirrored locally so that this module does not
/// depend on the `pub(crate)` AABB type inside `unified_gpu_compute`. The
/// layout matches the CUDA `AABB` struct in `visionflow_unified.cu`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AabbBlock {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

unsafe impl DeviceCopy for AabbBlock {}
unsafe impl bytemuck::Zeroable for AabbBlock {}
unsafe impl bytemuck::Pod for AabbBlock {}

/// All per-node GPU buffers required by the physics simulation loop.
///
/// Buffers NOT included here (and that remain owned by `UnifiedGPUCompute`):
///
/// - Edge CSR buffers (`edge_row_offsets`, `edge_col_indices`,
///   `edge_weights`) — topology, not per-node state.
/// - Spatial-hash grid buffers (`cell_keys`, `cell_start`, `cell_end`).
/// - Clustering / anomaly / community-detection buffers (analytics, not
///   physics).
/// - SSSP buffers (`dist`, frontiers, parents).
/// - Constraint data buffers.
pub struct PhysicsGpuBuffers {
    // --- positions (ping-pong pair) ---
    pub pos_in_x: DeviceBuffer<f32>,
    pub pos_in_y: DeviceBuffer<f32>,
    pub pos_in_z: DeviceBuffer<f32>,
    pub pos_out_x: DeviceBuffer<f32>,
    pub pos_out_y: DeviceBuffer<f32>,
    pub pos_out_z: DeviceBuffer<f32>,

    // --- velocities (ping-pong pair) ---
    pub vel_in_x: DeviceBuffer<f32>,
    pub vel_in_y: DeviceBuffer<f32>,
    pub vel_in_z: DeviceBuffer<f32>,
    pub vel_out_x: DeviceBuffer<f32>,
    pub vel_out_y: DeviceBuffer<f32>,
    pub vel_out_z: DeviceBuffer<f32>,

    // --- per-node forces ---
    pub force_x: DeviceBuffer<f32>,
    pub force_y: DeviceBuffer<f32>,
    pub force_z: DeviceBuffer<f32>,

    // --- FA2 LinLog adaptive speed (previous force per node) ---
    pub prev_force_x: DeviceBuffer<f32>,
    pub prev_force_y: DeviceBuffer<f32>,
    pub prev_force_z: DeviceBuffer<f32>,

    // --- mass model (ADR-01 D6) ---
    /// Per-node log-mass: `1.0 + log2(1 + degree)`.
    pub masses: DeviceBuffer<f32>,
    /// Per-node domain-cluster integer (NOT wire flag bits — see module docs).
    pub class_ids: DeviceBuffer<i32>,
    /// Optional per-class mass override; same length as `masses`.
    pub class_masses: DeviceBuffer<f32>,

    // --- reduction workspace ---
    /// One AABB entry per AABB block (`(node_count + 255) / 256`).
    pub aabb_block_results: DeviceBuffer<AabbBlock>,
    /// One f32 entry per kinetic-energy block (`(node_count + 255) / 256`).
    pub partial_kinetic_energy: DeviceBuffer<f32>,
    /// Scratch zeroing buffer used by reductions.
    pub zero_buffer: DeviceBuffer<u8>,

    // --- bookkeeping ---
    /// Logical node count (≤ `capacity`).
    pub node_count: usize,
    /// Allocated capacity. May exceed `node_count`.
    pub capacity: usize,
    /// Number of AABB reduction blocks at the current capacity.
    pub aabb_num_blocks: usize,
}

impl PhysicsGpuBuffers {
    /// Allocate buffers sized to `initial_capacity`, applying the ADR-01 D3
    /// initial-capacity policy:
    /// `ceil_to_power_of_two(min(node_count, INITIAL_CAPACITY_CEILING))`.
    pub fn new(initial_capacity: usize) -> Result<Self> {
        let capacity = seeded_capacity(initial_capacity);
        let aabb_num_blocks = aabb_blocks_for(capacity);

        Ok(Self {
            pos_in_x: DeviceBuffer::zeroed(capacity)?,
            pos_in_y: DeviceBuffer::zeroed(capacity)?,
            pos_in_z: DeviceBuffer::zeroed(capacity)?,
            pos_out_x: DeviceBuffer::zeroed(capacity)?,
            pos_out_y: DeviceBuffer::zeroed(capacity)?,
            pos_out_z: DeviceBuffer::zeroed(capacity)?,

            vel_in_x: DeviceBuffer::zeroed(capacity)?,
            vel_in_y: DeviceBuffer::zeroed(capacity)?,
            vel_in_z: DeviceBuffer::zeroed(capacity)?,
            vel_out_x: DeviceBuffer::zeroed(capacity)?,
            vel_out_y: DeviceBuffer::zeroed(capacity)?,
            vel_out_z: DeviceBuffer::zeroed(capacity)?,

            force_x: DeviceBuffer::zeroed(capacity)?,
            force_y: DeviceBuffer::zeroed(capacity)?,
            force_z: DeviceBuffer::zeroed(capacity)?,

            prev_force_x: DeviceBuffer::zeroed(capacity)?,
            prev_force_y: DeviceBuffer::zeroed(capacity)?,
            prev_force_z: DeviceBuffer::zeroed(capacity)?,

            masses: DeviceBuffer::from_slice(&vec![1.0f32; capacity])?,
            class_ids: DeviceBuffer::zeroed(capacity)?,
            class_masses: DeviceBuffer::from_slice(&vec![1.0f32; capacity])?,

            aabb_block_results: DeviceBuffer::zeroed(aabb_num_blocks)?,
            partial_kinetic_energy: DeviceBuffer::zeroed(aabb_num_blocks)?,
            zero_buffer: DeviceBuffer::zeroed(capacity * std::mem::size_of::<f32>())?,

            node_count: 0,
            capacity,
            aabb_num_blocks,
        })
    }

    /// Resize every buffer to fit `new_node_count` nodes.
    ///
    /// Transactional semantics: either all buffers reach the new capacity, or
    /// the function returns `Err` and the struct is left exactly as it was on
    /// entry. The growth policy is
    /// `new_capacity = max(new_node_count, current_capacity * 2)`.
    ///
    /// On success, `node_count` is updated to `new_node_count` and `capacity`
    /// reflects the chosen growth target (possibly unchanged if the existing
    /// capacity already accommodates `new_node_count`).
    pub fn resize(&mut self, new_node_count: usize) -> Result<()> {
        // Fast path: existing capacity accommodates the new node count.
        if new_node_count <= self.capacity {
            self.node_count = new_node_count;
            return Ok(());
        }

        let target_capacity = growth_capacity(new_node_count, self.capacity);
        if target_capacity > INITIAL_CAPACITY_CEILING {
            log::warn!(
                "PhysicsGpuBuffers::resize allocating {} > ceiling {} (node_count={})",
                target_capacity,
                INITIAL_CAPACITY_CEILING,
                new_node_count,
            );
        }
        let new_aabb_blocks = aabb_blocks_for(target_capacity);

        // Allocate every replacement buffer up front. If any single allocation
        // fails, `?` propagates the error and the temporaries are dropped
        // before any field of `self` is touched. This is the atomicity
        // guarantee: partial failure leaves `self` unchanged.
        let staged = Staged::allocate(target_capacity, new_aabb_blocks)
            .with_context(|| {
                format!(
                    "PhysicsGpuBuffers::resize allocation failed (target_capacity={}, aabb_blocks={})",
                    target_capacity, new_aabb_blocks,
                )
            })?;

        // All allocations succeeded — commit by moving every replacement into
        // `self` in one synchronous block. No `?` past this point.
        staged.commit_into(self);
        self.capacity = target_capacity;
        self.aabb_num_blocks = new_aabb_blocks;
        self.node_count = new_node_count;
        Ok(())
    }

    /// Reset every velocity component to zero. Used by `SetLayoutMode` to
    /// prevent ghost forces when switching engines (per WORKTREE-PLAN §3).
    pub fn reset_velocities_to_zero(&mut self) -> Result<()> {
        let zeros = vec![0.0f32; self.capacity];
        self.vel_in_x.copy_from(&zeros)?;
        self.vel_in_y.copy_from(&zeros)?;
        self.vel_in_z.copy_from(&zeros)?;
        self.vel_out_x.copy_from(&zeros)?;
        self.vel_out_y.copy_from(&zeros)?;
        self.vel_out_z.copy_from(&zeros)?;
        Ok(())
    }

    /// Upload positions, padding to `capacity` with zeros if `xs.len() < capacity`.
    pub fn upload_positions(&mut self, xs: &[f32], ys: &[f32], zs: &[f32]) -> Result<()> {
        if xs.len() != ys.len() || ys.len() != zs.len() {
            return Err(anyhow!(
                "upload_positions: component length mismatch (x={}, y={}, z={})",
                xs.len(),
                ys.len(),
                zs.len()
            ));
        }
        if xs.len() > self.capacity {
            return Err(anyhow!(
                "upload_positions: input length {} exceeds capacity {}",
                xs.len(),
                self.capacity
            ));
        }
        let mut padded_x = vec![0.0f32; self.capacity];
        let mut padded_y = vec![0.0f32; self.capacity];
        let mut padded_z = vec![0.0f32; self.capacity];
        padded_x[..xs.len()].copy_from_slice(xs);
        padded_y[..ys.len()].copy_from_slice(ys);
        padded_z[..zs.len()].copy_from_slice(zs);
        self.pos_in_x.copy_from(&padded_x)?;
        self.pos_in_y.copy_from(&padded_y)?;
        self.pos_in_z.copy_from(&padded_z)?;
        Ok(())
    }

    /// Derive log-masses from a degree slice and upload to `self.masses`.
    /// ADR-01 D6: `mass = 1.0 + log2(1 + degree)`.
    pub fn upload_masses_from_degrees(&mut self, degrees: &[u32]) -> Result<()> {
        if degrees.len() > self.capacity {
            return Err(anyhow!(
                "upload_masses_from_degrees: input length {} exceeds capacity {}",
                degrees.len(),
                self.capacity
            ));
        }
        let mut masses = vec![1.0f32; self.capacity];
        for (i, &d) in degrees.iter().enumerate() {
            masses[i] = derive_log_mass(d);
        }
        self.masses.copy_from(&masses)?;
        Ok(())
    }

    /// Upload unified per-node class metadata (ids + mass overrides) atomically.
    pub fn upload_class_metadata(
        &mut self,
        class_ids: &[i32],
        class_masses: &[f32],
    ) -> Result<()> {
        if class_ids.len() != class_masses.len() {
            return Err(anyhow!(
                "upload_class_metadata: length mismatch (ids={}, masses={})",
                class_ids.len(),
                class_masses.len()
            ));
        }
        if class_ids.len() > self.capacity {
            return Err(anyhow!(
                "upload_class_metadata: input length {} exceeds capacity {}",
                class_ids.len(),
                self.capacity
            ));
        }
        let mut padded_ids = vec![0i32; self.capacity];
        let mut padded_masses = vec![1.0f32; self.capacity];
        padded_ids[..class_ids.len()].copy_from_slice(class_ids);
        padded_masses[..class_masses.len()].copy_from_slice(class_masses);
        self.class_ids.copy_from(&padded_ids)?;
        self.class_masses.copy_from(&padded_masses)?;
        Ok(())
    }
}

/// Holds replacement buffers during a `resize` until every allocation has
/// succeeded. Constructed atomically; on success `commit_into` swaps every
/// field of the target struct in one synchronous block.
struct Staged {
    pos_in_x: DeviceBuffer<f32>,
    pos_in_y: DeviceBuffer<f32>,
    pos_in_z: DeviceBuffer<f32>,
    pos_out_x: DeviceBuffer<f32>,
    pos_out_y: DeviceBuffer<f32>,
    pos_out_z: DeviceBuffer<f32>,
    vel_in_x: DeviceBuffer<f32>,
    vel_in_y: DeviceBuffer<f32>,
    vel_in_z: DeviceBuffer<f32>,
    vel_out_x: DeviceBuffer<f32>,
    vel_out_y: DeviceBuffer<f32>,
    vel_out_z: DeviceBuffer<f32>,
    force_x: DeviceBuffer<f32>,
    force_y: DeviceBuffer<f32>,
    force_z: DeviceBuffer<f32>,
    prev_force_x: DeviceBuffer<f32>,
    prev_force_y: DeviceBuffer<f32>,
    prev_force_z: DeviceBuffer<f32>,
    masses: DeviceBuffer<f32>,
    class_ids: DeviceBuffer<i32>,
    class_masses: DeviceBuffer<f32>,
    aabb_block_results: DeviceBuffer<AabbBlock>,
    partial_kinetic_energy: DeviceBuffer<f32>,
    zero_buffer: DeviceBuffer<u8>,
}

impl Staged {
    fn allocate(capacity: usize, aabb_blocks: usize) -> Result<Self> {
        Ok(Self {
            pos_in_x: DeviceBuffer::zeroed(capacity)?,
            pos_in_y: DeviceBuffer::zeroed(capacity)?,
            pos_in_z: DeviceBuffer::zeroed(capacity)?,
            pos_out_x: DeviceBuffer::zeroed(capacity)?,
            pos_out_y: DeviceBuffer::zeroed(capacity)?,
            pos_out_z: DeviceBuffer::zeroed(capacity)?,
            vel_in_x: DeviceBuffer::zeroed(capacity)?,
            vel_in_y: DeviceBuffer::zeroed(capacity)?,
            vel_in_z: DeviceBuffer::zeroed(capacity)?,
            vel_out_x: DeviceBuffer::zeroed(capacity)?,
            vel_out_y: DeviceBuffer::zeroed(capacity)?,
            vel_out_z: DeviceBuffer::zeroed(capacity)?,
            force_x: DeviceBuffer::zeroed(capacity)?,
            force_y: DeviceBuffer::zeroed(capacity)?,
            force_z: DeviceBuffer::zeroed(capacity)?,
            prev_force_x: DeviceBuffer::zeroed(capacity)?,
            prev_force_y: DeviceBuffer::zeroed(capacity)?,
            prev_force_z: DeviceBuffer::zeroed(capacity)?,
            masses: DeviceBuffer::from_slice(&vec![1.0f32; capacity])?,
            class_ids: DeviceBuffer::zeroed(capacity)?,
            class_masses: DeviceBuffer::from_slice(&vec![1.0f32; capacity])?,
            aabb_block_results: DeviceBuffer::zeroed(aabb_blocks)?,
            partial_kinetic_energy: DeviceBuffer::zeroed(aabb_blocks)?,
            zero_buffer: DeviceBuffer::zeroed(capacity * std::mem::size_of::<f32>())?,
        })
    }

    fn commit_into(self, target: &mut PhysicsGpuBuffers) {
        target.pos_in_x = self.pos_in_x;
        target.pos_in_y = self.pos_in_y;
        target.pos_in_z = self.pos_in_z;
        target.pos_out_x = self.pos_out_x;
        target.pos_out_y = self.pos_out_y;
        target.pos_out_z = self.pos_out_z;
        target.vel_in_x = self.vel_in_x;
        target.vel_in_y = self.vel_in_y;
        target.vel_in_z = self.vel_in_z;
        target.vel_out_x = self.vel_out_x;
        target.vel_out_y = self.vel_out_y;
        target.vel_out_z = self.vel_out_z;
        target.force_x = self.force_x;
        target.force_y = self.force_y;
        target.force_z = self.force_z;
        target.prev_force_x = self.prev_force_x;
        target.prev_force_y = self.prev_force_y;
        target.prev_force_z = self.prev_force_z;
        target.masses = self.masses;
        target.class_ids = self.class_ids;
        target.class_masses = self.class_masses;
        target.aabb_block_results = self.aabb_block_results;
        target.partial_kinetic_energy = self.partial_kinetic_energy;
        target.zero_buffer = self.zero_buffer;
    }
}

fn seeded_capacity(node_count: usize) -> usize {
    let seed = node_count.min(INITIAL_CAPACITY_CEILING).max(1);
    ceil_pow2(seed)
}

fn growth_capacity(new_node_count: usize, current_capacity: usize) -> usize {
    let doubled = current_capacity.saturating_mul(2);
    new_node_count.max(doubled).max(1)
}

fn ceil_pow2(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let mut p = 1usize;
    while p < n {
        p = p.saturating_mul(2);
        if p == 0 {
            // Overflow: clamp to the largest representable power of two.
            return 1 << (usize::BITS as usize - 1);
        }
    }
    p
}

fn aabb_blocks_for(capacity: usize) -> usize {
    (capacity + 255) / 256
}

/// Mass derivation per ADR-01 D6. Exposed as a free function so unit tests
/// can validate it without a CUDA context.
pub fn derive_log_mass(degree: u32) -> f32 {
    let d = degree.saturating_add(1) as f32;
    1.0_f32 + d.log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_seed_rounds_to_power_of_two() {
        assert_eq!(seeded_capacity(0), 1);
        assert_eq!(seeded_capacity(1), 1);
        assert_eq!(seeded_capacity(100), 128);
        assert_eq!(seeded_capacity(1023), 1024);
        assert_eq!(seeded_capacity(1024), 1024);
        assert_eq!(seeded_capacity(20_000), ceil_pow2(INITIAL_CAPACITY_CEILING));
    }

    #[test]
    fn growth_doubles_until_node_count_exceeds() {
        assert_eq!(growth_capacity(10, 100), 200);
        assert_eq!(growth_capacity(500, 100), 500);
        assert_eq!(growth_capacity(0, 0), 1);
    }

    #[test]
    fn aabb_blocks_match_kernel_assumption() {
        assert_eq!(aabb_blocks_for(0), 0);
        assert_eq!(aabb_blocks_for(1), 1);
        assert_eq!(aabb_blocks_for(255), 1);
        assert_eq!(aabb_blocks_for(256), 1);
        assert_eq!(aabb_blocks_for(257), 2);
        assert_eq!(aabb_blocks_for(1024), 4);
    }

    #[test]
    fn log_mass_monotonic_in_degree() {
        let m0 = derive_log_mass(0);
        let m1 = derive_log_mass(1);
        let m10 = derive_log_mass(10);
        let m100 = derive_log_mass(100);
        assert!(m0 < m1);
        assert!(m1 < m10);
        assert!(m10 < m100);
        // Verify the formula: 1 + log2(1 + degree) → log_mass(0) = 1.
        assert!((m0 - 1.0).abs() < 1e-6);
        // log_mass(1) = 1 + log2(2) = 2.
        assert!((m1 - 2.0).abs() < 1e-6);
    }
}
