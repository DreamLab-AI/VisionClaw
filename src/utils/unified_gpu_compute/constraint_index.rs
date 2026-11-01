//! Per-node constraint index for the force pass.
//!
//! The force kernels used to find a node's constraints by having every thread walk *all*
//! `num_constraints` entries and test membership, which is O(nodes x constraints) per tick
//! (about 0.94 billion checks on the live graph: 9,688 nodes x 96,742 constraints). This module
//! builds the inverse mapping on the host, once per constraint upload, as a CSR: `offsets[n]..
//! offsets[n + 1]` slices `ids` to the constraints node `n` takes part in.
//!
//! The mapping is exact. A constraint lists a node when the old test (`node_idx[k] == n` for some
//! `k < min(count, 4)`) would have matched, once per constraint even if the node fills two slots,
//! and each node's list is in ascending constraint order, which is the order the old loop
//! accumulated forces in, so the floating-point sums are unchanged.

use crate::models::constraints::ConstraintData;

/// Most node slots a constraint can use (the kernel's `n < 4` bound).
const MAX_SLOTS: usize = 4;

/// CSR from node index to the constraints that involve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstraintIndex {
    /// `num_slots + 1` offsets into `ids`.
    pub offsets: Vec<i32>,
    /// Constraint ids, ascending within each node's slice.
    pub ids: Vec<i32>,
}

/// The distinct in-range nodes constraint `c` touches, as the kernel's membership test sees them.
fn involved_nodes(c: &ConstraintData, num_slots: usize) -> ([i32; MAX_SLOTS], usize) {
    let mut seen = [0i32; MAX_SLOTS];
    let mut n = 0;
    let used = (c.count.max(0) as usize).min(MAX_SLOTS);
    for &node in &c.node_idx[..used] {
        if node >= 0 && (node as usize) < num_slots && !seen[..n].contains(&node) {
            seen[n] = node;
            n += 1;
        }
    }
    (seen, n)
}

/// Build the index over `num_slots` nodes (the allocated node capacity, so any node the kernel can
/// evaluate has a slice). Constraints naming nodes outside `0..num_slots` are simply not listed
/// for them; the kernel can never evaluate those nodes.
pub(crate) fn build_constraint_index(
    constraints: &[ConstraintData],
    num_slots: usize,
) -> ConstraintIndex {
    let mut offsets = vec![0i32; num_slots + 1];
    for c in constraints {
        let (nodes, n) = involved_nodes(c, num_slots);
        for &node in &nodes[..n] {
            offsets[node as usize + 1] += 1;
        }
    }
    for i in 0..num_slots {
        offsets[i + 1] += offsets[i];
    }
    let mut ids = vec![0i32; offsets[num_slots] as usize];
    let mut cursor: Vec<i32> = offsets[..num_slots].to_vec();
    for (id, c) in constraints.iter().enumerate() {
        let (nodes, n) = involved_nodes(c, num_slots);
        for &node in &nodes[..n] {
            let slot = &mut cursor[node as usize];
            ids[*slot as usize] = id as i32;
            *slot += 1;
        }
    }
    ConstraintIndex { offsets, ids }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(kind: i32, a: i32, b: i32) -> ConstraintData {
        ConstraintData {
            kind,
            count: 2,
            node_idx: [a, b, 0, 0],
            ..ConstraintData::default()
        }
    }

    /// The old kernel's test, verbatim: first matching slot among the first `min(count, 4)`.
    fn old_scan(constraints: &[ConstraintData], node: i32) -> Vec<i32> {
        constraints
            .iter()
            .enumerate()
            .filter(|(_, c)| (0..(c.count.max(0) as usize).min(4)).any(|n| c.node_idx[n] == node))
            .map(|(i, _)| i as i32)
            .collect()
    }

    fn list(ix: &ConstraintIndex, node: usize) -> &[i32] {
        &ix.ids[ix.offsets[node] as usize..ix.offsets[node + 1] as usize]
    }

    #[test]
    fn matches_the_old_full_scan_for_every_node() {
        // Deterministic pseudo-random constraints, including repeated slots, counts of 0..=6,
        // negative and out-of-range indices.
        let mut s = 0x1234_5678_9ABC_DEF0u64;
        let mut next = move || {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            s.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        let nodes = 64usize;
        let constraints: Vec<ConstraintData> = (0..2_000)
            .map(|_| {
                let count = (next() % 7) as i32;
                let mut node_idx = [0i32; 4];
                for slot in node_idx.iter_mut() {
                    *slot = (next() % (nodes as u64 + 8)) as i32 - 3;
                }
                ConstraintData {
                    count,
                    node_idx,
                    ..ConstraintData::default()
                }
            })
            .collect();
        let ix = build_constraint_index(&constraints, nodes);
        assert_eq!(ix.offsets.len(), nodes + 1);
        for node in 0..nodes {
            assert_eq!(list(&ix, node), old_scan(&constraints, node as i32), "node {node}");
        }
    }

    #[test]
    fn lists_are_ascending_and_a_node_in_two_slots_is_listed_once() {
        let mut twice = pair(0, 5, 5);
        twice.count = 4;
        twice.node_idx = [5, 5, 5, 5];
        let constraints = vec![pair(0, 1, 5), twice, pair(6, 5, 2)];
        let ix = build_constraint_index(&constraints, 8);
        assert_eq!(list(&ix, 5), &[0, 1, 2]);
        assert_eq!(list(&ix, 1), &[0]);
        assert_eq!(list(&ix, 2), &[2]);
        assert!(list(&ix, 0).is_empty());
    }

    #[test]
    fn slots_beyond_count_and_out_of_range_nodes_are_ignored() {
        let mut c = pair(0, 1, 2);
        c.count = 1; // node_idx[1] is padding, not a participant
        let out = pair(0, 3, 99);
        let ix = build_constraint_index(&[c, out], 8);
        assert_eq!(list(&ix, 1), &[0]);
        assert!(list(&ix, 2).is_empty());
        assert_eq!(list(&ix, 3), &[1]);
        assert_eq!(*ix.offsets.last().unwrap(), 2);
    }

    #[test]
    fn empty_input_gives_empty_slices() {
        let ix = build_constraint_index(&[], 4);
        assert_eq!(ix.offsets, vec![0; 5]);
        assert!(ix.ids.is_empty());
    }
}
