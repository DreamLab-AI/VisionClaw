//! Where the memory cloud sits relative to the graph (desktop parity).
//!
//! Port of `client/src/utils/robustBounds.ts` and
//! `client/src/features/visualisation/memoryCloud/cloudFrame.ts`, plus the
//! placement loop of `EmbeddingCloudLayer.tsx`:
//!
//! - the graph's extent is the 5th–95th percentile box of its node positions
//!   ([`robust_bounds`]), so a few far outliers cannot shrink the fit;
//! - an outer node sits at the graph's robust centre and scales the cloud's
//!   robust radius to the graph's; an inner node shifts the cloud so its own
//!   robust centre is the pivot ([`cloud_placement`]). `cloud_scale` keeps its
//!   desktop meaning: the default 5 makes the two radii equal, linear from
//!   there;
//! - the graph extent is re-read once a second while physics moves it, and the
//!   cloud glides to each new placement over ~0.8 s (snapping on the first
//!   placement and under reduced motion) so it never jitters
//!   ([`PlacementGlide`]).
//!
//! - with Graph Separation > 0 (ADR-2135) the graph extent is measured on
//!   positions folded back into each graph's own frame ([`graph_bounds_for`])
//!   and the cloud moves to the triangle's memory vertex, the shared
//!   `visionclaw-tri-layout` crate the server projects with.
//!
//! In the headset the layer lives under `GraphRoot`, whose space is the
//! server's, so the desktop's world placement applies unchanged.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use visionclaw_tri_layout::{TriangleFrame, Vertex};

/// `cloudFrame.ts` `DEFAULT_CLOUD_SCALE`: the `cloudScale` that fits the cloud
/// radius to the graph radius.
pub const DEFAULT_CLOUD_SCALE: f32 = 5.0;
/// Smallest accepted `cloudScale` (`Math.max(0.1, cloudScale)`).
pub const MIN_CLOUD_SCALE: f32 = 0.1;
/// Percentile box of [`robust_bounds`] (`robustBounds.ts` defaults).
pub const LOW_FRACTION: f64 = 0.05;
pub const HIGH_FRACTION: f64 = 0.95;
/// Floor on a robust radius (`Math.max(0.5 * diag, 1)`).
pub const MIN_RADIUS: f32 = 1.0;
/// Seconds between re-reads of the graph's extent (`GRAPH_BOUNDS_EVERY`).
pub const GRAPH_BOUNDS_EVERY_S: f32 = 1.0;
/// Seconds for the cloud to glide to a new placement (`PLACE_GLIDE`).
pub const PLACE_GLIDE_S: f32 = 0.8;
/// Smallest world point size (`cloudPointSize`'s `Math.max(0.5, …)`).
pub const MIN_POINT_SIZE: f32 = 0.5;

pub type Vec3 = [f32; 3];

/// Outlier-tolerant bounds: centre of the per-axis percentile box and half its
/// diagonal (floored at [`MIN_RADIUS`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobustBounds {
    pub centre: Vec3,
    pub radius: f32,
}

/// Bounds of `points` over the 5th–95th percentile on each axis
/// (`robustBounds.ts`). Points with a non-finite coordinate are skipped; `None`
/// when no finite point remains. The order statistics are the TS ones —
/// `lo = floor(n · 0.05)`, `hi = max(lo, ceil(n · 0.95) − 1)` — found by
/// selection rather than a full sort (same values, O(n)).
pub fn robust_bounds<I>(points: I) -> Option<RobustBounds>
where
    I: IntoIterator<Item = Vec3>,
{
    let mut ax: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for p in points {
        if p.iter().all(|c| c.is_finite()) {
            for k in 0..3 {
                ax[k].push(p[k] as f64);
            }
        }
    }
    let n = ax[0].len();
    if n == 0 {
        return None;
    }
    let lo = (n as f64 * LOW_FRACTION).floor() as usize;
    let hi = lo
        .max(((n as f64 * HIGH_FRACTION).ceil() as usize).saturating_sub(1))
        .min(n - 1);
    let mut centre = [0.0f32; 3];
    let mut d2 = 0.0f64;
    for k in 0..3 {
        let a = &mut ax[k];
        let (_, &mut hv, _) = a.select_nth_unstable_by(hi, f64::total_cmp);
        let (_, &mut lv, _) = a[..=hi].select_nth_unstable_by(lo, f64::total_cmp);
        centre[k] = ((lv + hv) / 2.0) as f32;
        d2 += (hv - lv) * (hv - lv);
    }
    Some(RobustBounds {
        centre,
        radius: ((0.5 * d2.sqrt()) as f32).max(MIN_RADIUS),
    })
}

/// [`robust_bounds`] over a flat `[x, y, z, …]` buffer (the snapshot layout).
pub fn robust_bounds_flat(flat: &[f32]) -> Option<RobustBounds> {
    robust_bounds(flat.as_chunks::<3>().0.iter().copied())
}

/// Outer node position and uniform scale, inner node offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CloudPlacement {
    /// Outer node position: the graph's centre (GraphRoot space).
    pub position: Vec3,
    /// Outer node uniform scale.
    pub scale: f32,
    /// Inner node position: minus the cloud's own centre (cloud-local).
    pub offset: Vec3,
}

/// The `cloudScale` actually applied (`Number.isFinite` guard, 0.1 floor).
pub fn effective_cloud_scale(cloud_scale: f32) -> f32 {
    if cloud_scale.is_finite() {
        cloud_scale.max(MIN_CLOUD_SCALE)
    } else {
        DEFAULT_CLOUD_SCALE
    }
}

/// `cloudFrame.ts` `cloudPlacement`. Without a graph (or a cloud) the cloud
/// keeps the old placement: server origin, scale `cloud_scale`. Either way it
/// is moved by the memory vertex of the separated-layout triangle
/// (ADR-2135), which is the origin at separation 0.
pub fn cloud_placement(
    cloud: Option<RobustBounds>,
    graph: Option<RobustBounds>,
    cloud_scale: f32,
    separation: f32,
) -> CloudPlacement {
    let k = effective_cloud_scale(cloud_scale);
    let offset = cloud.map_or([0.0; 3], |c| [-c.centre[0], -c.centre[1], -c.centre[2]]);
    let m = TriangleFrame::new(separation).vertex(Vertex::Memory);
    match (cloud, graph) {
        (Some(c), Some(g)) => CloudPlacement {
            position: [g.centre[0] + m[0], g.centre[1] + m[1], g.centre[2] + m[2]],
            scale: (k / DEFAULT_CLOUD_SCALE) * (g.radius / c.radius),
            offset,
        },
        _ => CloudPlacement {
            position: m,
            scale: k,
            offset,
        },
    }
}

/// `cloudFrame.ts` `graphBoundsFor`: robust bounds of graph positions with
/// the separated layout folded out (each point mapped back into the frame of
/// its nearest graph vertex), so the result is one graph's extent rather
/// than the whole triangle's. Plain [`robust_bounds`] at separation 0.
pub fn graph_bounds_for<I>(points: I, separation: f32) -> Option<RobustBounds>
where
    I: IntoIterator<Item = Vec3>,
{
    let frame = TriangleFrame::new(separation);
    robust_bounds(points.into_iter().map(|p| frame.fold(p)))
}

/// `cloudFrame.ts` `cloudPointSize`: the desktop's world point size for a
/// placement scale (size attenuation ignores object scale, so the size follows
/// the placement; floored at [`MIN_POINT_SIZE`]).
pub fn cloud_point_size(point_size: f32, placement_scale: f32) -> f32 {
    (point_size * placement_scale / DEFAULT_CLOUD_SCALE).max(MIN_POINT_SIZE)
}

/// Sprite diameter in cloud-local units for a placement scale: the desktop
/// world point size (`cloud_point_size`) as a world diameter
/// (`size · tan(fov/2)`, `memory_cloud::sprite_local_size`) divided by the
/// placement scale. Above the floor this is independent of the scale, exactly
/// as on the desktop.
pub fn sprite_local_for_scale(point_size: f32, fov_deg: f32, placement_scale: f32) -> f32 {
    let half = (fov_deg.to_radians() * 0.5).tan();
    cloud_point_size(point_size, placement_scale) * half / placement_scale.max(1e-6)
}

/// The desktop placement loop: re-read the graph at 1 Hz, glide to the target.
#[derive(Debug, Clone)]
pub struct PlacementGlide {
    /// Cloud bounds in cloud-local units (set when a snapshot loads).
    pub cloud: Option<RobustBounds>,
    /// Last graph bounds read.
    pub graph: Option<RobustBounds>,
    /// Graph Separation slider (ADR-2135); 0 = merged.
    pub separation: f32,
    since_read: f32,
    placed: bool,
    position: Vec3,
    scale: f32,
}

impl Default for PlacementGlide {
    fn default() -> Self {
        PlacementGlide {
            cloud: None,
            graph: None,
            separation: 0.0,
            since_read: f32::INFINITY,
            placed: false,
            position: [0.0; 3],
            scale: DEFAULT_CLOUD_SCALE,
        }
    }
}

impl PlacementGlide {
    /// Advance the read clock by `dt`; true when the graph extent should be
    /// re-read now (at once on the first call, then every
    /// [`GRAPH_BOUNDS_EVERY_S`]).
    pub fn read_due(&mut self, dt: f32) -> bool {
        self.since_read += dt.max(0.0);
        if self.since_read >= GRAPH_BOUNDS_EVERY_S {
            self.since_read = 0.0;
            true
        } else {
            false
        }
    }

    /// A new snapshot: frame on its bounds. Snaps to the new placement on the
    /// next step (a different sample is a new cloud, not a move).
    pub fn set_cloud(&mut self, cloud: Option<RobustBounds>) {
        self.cloud = cloud;
        self.placed = false;
    }

    pub fn set_graph(&mut self, graph: Option<RobustBounds>) {
        self.graph = graph;
    }

    /// The Graph Separation slider. A change makes the graph read due at
    /// once (the bounds fold depends on it), as the desktop layer does.
    pub fn set_separation(&mut self, separation: f32) {
        let s = if separation.is_finite() {
            separation.max(0.0)
        } else {
            0.0
        };
        if s != self.separation {
            self.separation = s;
            self.since_read = f32::INFINITY;
        }
    }

    /// One frame: the smoothed placement. The outer position and scale glide
    /// by `min(1, dt / PLACE_GLIDE_S)` towards the target; the first placement
    /// and reduced motion snap. The inner offset is applied at once (it only
    /// changes with the snapshot).
    pub fn step(&mut self, dt: f32, cloud_scale: f32, reduced_motion: bool) -> CloudPlacement {
        let target = cloud_placement(self.cloud, self.graph, cloud_scale, self.separation);
        let f = if !self.placed || reduced_motion {
            1.0
        } else {
            (dt.max(0.0) / PLACE_GLIDE_S).min(1.0)
        };
        for k in 0..3 {
            self.position[k] += (target.position[k] - self.position[k]) * f;
        }
        self.scale += (target.scale - self.scale) * f;
        self.placed = true;
        CloudPlacement {
            position: self.position,
            scale: self.scale,
            offset: target.offset,
        }
    }
}

// ── Godot adapter ──

#[cfg(not(test))]
mod godot_api_impl {
    use super::*;
    use godot::prelude::*;

    /// GDScript handle on a [`PlacementGlide`] (`memory_cloud_layer.gd`).
    #[derive(GodotClass)]
    #[class(no_init, base = RefCounted)]
    pub struct CloudFrame {
        glide: PlacementGlide,
        base: Base<RefCounted>,
    }

    #[godot_api]
    impl CloudFrame {
        #[func]
        fn create() -> Gd<Self> {
            Gd::from_init_fn(|base| Self {
                glide: PlacementGlide::default(),
                base,
            })
        }

        /// Frame a newly loaded snapshot (its flat 3·count positions).
        #[func]
        fn set_cloud_positions(&mut self, positions: PackedFloat32Array) {
            self.glide
                .set_cloud(robust_bounds_flat(positions.as_slice()));
        }

        /// True when the graph extent should be re-read this frame (1 Hz).
        #[func]
        fn graph_read_due(&mut self, dt: f32) -> bool {
            self.glide.read_due(dt)
        }

        /// The Graph Separation slider (ADR-2135); the next graph read is
        /// due at once when it changes.
        #[func]
        fn set_separation(&mut self, separation: f32) {
            self.glide.set_separation(separation);
        }

        /// Graph bounds as `[cx, cy, cz, radius]` (from
        /// `BinaryProtocolClient.graph_robust_bounds(separation)`); an empty array means
        /// no graph, and the cloud falls back to the origin placement.
        #[func]
        fn set_graph_bounds(&mut self, b: PackedFloat32Array) {
            let s = b.as_slice();
            self.glide.set_graph(
                (s.len() == 4 && s.iter().all(|v| v.is_finite()) && s[3] > 0.0).then(|| {
                    RobustBounds {
                        centre: [s[0], s[1], s[2]],
                        radius: s[3],
                    }
                }),
            );
        }

        /// One frame of placement: `{position: Vector3, scale: float,
        /// offset: Vector3, sprite: float}` (sprite = cloud-local diameter
        /// for the desktop default point size).
        #[func]
        fn step(&mut self, dt: f32, cloud_scale: f32, reduced_motion: bool) -> Dictionary {
            let p = self.glide.step(dt, cloud_scale, reduced_motion);
            let mut d = Dictionary::new();
            d.set(
                "position",
                Vector3::new(p.position[0], p.position[1], p.position[2]),
            );
            d.set("scale", p.scale);
            d.set(
                "offset",
                Vector3::new(p.offset[0], p.offset[1], p.offset[2]),
            );
            d.set(
                "sprite",
                sprite_local_for_scale(
                    crate::memory_cloud::DESKTOP_POINT_SIZE,
                    crate::memory_cloud::DESKTOP_FOV_DEG,
                    p.scale,
                ),
            );
            d
        }

        /// `[cx, cy, cz, radius]` of the last graph read, empty when none.
        #[func]
        fn graph_bounds(&self) -> PackedFloat32Array {
            self.glide.graph.map_or(PackedFloat32Array::new(), |g| {
                PackedFloat32Array::from(&[g.centre[0], g.centre[1], g.centre[2], g.radius][..])
            })
        }

        /// `[cx, cy, cz, radius]` of the cloud (cloud-local), empty when none.
        #[func]
        fn cloud_bounds(&self) -> PackedFloat32Array {
            self.glide.cloud.map_or(PackedFloat32Array::new(), |g| {
                PackedFloat32Array::from(&[g.centre[0], g.centre[1], g.centre[2], g.radius][..])
            })
        }

        #[func]
        fn default_cloud_scale(&self) -> f32 {
            DEFAULT_CLOUD_SCALE
        }
    }
}

#[cfg(not(test))]
pub use godot_api_impl::CloudFrame;

#[cfg(test)]
mod tests {
    use super::*;

    fn ts_source(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// Number literal following `needle` in `src`.
    fn literal_after(src: &str, needle: &str) -> f32 {
        let at = src
            .find(needle)
            .unwrap_or_else(|| panic!("`{needle}` not found"))
            + needle.len();
        let lit: String = src[at..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        lit.parse()
            .unwrap_or_else(|_| panic!("no number after `{needle}`"))
    }

    // ── parity with the desktop source ──

    #[test]
    fn constants_match_cloud_frame_ts_and_the_layer() {
        let frame = ts_source("client/src/features/visualisation/memoryCloud/cloudFrame.ts");
        assert_eq!(
            literal_after(&frame, "export const DEFAULT_CLOUD_SCALE = "),
            DEFAULT_CLOUD_SCALE
        );
        assert!(
            frame.contains("Math.max(0.1, cloudScale)"),
            "cloudScale floor"
        );
        assert_eq!(MIN_CLOUD_SCALE, 0.1);
        assert!(
            frame.contains("scale: (k / DEFAULT_CLOUD_SCALE) * (graph.radius / cloud.radius)"),
            "placement scale formula"
        );
        assert!(
            frame.contains(
                "position: [graph.centre[0] + m[0], graph.centre[1] + m[1], graph.centre[2] + m[2]]"
            ),
            "outer node at the graph centre plus the memory vertex"
        );
        assert!(
            frame.contains("const m = triangleFrame(separation).vertices[Vertex.Memory];"),
            "memory vertex from the shared triangle"
        );
        assert!(
            frame.contains(
                "return robustBounds(foldPositions(triangleFrame(separation), positions, count), count);"
            ),
            "graphBoundsFor folds before measuring"
        );
        assert!(
            frame.contains("[-cloud.centre[0], -cloud.centre[1], -cloud.centre[2]]"),
            "inner offset"
        );
        assert!(
            frame.contains("return { position: [m[0], m[1], m[2]], scale: k, offset };"),
            "no-graph fallback"
        );
        assert!(
            frame.contains("Math.max(0.5, (pointSize * placementScale) / DEFAULT_CLOUD_SCALE)"),
            "cloudPointSize"
        );
        assert_eq!(MIN_POINT_SIZE, 0.5);

        let layer =
            ts_source("client/src/features/visualisation/components/EmbeddingCloudLayer.tsx");
        assert_eq!(
            literal_after(&layer, "const GRAPH_BOUNDS_EVERY = "),
            GRAPH_BOUNDS_EVERY_S
        );
        assert_eq!(literal_after(&layer, "const PLACE_GLIDE = "), PLACE_GLIDE_S);
        assert!(
            layer.contains(
                "const f = !ps.placed || reducedMotion ? 1 : Math.min(1, dt / PLACE_GLIDE);"
            ),
            "glide factor: snap first and under reduced motion"
        );

        let rb = ts_source("client/src/utils/robustBounds.ts");
        assert_eq!(literal_after(&rb, "lowFraction = "), LOW_FRACTION as f32);
        assert_eq!(literal_after(&rb, "highFraction = "), HIGH_FRACTION as f32);
        assert!(rb.contains("const lo = Math.floor(n * lowFraction);"));
        assert!(rb.contains("const hi = Math.max(lo, Math.ceil(n * highFraction) - 1);"));
        assert!(rb.contains("radius: Math.max(0.5 * diag, 1)"));
        assert!(rb.contains("Number.isFinite(x)"), "non-finite rows skipped");
    }

    /// The memory_cloud default scale and this module's must be one value.
    #[test]
    fn default_scale_is_the_memory_cloud_one() {
        assert_eq!(
            DEFAULT_CLOUD_SCALE,
            crate::memory_cloud::DESKTOP_CLOUD_SCALE
        );
    }

    // ── robust bounds ──

    /// Reference implementation, line for line from robustBounds.ts (full sort).
    fn reference(points: &[Vec3]) -> Option<RobustBounds> {
        let rows: Vec<&Vec3> = points
            .iter()
            .filter(|p| p.iter().all(|c| c.is_finite()))
            .collect();
        let n = rows.len();
        if n == 0 {
            return None;
        }
        let mut ax: Vec<Vec<f64>> = (0..3)
            .map(|k| rows.iter().map(|p| p[k] as f64).collect())
            .collect();
        for a in &mut ax {
            a.sort_by(f64::total_cmp);
        }
        let lo = (n as f64 * 0.05).floor() as usize;
        let hi = lo.max((n as f64 * 0.95).ceil() as usize - 1);
        let centre = [0, 1, 2].map(|k| ((ax[k][lo] + ax[k][hi]) / 2.0) as f32);
        let diag = (0..3)
            .map(|k| (ax[k][hi] - ax[k][lo]).powi(2))
            .sum::<f64>()
            .sqrt();
        Some(RobustBounds {
            centre,
            radius: ((0.5 * diag) as f32).max(1.0),
        })
    }

    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*seed >> 33) as f64 / (1u64 << 31) as f64) as f32
    }

    #[test]
    fn selection_matches_the_sorted_reference_at_every_size() {
        let mut seed = 7u64;
        for n in [1usize, 2, 3, 19, 20, 21, 100, 1_000, 13_164] {
            let pts: Vec<Vec3> = (0..n)
                .map(|_| {
                    [
                        lcg(&mut seed) * 800.0 - 300.0,
                        lcg(&mut seed) * 50.0,
                        lcg(&mut seed) * -900.0,
                    ]
                })
                .collect();
            assert_eq!(
                robust_bounds(pts.iter().copied()),
                reference(&pts),
                "n = {n}"
            );
        }
    }

    #[test]
    fn far_outliers_do_not_move_the_fit() {
        // a 400-wide core centred on (90, -3, -14) plus a handful at |p| ~ 3000
        let mut seed = 11u64;
        let mut pts: Vec<Vec3> = (0..2_000)
            .map(|_| {
                [
                    90.0 + lcg(&mut seed) * 400.0 - 200.0,
                    -3.0 + lcg(&mut seed) * 400.0 - 200.0,
                    -14.0 + lcg(&mut seed) * 400.0 - 200.0,
                ]
            })
            .collect();
        let core = robust_bounds(pts.iter().copied()).unwrap();
        for i in 0..20 {
            pts.push([
                3_000.0 * if i % 2 == 0 { 1.0 } else { -1.0 },
                2_900.0,
                -3_100.0,
            ]);
        }
        let b = robust_bounds(pts.iter().copied()).unwrap();
        for k in 0..3 {
            assert!(
                (b.centre[k] - core.centre[k]).abs() < 25.0,
                "axis {k}: {b:?} vs {core:?}"
            );
        }
        assert!(
            (b.radius - core.radius).abs() / core.radius < 0.1,
            "{b:?} vs {core:?}"
        );
        assert!((b.centre[0] - 90.0).abs() < 25.0);
    }

    #[test]
    fn non_finite_rows_are_skipped_and_empty_is_none() {
        assert_eq!(robust_bounds(std::iter::empty()), None);
        assert_eq!(
            robust_bounds([[f32::NAN, 0.0, 0.0], [0.0, f32::INFINITY, 0.0]]),
            None
        );
        let b = robust_bounds([[f32::NAN, 0.0, 0.0], [4.0, 6.0, 8.0]]).unwrap();
        assert_eq!(b.centre, [4.0, 6.0, 8.0]);
        assert_eq!(b.radius, MIN_RADIUS, "a single point floors at 1");
        assert_eq!(
            robust_bounds_flat(&[1.0, 2.0, 3.0, 9.0]),
            robust_bounds([[1.0, 2.0, 3.0]]),
            "trailing partial row ignored"
        );
    }

    // ── placement ──

    fn rb(c: Vec3, r: f32) -> Option<RobustBounds> {
        Some(RobustBounds {
            centre: c,
            radius: r,
        })
    }

    #[test]
    fn default_scale_makes_the_radii_equal_and_scales_linearly() {
        let cloud = rb([10.0, -4.0, 2.0], 60.0);
        let graph = rb([90.0, -3.0, -14.0], 300.0);
        let p = cloud_placement(cloud, graph, DEFAULT_CLOUD_SCALE, 0.0);
        assert_eq!(p.position, [90.0, -3.0, -14.0]);
        assert_eq!(p.offset, [-10.0, 4.0, -2.0]);
        assert!(
            (p.scale * 60.0 - 300.0).abs() < 1e-3,
            "cloud radius × scale = graph radius"
        );
        let p2 = cloud_placement(cloud, graph, 10.0, 0.0);
        assert!(
            (p2.scale - 2.0 * p.scale).abs() < 1e-5,
            "linear in cloudScale"
        );
    }

    #[test]
    fn without_a_graph_the_old_placement_holds() {
        let p = cloud_placement(rb([10.0, 0.0, 0.0], 60.0), None, 5.0, 0.0);
        assert_eq!(
            (p.position, p.scale, p.offset),
            ([0.0; 3], 5.0, [-10.0, 0.0, 0.0])
        );
        let q = cloud_placement(None, rb([1.0, 1.0, 1.0], 3.0), 5.0, 0.0);
        assert_eq!((q.position, q.scale, q.offset), ([0.0; 3], 5.0, [0.0; 3]));
        assert_eq!(
            cloud_placement(None, None, f32::NAN, 0.0).scale,
            DEFAULT_CLOUD_SCALE
        );
        assert_eq!(cloud_placement(None, None, 0.0, 0.0).scale, MIN_CLOUD_SCALE);
    }

    #[test]
    fn sprite_size_is_scale_independent_above_the_floor() {
        let a = sprite_local_for_scale(7.5, 75.0, 5.0);
        assert!(
            (a - crate::memory_cloud::sprite_local_size(7.5, 75.0, 5.0)).abs() < 1e-6,
            "same as the old fixed ×5"
        );
        let b = sprite_local_for_scale(7.5, 75.0, 2.0);
        assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        // floor: pointSize · s / 5 < 0.5 → s < 1/3 for the default 7.5
        let tiny = sprite_local_for_scale(7.5, 75.0, 0.1);
        assert!(
            tiny > a,
            "floored sprites grow in cloud-local units as the cloud shrinks"
        );
    }

    // ── glide ──

    #[test]
    fn first_placement_snaps_then_glides_at_the_desktop_rate() {
        let mut g = PlacementGlide::default();
        g.set_cloud(rb([0.0; 3], 50.0));
        g.set_graph(rb([100.0, 0.0, 0.0], 250.0));
        let p = g.step(1.0 / 90.0, 5.0, false);
        assert_eq!(p.position, [100.0, 0.0, 0.0], "snap on the first placement");
        assert!((p.scale - 5.0).abs() < 1e-5);
        // physics moves the graph by 40 units: one 90 Hz frame moves 1/72 of it
        g.set_graph(rb([140.0, 0.0, 0.0], 250.0));
        let dt = 1.0 / 90.0;
        let q = g.step(dt, 5.0, false);
        assert!(
            (q.position[0] - (100.0 + 40.0 * dt / PLACE_GLIDE_S)).abs() < 1e-3,
            "{q:?}"
        );
        // exponential approach, time constant 0.8 s: within 0.1 after 8 s
        let mut last = q;
        for _ in 0..720 {
            last = g.step(dt, 5.0, false);
        }
        assert!((last.position[0] - 140.0).abs() < 0.1, "{last:?}");
    }

    #[test]
    fn reduced_motion_snaps_and_a_new_snapshot_snaps() {
        let mut g = PlacementGlide::default();
        g.set_cloud(rb([0.0; 3], 50.0));
        g.set_graph(rb([0.0; 3], 250.0));
        g.step(0.011, 5.0, false);
        g.set_graph(rb([60.0, 0.0, 0.0], 250.0));
        assert_eq!(
            g.step(0.011, 5.0, true).position[0],
            60.0,
            "reduced motion: no glide"
        );
        g.set_graph(rb([0.0; 3], 250.0));
        g.set_cloud(rb([5.0, 0.0, 0.0], 25.0));
        let p = g.step(0.011, 5.0, false);
        assert_eq!(p.position[0], 0.0, "new sample snaps");
        assert!((p.scale - 10.0).abs() < 1e-5);
    }

    #[test]
    fn the_user_scale_setting_is_kept_through_the_glide() {
        let mut g = PlacementGlide::default();
        g.set_cloud(rb([0.0; 3], 50.0));
        g.set_graph(rb([0.0; 3], 250.0));
        let p = g.step(0.011, 2.5, false);
        assert!(
            (p.scale - 2.5).abs() < 1e-5,
            "cloudScale 2.5 = half the graph radius"
        );
    }

    #[test]
    fn graph_reads_are_due_at_once_then_once_a_second() {
        let mut g = PlacementGlide::default();
        assert!(g.read_due(0.0), "first frame reads");
        let mut reads = 0;
        for _ in 0..450 {
            if g.read_due(1.0 / 90.0) {
                reads += 1;
            }
        }
        // 5 s at 90 Hz: one read per elapsed second (the 1/90 sum may land the
        // fifth exactly on the last frame or one frame later)
        assert!((4..=5).contains(&reads), "{reads} reads");
    }

    #[test]
    fn settling_physics_does_not_jitter_the_cloud() {
        // graph centre wobbles ±5 units every frame; cloud reads at 1 Hz and glides
        let mut g = PlacementGlide::default();
        g.set_cloud(rb([0.0; 3], 50.0));
        let mut seed = 3u64;
        let mut max_step = 0.0f32;
        let mut prev: Option<f32> = None;
        for _ in 0..900 {
            let wob = lcg(&mut seed) * 10.0 - 5.0;
            if g.read_due(1.0 / 90.0) {
                g.set_graph(rb([wob, 0.0, 0.0], 250.0));
            }
            let x = g.step(1.0 / 90.0, 5.0, false).position[0];
            if let Some(p) = prev {
                max_step = max_step.max((x - p).abs());
            }
            prev = Some(x);
        }
        // 10 units × (1/90)/0.8 ≈ 0.14 units per frame at worst
        assert!(max_step < 0.15, "per-frame move {max_step}");
    }

    // ── separated layout (ADR-2135) ──

    fn fixture() -> serde_json::Value {
        let raw = ts_source("crates/visionclaw-tri-layout/fixtures/tri_layout_fixture.json");
        serde_json::from_str(&raw).expect("fixture parses")
    }

    #[test]
    fn the_layer_rereads_bounds_when_the_slider_moves() {
        let layer =
            ts_source("client/src/features/visualisation/components/EmbeddingCloudLayer.tsx");
        assert!(layer
            .contains("if (ps.sinceRead >= GRAPH_BOUNDS_EVERY || ps.separation !== separation) {"));
        assert!(layer.contains(
            "const place = cloudPlacement(cloudBounds, ps.graph, cloudScale, separation);"
        ));
        let mut g = PlacementGlide::default();
        assert!(g.read_due(0.0));
        assert!(!g.read_due(0.1));
        g.set_separation(200.0);
        assert!(g.read_due(0.0), "a new separation re-reads at once");
        g.set_separation(200.0);
        assert!(!g.read_due(0.0), "the same value does not");
    }

    #[test]
    fn separated_placement_sits_on_the_fixture_memory_vertex() {
        let cloud = rb([-10.0, 20.0, -15.0], 120.0);
        let graph = rb([90.0, -3.0, -14.0], 300.0);
        let fx = fixture();
        for case in fx["cases"].as_array().unwrap() {
            let sep = case["separation"].as_f64().unwrap() as f32;
            let m = &case["vertices"][2];
            let p = cloud_placement(cloud, graph, DEFAULT_CLOUD_SCALE, sep);
            for k in 0..3 {
                let want = graph.unwrap().centre[k] + m[k].as_f64().unwrap() as f32;
                assert!(
                    (p.position[k] - want).abs() < 2e-3,
                    "sep {sep} axis {k}: {} vs {want}",
                    p.position[k]
                );
            }
            let merged = cloud_placement(cloud, graph, DEFAULT_CLOUD_SCALE, 0.0);
            assert_eq!(p.scale, merged.scale, "size is kept relative to one graph");
            assert_eq!(p.offset, merged.offset);
        }
        assert_eq!(
            cloud_placement(cloud, graph, DEFAULT_CLOUD_SCALE, 0.0).position,
            graph.unwrap().centre,
            "separation 0 is the merged placement"
        );
    }

    #[test]
    fn graph_bounds_for_measures_one_graph_once_separated() {
        let frame = TriangleFrame::new(400.0);
        let mut seed = 99u64;
        let blob: Vec<Vec3> = (0..200)
            .map(|_| {
                [
                    lcg(&mut seed) * 150.0,
                    lcg(&mut seed) * 150.0,
                    lcg(&mut seed) * 150.0,
                ]
            })
            .collect();
        let placed: Vec<Vec3> = [Vertex::Knowledge, Vertex::Ontology]
            .iter()
            .flat_map(|&v| blob.iter().map(move |&p| frame.place(v, p)))
            .collect();
        let local = robust_bounds(blob.iter().copied()).unwrap();
        let folded = graph_bounds_for(placed.iter().copied(), 400.0).unwrap();
        let raw = robust_bounds(placed.iter().copied()).unwrap();
        assert!(
            (folded.radius - local.radius).abs() < 1.0,
            "{} vs {}",
            folded.radius,
            local.radius
        );
        assert!(raw.radius > 2.0 * local.radius);
        assert_eq!(
            graph_bounds_for(blob.iter().copied(), 0.0),
            robust_bounds(blob.iter().copied())
        );
    }

    #[test]
    fn the_cloud_glides_to_the_memory_vertex() {
        let mut g = PlacementGlide::default();
        g.set_cloud(rb([0.0; 3], 100.0));
        g.set_graph(rb([0.0; 3], 300.0));
        let start = g.step(0.016, DEFAULT_CLOUD_SCALE, false).position;
        assert_eq!(start, [0.0; 3]);
        g.set_separation(300.0);
        let target = TriangleFrame::new(300.0).vertex(Vertex::Memory);
        let mut prev = start;
        for _ in 0..600 {
            let p = g.step(1.0 / 60.0, DEFAULT_CLOUD_SCALE, false).position;
            let d = ((p[0] - prev[0]).powi(2) + (p[2] - prev[2]).powi(2)).sqrt();
            assert!(d < 20.0, "glides, no jump: {d}");
            prev = p;
        }
        assert!(
            (prev[2] - target[2]).abs() < 1.0,
            "arrives: {prev:?} vs {target:?}"
        );
        g.set_separation(0.0);
        let snapped = g.step(1.0 / 60.0, DEFAULT_CLOUD_SCALE, true).position;
        assert_eq!(snapped, [0.0; 3], "reduced motion snaps");
    }
}
