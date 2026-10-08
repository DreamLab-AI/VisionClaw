//! Display-only layout projection for the position broadcast (ADR-2135).
//!
//! The physics integrator always runs on the merged layout. Just before
//! positions are broadcast, this module rewrites the broadcast copy so the
//! populations sit apart, always (operator decision 2026-10-08: there is no
//! separation control):
//!
//! * **Triangle** — whenever the population table is present: each population
//!   is re-centred on its own median, Z-compressed, then placed on its vertex
//!   of [`TriangleFrame::separated`] (knowledge and ontology yawed to face the
//!   centroid; agent nodes at the centroid). With dual-disc on, each graph is
//!   also rim-clamped into a disc. The agents' drift towards the graphs they
//!   work on is applied by the clients, which own every agent body they draw.
//! * **Z-scale** / **Identity** — the fallback while no population table has
//!   arrived: `axis_compression_z < 1` scales every Z, otherwise nothing.
//!
//! The caller restores the pristine physics positions before the next step.
//! The projection must never feed back into the simulation buffer: the
//! ~56k knowledge↔ontology cross-links (rest length ~30) would span the gap
//! every frame and pull the populations back together.

use glam::Vec3;
use visionclaw_tri_layout::{TriangleFrame, Vertex};

/// Per-node population, the GPU-local mirror of the canonical
/// [`visionclaw_domain::models::Population`] (the single classifier is
/// [`visionclaw_domain::models::Node::population`]). Produced only through the
/// `From<Population>` conversion; never re-derived from fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphPopulation {
    Knowledge,
    Ontology,
    Agent,
}

impl From<visionclaw_domain::models::Population> for GraphPopulation {
    fn from(p: visionclaw_domain::models::Population) -> Self {
        match p {
            visionclaw_domain::models::Population::Knowledge => GraphPopulation::Knowledge,
            visionclaw_domain::models::Population::Ontology => GraphPopulation::Ontology,
            visionclaw_domain::models::Population::Agent => GraphPopulation::Agent,
        }
    }
}

impl GraphPopulation {
    fn index(self) -> usize {
        match self {
            GraphPopulation::Knowledge => 0,
            GraphPopulation::Ontology => 1,
            GraphPopulation::Agent => 2,
        }
    }

    /// The triangle vertex a population lives on; agents have none (they
    /// keep the centroid).
    pub(crate) fn vertex(self) -> Option<Vertex> {
        match self {
            GraphPopulation::Knowledge => Some(Vertex::Knowledge),
            GraphPopulation::Ontology => Some(Vertex::Ontology),
            GraphPopulation::Agent => None,
        }
    }
}

/// Rim-clamp radius for each disc when dual-disc is on. Separation-independent
/// so the discs keep their full size when pulled close; the healthy layout
/// radius is ~±2400, so this only catches runaways that would otherwise spray
/// across the gap and visually merge the populations.
pub(crate) const DISC_RIM_RADIUS: f32 = 2600.0;

/// Floor for the continuous `axis_compression_z` Z-scale (1.0 = fully 3D).
pub(crate) const Z_SCALE_MIN: f32 = 0.05;

/// Clamp `axis_compression_z` into `[Z_SCALE_MIN, 1.0]`.
#[inline]
pub(crate) fn clamp_z_scale(axis_compression_z: f32) -> f32 {
    axis_compression_z.clamp(Z_SCALE_MIN, 1.0)
}

/// The two layout controls that shape the projection, read once per broadcast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutParams {
    /// Clamped `axis_compression_z`.
    pub face_scale: f32,
    /// `enable_dual_disc_layout`: shape each graph as a disc facing the centre.
    pub dual_disc: bool,
}

impl LayoutParams {
    pub(crate) fn new(axis_compression_z: f32, dual_disc: bool) -> Self {
        LayoutParams {
            face_scale: clamp_z_scale(axis_compression_z),
            dual_disc,
        }
    }
}

/// Which rewrite a broadcast gets; anything but `Identity` must be undone
/// before the next physics step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    Identity,
    ZScale,
    Triangle,
}

/// The mode for these controls. The triangle needs the population table and
/// is always on once it is there; without it the Z-scale fallback applies.
pub(crate) fn display_mode(params: &LayoutParams, populations_len: usize) -> DisplayMode {
    if populations_len > 0 {
        DisplayMode::Triangle
    } else if (params.face_scale - 1.0).abs() > f32::EPSILON {
        DisplayMode::ZScale
    } else {
        DisplayMode::Identity
    }
}

/// Anything carrying a broadcast position.
pub(crate) trait HasPosition {
    fn position(&self) -> Vec3;
    fn position_mut(&mut self) -> &mut Vec3;
}

impl HasPosition for Vec3 {
    fn position(&self) -> Vec3 {
        *self
    }
    fn position_mut(&mut self) -> &mut Vec3 {
        self
    }
}

/// `(position, velocity)`, the force actor's working buffer.
impl HasPosition for (Vec3, Vec3) {
    fn position(&self) -> Vec3 {
        self.0
    }
    fn position_mut(&mut self) -> &mut Vec3 {
        &mut self.0
    }
}

/// `(node_id, position, velocity)`, the last-known-good snapshot.
impl HasPosition for (u32, Vec3, Vec3) {
    fn position(&self) -> Vec3 {
        self.1
    }
    fn position_mut(&mut self) -> &mut Vec3 {
        &mut self.1
    }
}

/// Median position per population, indexed `[Knowledge, Ontology, Agent]`.
/// Median, not mean, so a few nodes flung to the boundary cannot drag a
/// population's centre. Non-finite positions are skipped; an empty
/// population's centre is the origin.
pub(crate) fn population_centroids<T: HasPosition>(
    items: &[T],
    populations: &[GraphPopulation],
) -> [Vec3; 3] {
    let mut axes: [[Vec<f32>; 3]; 3] = Default::default();
    for (item, pop) in items.iter().zip(populations) {
        let p = item.position();
        if p.is_finite() {
            let b = &mut axes[pop.index()];
            b[0].push(p.x);
            b[1].push(p.y);
            b[2].push(p.z);
        }
    }
    let median = |v: &mut Vec<f32>| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        let mid = v.len() / 2;
        *v.select_nth_unstable_by(mid, |a, b| a.total_cmp(b)).1
    };
    let mut out = [Vec3::ZERO; 3];
    for (o, b) in out.iter_mut().zip(axes.iter_mut()) {
        *o = Vec3::new(median(&mut b[0]), median(&mut b[1]), median(&mut b[2]));
    }
    out
}

/// Rewrite the broadcast positions in `items` for `params`, returning the
/// mode applied. `populations[i]` classifies `items[i]`. Items beyond the
/// population table are left alone in triangle mode, as before.
pub(crate) fn project_display<T: HasPosition>(
    items: &mut [T],
    populations: &[GraphPopulation],
    params: &LayoutParams,
) -> DisplayMode {
    let mode = display_mode(params, populations.len());
    match mode {
        DisplayMode::Identity => {}
        DisplayMode::ZScale => {
            for item in items.iter_mut() {
                item.position_mut().z *= params.face_scale;
            }
        }
        DisplayMode::Triangle => {
            let frame = TriangleFrame::separated();
            let centroids = population_centroids(items, populations);
            for (item, &pop) in items.iter_mut().zip(populations) {
                let p = item.position_mut();
                let mut local = *p - centroids[pop.index()];
                if params.dual_disc {
                    let d2 = local.x * local.x + local.y * local.y;
                    if d2 > DISC_RIM_RADIUS * DISC_RIM_RADIUS {
                        let s = DISC_RIM_RADIUS / d2.sqrt();
                        local.x *= s;
                        local.y *= s;
                    }
                }
                local.z *= params.face_scale;
                let placed = match pop.vertex() {
                    Some(v) => frame.place(v, local.to_array()),
                    // agent nodes keep the centroid; the clients add the drift
                    None => local.to_array(),
                };
                *p = Vec3::from_array(placed);
            }
        }
    }
    mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use GraphPopulation::{Agent, Knowledge, Ontology};

    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }

    fn sample(n: usize, seed: u64) -> (Vec<Vec3>, Vec<GraphPopulation>) {
        let mut s = seed;
        let mut pos = Vec::with_capacity(n);
        let mut pops = Vec::with_capacity(n);
        for i in 0..n {
            let pop = [Knowledge, Ontology, Agent][i % 3];
            let off = match pop {
                Knowledge => Vec3::new(40.0, -10.0, 25.0),
                Ontology => Vec3::new(-60.0, 15.0, -30.0),
                Agent => Vec3::new(5.0, 5.0, 5.0),
            };
            pos.push(off + Vec3::new(lcg(&mut s), lcg(&mut s), lcg(&mut s)) * 400.0);
            pops.push(pop);
        }
        // a runaway the rim clamp must catch
        pos[0] = Vec3::new(9000.0, -4000.0, 300.0);
        (pos, pops)
    }

    #[test]
    fn the_triangle_is_always_on_once_populations_are_known() {
        let m = |axis, dual, n| display_mode(&LayoutParams::new(axis, dual), n);
        for axis in [1.0, 0.4, 0.0] {
            for dual in [false, true] {
                assert_eq!(m(axis, dual, 10), DisplayMode::Triangle, "{axis} {dual}");
            }
        }
        assert_eq!(
            m(0.4, false, 0),
            DisplayMode::ZScale,
            "no population table: fallback"
        );
        assert_eq!(
            m(1.0, true, 0),
            DisplayMode::Identity,
            "no population table: fallback"
        );
    }

    #[test]
    fn separated_populations_sit_on_their_vertices() {
        let (orig, pops) = sample(900, 11);
        for dual in [false, true] {
            let mut pos = orig.clone();
            let params = LayoutParams::new(0.2, dual);
            project_display(&mut pos, &pops, &params);
            // A per-axis median is not rotation-equivariant, so check it in each
            // body's own frame: unplace() undoes the yaw and the vertex offset,
            // leaving the re-centred population, whose median is the origin.
            let f = TriangleFrame::separated();
            let local: Vec<Vec3> = pos
                .iter()
                .zip(&pops)
                .map(|(p, pop)| match pop.vertex() {
                    Some(v) => Vec3::from_array(f.unplace(v, p.to_array())),
                    None => *p,
                })
                .collect();
            let c = population_centroids(&local, &pops);
            for pop in [Knowledge, Ontology, Agent] {
                assert!(
                    c[pop.index()].length() < 1e-3,
                    "dual={dual} {pop:?} median {}",
                    c[pop.index()]
                );
            }
            // and in the scene the bodies are where the triangle says
            let scene = population_centroids(&pos, &pops);
            for (pop, v) in [(Knowledge, Vertex::Knowledge), (Ontology, Vertex::Ontology)] {
                let want = Vec3::from_array(f.vertex(v));
                assert!(
                    (scene[pop.index()] - want).length() < 40.0,
                    "dual={dual} {pop:?}"
                );
            }
        }
    }

    /// Two populations drawn inside one shared ball the size of the live
    /// graphs (radius `LIVE_GRAPH_RADIUS`, fully interpenetrating in physics
    /// space) come out as two disjoint bodies with an empty gap between them.
    #[test]
    fn the_two_graphs_never_overlap_at_live_scale() {
        use visionclaw_tri_layout::LIVE_GRAPH_RADIUS;
        let mut seed = 23u64;
        let mut pos = Vec::new();
        let mut pops = Vec::new();
        while pos.len() < 4000 {
            let p = Vec3::new(lcg(&mut seed), lcg(&mut seed), lcg(&mut seed));
            if p.length() <= 1.0 {
                pos.push(Vec3::new(90.0, 0.0, 50.0) + p * LIVE_GRAPH_RADIUS);
                pops.push(if pos.len() % 2 == 0 {
                    Knowledge
                } else {
                    Ontology
                });
            }
        }
        for dual in [false, true] {
            let mut out = pos.clone();
            project_display(&mut out, &pops, &LayoutParams::new(1.0, dual));
            let (k, o): (Vec<_>, Vec<_>) =
                out.iter().zip(&pops).partition(|(_, p)| **p == Knowledge);
            let gap = k
                .iter()
                .flat_map(|(a, _)| o.iter().map(move |(b, _)| (**a - **b).length()))
                .fold(f32::INFINITY, f32::min);
            // the bodies are ≤ 2 × LIVE_GRAPH_RADIUS wide once re-centred on
            // their medians; the clearance leaves daylight between them
            assert!(
                gap > 0.1 * LIVE_GRAPH_RADIUS,
                "dual={dual}: nearest pair {gap}"
            );
        }
    }

    #[test]
    fn separated_discs_face_the_centroid() {
        // A thin disc (face_scale 0.05): its thin axis must be the radial
        // direction through the centroid.
        let (orig, pops) = sample(600, 3);
        let mut pos = orig.clone();
        project_display(&mut pos, &pops, &LayoutParams::new(0.0, true));
        let f = TriangleFrame::separated();
        for v in [Vertex::Knowledge, Vertex::Ontology] {
            let c = Vec3::from_array(f.vertex(v));
            let radial = c.normalize();
            let pop = if v == Vertex::Knowledge {
                Knowledge
            } else {
                Ontology
            };
            let spread = pos
                .iter()
                .zip(&pops)
                .filter(|(_, p)| **p == pop)
                .map(|(p, _)| (*p - c).dot(radial).abs())
                .fold(0.0f32, f32::max);
            // raw |z| < ~460 before compression; ×0.05 → < ~25 along the normal
            assert!(
                spread < 30.0,
                "{v:?} thickness along the radial normal {spread}"
            );
        }
    }

    #[test]
    fn dual_disc_rim_clamps_a_runaway() {
        let (orig, pops) = sample(300, 9);
        let mut pos = orig.clone();
        project_display(&mut pos, &pops, &LayoutParams::new(1.0, true));
        let f = TriangleFrame::separated();
        // sample() puts a 9000-unit runaway at index 0 (knowledge)
        let local = Vec3::from_array(f.unplace(Vertex::Knowledge, pos[0].to_array()));
        let r = (local.x * local.x + local.y * local.y).sqrt();
        assert!(r <= DISC_RIM_RADIUS + 1.0, "in-plane radius {r}");
    }

    #[test]
    fn agent_nodes_sit_at_the_centroid_unrotated() {
        // Agents are re-centred on their own median and never yawed or moved to
        // a vertex: the clients add the activity drift (ADR-2135).
        let pops = vec![Knowledge, Agent, Ontology, Agent];
        let orig = vec![
            Vec3::new(50.0, 0.0, 0.0),
            Vec3::new(10.0, 2.0, -4.0),
            Vec3::new(-50.0, 0.0, 0.0),
            Vec3::new(14.0, 6.0, 8.0),
        ];
        let mut pos = orig.clone();
        project_display(&mut pos, &pops, &LayoutParams::new(1.0, false));
        // agent median = (14, 6, 8) (upper median of two); offsets kept unrotated
        assert!(
            (pos[1] - Vec3::new(-4.0, -4.0, -12.0)).length() < 1e-4,
            "{}",
            pos[1]
        );
        assert!(pos[3].length() < 1e-4, "{}", pos[3]);
    }

    #[test]
    fn centroids_are_medians_and_skip_non_finite() {
        let pops = vec![Knowledge, Knowledge, Knowledge, Ontology];
        let pos = vec![
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(2.0, 50.0, -3.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(4.0, 4.0, 4.0),
        ];
        let c = population_centroids(&pos, &pops);
        assert_eq!(c[0], Vec3::new(2.0, 50.0, 1.0));
        assert_eq!(c[1], Vec3::new(4.0, 4.0, 4.0));
        assert_eq!(c[2], Vec3::ZERO);
    }

    #[test]
    fn clamp_z_scale_bounds() {
        assert_eq!(clamp_z_scale(1.0), 1.0);
        assert_eq!(clamp_z_scale(0.5), 0.5);
        assert_eq!(clamp_z_scale(0.0), Z_SCALE_MIN);
        assert_eq!(clamp_z_scale(-3.0), Z_SCALE_MIN);
        assert_eq!(clamp_z_scale(2.0), 1.0);
    }
}
