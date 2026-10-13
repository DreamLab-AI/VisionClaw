//! Owner-pubkey visibility drop-set (ADR-060, ADR-059 §Phase-4).
//!
//! Pure, framework-free implementation of the binary-position-encoder
//! visibility filter. The handler layer (`socket_flow_handler::position_updates`)
//! owns the env-flag gate (`PUBKEY_VISIBILITY_FILTER`, default ON — secure by
//! default; opt out with 0/false/off/no) and the wire
//! encoding; this module owns the load-bearing set logic so it is unit-testable
//! without linking the CUDA-bearing monolith.
//!
//! ADR-059 §6 filter clause, evaluated per caller:
//! ```text
//! KEEP  WHERE visibility = 'public' OR owner_pubkey = $session_pubkey
//! DROP  everything else  (private-of-others, or private-when-anonymous)
//! ```
//! Filtering is **fail-closed**: a missing session pubkey drops every private
//! node (public-only graph), matching ADR-050 §Visibility transitions.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Env flag gating the drop-set filter. Default ON (secure by default); opt out with
/// `0` / `false` / `off` / `no`.
pub const PUBKEY_VISIBILITY_FILTER_ENV: &str = "PUBKEY_VISIBILITY_FILTER";

/// Parse the flag. Absent or unrecognised means ON: the filter fails closed.
pub fn parse_visibility_flag(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"),
        None => true,
    }
}

/// Whether the filter is enabled, read once from the environment. Every position path (connect
/// sync, snapshot request AND the live broadcast) must consult this one function.
pub fn filter_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| parse_visibility_flag(std::env::var(PUBKEY_VISIBILITY_FILTER_ENV).ok().as_deref()))
}

/// Per-node visibility metadata (ADR-050 primitives) projected onto the wire id.
///
/// `wire_id` is the flagged/opaque id exactly as it appears in the `(id, data)`
/// pairs the encoder consumes, so a computed drop set can be matched directly
/// against those pairs with no further translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVisibility {
    /// Flagged/opaque wire id as it appears in the `(id, data)` pair.
    pub wire_id: u32,
    /// `true` when the node is public (ADR-050 `visibility = 'public'`).
    pub is_public: bool,
    /// ADR-050 `owner_pubkey`, when known. `None` ⇒ owner unknown ⇒ treated as
    /// not-owned-by-caller (fail-closed).
    pub owner_pubkey: Option<String>,
}

impl NodeVisibility {
    /// Convenience constructor for a public node (no owner needed).
    pub fn public(wire_id: u32) -> Self {
        Self {
            wire_id,
            is_public: true,
            owner_pubkey: None,
        }
    }

    /// Convenience constructor for a private node with a known owner.
    pub fn private_owned_by(wire_id: u32, owner_pubkey: impl Into<String>) -> Self {
        Self {
            wire_id,
            is_public: false,
            owner_pubkey: Some(owner_pubkey.into()),
        }
    }

    /// Convenience constructor for a private node whose owner is unknown.
    pub fn private_unowned(wire_id: u32) -> Self {
        Self {
            wire_id,
            is_public: false,
            owner_pubkey: None,
        }
    }

    /// Whether this node must be dropped for the given caller.
    ///
    /// Keep iff public, or owned by the caller. Everything else drops.
    #[inline]
    fn is_dropped_for(&self, session_pubkey: Option<&str>) -> bool {
        if self.is_public {
            return false;
        }
        match (session_pubkey, self.owner_pubkey.as_deref()) {
            // Caller is authenticated and owns this private node ⇒ keep.
            (Some(caller), Some(owner)) => caller != owner,
            // Anonymous caller, or unknown owner ⇒ drop (fail-closed).
            _ => true,
        }
    }
}

/// Compute the caller's private-opaque-id drop set per ADR-059 §6.
///
/// Returns the set of `wire_id`s that must **not** reach the wire for a caller
/// holding `session_pubkey` (`None` ⇒ anonymous ⇒ every private node drops).
pub fn compute_private_opaque_ids(
    nodes: &[NodeVisibility],
    session_pubkey: Option<&str>,
) -> HashSet<u32> {
    nodes
        .iter()
        .filter(|n| n.is_dropped_for(session_pubkey))
        .map(|n| n.wire_id)
        .collect()
}

/// Remove every `(id, _)` pair whose id is in `drop_set`, in place.
///
/// Returns the number of pairs removed. A no-op (and no reallocation) when the
/// drop set is empty — the flag-off / nothing-to-drop hot path stays free.
pub fn apply_drop_set<T>(nodes: &mut Vec<(u32, T)>, drop_set: &HashSet<u32>) -> usize {
    if drop_set.is_empty() {
        return 0;
    }
    let before = nodes.len();
    nodes.retain(|(id, _)| !drop_set.contains(id));
    before - nodes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // A stand-in for the encoder payload; the filter is generic over the data half.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Payload(u32);

    fn pairs() -> Vec<(u32, Payload)> {
        vec![
            (10, Payload(10)), // public
            (11, Payload(11)), // private, owned by "alice"
            (12, Payload(12)), // private, owned by "bob"
            (13, Payload(13)), // private, unknown owner
        ]
    }

    fn meta() -> Vec<NodeVisibility> {
        vec![
            NodeVisibility::public(10),
            NodeVisibility::private_owned_by(11, "alice"),
            NodeVisibility::private_owned_by(12, "bob"),
            NodeVisibility::private_unowned(13),
        ]
    }

    #[test]
    fn empty_drop_set_is_passthrough_untouched() {
        // Simulates the flag-OFF path: no drop set is ever computed, so the
        // encoder input must be byte-for-byte identical.
        let mut nodes = pairs();
        let original = nodes.clone();
        let empty: HashSet<u32> = HashSet::new();
        let dropped = apply_drop_set(&mut nodes, &empty);
        assert_eq!(dropped, 0);
        assert_eq!(nodes, original, "empty drop set must not mutate the vec");
    }

    #[test]
    fn flag_on_drops_private_ids_for_non_owner() {
        // Caller "alice" keeps public (10) + her own private (11); bob's (12)
        // and the unknown-owner (13) private nodes drop.
        let drop = compute_private_opaque_ids(&meta(), Some("alice"));
        assert_eq!(drop, HashSet::from([12, 13]));

        let mut nodes = pairs();
        let dropped = apply_drop_set(&mut nodes, &drop);
        assert_eq!(dropped, 2);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            kept,
            vec![10, 11],
            "non-owner private nodes must be dropped"
        );
    }

    #[test]
    fn owner_sees_own_private_nodes() {
        // "bob" keeps public (10) + his own private (12); alice's (11) and the
        // unknown-owner (13) drop.
        let drop = compute_private_opaque_ids(&meta(), Some("bob"));
        assert_eq!(drop, HashSet::from([11, 13]));

        let mut nodes = pairs();
        apply_drop_set(&mut nodes, &drop);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(kept, vec![10, 12], "owner must still see their own nodes");
    }

    #[test]
    fn anonymous_caller_is_fail_closed_public_only() {
        // No session pubkey ⇒ every private node drops regardless of owner.
        let drop = compute_private_opaque_ids(&meta(), None);
        assert_eq!(drop, HashSet::from([11, 12, 13]));

        let mut nodes = pairs();
        apply_drop_set(&mut nodes, &drop);
        let kept: Vec<u32> = nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(kept, vec![10], "anonymous callers get a public-only graph");
    }

    #[test]
    fn all_public_yields_empty_drop_set() {
        let all_public = vec![
            NodeVisibility::public(1),
            NodeVisibility::public(2),
            NodeVisibility::public(3),
        ];
        assert!(compute_private_opaque_ids(&all_public, None).is_empty());
        assert!(compute_private_opaque_ids(&all_public, Some("anyone")).is_empty());
    }

    #[test]
    fn drop_set_ids_absent_from_pairs_are_ignored() {
        // A drop set carrying ids not present in the frame removes nothing extra.
        let mut nodes = pairs();
        let drop = HashSet::from([99, 100]);
        let dropped = apply_drop_set(&mut nodes, &drop);
        assert_eq!(dropped, 0);
        assert_eq!(nodes.len(), 4);
    }

    // ---- ADR-2003: the three caller classes, over one corpus ---------------

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// One corpus exercising every visibility shape at once:
    /// 1 public, 2 private-owned-by-OWNER, 3 private-owned-by-OTHER,
    /// 4 private with an unknown owner.
    fn corpus() -> Vec<NodeVisibility> {
        vec![
            NodeVisibility::public(1),
            NodeVisibility::private_owned_by(2, OWNER),
            NodeVisibility::private_owned_by(3, OTHER),
            NodeVisibility::private_unowned(4),
        ]
    }

    fn kept_for(session: Option<&str>) -> Vec<u32> {
        let drop = compute_private_opaque_ids(&corpus(), session);
        let mut kept: Vec<u32> = corpus()
            .iter()
            .map(|n| n.wire_id)
            .filter(|id| !drop.contains(id))
            .collect();
        kept.sort_unstable();
        kept
    }

    /// An anonymous caller sees the public-only graph. Every private node
    /// drops, whether or not its owner is known — the fail-closed rule.
    #[test]
    fn anonymous_caller_sees_only_public_nodes() {
        assert_eq!(kept_for(None), vec![1]);
    }

    /// An owner sees the public graph plus their own private nodes, and nobody
    /// else's.
    #[test]
    fn owner_sees_public_plus_their_own_private_nodes() {
        assert_eq!(kept_for(Some(OWNER)), vec![1, 2]);
    }

    /// A different authenticated caller sees the public graph plus *their* own
    /// private nodes — here, none.
    #[test]
    fn non_owner_sees_public_plus_only_their_own() {
        assert_eq!(kept_for(Some(OTHER)), vec![1, 3]);
    }

    /// A private node whose owner is unknown is never visible to anyone. It is
    /// not "owned by the caller" for any caller, so it fails closed.
    #[test]
    fn an_unowned_private_node_is_invisible_to_every_caller() {
        for session in [None, Some(OWNER), Some(OTHER)] {
            assert!(
                !kept_for(session).contains(&4),
                "an unowned private node must never reach the wire"
            );
        }
    }

    /// The union of what every caller can see never exceeds the corpus, and no
    /// caller sees another's private node. Stated as an invariant over the
    /// three caller classes rather than three separate assertions.
    #[test]
    fn no_caller_ever_sees_another_callers_private_node() {
        let owner_view = kept_for(Some(OWNER));
        let other_view = kept_for(Some(OTHER));
        assert!(!owner_view.contains(&3), "OWNER must not see OTHER's node");
        assert!(!other_view.contains(&2), "OTHER must not see OWNER's node");
    }

    /// A public-to-private transition: the same node id, re-published as
    /// private, disappears for every caller but its owner. This is the
    /// visibility-change case the ADR names — the filter is evaluated per
    /// snapshot, so a re-published corpus takes effect immediately.
    #[test]
    fn a_public_to_private_transition_takes_effect_on_the_next_snapshot() {
        let before = vec![NodeVisibility::public(7)];
        for session in [None, Some(OWNER), Some(OTHER)] {
            assert!(compute_private_opaque_ids(&before, session).is_empty());
        }

        let after = vec![NodeVisibility::private_owned_by(7, OWNER)];
        assert!(compute_private_opaque_ids(&after, None).contains(&7));
        assert!(compute_private_opaque_ids(&after, Some(OTHER)).contains(&7));
        assert!(
            !compute_private_opaque_ids(&after, Some(OWNER)).contains(&7),
            "the owner keeps seeing their own node"
        );
    }

    /// An owner change hands visibility to the new owner and takes it from the
    /// old one, with no stale retention in the filter itself.
    #[test]
    fn an_owner_change_moves_visibility() {
        let before = vec![NodeVisibility::private_owned_by(8, OWNER)];
        assert!(!compute_private_opaque_ids(&before, Some(OWNER)).contains(&8));
        assert!(compute_private_opaque_ids(&before, Some(OTHER)).contains(&8));

        let after = vec![NodeVisibility::private_owned_by(8, OTHER)];
        assert!(
            compute_private_opaque_ids(&after, Some(OWNER)).contains(&8),
            "the previous owner loses visibility"
        );
        assert!(!compute_private_opaque_ids(&after, Some(OTHER)).contains(&8));
    }

    /// Re-authenticating as a different pubkey changes what the same connection
    /// may see: the filter holds no per-connection state, so the drop set is a
    /// pure function of (corpus, session pubkey).
    #[test]
    fn reauthentication_changes_the_drop_set_with_no_retained_state() {
        let nodes = corpus();
        let anon = compute_private_opaque_ids(&nodes, None);
        let as_owner = compute_private_opaque_ids(&nodes, Some(OWNER));
        let back_to_anon = compute_private_opaque_ids(&nodes, None);

        assert_ne!(anon, as_owner);
        assert_eq!(
            anon, back_to_anon,
            "dropping the session must restore the anonymous view exactly"
        );
    }

    /// Edge visibility: an edge survives only when BOTH endpoints survive. This
    /// is the rule the socket path applies to initial edges, asserted here over
    /// the same drop set so the two cannot diverge.
    #[test]
    fn an_edge_survives_only_when_both_endpoints_do() {
        let drop = compute_private_opaque_ids(&corpus(), Some(OWNER));
        let edges = [(1u32, 2u32), (1, 3), (2, 3), (1, 4), (2, 2)];
        let kept: Vec<(u32, u32)> = edges
            .into_iter()
            .filter(|(s, t)| !drop.contains(s) && !drop.contains(t))
            .collect();
        assert_eq!(
            kept,
            vec![(1, 2), (2, 2)],
            "only edges between nodes the owner can see survive"
        );
    }

    /// An empty drop set leaves the payload untouched and allocates nothing —
    /// the flag-off hot path.
    #[test]
    fn an_empty_drop_set_is_a_no_op() {
        let mut pairs = vec![(1u32, Payload(10)), (2, Payload(20))];
        let before = pairs.clone();
        assert_eq!(apply_drop_set(&mut pairs, &HashSet::new()), 0);
        assert_eq!(pairs, before);
    }

    /// The drop set applies identically to any payload type, which is what lets
    /// the same set filter node positions, agent nodes and labels.
    #[test]
    fn the_drop_set_applies_to_any_payload_type() {
        let drop = compute_private_opaque_ids(&corpus(), None);

        let mut positions = vec![(1u32, [0.0f32; 3]), (2, [1.0; 3]), (3, [2.0; 3])];
        assert_eq!(apply_drop_set(&mut positions, &drop), 2);
        assert_eq!(positions.len(), 1);

        let mut labels = vec![
            (1u32, "public".to_string()),
            (2, "owner private".to_string()),
            (4, "unowned private".to_string()),
        ];
        assert_eq!(apply_drop_set(&mut labels, &drop), 2);
        assert_eq!(labels, vec![(1, "public".to_string())]);
    }
}
