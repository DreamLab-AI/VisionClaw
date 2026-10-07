//! Recall@k of an approximate nearest-neighbour result against exact truth.

use std::collections::HashSet;
use std::hash::Hash;

/// Fraction of the first `k` exact neighbours that appear among the first
/// `k` approximate ones. The denominator is `min(k, exact.len())`; when that
/// is zero there is nothing to recall and the result is `1.0`.
///
/// ```
/// use visionclaw_memory_cloud::recall::recall_at_k;
/// let exact = ["a", "b", "c", "d"];
/// let approx = ["a", "c", "x", "b"];
/// assert_eq!(recall_at_k(&approx, &exact, 3), 2.0 / 3.0);
/// ```
pub fn recall_at_k<T: Eq + Hash>(approx: &[T], exact: &[T], k: usize) -> f64 {
    let truth: HashSet<&T> = exact.iter().take(k).collect();
    if truth.is_empty() {
        return 1.0;
    }
    let found: HashSet<&T> = approx
        .iter()
        .take(k)
        .filter(|id| truth.contains(id))
        .collect();
    found.len() as f64 / truth.len() as f64
}

/// Tie-aware recall@k from distances.
///
/// Id-based recall is unreliable when many entries share an identical
/// embedding (the RuVector sidecar holds groups of hundreds): the exact
/// top-k then picks an arbitrary subset of equally distant rows and an
/// approximate search picking a different subset scores zero despite being
/// perfect. Here an approximate hit counts when its distance is no greater
/// than the k-th exact distance plus `eps`. `exact` must be ascending; the
/// result is capped at 1 and is `1.0` when `exact` is empty.
///
/// ```
/// use visionclaw_memory_cloud::recall::recall_at_k_by_distance;
/// // Five rows tie at distance 0; the index returns a different three.
/// let exact = [0.0, 0.0, 0.0];
/// let approx = [0.0, 0.0, 0.0];
/// assert_eq!(recall_at_k_by_distance(&approx, &exact, 3, 1e-6), 1.0);
/// assert_eq!(recall_at_k_by_distance(&[0.0, 0.2, 0.3], &[0.0, 0.1, 0.2], 3, 1e-6), 2.0 / 3.0);
/// ```
pub fn recall_at_k_by_distance(approx: &[f64], exact: &[f64], k: usize, eps: f64) -> f64 {
    let denom = k.min(exact.len());
    if denom == 0 {
        return 1.0;
    }
    let threshold = exact[denom - 1] + eps;
    let hits = approx
        .iter()
        .take(k)
        .filter(|&&d| d <= threshold)
        .count()
        .min(denom);
    hits as f64 / denom as f64
}

/// Arithmetic mean, `None` for an empty slice.
///
/// ```
/// use visionclaw_memory_cloud::recall::mean;
/// assert_eq!(mean(&[1.0, 0.5]), Some(0.75));
/// assert_eq!(mean(&[]), None);
/// ```
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_and_zero_recall() {
        assert_eq!(recall_at_k(&[1, 2, 3], &[3, 2, 1], 3), 1.0);
        assert_eq!(recall_at_k(&[4, 5, 6], &[1, 2, 3], 3), 0.0);
    }

    #[test]
    fn only_the_first_k_count_on_both_sides() {
        // 3 is in approx but beyond k.
        assert_eq!(recall_at_k(&[1, 9, 3], &[1, 3, 7], 2), 0.5);
    }

    #[test]
    fn distance_recall_tolerates_ties_but_not_misses() {
        // Ids would disagree completely; distances show the index was right.
        assert_eq!(
            recall_at_k_by_distance(&[0.1, 0.1], &[0.1, 0.1], 2, 1e-9),
            1.0
        );
        // A genuinely worse neighbour is a miss.
        assert_eq!(
            recall_at_k_by_distance(&[0.1, 0.5], &[0.1, 0.2], 2, 1e-9),
            0.5
        );
        // Fewer approximate results than k.
        assert_eq!(recall_at_k_by_distance(&[0.1], &[0.1, 0.2], 2, 1e-9), 0.5);
        // Short truth caps the denominator; the numerator is capped too.
        assert_eq!(
            recall_at_k_by_distance(&[0.0, 0.0, 0.0], &[0.0], 10, 1e-9),
            1.0
        );
        assert_eq!(recall_at_k_by_distance(&[], &[], 10, 1e-9), 1.0);
        // Only the first k approximate results count.
        assert_eq!(
            recall_at_k_by_distance(&[0.9, 0.1], &[0.1, 0.2], 1, 1e-9),
            0.0
        );
    }

    #[test]
    fn short_truth_and_duplicates() {
        assert_eq!(recall_at_k(&[1, 2], &[1], 10), 1.0);
        assert_eq!(recall_at_k(&[1, 1, 1], &[1, 2, 3], 3), 1.0 / 3.0);
        assert_eq!(recall_at_k::<u8>(&[], &[], 10), 1.0);
        assert_eq!(recall_at_k(&[], &[1], 10), 0.0);
    }
}
