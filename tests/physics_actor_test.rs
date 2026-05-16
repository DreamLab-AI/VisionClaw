//! Phase 5 / ADR-01 acceptance tests for the GPU physics subsystem.
//!
//! These tests cover the four scenarios called out in WORKTREE-PLAN §8 / T8:
//!
//! - `nan_injection_recovery`            — D8 sentinel kernel + PhysicsClamped event.
//! - `panic_recovery_via_supervisor_restart` — D4 3-per-60s restart budget.
//! - `engine_switch_preserves_buffers`   — D5 SetLayoutMode handler.
//! - `buffer_resize_atomic`              — D1/D3 PhysicsGpuBuffers::resize transactional contract.
//!
//! The whole file is gated behind the `physics-v2` Cargo feature because the
//! `PhysicsGpuBuffers` struct and `PhysicsEvent` enum are themselves
//! feature-gated. Tests that need an actual CUDA device are additionally
//! gated behind `#[ignore]` so `cargo test` still runs without a GPU
//! attached; CI must opt in via `cargo test --features physics-v2 -- --ignored`.

#![cfg(feature = "physics-v2")]

use webxr::actors::messages::{ClampKind, PhysicsEvent};
use webxr::gpu::buffers::{derive_log_mass, PhysicsGpuBuffers, MAX_VELOCITY};

// ---------------------------------------------------------------------------
// Pure-Rust unit tests (no CUDA required)
// ---------------------------------------------------------------------------

#[test]
fn log_mass_matches_adr01_d6_formula() {
    // ADR-01 D6: mass = 1.0 + log2(1 + degree)
    assert!((derive_log_mass(0) - 1.0).abs() < 1e-6);
    assert!((derive_log_mass(1) - 2.0).abs() < 1e-6);
    // degree=3 → 1 + log2(4) = 3
    assert!((derive_log_mass(3) - 3.0).abs() < 1e-6);
    // degree=7 → 1 + log2(8) = 4
    assert!((derive_log_mass(7) - 4.0).abs() < 1e-6);
}

#[test]
fn max_velocity_constant_matches_adr01_d8() {
    // ADR-01 D8 specifies MAX_VELOCITY = 100.0.
    assert!((MAX_VELOCITY - 100.0).abs() < f32::EPSILON);
}

#[test]
fn physics_event_clamp_kind_round_trips() {
    let kinds = [ClampKind::NaN, ClampKind::Inf, ClampKind::VelocityCap];
    for k in kinds {
        let json = serde_json::to_string(&k).expect("ClampKind serialises");
        let back: ClampKind = serde_json::from_str(&json).expect("ClampKind deserialises");
        assert_eq!(back, k);
    }
}

#[test]
fn physics_event_layout_settled_serialises() {
    let ev = PhysicsEvent::LayoutSettled {
        iteration: 600,
        rms_velocity: 0.008,
    };
    let json = serde_json::to_string(&ev).expect("PhysicsEvent serialises");
    assert!(json.contains("LayoutSettled"));
    assert!(json.contains("600"));
}

// ---------------------------------------------------------------------------
// CUDA-bound tests (require `--features physics-v2 -- --ignored`)
// ---------------------------------------------------------------------------

/// T8e: `PhysicsGpuBuffers::resize` is transactional. After a successful
/// resize, `node_count` and `capacity` reflect the new size. After a failed
/// resize (simulated by requesting an absurd capacity), the previous
/// `node_count` and `capacity` are preserved.
#[test]
#[ignore = "requires CUDA device — run with --features physics-v2 -- --ignored"]
fn buffer_resize_atomic() {
    let mut buffers =
        PhysicsGpuBuffers::new(64).expect("seeded allocation succeeds on any CUDA device");
    let original_capacity = buffers.capacity;
    assert_eq!(buffers.node_count, 0);

    // Successful resize — fast path (within capacity).
    buffers.resize(32).expect("within-capacity resize succeeds");
    assert_eq!(buffers.node_count, 32);
    assert_eq!(buffers.capacity, original_capacity);

    // Successful growth — capacity should at least double.
    buffers
        .resize(original_capacity + 1)
        .expect("growth resize succeeds");
    assert!(buffers.capacity >= original_capacity * 2);

    // Atomicity: requesting a capacity that exceeds device memory must fail
    // without corrupting the struct. We approximate "out of memory" by
    // asking for an obviously-impossible size.
    let pre_fail_cap = buffers.capacity;
    let pre_fail_count = buffers.node_count;
    let huge = usize::MAX / 32; // very likely to fail to allocate
    let result = buffers.resize(huge);
    assert!(result.is_err(), "absurd capacity must fail");
    assert_eq!(
        buffers.capacity, pre_fail_cap,
        "capacity must be unchanged after failed resize"
    );
    assert_eq!(
        buffers.node_count, pre_fail_count,
        "node_count must be unchanged after failed resize"
    );
}

/// T8a: deliberate NaN/Inf injection produces `PhysicsClamped` events with
/// `count > 0` and downstream positions remain finite.
///
/// Implementation requires the `numerical_safety_kernel` to be in place; the
/// test is a placeholder until T6 lands. Once the kernel is wired, replace
/// the body with: (1) construct two coincident nodes, (2) advance one tick,
/// (3) inspect the event channel for a `PhysicsClamped { kind: NaN }` with
/// non-zero count.
#[test]
#[ignore = "T6 (numerical_safety kernel) not yet wired"]
fn nan_injection_recovery() {
    panic!("placeholder — implement once T6 lands");
}

/// T8b: deliberate CUDA panic (invalid kernel launch) → actor restarts via
/// supervisor. Verifies ADR-01 D4 3-per-60s budget: actor survives 3
/// panics, supervisor stops issuing restart messages on the 4th.
#[test]
#[ignore = "requires full actor system — implement alongside ForceComputeActor migration"]
fn panic_recovery_via_supervisor_restart() {
    panic!("placeholder — implement alongside actor migration");
}

/// T8c: switching from `ForceDirected` to `StressMajorization` mid-run
/// preserves node positions (no teardown / ghost positions) and emits a
/// `LayoutDestabilised` + subsequent `LayoutStarted` pair.
#[test]
#[ignore = "requires SetLayoutMode handler in ForceComputeActor"]
fn engine_switch_preserves_buffers() {
    panic!("placeholder — implement alongside actor migration");
}
