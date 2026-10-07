//! Principal-component projection of the sample to 3-D.
//!
//! It replaces the retired `compute-umap-projection.mjs`, whose power
//! iteration started from `Math.random()` (so the cloud's orientation and
//! handedness changed on every run), stopped after a fixed 100 iterations
//! without a convergence test, and scaled by the single largest radius (one
//! outlier squashed everything else). This implementation:
//!
//! 1. centres the data on its column means;
//! 2. forms the `dim × dim` sample covariance once (f64);
//! 3. finds the top three eigenvectors by power iteration, deflating the
//!    covariance (`C ← C − λ v vᵀ`) after each one;
//! 4. projects the **centred original rows** onto those axes;
//! 5. scales every coordinate by one factor so the 99th percentile of
//!    `|coordinate|` is [`TARGET_P99`], keeping outliers from squashing the
//!    cloud.
//!
//! Each axis's sign is fixed so its largest-magnitude loading is positive,
//! which keeps successive snapshots from mirroring at random.

use thiserror::Error;

/// Target value of the 99th-percentile absolute coordinate after scaling.
pub const TARGET_P99: f64 = 100.0;

const MAX_ITERATIONS: usize = 1000;
const CONVERGENCE: f64 = 1e-10;

/// Input that cannot be projected.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PcaError {
    /// `dim` was zero.
    #[error("dimension must be positive")]
    ZeroDimension,
    /// The flat input length is not a multiple of `dim`.
    #[error("input length {len} is not a multiple of dimension {dim}")]
    Ragged {
        /// Flat input length.
        len: usize,
        /// Declared dimension.
        dim: usize,
    },
}

/// Result of [`project_to_3d`].
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    /// `3 * n` coordinates, row-major (`x, y, z` per input row).
    pub positions: Vec<f32>,
    /// Unit principal axes, each of length `dim` (all-zero when the data
    /// has fewer than three directions of variance).
    pub axes: [Vec<f64>; 3],
    /// Variance captured by each axis, descending.
    pub variances: [f64; 3],
    /// Factor applied to the raw projected coordinates.
    pub scale: f64,
}

/// Project `n` row-major vectors of length `dim` onto their top three
/// principal components.
///
/// ```
/// use visionclaw_memory_cloud::pca::project_to_3d;
/// // Points spread along x far more than along y.
/// let rows = [10.0_f32, 0.0, -10.0, 1.0, 5.0, -1.0, -5.0, 0.0];
/// let p = project_to_3d(&rows, 2).unwrap();
/// assert!(p.axes[0][0].abs() > 0.99);
/// assert!(p.variances[0] > p.variances[1]);
/// ```
pub fn project_to_3d(rows: &[f32], dim: usize) -> Result<Projection, PcaError> {
    if dim == 0 {
        return Err(PcaError::ZeroDimension);
    }
    if !rows.len().is_multiple_of(dim) {
        return Err(PcaError::Ragged {
            len: rows.len(),
            dim,
        });
    }
    let n = rows.len() / dim;

    let mut mean = vec![0.0f64; dim];
    for row in rows.chunks_exact(dim) {
        for (m, &x) in mean.iter_mut().zip(row) {
            *m += f64::from(x);
        }
    }
    if n > 0 {
        for m in &mut mean {
            *m /= n as f64;
        }
    }
    let centred: Vec<f64> = rows
        .chunks_exact(dim)
        .flat_map(|row| row.iter().zip(&mean).map(|(&x, m)| f64::from(x) - m))
        .collect();

    // Upper triangle of XᵀX, then mirrored and divided by (n − 1).
    let mut cov = vec![0.0f64; dim * dim];
    for row in centred.chunks_exact(dim) {
        for i in 0..dim {
            let xi = row[i];
            if xi == 0.0 {
                continue;
            }
            let dst = &mut cov[i * dim + i..(i + 1) * dim];
            for (c, &xj) in dst.iter_mut().zip(&row[i..]) {
                *c += xi * xj;
            }
        }
    }
    let denom = n.saturating_sub(1).max(1) as f64;
    for i in 0..dim {
        for j in i..dim {
            let v = cov[i * dim + j] / denom;
            cov[i * dim + j] = v;
            cov[j * dim + i] = v;
        }
    }

    let mut axes: [Vec<f64>; 3] = [vec![0.0; dim], vec![0.0; dim], vec![0.0; dim]];
    let mut variances = [0.0f64; 3];
    for k in 0..3 {
        match leading_eigenpair(&cov, dim, &axes[..k]) {
            Some((lambda, v)) => {
                // Deflate: C ← C − λ v vᵀ.
                for i in 0..dim {
                    for j in 0..dim {
                        cov[i * dim + j] -= lambda * v[i] * v[j];
                    }
                }
                variances[k] = lambda;
                axes[k] = v;
            }
            None => break,
        }
    }

    let mut raw = Vec::with_capacity(n * 3);
    for row in centred.chunks_exact(dim) {
        for axis in &axes {
            raw.push(row.iter().zip(axis).map(|(x, a)| x * a).sum::<f64>());
        }
    }
    let scale = match percentile_abs(&raw, 0.99) {
        Some(p) if p > f64::EPSILON => TARGET_P99 / p,
        _ => 1.0,
    };
    let positions = raw.iter().map(|&c| (c * scale) as f32).collect();

    Ok(Projection {
        positions,
        axes,
        variances,
        scale,
    })
}

/// Leading eigenpair of the symmetric PSD matrix `m` by power iteration,
/// starting from a fixed vector orthogonalised against `previous`. Returns
/// `None` when the remaining spectrum is numerically zero.
fn leading_eigenpair(m: &[f64], dim: usize, previous: &[Vec<f64>]) -> Option<(f64, Vec<f64>)> {
    // Deterministic, non-degenerate start: no symmetric pattern that could be
    // orthogonal to a structured eigenvector.
    let mut v: Vec<f64> = (0..dim)
        .map(|i| 1.0 + ((i as f64 + 1.0) * 0.618_033_988_75).fract())
        .collect();
    orthogonalise(&mut v, previous);
    normalise(&mut v)?;

    let mut w = vec![0.0f64; dim];
    let mut lambda = 0.0;
    for _ in 0..MAX_ITERATIONS {
        for (i, wi) in w.iter_mut().enumerate() {
            *wi = m[i * dim..(i + 1) * dim]
                .iter()
                .zip(&v)
                .map(|(a, b)| a * b)
                .sum();
        }
        // Deflation removes the earlier axes analytically; re-orthogonalising
        // only scrubs accumulated rounding error.
        orthogonalise(&mut w, previous);
        lambda = v.iter().zip(&w).map(|(a, b)| a * b).sum();
        let norm = normalise(&mut w);
        if norm.is_none() || lambda <= f64::EPSILON {
            return None;
        }
        let delta: f64 = v
            .iter()
            .zip(&w)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            .sqrt();
        std::mem::swap(&mut v, &mut w);
        if delta < CONVERGENCE {
            break;
        }
    }

    let pivot = v
        .iter()
        .copied()
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .unwrap_or(0.0);
    if pivot < 0.0 {
        for x in &mut v {
            *x = -*x;
        }
    }
    Some((lambda, v))
}

fn orthogonalise(v: &mut [f64], basis: &[Vec<f64>]) {
    for b in basis {
        let d: f64 = v.iter().zip(b).map(|(x, y)| x * y).sum();
        for (x, y) in v.iter_mut().zip(b) {
            *x -= d * y;
        }
    }
}

fn normalise(v: &mut [f64]) -> Option<f64> {
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm <= 1e-12 || !norm.is_finite() {
        return None;
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
    Some(norm)
}

/// The `q`-quantile of `|values|` (nearest-rank), or `None` when empty.
fn percentile_abs(values: &[f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut abs: Vec<f64> = values.iter().map(|v| v.abs()).collect();
    let rank = ((q * abs.len() as f64).ceil() as usize).clamp(1, abs.len()) - 1;
    let (_, nth, _) = abs.select_nth_unstable_by(rank, f64::total_cmp);
    Some(*nth)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic generator (SplitMix64) so tests need no `rand`.
    struct Rng(u64);
    impl Rng {
        fn next_f64(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        }
        /// Approximately standard normal (Irwin–Hall, 12 uniforms).
        fn normal(&mut self) -> f64 {
            (0..12).map(|_| self.next_f64()).sum::<f64>() - 6.0
        }
    }

    fn unit(mut v: Vec<f64>) -> Vec<f64> {
        normalise(&mut v).unwrap();
        v
    }

    /// Data with standard deviations 10, 5, 2 along three known orthonormal
    /// directions in 16-D, small isotropic noise, and a large mean offset.
    fn anisotropic(n: usize) -> (Vec<f32>, [Vec<f64>; 3]) {
        let dim = 16;
        let a = unit((0..dim).map(|i| if i < 4 { 1.0 } else { 0.0 }).collect());
        let b = unit(
            (0..dim)
                .map(|i| match i {
                    0 | 1 => 1.0,
                    2 | 3 => -1.0,
                    _ => 0.0,
                })
                .collect(),
        );
        let c = unit((0..dim).map(|i| if i == 9 { 1.0 } else { 0.0 }).collect());
        let mut rng = Rng(7);
        let mut rows = Vec::with_capacity(n * dim);
        for _ in 0..n {
            let (s1, s2, s3) = (10.0 * rng.normal(), 5.0 * rng.normal(), 2.0 * rng.normal());
            for i in 0..dim {
                let x = 50.0 + s1 * a[i] + s2 * b[i] + s3 * c[i] + 0.05 * rng.normal();
                rows.push(x as f32);
            }
        }
        (rows, [a, b, c])
    }

    fn dotf(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn recovers_known_axes_in_variance_order() {
        let (rows, truth) = anisotropic(4000);
        let p = project_to_3d(&rows, 16).unwrap();
        for (k, (axis, want)) in p.axes.iter().zip(&truth).enumerate() {
            let align = dotf(axis, want).abs();
            assert!(align > 0.995, "axis {k} alignment {align}");
        }
        assert!(p.variances[0] > p.variances[1] && p.variances[1] > p.variances[2]);
        // Variances near 100, 25, 4.
        assert!(
            (p.variances[0] / 100.0 - 1.0).abs() < 0.1,
            "{:?}",
            p.variances
        );
        assert!(
            (p.variances[1] / 25.0 - 1.0).abs() < 0.1,
            "{:?}",
            p.variances
        );
        assert!(
            (p.variances[2] / 4.0 - 1.0).abs() < 0.15,
            "{:?}",
            p.variances
        );
    }

    #[test]
    fn axes_are_orthonormal() {
        let (rows, _) = anisotropic(1000);
        let p = project_to_3d(&rows, 16).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                let d = dotf(&p.axes[i], &p.axes[j]);
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((d - want).abs() < 1e-8, "axes {i},{j}: {d}");
            }
        }
    }

    #[test]
    fn positions_are_centred_and_scaled_to_p99() {
        let (rows, _) = anisotropic(2000);
        let p = project_to_3d(&rows, 16).unwrap();
        assert_eq!(p.positions.len(), 2000 * 3);
        for axis in 0..3 {
            let mean: f64 = p
                .positions
                .iter()
                .skip(axis)
                .step_by(3)
                .map(|&v| f64::from(v))
                .sum::<f64>()
                / 2000.0;
            assert!(mean.abs() < 1e-3, "axis {axis} mean {mean}");
        }
        let all: Vec<f64> = p.positions.iter().map(|&v| f64::from(v)).collect();
        let p99 = percentile_abs(&all, 0.99).unwrap();
        assert!((p99 - 100.0).abs() < 1e-3, "p99 {p99}");
    }

    #[test]
    fn mean_offset_does_not_change_the_projection() {
        let (rows, _) = anisotropic(500);
        let shifted: Vec<f32> = rows.iter().map(|v| v + 1000.0).collect();
        let a = project_to_3d(&rows, 16).unwrap();
        let b = project_to_3d(&shifted, 16).unwrap();
        for (x, y) in a.positions.iter().zip(&b.positions) {
            assert!((x - y).abs() < 0.05, "{x} vs {y}");
        }
    }

    #[test]
    fn sign_convention_is_stable() {
        let (rows, _) = anisotropic(800);
        let p = project_to_3d(&rows, 16).unwrap();
        for axis in &p.axes {
            let pivot = axis
                .iter()
                .copied()
                .max_by(|a, b| a.abs().total_cmp(&b.abs()))
                .unwrap();
            assert!(pivot > 0.0);
        }
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert_eq!(project_to_3d(&[], 4).unwrap().positions, Vec::<f32>::new());
        let single = project_to_3d(&[1.0, 2.0, 3.0], 3).unwrap();
        assert_eq!(single.positions, vec![0.0, 0.0, 0.0]);
        // Collinear data: one real axis, the others zero.
        let line = project_to_3d(&[0.0, 0.0, 1.0, 1.0, 2.0, 2.0], 2).unwrap();
        assert!(line.variances[0] > 0.0);
        assert_eq!(line.variances[1], 0.0);
        assert!(line.positions.iter().all(|v| v.is_finite()));
        assert_eq!(project_to_3d(&[1.0], 0), Err(PcaError::ZeroDimension));
        assert_eq!(
            project_to_3d(&[1.0, 2.0, 3.0], 2),
            Err(PcaError::Ragged { len: 3, dim: 2 })
        );
    }
}
