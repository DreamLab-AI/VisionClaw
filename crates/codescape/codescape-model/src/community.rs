//! Stable matching of community labels across recomputes.
//!
//! Leiden assigns arbitrary integer labels, so a recompute that leaves the
//! partition almost unchanged would still recolour every community. Each new
//! community takes the label of the previous community it overlaps most
//! (Jaccard), greedily by overlap, and unmatched new communities get fresh
//! labels above the previous maximum.

use std::collections::{HashMap, HashSet};

/// Result of a matching pass.
#[derive(Debug, Clone, PartialEq)]
pub struct StableMatch {
    /// New label → stable label.
    pub relabel: HashMap<u32, u32>,
    /// Fraction of nodes that keep the label they had before (1.0 = no flicker).
    pub retained: f32,
    /// Next unused stable label.
    pub next_label: u32,
}

impl StableMatch {
    /// The stable label for a new label (identity when unknown).
    pub fn map(&self, new_label: u32) -> u32 {
        self.relabel.get(&new_label).copied().unwrap_or(new_label)
    }
}

/// Match `new` labels to `previous` labels by maximum Jaccard overlap.
///
/// Both inputs are `(node_id, community)` pairs; label 0 means unassigned and
/// is passed through untouched.
pub fn stable_match(previous: &[(u32, u32)], new: &[(u32, u32)]) -> StableMatch {
    let mut prev_members: HashMap<u32, HashSet<u32>> = HashMap::new();
    let mut prev_of: HashMap<u32, u32> = HashMap::new();
    for &(n, c) in previous {
        if c != 0 {
            prev_members.entry(c).or_default().insert(n);
            prev_of.insert(n, c);
        }
    }
    let mut new_members: HashMap<u32, HashSet<u32>> = HashMap::new();
    for &(n, c) in new {
        if c != 0 {
            new_members.entry(c).or_default().insert(n);
        }
    }
    let mut next_label = prev_members.keys().copied().max().unwrap_or(0) + 1;

    // Candidate pairs scored by Jaccard, best first.
    let mut candidates: Vec<(f64, u32, u32)> = Vec::new();
    for (&nc, nm) in &new_members {
        let mut overlap: HashMap<u32, usize> = HashMap::new();
        for n in nm {
            if let Some(&pc) = prev_of.get(n) {
                *overlap.entry(pc).or_default() += 1;
            }
        }
        for (pc, inter) in overlap {
            let union = nm.len() + prev_members[&pc].len() - inter;
            candidates.push((inter as f64 / union as f64, nc, pc));
        }
    }
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap()
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });

    let mut relabel: HashMap<u32, u32> = HashMap::new();
    let mut taken: HashSet<u32> = HashSet::new();
    for (_, nc, pc) in candidates {
        if relabel.contains_key(&nc) || taken.contains(&pc) {
            continue;
        }
        relabel.insert(nc, pc);
        taken.insert(pc);
    }
    let mut labels: Vec<u32> = new_members.keys().copied().collect();
    labels.sort_unstable();
    for nc in labels {
        if let std::collections::hash_map::Entry::Vacant(slot) = relabel.entry(nc) {
            slot.insert(next_label);
            next_label += 1;
        }
    }

    let mut kept = 0usize;
    let mut counted = 0usize;
    for &(n, c) in new {
        if c == 0 {
            continue;
        }
        if let Some(&pc) = prev_of.get(&n) {
            counted += 1;
            if relabel[&c] == pc {
                kept += 1;
            }
        }
    }
    let retained = if counted == 0 {
        1.0
    } else {
        kept as f32 / counted as f32
    };
    StableMatch {
        relabel,
        retained,
        next_label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permuted_labels_are_restored() {
        let prev = vec![(1, 10), (2, 10), (3, 10), (4, 20), (5, 20), (6, 30)];
        // Same partition, labels shuffled.
        let new = vec![(1, 7), (2, 7), (3, 7), (4, 8), (5, 8), (6, 9)];
        let m = stable_match(&prev, &new);
        assert_eq!(m.map(7), 10);
        assert_eq!(m.map(8), 20);
        assert_eq!(m.map(9), 30);
        assert_eq!(m.retained, 1.0);
    }

    #[test]
    fn a_split_keeps_the_bigger_half_and_mints_a_label_for_the_rest() {
        let prev = vec![(1, 10), (2, 10), (3, 10), (4, 10)];
        let new = vec![(1, 1), (2, 1), (3, 1), (4, 2)];
        let m = stable_match(&prev, &new);
        assert_eq!(m.map(1), 10);
        assert_eq!(m.map(2), 11, "fresh label above previous max");
        assert!((m.retained - 0.75).abs() < 1e-6);
        assert_eq!(m.next_label, 12);
    }

    #[test]
    fn unassigned_zero_passes_through() {
        let m = stable_match(&[(1, 0)], &[(1, 0), (2, 5)]);
        assert_eq!(m.map(0), 0);
        assert_eq!(m.map(5), 1);
    }
}
