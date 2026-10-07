//! The server relay (`src/handlers/socket_flow_handler/session_relay.rs`) and
//! the headset parser must agree on which `memoryRoute` frames are valid, or the
//! relay could forward frames the headset drops. Both suites read
//! `fixtures/memory_route_cases.json`; this is the headset half.

use visionclaw_xr_gdext::memory_route::parse_route;

#[test]
fn headset_parser_agrees_with_the_shared_relay_cases() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/memory_route_cases.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 10);
    for c in cases {
        let frame = c["frame"].to_string();
        let ok = parse_route(&frame).is_ok();
        assert_eq!(ok, c["accept"].as_bool().unwrap(), "{frame}");
    }
}
