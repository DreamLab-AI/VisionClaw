//! Geometry of the VisionClaw separated layout (ADR-2135).
//!
//! The scene's three bodies are always apart (operator decision 2026-10-08):
//! the knowledge graph, the formal ontology and the memory cloud each sit on
//! one vertex of an equilateral triangle in the ground plane, and the agents
//! keep the centroid, drifting towards whichever graph they are working on.
//! This crate is the single definition of that geometry. The server's display
//! projection, the Quest client (`xr-client/rust`) and the desktop TypeScript
//! port (`client/src/features/graph/triLayout.ts`, checked against
//! `fixtures/tri_layout_fixture.json`) all follow it.
//!
//! # Frame
//!
//! Coordinates are the scene's: three.js and Godot are both Y-up, so the
//! ground plane is X–Z. A vertex angle `θ` is a yaw about +Y measured from +Z
//! towards +X, so the vertex direction is `(sin θ, 0, cos θ)`. The default
//! desktop camera sits on +Z looking towards −Z, which is why the knowledge
//! graph is front-left (−60°), the ontology front-right (+60°) and the memory
//! cloud at the back (180°): the two graphs never hide each other, the cloud
//! is the backdrop, and the agents at the centre stay visible between them.
//!
//! # Separation
//!
//! There is no separation control any more. The layout uses one fixed
//! [`SEPARATION`], derived from the graphs' measured radii by
//! [`separation_for_radii`] so the two graph bodies never overlap at live
//! scale. [`TriangleFrame::separated`] is the frame every reader uses.
//! [`TriangleFrame::new`] still builds the frame for any separation (the
//! geometry is general, and the tests and fixture exercise it):
//!
//! * the circumradius `R = separation × 2/√3`, so the distance between two
//!   vertices equals `2 × separation`;
//! * the strength `s`, a smoothstep from 0 at separation 0 to 1 at
//!   [`FULL_STRENGTH_SEPARATION`]. Each graph is yawed by `θ × s` so that, once
//!   separated, its disc normal (local +Z) points along the line through the
//!   centroid. [`SEPARATION`] is past full strength.
//!
//! # Memory body
//!
//! The memory cloud is drawn [`MEMORY_BODY_SCALE`] times one graph's size, so
//! it would swallow the triangle if it sat on its vertex. It sits on the
//! memory vertex's ray instead, at [`TriangleFrame::memory_centre`]:
//! [`MEMORY_DISTANCE_FACTOR`] (one half, operator decision 2026-10-08) of the
//! distance from the centroid at which its sphere would clear both graph
//! spheres by [`CLEARANCE`] ([`TriangleFrame::memory_clear_distance`]). At
//! live scale that closer body overlaps the graphs: its robust sphere
//! contains both of theirs.
//!
//! ```
//! use visionclaw_tri_layout::{TriangleFrame, Vertex, SEPARATION};
//!
//! let f = TriangleFrame::separated();
//! assert_eq!(f.separation, SEPARATION);
//! let k = f.vertex(Vertex::Knowledge);
//! let o = f.vertex(Vertex::Ontology);
//! let gap = ((k[0] - o[0]).powi(2) + (k[2] - o[2]).powi(2)).sqrt();
//! assert!((gap - 2.0 * SEPARATION).abs() < 1e-3);
//!
//! // a cloud ten graphs wide sits beyond the memory vertex, at half the
//! // distance that would clear both graphs
//! let m = f.memory_centre(93.0, 930.0);
//! assert!(m[2] < f.vertex(Vertex::Memory)[2]);
//! assert!((-m[2] - 0.5 * f.memory_clear_distance(93.0, 930.0)).abs() < 1e-3);
//! ```
//!
//! # Agents
//!
//! [`drift`] holds the rule that moves each agent from the centroid towards
//! the vertices of the graphs it has recently acted on.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod drift;

/// A point or direction in scene space, `[x, y, z]`, Y up.
pub type Vec3 = [f32; 3];

/// Radius of the larger graph body at live scale, scene units: the 99th
/// percentile distance of the ontology's nodes from their median, measured on
/// the live backend on 2026-10-08 (9,473 nodes: 9,367 ontology, p99 152; 106
/// knowledge, p99 106). Each population is re-centred on its median before it
/// is placed, so this is the radius the body has at its vertex.
pub const LIVE_GRAPH_RADIUS: f32 = 152.0;

/// Clearance factor between two bodies: their centres sit at least
/// `CLEARANCE × (r₁ + r₂)` apart, a quarter of their combined radii as an
/// empty margin between them.
pub const CLEARANCE: f32 = 1.25;

/// Smallest separation at which two graph bodies of radii `a` and `b` clear
/// each other by [`CLEARANCE`]: adjacent vertices sit `2 × separation` apart,
/// so `2 × separation = CLEARANCE × (a + b)`.
///
/// ```
/// use visionclaw_tri_layout::{separation_for_radii, CLEARANCE};
/// assert_eq!(separation_for_radii(100.0, 60.0), CLEARANCE * 80.0);
/// ```
pub const fn separation_for_radii(a: f32, b: f32) -> f32 {
    CLEARANCE * (a + b) / 2.0
}

/// The layout's one separation, derived from [`LIVE_GRAPH_RADIUS`] for both
/// graphs: 190 scene units, so the knowledge and ontology centres sit 380
/// apart and their live bodies (p99 radii 106 and 152) never touch.
pub const SEPARATION: f32 = separation_for_radii(LIVE_GRAPH_RADIUS, LIVE_GRAPH_RADIUS);

/// The memory cloud's size relative to one graph: its robust radius is this
/// many times the graph's (operator decision 2026-10-08; it was 1).
pub const MEMORY_BODY_SCALE: f32 = 10.0;

/// The memory body's distance from the centroid, as a fraction of
/// [`TriangleFrame::memory_clear_distance`] (operator decision 2026-10-08,
/// "bring the memories about 50% closer to the centre"; it was 1).
pub const MEMORY_DISTANCE_FACTOR: f32 = 0.5;

/// Separation at which the triangle reaches full strength: each graph is
/// fully yawed to face the centroid and, without dual-disc, fully re-centred
/// on its vertex. Below it the strength eases in with a smoothstep.
pub const FULL_STRENGTH_SEPARATION: f32 = 100.0;

/// Circumradius per unit of separation, `2/√3`: two vertices then sit
/// `2 × separation` apart, the old knowledge↔ontology disc gap.
pub const RADIUS_PER_SEPARATION: f32 = 1.154_700_5;

/// Vertex yaw angles in degrees, indexed by [`Vertex`]: knowledge −60°
/// (front-left), ontology +60° (front-right), memory 180° (back).
pub const VERTEX_ANGLES_DEG: [f32; 3] = [-60.0, 60.0, 180.0];

/// One of the three bodies the triangle separates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vertex {
    /// The knowledge working graph (pages and linked pages).
    Knowledge = 0,
    /// The formal ontology (OWL classes, individuals, properties).
    Ontology = 1,
    /// The RuVector memory cloud.
    Memory = 2,
}

impl Vertex {
    /// All three vertices in index order.
    pub const ALL: [Vertex; 3] = [Vertex::Knowledge, Vertex::Ontology, Vertex::Memory];

    /// Index into per-vertex arrays such as [`VERTEX_ANGLES_DEG`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Yaw of this vertex's direction, in radians.
    pub fn angle_rad(self) -> f32 {
        VERTEX_ANGLES_DEG[self.index()].to_radians()
    }
}

/// Smoothstep strength of a separation: 0 at or below 0, 1 at or above
/// [`FULL_STRENGTH_SEPARATION`], `t²(3 − 2t)` between. Non-finite input is 0.
pub fn strength(separation: f32) -> f32 {
    if !separation.is_finite() {
        return 0.0;
    }
    let t = (separation / FULL_STRENGTH_SEPARATION).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Circumradius of the triangle for a separation; negative or non-finite
/// separations are 0 (merged).
pub fn circumradius(separation: f32) -> f32 {
    if separation.is_finite() && separation > 0.0 {
        separation * RADIUS_PER_SEPARATION
    } else {
        0.0
    }
}

/// Rotate `v` about +Y by `yaw` radians (+Z turns towards +X).
pub fn rotate_y(v: Vec3, yaw: f32) -> Vec3 {
    let (s, c) = yaw.sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

/// The triangle for one separation value: vertex positions and per-graph
/// yaw, centred on the scene origin (the shared centre the server
/// re-centres every population on).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriangleFrame {
    /// The slider value this frame was built from (sanitised to ≥ 0).
    pub separation: f32,
    /// Circumradius, see [`circumradius`].
    pub radius: f32,
    /// Strength, see [`strength`].
    pub strength: f32,
    /// Vertex positions, indexed by [`Vertex`]; `y` is always 0.
    pub vertices: [Vec3; 3],
    /// Yaw applied to each body's local frame, radians, indexed by [`Vertex`].
    pub yaws: [f32; 3],
}

impl TriangleFrame {
    /// Build the frame for a slider value.
    pub fn new(separation: f32) -> Self {
        let sep = if separation.is_finite() {
            separation.max(0.0)
        } else {
            0.0
        };
        let radius = circumradius(sep);
        let s = strength(sep);
        let mut vertices = [[0.0; 3]; 3];
        let mut yaws = [0.0; 3];
        for v in Vertex::ALL {
            let a = v.angle_rad();
            vertices[v.index()] = [radius * a.sin(), 0.0, radius * a.cos()];
            yaws[v.index()] = a * s;
        }
        TriangleFrame {
            separation: sep,
            radius,
            strength: s,
            vertices,
            yaws,
        }
    }

    /// The layout every reader uses: [`TriangleFrame::new`] at [`SEPARATION`].
    pub fn separated() -> Self {
        Self::new(SEPARATION)
    }

    /// Centre of the memory body, given the robust radius of one graph and of
    /// the memory cloud as drawn (both in scene units): on the memory vertex's
    /// ray from the centroid, [`MEMORY_DISTANCE_FACTOR`] of the way out to
    /// [`memory_clear_distance`](Self::memory_clear_distance).
    pub fn memory_centre(&self, graph_radius: f32, memory_radius: f32) -> Vec3 {
        let dir = rotate_y([0.0, 0.0, 1.0], Vertex::Memory.angle_rad());
        let d = MEMORY_DISTANCE_FACTOR * self.memory_clear_distance(graph_radius, memory_radius);
        [dir[0] * d, 0.0, dir[2] * d]
    }

    /// Distance from the centroid, along the memory vertex's ray, at which the
    /// memory cloud's sphere clears each graph's sphere by [`CLEARANCE`]
    /// (`|centre − vertexᵍ| ≥ CLEARANCE × (graph + memory)`), and never less
    /// than the vertex's own distance. Non-finite or negative radii count as 0.
    pub fn memory_clear_distance(&self, graph_radius: f32, memory_radius: f32) -> f32 {
        let r = |x: f32| if x.is_finite() { x.max(0.0) } else { 0.0 };
        let need = CLEARANCE * (r(graph_radius) + r(memory_radius));
        let dir = rotate_y([0.0, 0.0, 1.0], Vertex::Memory.angle_rad());
        let mut dist = self.radius;
        for v in [Vertex::Knowledge, Vertex::Ontology] {
            // |g − D·dir|² ≥ need²  ⇔  D² − 2(g·dir)D + |g|² − need² ≥ 0
            let g = self.vertex(v);
            let b = g[0] * dir[0] + g[2] * dir[2];
            let disc = b * b - (g[0] * g[0] + g[2] * g[2]) + need * need;
            if disc > 0.0 {
                dist = dist.max(b + disc.sqrt());
            }
        }
        dist
    }

    /// True at separation 0: every vertex is the origin and every yaw is 0,
    /// so [`place`](Self::place) and [`fold`](Self::fold) are the identity.
    pub fn is_merged(&self) -> bool {
        self.radius == 0.0 && self.strength == 0.0
    }

    /// Position of a vertex.
    pub fn vertex(&self, v: Vertex) -> Vec3 {
        self.vertices[v.index()]
    }

    /// Yaw of a body's local frame, radians.
    pub fn yaw(&self, v: Vertex) -> f32 {
        self.yaws[v.index()]
    }

    /// The shared centre: the agents' home and the triangle's centroid.
    pub fn centroid(&self) -> Vec3 {
        [0.0; 3]
    }

    /// Body-local point → scene: yaw about +Y, then move to the vertex.
    pub fn place(&self, v: Vertex, local: Vec3) -> Vec3 {
        let r = rotate_y(local, self.yaw(v));
        let c = self.vertex(v);
        [r[0] + c[0], r[1] + c[1], r[2] + c[2]]
    }

    /// Scene point → body-local: the inverse of [`place`](Self::place).
    pub fn unplace(&self, v: Vertex, world: Vec3) -> Vec3 {
        let c = self.vertex(v);
        rotate_y(
            [world[0] - c[0], world[1] - c[1], world[2] - c[2]],
            -self.yaw(v),
        )
    }

    /// The graph vertex (knowledge or ontology) nearest `world` in the
    /// ground plane; a tie goes to knowledge.
    pub fn nearest_graph_vertex(&self, world: Vec3) -> Vertex {
        let d2 = |v: Vertex| {
            let c = self.vertex(v);
            (world[0] - c[0]).powi(2) + (world[2] - c[2]).powi(2)
        };
        if d2(Vertex::Ontology) < d2(Vertex::Knowledge) {
            Vertex::Ontology
        } else {
            Vertex::Knowledge
        }
    }

    /// Fold a projected graph position back into its body's local frame,
    /// choosing the body by [`nearest_graph_vertex`](Self::nearest_graph_vertex).
    /// Clients use it to measure one graph's extent from the projected
    /// buffer (which spans the whole triangle) without knowing populations:
    /// once the bodies are apart the nearest vertex is the right one, and
    /// while they still overlap the error is bounded by the vertex gap.
    pub fn fold(&self, world: Vec3) -> Vec3 {
        if self.is_merged() {
            return world;
        }
        self.unplace(self.nearest_graph_vertex(world), world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= eps)
    }

    fn dist(a: Vec3, b: Vec3) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn radius_constant_is_two_over_root_three() {
        assert!((RADIUS_PER_SEPARATION - 2.0 / 3f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn separation_zero_is_the_identity() {
        let f = TriangleFrame::new(0.0);
        assert!(f.is_merged());
        for v in Vertex::ALL {
            assert_eq!(f.vertex(v), [0.0; 3]);
            assert_eq!(f.yaw(v), 0.0);
            let p = [12.5, -3.0, 40.0];
            assert_eq!(f.place(v, p), p);
            assert_eq!(f.fold(p), p);
        }
    }

    #[test]
    fn bad_separations_are_merged() {
        for s in [-50.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(TriangleFrame::new(s).is_merged(), "{s}");
        }
    }

    #[test]
    fn vertices_form_an_equilateral_triangle_centred_on_the_origin() {
        for sep in [25.0, 100.0, 250.0, 400.0] {
            let f = TriangleFrame::new(sep);
            let [k, o, m] = f.vertices;
            let side = 2.0 * sep;
            assert!((dist(k, o) - side).abs() < 1e-2, "k-o at {sep}");
            assert!((dist(o, m) - side).abs() < 1e-2, "o-m at {sep}");
            assert!((dist(m, k) - side).abs() < 1e-2, "m-k at {sep}");
            let c = [(k[0] + o[0] + m[0]) / 3.0, 0.0, (k[2] + o[2] + m[2]) / 3.0];
            assert!(close(c, f.centroid(), 1e-3), "centroid at {sep}: {c:?}");
            for v in f.vertices {
                assert_eq!(v[1], 0.0, "ground plane");
            }
        }
    }

    #[test]
    fn vertex_placement_suits_a_camera_on_plus_z() {
        let f = TriangleFrame::new(200.0);
        let [k, o, m] = f.vertices;
        assert!(k[0] < 0.0 && k[2] > 0.0, "knowledge front-left {k:?}");
        assert!(o[0] > 0.0 && o[2] > 0.0, "ontology front-right {o:?}");
        assert!(m[0].abs() < 1e-3 && m[2] < 0.0, "memory at the back {m:?}");
    }

    #[test]
    fn vertices_move_continuously_with_the_slider() {
        let mut prev = TriangleFrame::new(0.0);
        let mut sep = 0.0;
        while sep < 400.0 {
            sep += 0.5;
            let f = TriangleFrame::new(sep);
            for v in Vertex::ALL {
                assert!(
                    dist(prev.vertex(v), f.vertex(v)) < 0.6,
                    "vertex jump at {sep}"
                );
                assert!((prev.yaw(v) - f.yaw(v)).abs() < 0.03, "yaw jump at {sep}");
            }
            prev = f;
        }
    }

    #[test]
    fn strength_is_a_monotone_smoothstep() {
        assert_eq!(strength(0.0), 0.0);
        assert_eq!(strength(FULL_STRENGTH_SEPARATION), 1.0);
        assert_eq!(strength(1000.0), 1.0);
        assert!((strength(FULL_STRENGTH_SEPARATION / 2.0) - 0.5).abs() < 1e-6);
        let mut last = 0.0;
        for i in 0..=200 {
            let s = strength(i as f32);
            assert!(s >= last);
            last = s;
        }
    }

    #[test]
    fn full_strength_discs_face_the_centroid() {
        let f = TriangleFrame::new(300.0);
        for v in [Vertex::Knowledge, Vertex::Ontology] {
            // The disc normal is the body's local +Z; it must lie on the line
            // from the vertex through the centroid.
            let n = rotate_y([0.0, 0.0, 1.0], f.yaw(v));
            let c = f.vertex(v);
            let len = (c[0] * c[0] + c[2] * c[2]).sqrt();
            let radial = [c[0] / len, 0.0, c[2] / len];
            let cross = n[0] * radial[2] - n[2] * radial[0];
            assert!(
                cross.abs() < 1e-5,
                "{v:?} normal {n:?} vs radial {radial:?}"
            );
        }
    }

    #[test]
    fn unplace_inverts_place() {
        let f = TriangleFrame::new(180.0);
        let p = [33.0, -7.0, 120.0];
        for v in Vertex::ALL {
            assert!(close(f.unplace(v, f.place(v, p)), p, 1e-3));
        }
    }

    #[test]
    fn fold_recovers_local_positions_once_the_bodies_are_apart() {
        let f = TriangleFrame::new(400.0);
        for v in [Vertex::Knowledge, Vertex::Ontology] {
            for p in [[0.0, 0.0, 0.0], [150.0, 90.0, -60.0], [-200.0, 10.0, 80.0]] {
                assert!(close(f.fold(f.place(v, p)), p, 1e-3), "{v:?} {p:?}");
            }
        }
    }

    #[test]
    fn the_fixed_separation_is_derived_from_the_live_radii() {
        assert_eq!(SEPARATION, CLEARANCE * LIVE_GRAPH_RADIUS);
        assert!((SEPARATION - 190.0).abs() < 1e-4);
        let f = TriangleFrame::separated();
        assert!(!f.is_merged(), "the layout is never merged");
        assert_eq!(f.strength, 1.0, "past full strength");
        assert_eq!(f, TriangleFrame::new(SEPARATION));
    }

    #[test]
    fn the_two_graphs_never_overlap_at_live_scale() {
        let f = TriangleFrame::separated();
        let gap = dist(f.vertex(Vertex::Knowledge), f.vertex(Vertex::Ontology));
        // the measured p99 radii (knowledge 106, ontology 152), with margin
        assert!(gap >= CLEARANCE * (106.0 + 152.0), "gap {gap}");
        assert!(
            gap >= CLEARANCE * 2.0 * LIVE_GRAPH_RADIUS - 1e-3,
            "gap {gap}"
        );
    }

    #[test]
    fn memory_clear_distance_clears_both_graphs() {
        let f = TriangleFrame::separated();
        let mv = f.vertex(Vertex::Memory);
        let dir = [0.0, 0.0, -1.0];
        for (g, m) in [
            (93.0, 930.0),
            (152.0, 1520.0),
            (40.0, 400.0),
            (93.0, 93.0),
            (0.0, 0.0),
        ] {
            let d = f.memory_clear_distance(g, m);
            assert!(d >= f.radius - 1e-3, "never nearer than the vertex {d}");
            let c = [dir[0] * d, 0.0, dir[2] * d];
            for v in [Vertex::Knowledge, Vertex::Ontology] {
                assert!(
                    dist(c, f.vertex(v)) >= CLEARANCE * (g + m) - 1e-2,
                    "{g}/{m}"
                );
            }
        }
        // the memory vertex lies on -z, the ray the distance is measured along
        assert!(mv[0].abs() < 1e-3 && mv[2] < 0.0, "{mv:?}");
    }

    #[test]
    fn memory_clear_distance_is_the_tightest_clear_point() {
        let f = TriangleFrame::separated();
        let (g, m) = (93.0, 930.0);
        let d = f.memory_clear_distance(g, m);
        let c = [0.0, 0.0, -d];
        let gap = dist(c, f.vertex(Vertex::Knowledge));
        assert!((gap - CLEARANCE * (g + m)).abs() < 0.05, "{gap}");
        // a small cloud keeps the vertex; bad radii count as zero
        assert!((f.memory_clear_distance(10.0, 10.0) - f.radius).abs() < 1e-3);
        assert!((f.memory_clear_distance(f32::NAN, -5.0) - f.radius).abs() < 1e-3);
    }

    #[test]
    fn memory_centre_is_half_the_clear_distance_on_the_memory_ray() {
        // operator decision 2026-10-08: the memory body sits at half the
        // distance from the centroid that would clear both graphs
        assert_eq!(MEMORY_DISTANCE_FACTOR, 0.5);
        let f = TriangleFrame::separated();
        for (g, m) in [(93.0, 930.0), (152.0, 1520.0), (93.0, 93.0), (0.0, 0.0)] {
            let c = f.memory_centre(g, m);
            let want = MEMORY_DISTANCE_FACTOR * f.memory_clear_distance(g, m);
            assert!(
                c[0].abs() < 1e-3 && c[1] == 0.0 && c[2] < 0.0,
                "on the ray {c:?}"
            );
            assert!((dist(c, [0.0; 3]) - want).abs() < 1e-2, "{g}/{m}: {c:?}");
        }
        assert!(close(
            f.memory_centre(f32::NAN, -5.0),
            [0.0, 0.0, -MEMORY_DISTANCE_FACTOR * f.radius],
            1e-3
        ));
    }

    #[test]
    fn at_live_scale_the_closer_memory_body_encloses_both_graphs() {
        // Measured, not hidden: at half the clear distance a cloud ten graphs
        // wide overlaps the graphs; its robust sphere contains both of theirs.
        let f = TriangleFrame::separated();
        let m = MEMORY_BODY_SCALE * LIVE_GRAPH_RADIUS;
        let c = f.memory_centre(LIVE_GRAPH_RADIUS, m);
        for v in [Vertex::Knowledge, Vertex::Ontology] {
            let d = dist(c, f.vertex(v));
            assert!(
                d + LIVE_GRAPH_RADIUS < m,
                "graph sphere inside the cloud: {d}"
            );
            assert!(d > 0.7 * m, "but well off the cloud's centre: {d}");
        }
    }

    #[test]
    fn rotate_y_turns_plus_z_towards_plus_x() {
        let r = rotate_y([0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2);
        assert!(close(r, [1.0, 0.0, 0.0], 1e-6));
    }

    /// `fixtures/tri_layout_fixture.json` is the cross-language contract the
    /// TypeScript port and the XR client check against. This test fails when
    /// the geometry and the committed fixture disagree; regenerate with
    /// `TRI_LAYOUT_WRITE_FIXTURE=1 cargo test -p visionclaw-tri-layout`.
    #[test]
    fn fixture_is_current() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/tri_layout_fixture.json"
        );
        let fresh = fixture_json();
        if std::env::var_os("TRI_LAYOUT_WRITE_FIXTURE").is_some() {
            std::fs::write(path, &fresh).expect("write fixture");
        }
        let committed = std::fs::read_to_string(path).expect("fixture present");
        let a: serde_json::Value = serde_json::from_str(&committed).expect("fixture parses");
        let b: serde_json::Value = serde_json::from_str(&fresh).expect("fresh parses");
        assert_eq!(a, b, "tri_layout_fixture.json is stale: regenerate it");
    }

    fn round(x: f32) -> f64 {
        ((x as f64) * 1e4).round() / 1e4
    }

    fn v3(v: Vec3) -> serde_json::Value {
        serde_json::json!([round(v[0]), round(v[1]), round(v[2])])
    }

    fn fixture_json() -> String {
        let samples: [Vec3; 3] = [
            [0.0, 0.0, 0.0],
            [120.0, -40.0, 75.0],
            [-310.0, 220.0, -15.0],
        ];
        let cases: Vec<serde_json::Value> = [0.0, 12.5, 25.0, 50.0, 100.0, 250.0, 400.0]
            .iter()
            .map(|&sep| {
                let f = TriangleFrame::new(sep);
                let place: Vec<serde_json::Value> = Vertex::ALL
                    .iter()
                    .flat_map(|&v| samples.iter().map(move |&p| (v, p)))
                    .map(|(v, p)| {
                        serde_json::json!({
                            "vertex": v.index(),
                            "local": v3(p),
                            "world": v3(f.place(v, p)),
                        })
                    })
                    .collect();
                let fold: Vec<serde_json::Value> = samples
                    .iter()
                    .map(|&p| {
                        let w = f.place(Vertex::Ontology, p);
                        serde_json::json!({ "world": v3(w), "local": v3(f.fold(w)) })
                    })
                    .collect();
                serde_json::json!({
                    "separation": sep,
                    "radius": round(f.radius),
                    "strength": round(f.strength),
                    "vertices": f.vertices.iter().map(|&v| v3(v)).collect::<Vec<_>>(),
                    "yaws": f.yaws.iter().map(|&y| round(y)).collect::<Vec<_>>(),
                    "place": place,
                    "fold": fold,
                })
            })
            .collect();
        // A scripted drift run: the TS port replays the same events and steps.
        // Event kinds: "action" (agent, vertex), "memory" (agent or null, all).
        let events: Vec<(f64, &str, Option<u32>, usize)> = vec![
            (0.0, "action", Some(1), 0),
            (0.5, "action", Some(2), 1),
            (1.0, "memory", None, 2),
            (2.0, "action", Some(1), 0),
            (2.5, "memory", Some(2), 2),
            (4.0, "action", Some(3), 1),
        ];
        let all = [1u32, 2, 3];
        let mut field = drift::DriftField::default();
        let mut checkpoints = Vec::new();
        let mut next = 0;
        for i in 0..=150 {
            let t = i as f64 * 0.1;
            while next < events.len() && events[next].0 <= t + 1e-9 {
                let (et, kind, agent, v) = events[next];
                match kind {
                    "action" => field.record_action(agent.unwrap(), Vertex::ALL[v], et),
                    _ => field.record_memory(agent, &all, et),
                }
                next += 1;
            }
            field.step(t);
            if i % 10 == 0 {
                let coeffs: Vec<serde_json::Value> = all
                    .iter()
                    .map(|&a| {
                        let c = field.get(a).map_or([0.0; 3], |x| x.coeffs());
                        serde_json::json!([round(c[0]), round(c[1]), round(c[2])])
                    })
                    .collect();
                checkpoints
                    .push(serde_json::json!({ "t": (t * 10.0).round() / 10.0, "coeffs": coeffs }));
            }
        }
        let drift_doc = serde_json::json!({
            "half_life_s": drift::HALF_LIFE_S,
            "idle_weight": drift::IDLE_WEIGHT,
            "reach": drift::REACH,
            "follow_s": drift::FOLLOW_S,
            "settled_epsilon": drift::SETTLED_EPSILON,
            "agents": all,
            "step_s": 0.1,
            "steps": 150,
            "events": events.iter().map(|(t, k, a, v)| serde_json::json!({
                "t": t, "kind": k, "agent": a, "vertex": v,
            })).collect::<Vec<_>>(),
            "checkpoints": checkpoints,
        });
        let sep = TriangleFrame::separated();
        let memory: Vec<serde_json::Value> = [
            (93.0f32, 930.0f32),
            (152.0, 1520.0),
            (93.0, 93.0),
            (40.0, 400.0),
        ]
        .iter()
        .map(|&(g, m)| {
            serde_json::json!({
                "graph_radius": g,
                "memory_radius": m,
                "centre": v3(sep.memory_centre(g, m)),
            })
        })
        .collect();
        let doc = serde_json::json!({
            "contract": "ADR-2135 separated layout triangle",
            "live_graph_radius": LIVE_GRAPH_RADIUS,
            "clearance": CLEARANCE,
            "separation": SEPARATION,
            "memory_body_scale": MEMORY_BODY_SCALE,
            "memory_distance_factor": MEMORY_DISTANCE_FACTOR,
            "memory_centre": memory,
            "drift": drift_doc,
            "full_strength_separation": FULL_STRENGTH_SEPARATION,
            "radius_per_separation": round(RADIUS_PER_SEPARATION),
            "vertex_angles_deg": VERTEX_ANGLES_DEG,
            "cases": cases,
        });
        serde_json::to_string_pretty(&doc).expect("serialise") + "\n"
    }
}
