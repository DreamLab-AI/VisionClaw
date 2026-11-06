/**
 * The analytic colour formulas, written once (VC-31 / VC-56 parallel colour
 * formulas): node encoding (knowledge nodes), GemNodes (ontology and agent
 * nodes), the column encoder and its scheme legend all read these, so a hue
 * on screen and its legend swatch cannot drift. HSL values are 0–1, as
 * `THREE.Color.setHSL` takes them.
 */
export type Hsl = readonly [h: number, s: number, l: number];
export type Rgb = readonly [r: number, g: number, b: number];

/** Louvain community (wire offset 44): a deterministic hue per community id. */
export function communityHsl(communityId: number): Hsl {
  return [((communityId * 83) % 360) / 360, 0.65, 0.5];
}

/** Cluster (wire offset 36): a golden-angle hue per cluster id. */
export function clusterHsl(clusterId: number): Hsl {
  return [((clusterId * 137) % 360) / 360, 0.7, 0.55];
}

/** Centrality ramp at `t` = centrality / live maximum, clamped to [0, 1]: blue → cyan → yellow. */
export function centralityHsl(t: number): Hsl {
  const u = t < 0 ? 0 : t > 1 ? 1 : t;
  return [0.62 - 0.62 * u, 0.85, 0.25 + 0.4 * u];
}

/** Anomaly overlay: red rising with the score (clamped to 1). */
export function anomalyRgb(anomalyScore: number): Rgb {
  const t = Math.min(anomalyScore, 1.0);
  return [0.9 * t + 0.1, 0.15 * (1 - t), 0.1];
}
