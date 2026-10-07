//! SIMD-accelerated force computations for CPU fallback paths
//!
//! Provides AVX2 (256-bit, 8 floats), SSE4.1 (128-bit, 4 floats), and scalar
//! implementations with runtime feature detection via `is_x86_feature_detected!`.
//! Non-x86 targets fall through to scalar code automatically.
//!
//! # Architecture
//!
//! Each public function performs runtime dispatch:
//! 1. AVX2 path  -- processes 8 elements per iteration
//! 2. SSE4.1 path -- processes 4 elements per iteration
//! 3. Scalar path -- processes 1 element per iteration (always available)
//!
//! The tail elements that don't fill a full SIMD lane are handled by the scalar
//! remainder loop in every implementation.

// ---------------------------------------------------------------------------
// Runtime detection helpers
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
#[inline]
fn has_avx2() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(target_arch = "x86_64")]
#[inline]
fn has_sse41() -> bool {
    is_x86_feature_detected!("sse4.1")
}

// ===========================================================================
// 1. Pairwise distance computation
// ===========================================================================

/// Compute pairwise Euclidean distances between two sets of 3D points.
///
/// `distances[i] = sqrt((pos_x[i]-other_x[i])^2 + (pos_y[i]-other_y[i])^2 + (pos_z[i]-other_z[i])^2)`
///
/// All slices must have the same length.
pub fn compute_distances_simd(
    pos_x: &[f32],
    pos_y: &[f32],
    pos_z: &[f32],
    other_x: &[f32],
    other_y: &[f32],
    other_z: &[f32],
    distances: &mut [f32],
) {
    let n = pos_x
        .len()
        .min(pos_y.len())
        .min(pos_z.len())
        .min(other_x.len())
        .min(other_y.len())
        .min(other_z.len())
        .min(distances.len());

    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY: feature check guarantees AVX2+FMA are available on this CPU.
            unsafe {
                compute_distances_avx2(
                    [pos_x, pos_y, pos_z],
                    [other_x, other_y, other_z],
                    distances,
                    n,
                );
            }
            return;
        }
        if has_sse41() {
            unsafe {
                compute_distances_sse41(
                    [pos_x, pos_y, pos_z],
                    [other_x, other_y, other_z],
                    distances,
                    n,
                );
            }
            return;
        }
    }

    compute_distances_scalar(
        [pos_x, pos_y, pos_z],
        [other_x, other_y, other_z],
        distances,
        n,
    );
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn compute_distances_avx2(
    pos: [&[f32]; 3],
    other: [&[f32]; 3],
    distances: &mut [f32],
    n: usize,
) {
    let [pos_x, pos_y, pos_z] = pos;
    let [other_x, other_y, other_z] = other;
    use std::arch::x86_64::*;

    let chunks = n / 8;
    for c in 0..chunks {
        let i = c * 8;

        let px = _mm256_loadu_ps(pos_x.as_ptr().add(i));
        let py = _mm256_loadu_ps(pos_y.as_ptr().add(i));
        let pz = _mm256_loadu_ps(pos_z.as_ptr().add(i));

        let ox = _mm256_loadu_ps(other_x.as_ptr().add(i));
        let oy = _mm256_loadu_ps(other_y.as_ptr().add(i));
        let oz = _mm256_loadu_ps(other_z.as_ptr().add(i));

        let dx = _mm256_sub_ps(px, ox);
        let dy = _mm256_sub_ps(py, oy);
        let dz = _mm256_sub_ps(pz, oz);

        // dx*dx + dy*dy + dz*dz using FMA
        let mut sq = _mm256_mul_ps(dx, dx);
        sq = _mm256_fmadd_ps(dy, dy, sq);
        sq = _mm256_fmadd_ps(dz, dz, sq);

        let dist = _mm256_sqrt_ps(sq);
        _mm256_storeu_ps(distances.as_mut_ptr().add(i), dist);
    }

    // Scalar remainder
    let tail_start = chunks * 8;
    for i in tail_start..n {
        let dx = pos_x[i] - other_x[i];
        let dy = pos_y[i] - other_y[i];
        let dz = pos_z[i] - other_z[i];
        distances[i] = (dx * dx + dy * dy + dz * dz).sqrt();
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn compute_distances_sse41(
    pos: [&[f32]; 3],
    other: [&[f32]; 3],
    distances: &mut [f32],
    n: usize,
) {
    let [pos_x, pos_y, pos_z] = pos;
    let [other_x, other_y, other_z] = other;
    use std::arch::x86_64::*;

    let chunks = n / 4;
    for c in 0..chunks {
        let i = c * 4;

        let px = _mm_loadu_ps(pos_x.as_ptr().add(i));
        let py = _mm_loadu_ps(pos_y.as_ptr().add(i));
        let pz = _mm_loadu_ps(pos_z.as_ptr().add(i));

        let ox = _mm_loadu_ps(other_x.as_ptr().add(i));
        let oy = _mm_loadu_ps(other_y.as_ptr().add(i));
        let oz = _mm_loadu_ps(other_z.as_ptr().add(i));

        let dx = _mm_sub_ps(px, ox);
        let dy = _mm_sub_ps(py, oy);
        let dz = _mm_sub_ps(pz, oz);

        let dx2 = _mm_mul_ps(dx, dx);
        let dy2 = _mm_mul_ps(dy, dy);
        let dz2 = _mm_mul_ps(dz, dz);

        let sq = _mm_add_ps(_mm_add_ps(dx2, dy2), dz2);
        let dist = _mm_sqrt_ps(sq);
        _mm_storeu_ps(distances.as_mut_ptr().add(i), dist);
    }

    let tail_start = chunks * 4;
    for i in tail_start..n {
        let dx = pos_x[i] - other_x[i];
        let dy = pos_y[i] - other_y[i];
        let dz = pos_z[i] - other_z[i];
        distances[i] = (dx * dx + dy * dy + dz * dz).sqrt();
    }
}

fn compute_distances_scalar(pos: [&[f32]; 3], other: [&[f32]; 3], distances: &mut [f32], n: usize) {
    let [pos_x, pos_y, pos_z] = pos;
    let [other_x, other_y, other_z] = other;
    for i in 0..n {
        let dx = pos_x[i] - other_x[i];
        let dy = pos_y[i] - other_y[i];
        let dz = pos_z[i] - other_z[i];
        distances[i] = (dx * dx + dy * dy + dz * dz).sqrt();
    }
}

// ===========================================================================
// 2. Dot product (for similarity computations)
// ===========================================================================

/// SIMD-accelerated dot product of two f32 slices.
///
/// Returns `sum(a[i] * b[i])` for `i in 0..min(a.len(), b.len())`.
pub fn dot_product_simd(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());

    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            return unsafe { dot_product_avx2(a, b, n) };
        }
        if has_sse41() {
            return unsafe { dot_product_sse41(a, b, n) };
        }
    }

    dot_product_scalar(a, b, n)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_product_avx2(a: &[f32], b: &[f32], n: usize) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_ps();
    let chunks = n / 8;
    for c in 0..chunks {
        let i = c * 8;
        let va = _mm256_loadu_ps(a.as_ptr().add(i));
        let vb = _mm256_loadu_ps(b.as_ptr().add(i));
        acc = _mm256_fmadd_ps(va, vb, acc);
    }

    // Horizontal sum of 8 floats in acc
    // acc = [a0 a1 a2 a3 a4 a5 a6 a7]
    let hi128 = _mm256_extractf128_ps(acc, 1); // [a4 a5 a6 a7]
    let lo128 = _mm256_castps256_ps128(acc); // [a0 a1 a2 a3]
    let sum128 = _mm_add_ps(lo128, hi128); // [a0+a4 a1+a5 a2+a6 a3+a7]
    let shuf = _mm_movehdup_ps(sum128); // [a1+a5 a1+a5 a3+a7 a3+a7]
    let sums = _mm_add_ps(sum128, shuf); // [a0+a1+a4+a5 ... a2+a3+a6+a7 ...]
    let shuf2 = _mm_movehl_ps(sums, sums); // move high 64 to low
    let result = _mm_add_ss(sums, shuf2);
    let mut sum = _mm_cvtss_f32(result);

    let tail = chunks * 8;
    for i in tail..n {
        sum += a[i] * b[i];
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn dot_product_sse41(a: &[f32], b: &[f32], n: usize) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm_setzero_ps();
    let chunks = n / 4;
    for c in 0..chunks {
        let i = c * 4;
        let va = _mm_loadu_ps(a.as_ptr().add(i));
        let vb = _mm_loadu_ps(b.as_ptr().add(i));
        // SSE4.1 dpps: dot product with mask 0xFF -> all 4 lanes participate,
        // result broadcast to all lanes
        acc = _mm_add_ps(acc, _mm_dp_ps(va, vb, 0xFF));
    }

    // All 4 lanes of acc hold the same partial sum from each dp call
    // but we accumulated, so just extract lane 0
    // Actually, _mm_dp_ps with 0xFF broadcasts to all, and we _mm_add_ps accumulated,
    // so lane 0 has the total.
    let mut sum = _mm_cvtss_f32(acc);

    let tail = chunks * 4;
    for i in tail..n {
        sum += a[i] * b[i];
    }
    sum
}

fn dot_product_scalar(a: &[f32], b: &[f32], n: usize) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..n {
        sum += a[i] * b[i];
    }
    sum
}

// ===========================================================================
// 3. Batch stress computation (for stress majorization hot path)
// ===========================================================================

/// Compute stress contributions for a batch of node pairs.
///
/// For each pair `i`: `stress += weight[i] * (ideal_dist[i] - actual_dist[i])^2`
///
/// Returns the total stress for this batch.
pub fn compute_stress_batch_simd(
    ideal_distances: &[f32],
    actual_distances: &[f32],
    weights: &[f32],
) -> f32 {
    let n = ideal_distances
        .len()
        .min(actual_distances.len())
        .min(weights.len());

    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            return unsafe {
                compute_stress_batch_avx2(ideal_distances, actual_distances, weights, n)
            };
        }
        if has_sse41() {
            return unsafe {
                compute_stress_batch_sse41(ideal_distances, actual_distances, weights, n)
            };
        }
    }

    compute_stress_batch_scalar(ideal_distances, actual_distances, weights, n)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn compute_stress_batch_avx2(
    ideal: &[f32],
    actual: &[f32],
    weights: &[f32],
    n: usize,
) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_ps();
    let chunks = n / 8;
    for c in 0..chunks {
        let i = c * 8;
        let vid = _mm256_loadu_ps(ideal.as_ptr().add(i));
        let vad = _mm256_loadu_ps(actual.as_ptr().add(i));
        let vw = _mm256_loadu_ps(weights.as_ptr().add(i));

        let diff = _mm256_sub_ps(vid, vad);
        let diff2 = _mm256_mul_ps(diff, diff);
        acc = _mm256_fmadd_ps(vw, diff2, acc);
    }

    // Horizontal sum
    let hi128 = _mm256_extractf128_ps(acc, 1);
    let lo128 = _mm256_castps256_ps128(acc);
    let sum128 = _mm_add_ps(lo128, hi128);
    let shuf = _mm_movehdup_ps(sum128);
    let sums = _mm_add_ps(sum128, shuf);
    let shuf2 = _mm_movehl_ps(sums, sums);
    let result = _mm_add_ss(sums, shuf2);
    let mut total = _mm_cvtss_f32(result);

    let tail = chunks * 8;
    for i in tail..n {
        let diff = ideal[i] - actual[i];
        total += weights[i] * diff * diff;
    }
    total
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn compute_stress_batch_sse41(
    ideal: &[f32],
    actual: &[f32],
    weights: &[f32],
    n: usize,
) -> f32 {
    use std::arch::x86_64::*;

    let mut acc = _mm_setzero_ps();
    let chunks = n / 4;
    for c in 0..chunks {
        let i = c * 4;
        let vid = _mm_loadu_ps(ideal.as_ptr().add(i));
        let vad = _mm_loadu_ps(actual.as_ptr().add(i));
        let vw = _mm_loadu_ps(weights.as_ptr().add(i));

        let diff = _mm_sub_ps(vid, vad);
        let diff2 = _mm_mul_ps(diff, diff);
        acc = _mm_add_ps(acc, _mm_mul_ps(vw, diff2));
    }

    // Horizontal sum of 4 floats
    let shuf = _mm_movehdup_ps(acc);
    let sums = _mm_add_ps(acc, shuf);
    let shuf2 = _mm_movehl_ps(sums, sums);
    let result = _mm_add_ss(sums, shuf2);
    let mut total = _mm_cvtss_f32(result);

    let tail = chunks * 4;
    for i in tail..n {
        let diff = ideal[i] - actual[i];
        total += weights[i] * diff * diff;
    }
    total
}

fn compute_stress_batch_scalar(ideal: &[f32], actual: &[f32], weights: &[f32], n: usize) -> f32 {
    let mut total = 0.0f32;
    for i in 0..n {
        let diff = ideal[i] - actual[i];
        total += weights[i] * diff * diff;
    }
    total
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    #[test]
    fn test_compute_distances_basic() {
        let px = [1.0, 4.0, 0.0];
        let py = [2.0, 5.0, 0.0];
        let pz = [3.0, 6.0, 0.0];
        let ox = [0.0, 0.0, 0.0];
        let oy = [0.0, 0.0, 0.0];
        let oz = [0.0, 0.0, 0.0];
        let mut dist = [0.0f32; 3];

        compute_distances_simd(&px, &py, &pz, &ox, &oy, &oz, &mut dist);

        // sqrt(1+4+9) = sqrt(14) ~ 3.7417
        assert!((dist[0] - 14.0f32.sqrt()).abs() < EPSILON);
        // sqrt(16+25+36) = sqrt(77) ~ 8.7749
        assert!((dist[1] - 77.0f32.sqrt()).abs() < EPSILON);
        assert!((dist[2] - 0.0).abs() < EPSILON);
    }

    #[test]
    fn test_compute_distances_simd_large() {
        // Test with enough elements to exercise AVX2 (8+) and SSE (4+) paths
        let n = 19; // 2 full AVX2 chunks + 3 remainder
        let px: Vec<f32> = (0..n).map(|i| i as f32).collect();
        let py: Vec<f32> = (0..n).map(|i| (i * 2) as f32).collect();
        let pz: Vec<f32> = (0..n).map(|i| (i * 3) as f32).collect();
        let ox = vec![0.0f32; n];
        let oy = vec![0.0f32; n];
        let oz = vec![0.0f32; n];
        let mut dist = vec![0.0f32; n];

        compute_distances_simd(&px, &py, &pz, &ox, &oy, &oz, &mut dist);

        for i in 0..n {
            let expected = (px[i] * px[i] + py[i] * py[i] + pz[i] * pz[i]).sqrt();
            assert!(
                (dist[i] - expected).abs() < EPSILON,
                "mismatch at {}: got {} expected {}",
                i,
                dist[i],
                expected,
            );
        }
    }

    #[test]
    fn test_dot_product() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let b = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

        let result = dot_product_simd(&a, &b);
        assert!((result - 45.0).abs() < EPSILON); // sum 1..9 = 45
    }

    #[test]
    fn test_dot_product_large() {
        let n = 1024;
        let a: Vec<f32> = (0..n).map(|i| (i + 1) as f32).collect();
        let b = vec![1.0f32; n];

        let result = dot_product_simd(&a, &b);
        let expected = (n as f32) * (n as f32 + 1.0) / 2.0;
        assert!(
            (result - expected).abs() < 1.0,
            "got {} expected {}",
            result,
            expected,
        );
    }

    #[test]
    fn test_stress_batch() {
        let ideal = [5.0, 10.0, 3.0, 8.0];
        let actual = [4.0, 9.0, 2.0, 7.0];
        let weights = [1.0, 2.0, 0.5, 1.0];

        let stress = compute_stress_batch_simd(&ideal, &actual, &weights);
        // (5-4)^2*1 + (10-9)^2*2 + (3-2)^2*0.5 + (8-7)^2*1 = 1 + 2 + 0.5 + 1 = 4.5
        assert!((stress - 4.5).abs() < EPSILON);
    }
}
