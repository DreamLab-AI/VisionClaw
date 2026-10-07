//! The server's query response (`crates/visionclaw-memory-cloud/src/wire.rs`,
//! `MemoryCloudQueryResponse`) and the headset parser must agree on the wire.
//! Both suites read `fixtures/memory_query_response.json`: the server half
//! round-trips it through its own types, this half parses it and builds the
//! route the headset draws.

use visionclaw_xr_gdext::memory_query::{parse_query_response, sidecar_route, Method};
use visionclaw_xr_gdext::memory_route::{RouteGate, RouteSource};

fn fixture() -> serde_json::Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/memory_query_response.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn headset_reads_every_hit_the_server_sends() {
    let v = fixture();
    let a = parse_query_response(&v.to_string()).unwrap();
    let results = v["sidecar"]["results"].as_array().unwrap();
    assert_eq!(a.hits.len(), results.len());
    for (h, w) in a.hits.iter().zip(results) {
        assert_eq!(h.key, w["key"].as_str().unwrap());
        assert_eq!(h.namespace, w["namespace"].as_str().unwrap());
        assert_eq!(h.source_type, w["sourceType"].as_str().unwrap());
        assert!((h.score as f64 - w["score"].as_f64().unwrap()).abs() < 1e-6);
        assert_eq!(h.row.map(u64::from), w["sampleIndex"].as_u64());
    }
    assert_eq!(a.snapshot_id, v["snapshotId"].as_str().unwrap());
    assert_eq!(a.method, Method::Hnsw);
}

#[test]
fn the_fixture_draws_a_sidecar_top_k_route() {
    let a = parse_query_response(&fixture().to_string()).unwrap();
    let f = sidecar_route(&a, 1.0, 1);
    assert_eq!(f.source, RouteSource::SidecarTopK);
    assert_eq!(f.path.len(), 4);
    let mut gate = RouteGate::default();
    assert!(matches!(
        gate.offer(f, Some("snap-7f3a"), 64),
        visionclaw_xr_gdext::memory_route::RouteDecision::Apply(_)
    ));
}
