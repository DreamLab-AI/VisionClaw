//! WP4 — translucent cluster hulls (desktop `ClusterHulls.tsx` parity).
//!
//! The desktop draws one `ConvexGeometry` per server cluster: nodes grouped by
//! the DBSCAN/k-means `cluster_id` the V3 wire carries at record offset 36, or —
//! only when `clusterHulls.communityFallback` is on — by Louvain `community_id`.
//! Clusters smaller than [`MIN_CLUSTER_SIZE`] are dropped, the rest ranked by
//! size and capped at [`DEFAULT_MAX_HULLS`]. Each hull's points are pushed out
//! from their centroid by `padding`, and a near-flat cluster is extruded along
//! its thinnest axis so the hull has volume. The material is unlit, translucent
//! (`opacity` 0.08), double-sided, no depth write.
//!
//! XR keeps those rules and the palette, and changes only the cost model for the
//! headset budget (≤ 50 draw calls, ≤ 100k triangles, `perf/README.md`):
//!
//! * every hull goes into **one** triangle list, so the whole layer is one
//!   `ArrayMesh` surface — one draw call however many clusters are shown;
//! * each cluster is reduced to its extreme points along [`HULL_DIRECTIONS`]
//!   fixed directions before the hull is built, which bounds a hull at
//!   `2·64 − 4 = 124` triangles (≤ 3 968 for 32 hulls) and the build cost at
//!   O(n · 64) however large a cluster grows. The reduced hull is inscribed in
//!   the exact one; the default 15 % padding more than covers the difference.
//!
//! Pure Rust (no Godot types): the hull, grouping and palette are unit-tested.

use std::collections::{BTreeMap, HashMap, HashSet};

/// Desktop `MIN_CLUSTER_SIZE`.
pub const MIN_CLUSTER_SIZE: usize = 4;
/// Desktop `DEFAULT_MAX_HULLS` (`clusterHulls.maxHulls`).
pub const DEFAULT_MAX_HULLS: usize = 32;
/// Hard ceiling on hulls regardless of settings — the draw-call cost is one call
/// for the layer, so the ceiling bounds triangles: 32 · 124 = 3 968.
pub const MAX_HULLS_CEILING: usize = 32;
/// Desktop `clusterHulls.padding` default.
pub const DEFAULT_PADDING: f32 = 0.15;
/// Desktop `clusterHulls.slabThickness` default (server units; XR's GraphRoot is
/// in server units too, so the value carries over unchanged).
pub const DEFAULT_SLAB: f32 = 35.0;
/// Desktop `clusterHulls.opacity` default.
pub const DEFAULT_OPACITY: f32 = 0.08;
/// Fixed sampling directions for the extreme-point reduction.
pub const HULL_DIRECTIONS: usize = 64;
/// Upper bound on triangles in one hull built from ≤ `HULL_DIRECTIONS` points.
pub const MAX_TRIS_PER_HULL: usize = 2 * HULL_DIRECTIONS - 4;

/// Desktop `GPU_CLUSTER_COLORS`, indexed `cluster_id % 20`.
pub const GPU_CLUSTER_COLORS: [&str; 20] = [
    "#4FC3F7", "#81C784", "#FFB74D", "#CE93D8", "#FFD54F",
    "#EF5350", "#4DB6AC", "#FF7043", "#78909C", "#AED581",
    "#F48FB1", "#80DEEA", "#FFCC80", "#B39DDB", "#A5D6A7",
    "#90CAF9", "#FFAB91", "#80CBC4", "#FFF176", "#E6EE9C",
];

/// Which server structure the hulls draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HullSource {
    #[default]
    Off,
    /// Desktop default: `cluster_id > 0` only. No server clusters → no hulls
    /// (ADR-031 D6: never fabricate a grouping).
    Clusters,
    /// Desktop `communityFallback: true`: `cluster_id` when any node has one,
    /// otherwise Louvain `community_id`.
    Communities,
}

impl HullSource {
    /// GDScript code: 0 off, 1 clusters, 2 communities. Unknown → off.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Clusters,
            2 => Self::Communities,
            _ => Self::Off,
        }
    }

    pub fn code(self) -> i64 {
        match self {
            Self::Off => 0,
            Self::Clusters => 1,
            Self::Communities => 2,
        }
    }

    /// The HUD cycle order: off → clusters → communities → off.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Clusters,
            Self::Clusters => Self::Communities,
            Self::Communities => Self::Off,
        }
    }
}

/// Grouping key of one hull.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HullKey {
    Cluster(u32),
    Community(u32),
}

/// sRGB colour of a hull, matching the desktop.
pub fn hull_color(key: HullKey) -> [f32; 3] {
    match key {
        HullKey::Cluster(id) => {
            crate::domain_palette::hex_to_rgb(GPU_CLUSTER_COLORS[id as usize % GPU_CLUSTER_COLORS.len()])
                .unwrap_or([0.5647, 0.6431, 0.6824])
        }
        // `getCommunityHullColor`: hue (id·83 mod 360)/360, `setHSL(·, 0.65, 0.5)`
        // in three.js's linear working space, displayed sRGB-encoded.
        HullKey::Community(id) => {
            let hue = ((id as u64 * 83) % 360) as f32 / 360.0;
            crate::domain_palette::three_hsl_to_srgb(hue, 0.65, 0.5)
        }
    }
}

/// Build knobs (desktop `settings.visualisation.clusterHulls.*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HullParams {
    pub padding: f32,
    pub slab: f32,
    pub max_hulls: usize,
}

impl Default for HullParams {
    fn default() -> Self {
        Self {
            padding: DEFAULT_PADDING,
            slab: DEFAULT_SLAB,
            max_hulls: DEFAULT_MAX_HULLS,
        }
    }
}

/// One node as the hull builder sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HullPoint {
    pub cluster_id: u32,
    pub community_id: u32,
    pub pos: [f32; 3],
}

/// Unindexed triangle list for one `ArrayMesh` surface: three vertices per
/// triangle, a flat face normal per vertex (the fresnel edge reads it), and the
/// hull colour per vertex.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HullMesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    /// Keys of the hulls actually emitted, largest cluster first.
    pub keys: Vec<HullKey>,
}

impl HullMesh {
    pub fn triangle_count(&self) -> usize {
        self.vertices.len() / 3
    }

    pub fn hull_count(&self) -> usize {
        self.keys.len()
    }
}

// --- grouping ---------------------------------------------------------------

/// Group points per the desktop rules, drop clusters under the minimum, rank by
/// size (ties by key, so the result is deterministic) and keep the top
/// `max_hulls` (clamped to [`MAX_HULLS_CEILING`]).
pub fn group_points(points: &[HullPoint], source: HullSource, max_hulls: usize) -> Vec<(HullKey, Vec<[f32; 3]>)> {
    if source == HullSource::Off {
        return Vec::new();
    }
    let any_cluster = points.iter().any(|p| p.cluster_id > 0);
    let mut groups: BTreeMap<HullKey, Vec<[f32; 3]>> = BTreeMap::new();
    for p in points {
        if !p.pos.iter().all(|c| c.is_finite()) {
            continue;
        }
        let key = if any_cluster {
            (p.cluster_id > 0).then_some(HullKey::Cluster(p.cluster_id))
        } else if source == HullSource::Communities {
            (p.community_id > 0).then_some(HullKey::Community(p.community_id))
        } else {
            None
        };
        if let Some(k) = key {
            groups.entry(k).or_default().push(p.pos);
        }
    }
    let mut ranked: Vec<(HullKey, Vec<[f32; 3]>)> = groups
        .into_iter()
        .filter(|(_, pts)| pts.len() >= MIN_CLUSTER_SIZE)
        .collect();
    // Stable sort on size only: BTreeMap order already breaks ties by key.
    ranked.sort_by_key(|g| std::cmp::Reverse(g.1.len()));
    ranked.truncate(max_hulls.min(MAX_HULLS_CEILING));
    ranked
}

// --- point preparation (desktop buildHullGeometry) -------------------------

/// Push every point out from the centroid by `padding`, then — when the
/// thinnest bounding-box axis is under 5 % of the fattest — duplicate each
/// point at ±`slab` along that axis. Mirrors `buildHullGeometry`.
pub fn prepare_points(points: &[[f32; 3]], padding: f32, slab: f32) -> Vec<[f64; 3]> {
    if points.is_empty() {
        return Vec::new();
    }
    let n = points.len() as f64;
    let mut c = [0.0f64; 3];
    for p in points {
        for k in 0..3 {
            c[k] += p[k] as f64;
        }
    }
    for v in &mut c {
        *v /= n;
    }
    let pad = 1.0 + padding as f64;
    let padded: Vec<[f64; 3]> = points
        .iter()
        .map(|p| {
            [
                (p[0] as f64 - c[0]) * pad + c[0],
                (p[1] as f64 - c[1]) * pad + c[1],
                (p[2] as f64 - c[2]) * pad + c[2],
            ]
        })
        .collect();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in &padded {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| ext[a].total_cmp(&ext[b]));
    let (thin, fat) = (order[0], order[2]);
    if ext[thin] < 0.05 * ext[fat] {
        let mut out = Vec::with_capacity(padded.len() * 2);
        for q in &padded {
            let mut a = *q;
            let mut b = *q;
            a[thin] += slab as f64;
            b[thin] -= slab as f64;
            out.push(a);
            out.push(b);
        }
        out
    } else {
        padded
    }
}

/// Fibonacci-sphere directions, deterministic.
fn directions() -> &'static [[f64; 3]] {
    use std::sync::OnceLock;
    static DIRS: OnceLock<Vec<[f64; 3]>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let n = HULL_DIRECTIONS as f64;
        let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
        (0..HULL_DIRECTIONS)
            .map(|i| {
                let y = 1.0 - 2.0 * (i as f64 + 0.5) / n;
                let r = (1.0 - y * y).sqrt();
                let t = golden * i as f64;
                [r * t.cos(), y, r * t.sin()]
            })
            .collect()
    })
}

/// Keep only the points that are extreme along some fixed direction. Every kept
/// point is a vertex of the exact hull, so the reduced hull is inscribed in it.
pub fn extreme_points(points: &[[f64; 3]]) -> Vec<[f64; 3]> {
    if points.len() <= HULL_DIRECTIONS {
        return points.to_vec();
    }
    let mut keep: Vec<usize> = Vec::with_capacity(HULL_DIRECTIONS);
    let mut seen: HashSet<usize> = HashSet::with_capacity(HULL_DIRECTIONS);
    for d in directions() {
        let mut best = 0usize;
        let mut best_v = f64::NEG_INFINITY;
        for (i, p) in points.iter().enumerate() {
            let v = p[0] * d[0] + p[1] * d[1] + p[2] * d[2];
            if v > best_v {
                best_v = v;
                best = i;
            }
        }
        if seen.insert(best) {
            keep.push(best);
        }
    }
    keep.into_iter().map(|i| points[i]).collect()
}

// --- convex hull ------------------------------------------------------------

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

#[derive(Clone, Copy)]
struct Face {
    v: [usize; 3],
    n: [f64; 3],
    d: f64,
}

impl Face {
    fn new(pts: &[[f64; 3]], a: usize, b: usize, c: usize) -> Self {
        let n0 = cross(sub(pts[b], pts[a]), sub(pts[c], pts[a]));
        let len = norm(n0);
        let n = if len > 0.0 { [n0[0] / len, n0[1] / len, n0[2] / len] } else { n0 };
        Self { v: [a, b, c], n, d: dot(n, pts[a]) }
    }
    fn dist(&self, p: [f64; 3]) -> f64 {
        dot(self.n, p) - self.d
    }
}

/// Convex hull of `pts` as outward-wound triangles (indices into `pts`).
/// `None` for fewer than four points or a set that is (numerically) collinear
/// or coplanar — the caller extrudes flat clusters before this point, exactly
/// as the desktop does, and skips anything still degenerate.
pub fn convex_hull(pts: &[[f64; 3]]) -> Option<Vec<[usize; 3]>> {
    if pts.len() < 4 || pts.iter().any(|p| p.iter().any(|c| !c.is_finite())) {
        return None;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let scale = norm(sub(hi, lo));
    if scale <= 0.0 {
        return None;
    }
    let eps = scale * 1e-9;

    // Initial tetrahedron from extreme points.
    let i0 = (0..pts.len()).min_by(|&a, &b| pts[a][0].total_cmp(&pts[b][0]))?;
    let i1 = (0..pts.len()).max_by(|&a, &b| norm(sub(pts[a], pts[i0])).total_cmp(&norm(sub(pts[b], pts[i0]))))?;
    if norm(sub(pts[i1], pts[i0])) <= eps {
        return None;
    }
    let line = sub(pts[i1], pts[i0]);
    let i2 = (0..pts.len())
        .max_by(|&a, &b| norm(cross(line, sub(pts[a], pts[i0]))).total_cmp(&norm(cross(line, sub(pts[b], pts[i0])))))?;
    let n012 = cross(line, sub(pts[i2], pts[i0]));
    if norm(n012) <= eps * scale {
        return None; // collinear
    }
    let i3 = (0..pts.len())
        .max_by(|&a, &b| dot(n012, sub(pts[a], pts[i0])).abs().total_cmp(&dot(n012, sub(pts[b], pts[i0])).abs()))?;
    if dot(n012, sub(pts[i3], pts[i0])).abs() / norm(n012) <= eps * 1e3 {
        return None; // coplanar
    }

    let centroid = {
        let mut c = [0.0; 3];
        for &i in &[i0, i1, i2, i3] {
            for k in 0..3 {
                c[k] += pts[i][k] / 4.0;
            }
        }
        c
    };
    let mut faces: Vec<Face> = Vec::new();
    for [a, b, c] in [[i0, i1, i2], [i0, i1, i3], [i0, i2, i3], [i1, i2, i3]] {
        let mut f = Face::new(pts, a, b, c);
        if f.dist(centroid) > 0.0 {
            f = Face::new(pts, a, c, b);
        }
        faces.push(f);
    }

    let tol = eps * 1e3;
    for p in 0..pts.len() {
        if [i0, i1, i2, i3].contains(&p) {
            continue;
        }
        let visible: Vec<usize> = (0..faces.len()).filter(|&f| faces[f].dist(pts[p]) > tol).collect();
        if visible.is_empty() {
            continue;
        }
        // Horizon: directed edges of visible faces whose reverse is not also on a
        // visible face. New faces (a, b, p) keep the outward winding.
        let mut edges: HashMap<(usize, usize), ()> = HashMap::new();
        for &f in &visible {
            let v = faces[f].v;
            for k in 0..3 {
                edges.insert((v[k], v[(k + 1) % 3]), ());
            }
        }
        let horizon: Vec<(usize, usize)> = edges
            .keys()
            .filter(|&&(a, b)| !edges.contains_key(&(b, a)))
            .copied()
            .collect();
        let vis: HashSet<usize> = visible.into_iter().collect();
        let mut kept: Vec<Face> = faces
            .iter()
            .enumerate()
            .filter(|(i, _)| !vis.contains(i))
            .map(|(_, f)| *f)
            .collect();
        for (a, b) in horizon {
            kept.push(Face::new(pts, a, b, p));
        }
        faces = kept;
    }
    let mut out: Vec<[usize; 3]> = faces.into_iter().map(|f| f.v).collect();
    out.sort_unstable();
    Some(out)
}

// --- mesh assembly ----------------------------------------------------------

/// Group, reduce, hull and pack every cluster into one triangle list.
pub fn build_hull_mesh(points: &[HullPoint], source: HullSource, params: HullParams) -> HullMesh {
    let mut mesh = HullMesh::default();
    for (key, pts) in group_points(points, source, params.max_hulls) {
        let prepared = prepare_points(&pts, params.padding, params.slab);
        let reduced = extreme_points(&prepared);
        let Some(tris) = convex_hull(&reduced) else {
            continue;
        };
        let col = hull_color(key);
        for [a, b, c] in tris {
            let (pa, pb, pc) = (reduced[a], reduced[b], reduced[c]);
            let n = cross(sub(pb, pa), sub(pc, pa));
            let len = norm(n);
            let nn = if len > 0.0 {
                [(n[0] / len) as f32, (n[1] / len) as f32, (n[2] / len) as f32]
            } else {
                [0.0, 1.0, 0.0]
            };
            for v in [pa, pb, pc] {
                mesh.vertices.push([v[0] as f32, v[1] as f32, v[2] as f32]);
                mesh.normals.push(nn);
                mesh.colors.push(col);
            }
        }
        mesh.keys.push(key);
    }
    mesh
}

/// Order-independent fingerprint of a hull input, quantised to `quantum`
/// server units. The scene polls at a low rate and rebuilds only when this
/// changes, so a settled layout costs one hash per poll and no mesh upload.
pub fn input_signature(points: &[HullPoint], source: HullSource, params: HullParams, quantum: f32) -> u64 {
    let q = if quantum > 0.0 { quantum } else { 1.0 };
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325 ^ source.code() as u64;
    for p in points {
        let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
        for v in [p.cluster_id as i64, p.community_id as i64] {
            h = (h ^ v as u64).wrapping_mul(0x0100_0000_01b3);
        }
        for c in p.pos {
            let qi = (c / q).round() as i64;
            h = (h ^ qi as u64).wrapping_mul(0x0100_0000_01b3);
        }
        // Sum of per-point hashes: insensitive to iteration order.
        acc = acc.wrapping_add(h ^ (h >> 29));
    }
    for v in [params.padding.to_bits() as u64, params.slab.to_bits() as u64, params.max_hulls as u64] {
        acc = (acc ^ v).wrapping_mul(0x0100_0000_01b3);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hull_volume(pts: &[[f64; 3]], tris: &[[usize; 3]]) -> f64 {
        tris.iter()
            .map(|t| dot(pts[t[0]], cross(pts[t[1]], pts[t[2]])) / 6.0)
            .sum()
    }

    /// Every input point is on or behind every face (outward winding, convexity).
    fn assert_contains_all(pts: &[[f64; 3]], tris: &[[usize; 3]]) {
        for t in tris {
            let f = Face::new(pts, t[0], t[1], t[2]);
            for p in pts {
                assert!(f.dist(*p) <= 1e-6, "point {p:?} outside face {t:?}");
            }
        }
    }

    /// Closed 2-manifold: every directed edge appears once and its reverse once.
    fn assert_closed(tris: &[[usize; 3]]) {
        let mut e: HashMap<(usize, usize), usize> = HashMap::new();
        for t in tris {
            for k in 0..3 {
                *e.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        for (&(a, b), &n) in &e {
            assert_eq!(n, 1, "edge {a}->{b} used {n} times");
            assert_eq!(e.get(&(b, a)), Some(&1), "edge {a}->{b} has no twin");
        }
    }

    #[test]
    fn tetrahedron_hull_is_four_outward_faces() {
        let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let tris = convex_hull(&pts).unwrap();
        assert_eq!(tris.len(), 4);
        assert_closed(&tris);
        assert_contains_all(&pts, &tris);
        assert!((hull_volume(&pts, &tris) - 1.0 / 6.0).abs() < 1e-12, "outward winding ⇒ positive volume");
    }

    #[test]
    fn cube_hull_ignores_interior_points() {
        let mut pts = Vec::new();
        for x in [0.0, 2.0] {
            for y in [0.0, 2.0] {
                for z in [0.0, 2.0] {
                    pts.push([x, y, z]);
                }
            }
        }
        pts.push([1.0, 1.0, 1.0]);
        pts.push([0.5, 1.5, 1.0]);
        let tris = convex_hull(&pts).unwrap();
        assert_eq!(tris.len(), 12, "6 faces × 2 triangles");
        assert_closed(&tris);
        assert_contains_all(&pts, &tris);
        assert!((hull_volume(&pts, &tris) - 8.0).abs() < 1e-9);
        let used: HashSet<usize> = tris.iter().flatten().copied().collect();
        assert!(!used.contains(&8) && !used.contains(&9), "interior points are not vertices");
    }

    #[test]
    fn degenerate_inputs_return_none_without_panicking() {
        assert!(convex_hull(&[]).is_none());
        assert!(convex_hull(&[[0.0; 3]; 3]).is_none(), "fewer than four");
        assert!(convex_hull(&[[1.0, 2.0, 3.0]; 9]).is_none(), "coincident");
        let line: Vec<[f64; 3]> = (0..10).map(|i| [i as f64, 2.0 * i as f64, 0.0]).collect();
        assert!(convex_hull(&line).is_none(), "collinear");
        let plane: Vec<[f64; 3]> = (0..25).map(|i| [(i % 5) as f64, (i / 5) as f64, 7.0]).collect();
        assert!(convex_hull(&plane).is_none(), "coplanar");
        assert!(convex_hull(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, f64::NAN]]).is_none());
    }

    #[test]
    fn random_cloud_hull_is_closed_convex_and_bounded() {
        // Deterministic LCG cloud; reduced through the extreme-point filter.
        let mut s: u64 = 42;
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) * 200.0 - 100.0
        };
        let pts: Vec<[f64; 3]> = (0..2_000).map(|_| [rnd(), rnd(), rnd()]).collect();
        let reduced = extreme_points(&pts);
        assert!(reduced.len() <= HULL_DIRECTIONS);
        let tris = convex_hull(&reduced).unwrap();
        assert!(tris.len() <= MAX_TRIS_PER_HULL, "{} tris", tris.len());
        assert_closed(&tris);
        assert_contains_all(&reduced, &tris);
        // The full hull on all 2 000 points is also valid (no reduction).
        let full = convex_hull(&pts).unwrap();
        assert_closed(&full);
        assert_contains_all(&pts, &full);
        assert!(hull_volume(&reduced, &tris) <= hull_volume(&pts, &full) + 1e-6, "reduced hull is inscribed");
    }

    #[test]
    fn flat_cluster_is_extruded_along_its_thin_axis_like_the_desktop() {
        // Points on the y = 5 plane: thin axis y, so each point is duplicated ±slab.
        let flat: Vec<[f32; 3]> = (0..16).map(|i| [(i % 4) as f32 * 10.0, 5.0, (i / 4) as f32 * 10.0]).collect();
        let prepared = prepare_points(&flat, 0.0, 35.0);
        assert_eq!(prepared.len(), 32);
        let ys: HashSet<i64> = prepared.iter().map(|p| p[1].round() as i64).collect();
        assert_eq!(ys, HashSet::from([40, -30]));
        assert!(convex_hull(&prepared).is_some(), "extruded slab has volume");
        // Padding pushes points out from the centroid.
        let fat = [[0.0f32, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
        let padded = prepare_points(&fat, 0.5, 35.0);
        assert_eq!(padded.len(), 4, "not flat → no extrusion");
        assert!((padded[1][0] - 13.75).abs() < 1e-9, "x: (10-2.5)*1.5+2.5");
    }

    fn pt(cluster: u32, community: u32, pos: [f32; 3]) -> HullPoint {
        HullPoint { cluster_id: cluster, community_id: community, pos }
    }

    fn blob(cluster: u32, community: u32, n: usize, off: f32) -> Vec<HullPoint> {
        (0..n)
            .map(|i| {
                let a = i as f32 * 2.399;
                pt(cluster, community, [off + a.cos() * 10.0, (i as f32 * 1.7) % 10.0, off + a.sin() * 10.0])
            })
            .collect()
    }

    #[test]
    fn grouping_prefers_cluster_id_and_never_fabricates() {
        let mut pts = blob(3, 9, 6, 0.0);
        pts.extend(blob(0, 9, 6, 100.0)); // unclustered — no hull even in community mode
        let g = group_points(&pts, HullSource::Clusters, 32);
        assert_eq!(g.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![HullKey::Cluster(3)]);
        let g2 = group_points(&pts, HullSource::Communities, 32);
        assert_eq!(g2.len(), 1, "cluster_id present ⇒ community fallback unused");
        // No cluster ids at all: clusters mode draws nothing; community mode groups.
        let comm = blob(0, 4, 5, 0.0);
        assert!(group_points(&comm, HullSource::Clusters, 32).is_empty());
        assert_eq!(group_points(&comm, HullSource::Communities, 32)[0].0, HullKey::Community(4));
        assert!(group_points(&comm, HullSource::Off, 32).is_empty());
    }

    #[test]
    fn grouping_drops_small_clusters_ranks_by_size_and_caps() {
        let mut pts = Vec::new();
        for c in 1..=40u32 {
            pts.extend(blob(c, 0, 4 + (c as usize % 7), c as f32 * 50.0));
        }
        pts.extend(blob(99, 0, 3, 0.0)); // below MIN_CLUSTER_SIZE
        let g = group_points(&pts, HullSource::Clusters, 1_000);
        assert_eq!(g.len(), MAX_HULLS_CEILING, "settings cannot exceed the ceiling");
        assert!(g.iter().all(|(k, _)| *k != HullKey::Cluster(99)));
        for w in g.windows(2) {
            assert!(w[0].1.len() >= w[1].1.len(), "ranked largest first");
        }
        assert_eq!(group_points(&pts, HullSource::Clusters, 5).len(), 5);
    }

    #[test]
    fn mesh_is_one_triangle_list_within_budget_at_the_cap() {
        let mut pts = Vec::new();
        for c in 1..=32u32 {
            pts.extend(blob(c, 0, 400, c as f32 * 60.0));
        }
        let mesh = build_hull_mesh(&pts, HullSource::Clusters, HullParams::default());
        assert_eq!(mesh.hull_count(), 32);
        assert_eq!(mesh.vertices.len(), mesh.triangle_count() * 3);
        assert_eq!(mesh.normals.len(), mesh.vertices.len());
        assert_eq!(mesh.colors.len(), mesh.vertices.len());
        assert!(mesh.triangle_count() <= 32 * MAX_TRIS_PER_HULL, "{}", mesh.triangle_count());
        assert!(mesh.triangle_count() <= 100_000 / 20, "hull layer stays a small slice of the 100k budget");
    }

    #[test]
    fn hull_colours_match_the_desktop_palette() {
        assert_eq!(hull_color(HullKey::Cluster(1)), crate::domain_palette::hex_to_rgb("#81C784").unwrap());
        assert_eq!(hull_color(HullKey::Cluster(21)), hull_color(HullKey::Cluster(1)), "mod 20");
        // Ground truth from three.js 0.183.0 (`client/node_modules/three`):
        // `new Color().setHSL(((id*83)%360)/360, 0.65, 0.5).getHexString()`.
        for (id, hex) in [(1u32, "#c8ea74"), (2, "#74ead6"), (7, "#74a6ea"), (433, "#e974ea")] {
            let c = hull_color(HullKey::Community(id));
            let want = crate::domain_palette::hex_to_rgb(hex).unwrap();
            for k in 0..3 {
                assert!((c[k] - want[k]).abs() <= 0.5 / 255.0 + 1e-4, "community {id} ch{k}: {} vs {hex}", c[k] * 255.0);
            }
        }
    }

    #[test]
    fn signature_is_order_independent_and_tracks_motion() {
        let a = blob(1, 0, 8, 0.0);
        let mut b = a.clone();
        b.reverse();
        let p = HullParams::default();
        assert_eq!(input_signature(&a, HullSource::Clusters, p, 1.0), input_signature(&b, HullSource::Clusters, p, 1.0));
        let mut moved = a.clone();
        moved[0].pos[0] += 0.2;
        assert_eq!(input_signature(&a, HullSource::Clusters, p, 1.0), input_signature(&moved, HullSource::Clusters, p, 1.0), "sub-quantum jitter ignored");
        moved[0].pos[0] += 5.0;
        assert_ne!(input_signature(&a, HullSource::Clusters, p, 1.0), input_signature(&moved, HullSource::Clusters, p, 1.0));
        assert_ne!(input_signature(&a, HullSource::Clusters, p, 1.0), input_signature(&a, HullSource::Communities, p, 1.0));
    }

    #[test]
    fn source_codes_cycle() {
        assert_eq!(HullSource::default(), HullSource::Off);
        assert_eq!(HullSource::Off.next().next().next(), HullSource::Off);
        for s in [HullSource::Off, HullSource::Clusters, HullSource::Communities] {
            assert_eq!(HullSource::from_code(s.code()), s);
        }
        assert_eq!(HullSource::from_code(7), HullSource::Off);
    }
}
