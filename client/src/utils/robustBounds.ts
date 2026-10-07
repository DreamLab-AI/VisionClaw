/**
 * Outlier-tolerant bounds of a flat `[x, y, z, …]` position buffer.
 *
 * The live graph layout carries a handful of far outliers (observed |p| up to
 * ~3000 against a ~400 core), and a raw min/max box fits those, shrinking the
 * visible graph to nothing. The box here spans the 5th–95th percentile on each
 * axis instead. Shared by the camera auto-fit (useCameraAutoFit) and the
 * memory cloud, which frames itself on the same graph extent.
 */

export interface RobustBounds {
  /** centre of the percentile box */
  centre: [number, number, number];
  /** half the box diagonal, floored at 1 */
  radius: number;
}

/**
 * Bounds of the first `count` rows of `positions`. Rows with a non-finite
 * coordinate are skipped. Returns null when no finite row remains. Sorting
 * three axis arrays is O(n log n): call it at fit or 1 Hz frequency, not
 * per frame.
 */
export function robustBounds(
  positions: ArrayLike<number>,
  count: number,
  lowFraction = 0.05,
  highFraction = 0.95,
): RobustBounds | null {
  const rows = Math.max(0, Math.min(Math.floor(count), Math.floor(positions.length / 3)));
  if (rows === 0) return null;
  const xs = new Float64Array(rows);
  const ys = new Float64Array(rows);
  const zs = new Float64Array(rows);
  let n = 0;
  for (let i = 0; i < rows; i++) {
    const x = positions[i * 3];
    const y = positions[i * 3 + 1];
    const z = positions[i * 3 + 2];
    if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) continue;
    xs[n] = x;
    ys[n] = y;
    zs[n] = z;
    n++;
  }
  if (n === 0) return null;
  const ax = [xs.subarray(0, n).sort(), ys.subarray(0, n).sort(), zs.subarray(0, n).sort()];
  const lo = Math.floor(n * lowFraction);
  const hi = Math.max(lo, Math.ceil(n * highFraction) - 1);
  const centre = ax.map((a) => (a[lo] + a[hi]) / 2) as [number, number, number];
  const diag = Math.hypot(ax[0][hi] - ax[0][lo], ax[1][hi] - ax[1][lo], ax[2][hi] - ax[2][lo]);
  return { centre, radius: Math.max(0.5 * diag, 1) };
}
