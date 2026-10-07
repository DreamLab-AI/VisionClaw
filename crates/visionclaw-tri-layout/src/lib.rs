//! Geometry of the VisionClaw separated layout (ADR-2135).
//!
//! The Graph Separation slider pulls the three bodies of the scene apart as
//! an equilateral triangle in the ground plane: the knowledge graph, the
//! formal ontology and the memory cloud each sit on one vertex, and the
//! agents keep the centroid, drifting towards whichever graph they are
//! working on. This crate is the single definition of that geometry. The
//! server's display projection, the Quest client (`xr-client/rust`) and the
//! desktop TypeScript port (`client/src/features/graph/triLayout.ts`, checked
//! against `fixtures/tri_layout_fixture.json`) all follow it.
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
//! `graph_separation` (the slider, 0–400) drives two quantities:
//!
//! * the circumradius `R = separation × 2/√3`, so the distance between two
//!   vertices equals `2 × separation` — exactly the gap the old ±Z disc pair
//!   put between the knowledge and ontology disc centres, so a saved value
//!   keeps its meaning;
//! * the strength `s`, a smoothstep from 0 at separation 0 to 1 at
//!   [`FULL_STRENGTH_SEPARATION`]. Each graph is yawed by `θ × s` so that, once
//!   separated, its disc normal (local +Z) points along the line through the
//!   centroid; at separation 0 every yaw is 0 and the old merged layout is
//!   reproduced exactly.
//!
//! ```
//! use visionclaw_tri_layout::{TriangleFrame, Vertex};
//!
//! let merged = TriangleFrame::new(0.0);
//! assert!(merged.is_merged());
//! assert_eq!(merged.place(Vertex::Knowledge, [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
//!
//! let apart = TriangleFrame::new(250.0);
//! let k = apart.vertex(Vertex::Knowledge);
//! let o = apart.vertex(Vertex::Ontology);
//! let gap = ((k[0] - o[0]).powi(2) + (k[2] - o[2]).powi(2)).sqrt();
//! assert!((gap - 500.0).abs() < 1e-3);
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
        let doc = serde_json::json!({
            "contract": "ADR-2135 separated layout triangle",
            "drift": drift_doc,
            "full_strength_separation": FULL_STRENGTH_SEPARATION,
            "radius_per_separation": round(RADIUS_PER_SEPARATION),
            "vertex_angles_deg": VERTEX_ANGLES_DEG,
            "cases": cases,
        });
        serde_json::to_string_pretty(&doc).expect("serialise") + "\n"
    }
}
