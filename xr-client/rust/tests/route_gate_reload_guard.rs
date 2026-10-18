//! `RouteGate` asks for one snapshot reload per mismatch episode, never a
//! reload loop. The guard must reset once the episode ends, or a later
//! mismatch for a snapshot that was reloaded for once is dropped for the rest
//! of the session (estate register ER-26, diagram VC-38 debt).

use visionclaw_xr_gdext::memory_route::{parse_route, RouteDecision, RouteGate};

fn frame(snapshot: &str, seq: u64, path: &[u32]) -> visionclaw_xr_gdext::memory_route::RouteFrame {
    let json = serde_json::json!({
        "type": "memoryRoute",
        "snapshotId": snapshot,
        "seq": seq,
        "sentAt": seq as f64,
        "path": path,
    });
    parse_route(&json.to_string()).expect("valid frame")
}

fn is_reload(d: &RouteDecision) -> bool {
    matches!(d, RouteDecision::Reload)
}

#[test]
fn a_second_mismatch_in_one_episode_is_dropped_not_reloaded() {
    let mut gate = RouteGate::default();
    assert!(is_reload(&gate.offer(frame("B", 1, &[0, 1]), Some("A"), 8)));
    // The reload did not bring B in: the re-offer must drop, not loop.
    assert!(matches!(
        gate.after_reload(Some("A"), 8),
        Some(RouteDecision::Drop)
    ));
    assert!(matches!(
        gate.offer(frame("B", 2, &[0, 1]), Some("A"), 8),
        RouteDecision::Drop
    ));
}

#[test]
fn an_applied_frame_ends_the_episode_so_the_same_snapshot_can_reload_again() {
    let mut gate = RouteGate::default();
    assert!(is_reload(&gate.offer(frame("B", 1, &[0, 1]), Some("A"), 8)));
    assert!(matches!(
        gate.after_reload(Some("B"), 8),
        Some(RouteDecision::Apply(_))
    ));
    // The cloud later moves back to A, and B is named again: a fresh episode.
    assert!(is_reload(&gate.offer(frame("B", 2, &[0, 1]), Some("A"), 8)));
}

#[test]
fn a_clear_ends_the_episode() {
    let mut gate = RouteGate::default();
    assert!(is_reload(&gate.offer(frame("B", 1, &[0, 1]), Some("A"), 8)));
    assert!(matches!(
        gate.after_reload(Some("A"), 8),
        Some(RouteDecision::Drop)
    ));
    assert!(matches!(
        gate.offer(frame("B", 2, &[]), Some("A"), 8),
        RouteDecision::Clear
    ));
    assert!(is_reload(&gate.offer(frame("B", 3, &[0, 1]), Some("A"), 8)));
}
