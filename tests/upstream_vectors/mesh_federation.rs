// tests/upstream_vectors/mesh_federation.rs
//! L1 reference vectors: DreamLab mesh federation (ADR-073 D2/D6/D9 + ADR-074 D9).
//!
//! Asserts fixture shape; substrate-side `MeshBridge` service wiring is
//! Phase 2 (per ADR-073 D10 third bullet).

#[path = "mod.rs"]
mod fixture_loader;
use fixture_loader::{assert_meta_block, load_fixture};

#[test]
fn mesh_federation_fixture_loads_and_metadata_is_canonical() {
    let f = load_fixture("mesh-federation.json");
    assert_meta_block(&f, "ADR-073");
    let vectors = f["vectors"].as_array().expect("vectors must be array");
    assert!(vectors.len() >= 9, "mesh-federation must have >= 9 vectors");
}

#[test]
fn mesh_federation_scenarios_cover_the_five_categories() {
    let f = load_fixture("mesh-federation.json");
    let vectors = f["vectors"].as_array().unwrap();
    let scenarios: std::collections::HashSet<&str> = vectors
        .iter()
        .filter_map(|v| v["scenario"].as_str())
        .collect();
    let expected = ["fanout", "lru-dedup", "loop-avoidance", "service-list", "mode-config"];
    for s in &expected {
        assert!(
            scenarios.contains(s),
            "fixture must cover scenario '{}', got: {:?}",
            s,
            scenarios
        );
    }
}

#[test]
#[ignore = "wires into src/services/mesh_bridge.rs (planned per ADR-073 D10); current substrate has no relay process so federation worker is a Phase 2 deliverable"]
fn mesh_federation_worker_honours_x_mesh_from_loop_avoidance() {
    let _ = load_fixture("mesh-federation.json");
}
