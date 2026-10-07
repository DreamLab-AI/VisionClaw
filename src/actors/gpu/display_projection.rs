//! Display-only layout projection for the position broadcast (ADR-2135).
//!
//! The physics integrator always runs on the merged, un-separated layout.
//! Just before positions are broadcast, this module rewrites the broadcast
//! copy so the populations sit where the separation controls ask:
//!
//! * **Identity** — separation 0, dual-disc off, no Z compression.
//! * **Z-scale** — separation 0, dual-disc off, `axis_compression_z < 1`:
//!   every Z is multiplied by the clamped compression.
//! * **Triangle** — separation > 0 or dual-disc on: each population is
//!   re-centred on its own median, Z-compressed, then placed on its vertex of
//!   the shared [`TriangleFrame`] (knowledge and ontology yawed to face the
//!   centroid; agents at the centroid plus their [`DriftField`] offset).
//!
//! The caller restores the pristine physics positions before the next step.
//! The projection must never feed back into the simulation buffer: the
//! ~56k knowledge↔ontology cross-links (rest length ~30) would span the gap
//! every frame and pull the populations back together.
//!
//! At separation 0 the triangle collapses to the origin with zero yaw, and
//! the projection is exactly the pre-triangle behaviour in every mode
//! (`legacy_parity` tests below hold it to the old code).

use glam::Vec3;
use visionclaw_tri_layout::{drift::DriftField, TriangleFrame, Vertex};

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

/// Low 26 bits of a wire node id; the high bits carry type flags
/// (`binary_protocol::NODE_ID_MASK`). Wire ids are compact GPU indices.
pub(crate) const NODE_ID_MASK: u32 = 0x03FF_FFFF;

/// Clamp `axis_compression_z` into `[Z_SCALE_MIN, 1.0]`.
#[inline]
pub(crate) fn clamp_z_scale(axis_compression_z: f32) -> f32 {
    axis_compression_z.clamp(Z_SCALE_MIN, 1.0)
}

/// The three separation controls, read once per broadcast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayoutParams {
    /// `graph_separation_x`, the Graph Separation slider.
    pub separation: f32,
    /// Clamped `axis_compression_z`.
    pub face_scale: f32,
    /// `enable_dual_disc_layout`.
    pub dual_disc: bool,
}

impl LayoutParams {
    pub(crate) fn new(graph_separation_x: f32, axis_compression_z: f32, dual_disc: bool) -> Self {
        LayoutParams {
            separation: if graph_separation_x.is_finite() {
                graph_separation_x.max(0.0)
            } else {
                0.0
            },
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

/// The mode for these controls. The triangle needs the population table;
/// without it the old Z-scale fallback applies.
pub(crate) fn display_mode(params: &LayoutParams, populations_len: usize) -> DisplayMode {
    if populations_len > 0 && (params.dual_disc || params.separation > 0.0) {
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
/// mode applied. `populations[i]` classifies `items[i]`; agents are offset
/// by `drift` (keyed by GPU index). Items beyond the population table are
/// left alone in triangle mode, as before.
pub(crate) fn project_display<T: HasPosition>(
    items: &mut [T],
    populations: &[GraphPopulation],
    params: &LayoutParams,
    drift: &DriftField,
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
            let frame = TriangleFrame::new(params.separation);
            let centroids = population_centroids(items, populations);
            // Dual-disc always re-centres in-plane (its discs are centred on
            // their medians at every separation); otherwise re-centring eases
            // in with the triangle so leaving separation 0 is continuous. Z is
            // re-centred only as the triangle opens, so separation 0 keeps the
            // old Z exactly.
            let w_xy = if params.dual_disc {
                1.0
            } else {
                frame.strength
            };
            let w_z = frame.strength;
            for (i, (item, &pop)) in items.iter_mut().zip(populations).enumerate() {
                let c = centroids[pop.index()];
                let p = item.position_mut();
                let mut local = Vec3::new(p.x - w_xy * c.x, p.y - w_xy * c.y, p.z - w_z * c.z);
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
                    None => {
                        let o = drift.offset(i as u32, &frame);
                        [local.x + o[0], local.y + o[1], local.z + o[2]]
                    }
                };
                *p = Vec3::from_array(placed);
            }
        }
    }
    mode
}

/// GPU index of the agent node a `0x23` `source_agent_id` names, if it is an
/// agent node of the current graph (the flag bits are masked off).
pub(crate) fn agent_index(populations: &[GraphPopulation], source_agent_id: u32) -> Option<u32> {
    let i = source_agent_id & NODE_ID_MASK;
    match populations.get(i as usize) {
        Some(GraphPopulation::Agent) => Some(i),
        _ => None,
    }
}

/// The vertex of the graph a `0x23` `target_node_id` belongs to; none for an
/// agent target or an id outside the graph.
pub(crate) fn target_vertex(
    populations: &[GraphPopulation],
    target_node_id: u32,
) -> Option<Vertex> {
    populations
        .get((target_node_id & NODE_ID_MASK) as usize)
        .and_then(|p| p.vertex())
}

/// GPU indices of every agent node.
pub(crate) fn agent_indices(populations: &[GraphPopulation]) -> Vec<u32> {
    populations
        .iter()
        .enumerate()
        .filter(|(_, p)| **p == GraphPopulation::Agent)
        .map(|(i, _)| i as u32)
        .collect()
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

    /// The pre-ADR-2135 projection, verbatim in behaviour: X-Y median
    /// re-centre, rim clamp, Z compression and the ±Z disc offset.
    fn legacy_project(pos: &mut [Vec3], pops: &[GraphPopulation], sep: f32, face: f32, dual: bool) {
        if dual && !pops.is_empty() {
            let mut bx: [Vec<f32>; 3] = Default::default();
            let mut by: [Vec<f32>; 3] = Default::default();
            for (p, pop) in pos.iter().zip(pops) {
                bx[pop.index()].push(p.x);
                by[pop.index()].push(p.y);
            }
            let med = |v: &mut Vec<f32>| {
                if v.is_empty() {
                    return 0.0;
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v[v.len() / 2]
            };
            let c: Vec<(f32, f32)> = (0..3).map(|b| (med(&mut bx[b]), med(&mut by[b]))).collect();
            for (p, pop) in pos.iter_mut().zip(pops) {
                let tz = match pop {
                    Knowledge => -sep,
                    Ontology => sep,
                    Agent => 0.0,
                };
                let (cx, cy) = c[pop.index()];
                let (mut dx, mut dy) = (p.x - cx, p.y - cy);
                let d2 = dx * dx + dy * dy;
                if d2 > DISC_RIM_RADIUS * DISC_RIM_RADIUS {
                    let s = DISC_RIM_RADIUS / d2.sqrt();
                    dx *= s;
                    dy *= s;
                }
                *p = Vec3::new(dx, dy, p.z * face + tz);
            }
        } else if (face - 1.0).abs() > f32::EPSILON {
            for p in pos.iter_mut() {
                p.z *= face;
            }
        }
    }

    #[test]
    fn legacy_parity_at_separation_zero_in_every_mode() {
        let drift = DriftField::default();
        for dual in [false, true] {
            for axis in [1.0, 0.5, 0.1, 0.0] {
                let (orig, pops) = sample(301, 7);
                let mut ours = orig.clone();
                let mut theirs = orig.clone();
                let params = LayoutParams::new(0.0, axis, dual);
                project_display(&mut ours, &pops, &params, &drift);
                legacy_project(&mut theirs, &pops, 0.0, clamp_z_scale(axis), dual);
                for (a, b) in ours.iter().zip(&theirs) {
                    assert!(
                        (*a - *b).length() < 1e-3,
                        "dual={dual} axis={axis}: {a} vs {b}"
                    );
                }
            }
        }
    }

    #[test]
    fn modes_follow_the_controls() {
        let m = |sep, axis, dual, n| display_mode(&LayoutParams::new(sep, axis, dual), n);
        assert_eq!(m(0.0, 1.0, false, 10), DisplayMode::Identity);
        assert_eq!(m(0.0, 0.4, false, 10), DisplayMode::ZScale);
        assert_eq!(m(0.0, 1.0, true, 10), DisplayMode::Triangle);
        assert_eq!(
            m(25.0, 1.0, false, 10),
            DisplayMode::Triangle,
            "separation alone opens the triangle"
        );
        assert_eq!(
            m(25.0, 0.4, false, 0),
            DisplayMode::ZScale,
            "no population table: fallback"
        );
        assert_eq!(m(f32::NAN, 1.0, false, 10), DisplayMode::Identity);
    }

    #[test]
    fn separated_populations_sit_on_their_vertices() {
        let (orig, pops) = sample(900, 11);
        for dual in [false, true] {
            let mut pos = orig.clone();
            let params = LayoutParams::new(300.0, 0.2, dual);
            project_display(&mut pos, &pops, &params, &DriftField::default());
            // A per-axis median is not rotation-equivariant, so check it in each
            // body's own frame: unplace() undoes the yaw and the vertex offset,
            // leaving the re-centred population, whose median is the origin.
            let f = TriangleFrame::new(300.0);
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

    #[test]
    fn separated_discs_face_the_centroid() {
        // A thin disc (face_scale 0.05) at full strength: its thin axis must be
        // the radial direction through the centroid.
        let (orig, pops) = sample(600, 3);
        let mut pos = orig.clone();
        project_display(
            &mut pos,
            &pops,
            &LayoutParams::new(400.0, 0.0, true),
            &DriftField::default(),
        );
        let f = TriangleFrame::new(400.0);
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
    fn projection_is_continuous_in_the_slider() {
        let (orig, pops) = sample(300, 5);
        for dual in [false, true] {
            let mut prev: Option<Vec<Vec3>> = None;
            let mut sep = 0.0;
            while sep <= 400.0 {
                let mut pos = orig.clone();
                project_display(
                    &mut pos,
                    &pops,
                    &LayoutParams::new(sep, 0.6, dual),
                    &DriftField::default(),
                );
                if let Some(p) = &prev {
                    let jump = p
                        .iter()
                        .zip(&pos)
                        .map(|(a, b)| (*a - *b).length())
                        .fold(0.0f32, f32::max);
                    // 0.25 of slider per step; the bound allows the yaw sweep of the
                    // unclamped 9000-unit runaway (dual off); a re-centre snap would be hundreds
                    assert!(jump < 50.0, "dual={dual} jump {jump} at sep {sep}");
                }
                prev = Some(pos);
                sep += 0.25;
            }
        }
    }

    #[test]
    fn agents_get_their_drift_offset() {
        let pops = vec![Knowledge, Agent, Ontology, Agent];
        let orig = vec![Vec3::ZERO; 4];
        let mut drift = DriftField::default();
        drift.step(0.0);
        drift.record_action(1, Vertex::Ontology, 0.0);
        for i in 1..=60 {
            drift.step(i as f64 * 0.1);
        }
        let params = LayoutParams::new(300.0, 1.0, false);
        let mut pos = orig.clone();
        project_display(&mut pos, &pops, &params, &drift);
        let f = TriangleFrame::new(300.0);
        let want = Vec3::from_array(drift.offset(1, &f));
        assert!(want.length() > 10.0);
        // agents' median is re-centred to the origin first; both agents sit at 0 here
        assert!((pos[1] - want).length() < 1e-3, "{} vs {}", pos[1], want);
        assert!(pos[3].length() < 1e-3, "idle agent stays at the centroid");
    }

    #[test]
    fn id_mapping_masks_flags_and_checks_populations() {
        let pops = vec![Knowledge, Ontology, Agent];
        assert_eq!(agent_index(&pops, 2 | 0x8000_0000), Some(2));
        assert_eq!(agent_index(&pops, 0), None, "not an agent node");
        assert_eq!(agent_index(&pops, 99), None);
        assert_eq!(
            target_vertex(&pops, 0x4000_0000),
            Some(Vertex::Knowledge)
        );
        assert_eq!(
            target_vertex(&pops, 1 | 0x0400_0000),
            Some(Vertex::Ontology)
        );
        assert_eq!(
            target_vertex(&pops, 2),
            None,
            "agent targets have no vertex"
        );
        assert_eq!(target_vertex(&pops, 7), None);
        assert_eq!(agent_indices(&pops), vec![2]);
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
