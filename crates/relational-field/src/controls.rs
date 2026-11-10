//! Controls added in deviation D4, with no pass mark: relation-label permutation (negative
//! control on M3) and degree-preserving rewiring (control on M2).

use crate::corpus::Triple;
use crate::split::pair;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, HashSet};

/// Shuffle the relation labels of `ts` among themselves, keeping every head and tail.
pub fn permute_labels(ts: &[Triple], seed: u64) -> Vec<Triple> {
    let mut rels: Vec<u16> = ts.iter().map(|t| t.rel).collect();
    rels.shuffle(&mut StdRng::seed_from_u64(seed));
    ts.iter()
        .zip(rels)
        .map(|(t, rel)| Triple { rel, ..*t })
        .collect()
}

/// Outcome of [`rewire`].
#[derive(Clone, Debug)]
pub struct Rewired {
    /// The rewired triples.
    pub triples: Vec<Triple>,
    /// Swap attempts made.
    pub attempts: usize,
    /// Swaps accepted.
    pub accepted: usize,
}

/// Degree-preserving double-edge swaps within one relation: `(a, r, b)` and `(c, r, d)` become
/// `(a, r, d)` and `(c, r, b)`, keeping every node's in- and out-degree per relation. The first
/// triple is drawn uniformly, the second uniformly among triples of the same relation. A swap is rejected if it
/// would create a self-loop, a pair already in the current graph, or a pair in `forbidden`
/// (every edge pair of every split, so no held-out pair can leak in). `attempts_per_edge`
/// attempts are made per triple.
pub fn rewire(
    ts: &[Triple],
    forbidden: &HashSet<(u32, u32)>,
    attempts_per_edge: usize,
    seed: u64,
) -> Rewired {
    let mut out = ts.to_vec();
    let mut present: HashMap<(u32, u32), usize> = HashMap::new();
    for t in &out {
        *present.entry(pair(t.head, t.tail)).or_default() += 1;
    }
    let n_rel = out
        .iter()
        .map(|t| usize::from(t.rel) + 1)
        .max()
        .unwrap_or(0);
    let mut by_rel: Vec<Vec<usize>> = vec![Vec::new(); n_rel];
    for (k, t) in out.iter().enumerate() {
        by_rel[t.rel as usize].push(k);
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let attempts = attempts_per_edge * out.len();
    let mut accepted = 0;
    if out.len() < 2 {
        return Rewired {
            triples: out,
            attempts: 0,
            accepted,
        };
    }
    for _ in 0..attempts {
        let x = rng.gen_range(0..out.len());
        let same = &by_rel[out[x].rel as usize];
        let y = same[rng.gen_range(0..same.len())];
        if x == y {
            continue;
        }
        let (t1, t2) = (out[x], out[y]);
        let (n1, n2) = (pair(t1.head, t2.tail), pair(t2.head, t1.tail));
        let bad = |p: (u32, u32)| p.0 == p.1 || forbidden.contains(&p) || present.contains_key(&p);
        if n1 == n2 || bad(n1) || bad(n2) {
            continue;
        }
        for old in [pair(t1.head, t1.tail), pair(t2.head, t2.tail)] {
            let c = present.get_mut(&old).expect("old pair is present");
            *c -= 1;
            if *c == 0 {
                present.remove(&old);
            }
        }
        present.insert(n1, 1);
        present.insert(n2, 1);
        out[x].tail = t2.tail;
        out[y].tail = t1.tail;
        accepted += 1;
    }
    Rewired {
        triples: out,
        attempts,
        accepted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(n: u32) -> Vec<Triple> {
        (0..n)
            .map(|i| Triple {
                head: i,
                rel: (i % 3) as u16,
                tail: (i + 1) % n,
            })
            .collect()
    }

    #[test]
    fn permutation_keeps_endpoints_and_label_multiset() {
        let ts = ring(60);
        let p = permute_labels(&ts, 4);
        assert!(ts
            .iter()
            .zip(&p)
            .all(|(a, b)| a.head == b.head && a.tail == b.tail));
        let count = |v: &[Triple]| {
            (0..3)
                .map(|r| v.iter().filter(|t| t.rel == r).count())
                .collect::<Vec<_>>()
        };
        assert_eq!(count(&ts), count(&p));
        assert_ne!(ts, p);
    }

    #[test]
    fn rewiring_preserves_typed_degrees_and_avoids_forbidden_pairs() {
        let ts = ring(200);
        let forbidden: HashSet<(u32, u32)> = (0..200u32).map(|i| pair(i, (i + 7) % 200)).collect();
        let rw = rewire(&ts, &forbidden, 10, 1);
        assert!(rw.accepted > 100, "too few swaps accepted: {}", rw.accepted);
        let deg = |v: &[Triple]| {
            let mut m: HashMap<(u32, u16, bool), usize> = HashMap::new();
            for t in v {
                *m.entry((t.head, t.rel, true)).or_default() += 1;
                *m.entry((t.tail, t.rel, false)).or_default() += 1;
            }
            m
        };
        assert_eq!(deg(&ts), deg(&rw.triples));
        let pairs: Vec<_> = rw.triples.iter().map(|t| pair(t.head, t.tail)).collect();
        assert!(pairs.iter().all(|p| p.0 != p.1 && !forbidden.contains(p)));
        assert_eq!(
            pairs.iter().collect::<HashSet<_>>().len(),
            pairs.len(),
            "duplicate pair created"
        );
        let original: HashSet<_> = ts.iter().map(|t| pair(t.head, t.tail)).collect();
        assert!(
            pairs.iter().filter(|p| original.contains(p)).count() < 100,
            "graph barely rewired"
        );
    }
}
