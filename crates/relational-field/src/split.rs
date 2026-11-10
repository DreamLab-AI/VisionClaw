//! Pair-level train / validation / test split (PREREGISTRATION.md, "Data and splits").
//!
//! The unit of holdout is the unordered pair `{i, j}`: every triple between the two nodes, in
//! either direction and under any relation key, lands in the same split, so a held-out edge cannot
//! leak back in through its recorded inverse on the other page.

use crate::corpus::Triple;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Train,
    Val,
    Test,
}

pub struct Split {
    pub train: Vec<Triple>,
    pub val: Vec<Triple>,
    pub test: Vec<Triple>,
    /// Every unordered pair that is an edge in any split, for drawing true non-edges.
    pub all_pairs: HashSet<(u32, u32)>,
    pub test_pairs: Vec<(u32, u32)>,
}

pub fn pair(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

pub fn split(triples: &[Triple], seed: u64) -> Split {
    let mut pairs: Vec<(u32, u32)> = triples
        .iter()
        .map(|t| pair(t.head, t.tail))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    pairs.sort_unstable(); // HashSet order is not deterministic; the shuffle must be
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    pairs.shuffle(&mut rng);

    let n = pairs.len();
    let (n_train, n_val) = (n * 8 / 10, n / 10);
    let mut part: HashMap<(u32, u32), Part> = HashMap::with_capacity(n);
    for (k, p) in pairs.iter().enumerate() {
        let which = if k < n_train {
            Part::Train
        } else if k < n_train + n_val {
            Part::Val
        } else {
            Part::Test
        };
        part.insert(*p, which);
    }

    let mut s = Split {
        train: Vec::new(),
        val: Vec::new(),
        test: Vec::new(),
        all_pairs: pairs.iter().copied().collect(),
        test_pairs: pairs[n_train + n_val..].to_vec(),
    };
    s.test_pairs.sort_unstable();
    for t in triples {
        match part[&pair(t.head, t.tail)] {
            Part::Train => s.train.push(*t),
            Part::Val => s.val.push(*t),
            Part::Test => s.test.push(*t),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_edges_share_a_split() {
        let mut ts = Vec::new();
        for i in 0..200u32 {
            ts.push(Triple {
                head: i,
                rel: 0,
                tail: i + 1,
            });
            ts.push(Triple {
                head: i + 1,
                rel: 1,
                tail: i,
            }); // recorded inverse
        }
        let s = split(&ts, 3);
        let train: HashSet<_> = s.train.iter().map(|t| pair(t.head, t.tail)).collect();
        for t in s.test.iter().chain(&s.val) {
            assert!(!train.contains(&pair(t.head, t.tail)));
        }
        assert_eq!(s.train.len() + s.val.len() + s.test.len(), ts.len());
    }

    #[test]
    fn split_is_deterministic_per_seed() {
        let ts: Vec<_> = (0..500u32)
            .map(|i| Triple {
                head: i,
                rel: 0,
                tail: i + 7,
            })
            .collect();
        assert_eq!(split(&ts, 1).test_pairs, split(&ts, 1).test_pairs);
        assert_ne!(split(&ts, 1).test_pairs, split(&ts, 2).test_pairs);
    }
}
