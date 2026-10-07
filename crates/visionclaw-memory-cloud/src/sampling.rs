//! Stratified sample allocation across namespaces.
//!
//! A uniform random sample of `memory_entries` is dominated by whichever
//! namespace is largest (`ruvnet-kb` is ~75 % of the table) and by hook noise,
//! so small but important namespaces (`project-state`, `patterns`,
//! `dream-cycle`) vanish. Allocation therefore works per namespace:
//!
//! 1. every namespace first receives a floor of `min(count, floor)` rows;
//! 2. the remaining budget is shared in proportion to `√count`, with noise
//!    namespaces (hook telemetry, command logs, file history) weighted by
//!    [`SamplingPolicy::noise_weight`];
//! 3. no namespace is ever asked for more rows than it has, and surplus from
//!    capped namespaces is redistributed (water-filling), so the total is met
//!    whenever the table holds enough rows.
//!
//! When the floors alone exceed the budget, the budget is shared by weight
//! with each namespace capped at its floor instead.

use crate::config::NamespacePatterns;

/// Namespace patterns treated as low-signal telemetry.
pub const DEFAULT_NOISE_PATTERNS: &[&str] = &[
    "hooks:*",
    "command-*",
    "legacy/*",
    "performance-metrics",
    "file-history",
];

/// Per-namespace floor before proportional sharing.
pub const DEFAULT_FLOOR: usize = 40;

/// Weight multiplier applied to noise namespaces.
pub const DEFAULT_NOISE_WEIGHT: f64 = 0.25;

/// How a sample budget is shared between namespaces.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplingPolicy {
    /// Total rows to sample.
    pub total: usize,
    /// Rows guaranteed to each namespace (capped at its size).
    pub floor: usize,
    /// Multiplier on the √count weight of a noise namespace.
    pub noise_weight: f64,
    /// Which namespaces count as noise.
    pub noise: NamespacePatterns,
}

impl SamplingPolicy {
    /// The standard policy for a given total.
    pub fn with_total(total: usize) -> Self {
        Self {
            total,
            floor: DEFAULT_FLOOR,
            noise_weight: DEFAULT_NOISE_WEIGHT,
            noise: NamespacePatterns::from_patterns(DEFAULT_NOISE_PATTERNS.iter().copied()),
        }
    }
}

/// Number of embedded rows in one namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceCount {
    /// Namespace name.
    pub namespace: String,
    /// Rows with a non-null embedding.
    pub embedded: u64,
}

/// The sample quota assigned to one namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    /// Namespace name.
    pub namespace: String,
    /// Rows with a non-null embedding.
    pub embedded: u64,
    /// Rows to sample from it.
    pub quota: usize,
}

/// Allocate `policy.total` sample rows across `counts`, omitting every
/// namespace matched by `excluded` and every namespace with no embedded rows.
/// The result is sorted by namespace.
///
/// ```
/// use visionclaw_memory_cloud::config::NamespacePatterns;
/// use visionclaw_memory_cloud::sampling::{allocate, NamespaceCount, SamplingPolicy};
///
/// let counts = vec![
///     NamespaceCount { namespace: "big".into(), embedded: 160_000 },
///     NamespaceCount { namespace: "small".into(), embedded: 30 },
///     NamespaceCount { namespace: "personal-context".into(), embedded: 32 },
/// ];
/// let excluded = NamespacePatterns::parse_list("personal-context");
/// let a = allocate(&SamplingPolicy::with_total(1000), &counts, &excluded);
/// assert_eq!(a.len(), 2);
/// assert_eq!(a.iter().map(|x| x.quota).sum::<usize>(), 1000);
/// assert_eq!(a.iter().find(|x| x.namespace == "small").unwrap().quota, 30);
/// ```
pub fn allocate(
    policy: &SamplingPolicy,
    counts: &[NamespaceCount],
    excluded: &NamespacePatterns,
) -> Vec<Allocation> {
    let mut eligible: Vec<&NamespaceCount> = counts
        .iter()
        .filter(|c| c.embedded > 0 && !excluded.matches(&c.namespace))
        .collect();
    eligible.sort_by(|a, b| a.namespace.cmp(&b.namespace));

    let sizes: Vec<usize> = eligible
        .iter()
        .map(|c| usize::try_from(c.embedded).unwrap_or(usize::MAX))
        .collect();
    let weights: Vec<f64> = eligible
        .iter()
        .map(|c| {
            let w = (c.embedded as f64).sqrt();
            if policy.noise.matches(&c.namespace) {
                w * policy.noise_weight
            } else {
                w
            }
        })
        .collect();
    let floors: Vec<usize> = sizes.iter().map(|&n| n.min(policy.floor)).collect();
    let floor_sum: usize = floors.iter().sum();

    let quotas = if floor_sum >= policy.total {
        apportion(&weights, &floors, policy.total)
    } else {
        let spare: Vec<usize> = sizes.iter().zip(&floors).map(|(s, f)| s - f).collect();
        let extra = apportion(&weights, &spare, policy.total - floor_sum);
        floors.iter().zip(extra).map(|(f, e)| f + e).collect()
    };

    eligible
        .into_iter()
        .zip(quotas)
        .map(|(c, quota)| Allocation {
            namespace: c.namespace.clone(),
            embedded: c.embedded,
            quota,
        })
        .collect()
}

/// Share `budget` integer units in proportion to `weights`, never giving
/// entry `i` more than `caps[i]`. Surplus from capped entries is
/// redistributed among the rest; the final integer split uses the largest
/// remainder method (ties broken by lower index). The result sums to
/// `min(budget, Σcaps)` provided every entry with spare capacity has a
/// positive weight.
fn apportion(weights: &[f64], caps: &[usize], budget: usize) -> Vec<usize> {
    let n = weights.len();
    let mut alloc = vec![0usize; n];
    let mut remaining = budget.min(caps.iter().sum());

    while remaining > 0 {
        let active: Vec<usize> = (0..n)
            .filter(|&i| alloc[i] < caps[i] && weights[i] > 0.0)
            .collect();
        if active.is_empty() {
            break;
        }
        let total_weight: f64 = active.iter().map(|&i| weights[i]).sum();
        let shares: Vec<(usize, f64)> = active
            .iter()
            .map(|&i| (i, remaining as f64 * weights[i] / total_weight))
            .collect();

        // Saturate every entry whose ideal share meets its spare capacity,
        // then reshare what is left among the others.
        let saturated: Vec<usize> = shares
            .iter()
            .filter(|(i, share)| *share >= (caps[*i] - alloc[*i]) as f64)
            .map(|(i, _)| *i)
            .collect();
        if !saturated.is_empty() {
            for i in saturated {
                remaining -= caps[i] - alloc[i];
                alloc[i] = caps[i];
            }
            continue;
        }

        // No entry saturates: every share is strictly below its spare
        // capacity, so floor + 1 never exceeds a cap.
        let mut given = 0usize;
        let mut fractions: Vec<(usize, f64)> = Vec::with_capacity(shares.len());
        for (i, share) in shares {
            let whole = share.floor() as usize;
            alloc[i] += whole;
            given += whole;
            fractions.push((i, share - whole as f64));
        }
        let mut leftover = remaining - given;
        fractions.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (i, _) in fractions {
            if leftover == 0 {
                break;
            }
            alloc[i] += 1;
            leftover -= 1;
        }
        remaining = leftover;
    }
    alloc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(pairs: &[(&str, u64)]) -> Vec<NamespaceCount> {
        pairs
            .iter()
            .map(|(n, c)| NamespaceCount {
                namespace: n.to_string(),
                embedded: *c,
            })
            .collect()
    }

    fn quota(a: &[Allocation], ns: &str) -> usize {
        a.iter()
            .find(|x| x.namespace == ns)
            .map(|x| x.quota)
            .unwrap()
    }

    /// The live table's shape on 2026-10-07 (abridged).
    fn live_like() -> Vec<NamespaceCount> {
        counts(&[
            ("ruvnet-kb", 162_534),
            ("knowledge/pages", 20_119),
            ("ontology-corpus", 8_146),
            ("coordination", 3_158),
            ("file-history", 3_153),
            ("hooks:post-edit", 3_152),
            ("hooks:pre-edit", 3_018),
            ("project-state", 2_196),
            ("patterns", 416),
            ("dream-cycle", 84),
            ("personal-context", 32),
            ("projects/minigolf", 16),
            ("empty", 0),
        ])
    }

    #[test]
    fn meets_total_and_respects_caps() {
        let excluded = NamespacePatterns::parse_list("personal-context");
        let a = allocate(&SamplingPolicy::with_total(6000), &live_like(), &excluded);
        assert_eq!(a.iter().map(|x| x.quota).sum::<usize>(), 6000);
        for x in &a {
            assert!(x.quota as u64 <= x.embedded, "{x:?}");
        }
    }

    #[test]
    fn excluded_and_empty_namespaces_never_appear() {
        let excluded = NamespacePatterns::parse_list("personal-context");
        let a = allocate(&SamplingPolicy::with_total(6000), &live_like(), &excluded);
        assert!(a.iter().all(|x| x.namespace != "personal-context"));
        assert!(a.iter().all(|x| x.namespace != "empty"));
        let names: Vec<_> = a.iter().map(|x| x.namespace.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "result is sorted by namespace");
    }

    #[test]
    fn small_namespaces_get_their_floor_or_everything() {
        let a = allocate(
            &SamplingPolicy::with_total(6000),
            &live_like(),
            &NamespacePatterns::default(),
        );
        assert_eq!(quota(&a, "projects/minigolf"), 16);
        assert!(quota(&a, "dream-cycle") >= 40);
        assert!(quota(&a, "patterns") >= 40);
        assert!(quota(&a, "personal-context") == 32, "not excluded here");
    }

    #[test]
    fn sqrt_weighting_tames_the_giant_namespace() {
        let a = allocate(
            &SamplingPolicy::with_total(6000),
            &live_like(),
            &NamespacePatterns::default(),
        );
        let kb = quota(&a, "ruvnet-kb") as f64;
        // Proportional sampling would give ruvnet-kb ~78 % of the sample.
        assert!(kb / 6000.0 < 0.5, "ruvnet-kb share {}", kb / 6000.0);
        // ...but it is still the largest stratum.
        assert!(a.iter().all(|x| x.quota as f64 <= kb));
    }

    #[test]
    fn noise_namespaces_are_down_weighted() {
        let a = allocate(
            &SamplingPolicy::with_total(6000),
            &live_like(),
            &NamespacePatterns::default(),
        );
        // coordination (3158) and file-history (3153) are near-equal in size;
        // the noise one must get roughly a quarter of the proportional part.
        let signal = quota(&a, "coordination") - 40;
        let noise = quota(&a, "file-history") - 40;
        let ratio = noise as f64 / signal as f64;
        assert!((ratio - 0.25).abs() < 0.02, "ratio {ratio}");
        assert!(quota(&a, "hooks:post-edit") < quota(&a, "project-state"));
    }

    #[test]
    fn floors_exceeding_budget_are_shared_by_weight() {
        let c = counts(&[("a", 100), ("b", 100), ("c", 10_000)]);
        let mut policy = SamplingPolicy::with_total(60);
        policy.floor = 40;
        let a = allocate(&policy, &c, &NamespacePatterns::default());
        assert_eq!(a.iter().map(|x| x.quota).sum::<usize>(), 60);
        assert!(a.iter().all(|x| x.quota <= 40));
        assert!(quota(&a, "c") >= quota(&a, "a"));
    }

    #[test]
    fn budget_larger_than_table_takes_everything() {
        let c = counts(&[("a", 10), ("b", 300)]);
        let a = allocate(
            &SamplingPolicy::with_total(6000),
            &c,
            &NamespacePatterns::default(),
        );
        assert_eq!(quota(&a, "a"), 10);
        assert_eq!(quota(&a, "b"), 300);
    }

    #[test]
    fn apportion_redistributes_capped_surplus() {
        // Equal weights, but entry 0 can only take 2 of its fair 5.
        let got = apportion(&[1.0, 1.0], &[2, 100], 10);
        assert_eq!(got, vec![2, 8]);
        // Largest remainder: 10 units over weights 1:1:1 -> 4,3,3.
        assert_eq!(
            apportion(&[1.0, 1.0, 1.0], &[10, 10, 10], 10),
            vec![4, 3, 3]
        );
        assert_eq!(apportion(&[], &[], 10), Vec::<usize>::new());
    }
}
