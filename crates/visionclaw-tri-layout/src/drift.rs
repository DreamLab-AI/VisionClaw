//! How agents move between the three graphs while the layout is separated.
//!
//! Every agent starts at the triangle's centroid. Each action it takes adds
//! credit to the vertex of the body it touched: a `0x23` action on a
//! knowledge node credits [`Vertex::Knowledge`], one on an ontology node
//! [`Vertex::Ontology`], and a `memory_flash` credits [`Vertex::Memory`].
//! Credit halves every [`HALF_LIFE_S`] seconds. The agent's target is
//!
//! ```text
//! target = REACH × Σ_v w_v · vertex_v / (IDLE_WEIGHT + Σ_v w_v)
//! ```
//!
//! — a convex combination of the centroid and the three vertices whose
//! vertex share never exceeds [`REACH`], so the target stays inside the
//! triangle, short of the graphs themselves, and falls back to the centroid
//! as the credit decays. The displayed position follows the target through
//! a first-order lag of time constant [`FOLLOW_S`]: it approaches without
//! overshoot, so an agent never oscillates, and a reduced-motion viewer sees
//! a calm glide rather than a jump.
//!
//! The state is kept as vertex *coefficients* rather than a position, so the
//! offset is recomputed from the current [`TriangleFrame`]: moving the slider
//! carries every agent with the triangle and none can end up outside it.
//!
//! Everything is a pure function of event times and step times (seconds on
//! any monotonic clock), so a replayed sequence gives identical positions.
//!
//! ```
//! use visionclaw_tri_layout::{drift::DriftField, TriangleFrame, Vertex};
//!
//! let frame = TriangleFrame::new(300.0);
//! let mut field = DriftField::default();
//! field.record_action(7, Vertex::Ontology, 0.0);
//! for i in 1..=50 {
//!     field.step(i as f64 * 0.1);
//! }
//! let p = field.offset(7, &frame);
//! assert!(p[0] > 0.0, "drifted towards the ontology, front-right");
//! assert_eq!(field.offset(8, &frame), [0.0; 3], "an idle agent stays home");
//! ```

use std::collections::BTreeMap;

use crate::{TriangleFrame, Vec3, Vertex};

/// Seconds for an agent's credit at a vertex to halve.
pub const HALF_LIFE_S: f64 = 6.0;

/// Weight of the centroid in the target: one fresh action pulls an agent
/// `REACH × 1/(1 + IDLE_WEIGHT)` of the way to that vertex.
pub const IDLE_WEIGHT: f32 = 1.0;

/// Largest fraction of the way to a vertex an agent can travel, so agents
/// hover beside a graph instead of landing inside it.
pub const REACH: f32 = 0.6;

/// Time constant, seconds, of the lag between target and displayed offset.
pub const FOLLOW_S: f64 = 1.5;

/// Below this total credit and coefficient an agent counts as home and its
/// state is dropped.
pub const SETTLED_EPSILON: f32 = 1e-3;

/// One agent's activity and eased offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentActivity {
    weights: [f32; 3],
    weights_at: f64,
    coeffs: [f32; 3],
    stepped_at: Option<f64>,
}

impl AgentActivity {
    /// A fresh agent at the centroid with no credit, clocked from `now`.
    pub fn new(now: f64) -> Self {
        AgentActivity {
            weights: [0.0; 3],
            weights_at: now,
            coeffs: [0.0; 3],
            stepped_at: None,
        }
    }

    /// Credit `credit` to vertex `v` at time `now` (earlier credit decays to
    /// `now` first). A non-positive or non-finite credit is ignored.
    pub fn record(&mut self, v: Vertex, credit: f32, now: f64) {
        if !(credit.is_finite() && credit > 0.0) {
            return;
        }
        self.weights = self.weights_at(now);
        self.weights_at = self.weights_at.max(now);
        self.weights[v.index()] += credit;
    }

    /// Credit per vertex decayed to `now`. A `now` before the last record
    /// does not grow the credit back.
    pub fn weights_at(&self, now: f64) -> [f32; 3] {
        let dt = (now - self.weights_at).max(0.0);
        let k = 0.5f64.powf(dt / HALF_LIFE_S) as f32;
        self.weights.map(|w| w * k)
    }

    /// Target vertex coefficients at `now`: each in `[0, REACH]`, summing to
    /// at most [`REACH`].
    pub fn target_coeffs(&self, now: f64) -> [f32; 3] {
        let w = self.weights_at(now);
        let total: f32 = w.iter().sum();
        let denom = IDLE_WEIGHT + total;
        w.map(|x| REACH * x / denom)
    }

    /// Advance the eased coefficients to `now` and return them. The first
    /// step only starts the clock: an agent appears at the centroid.
    pub fn step(&mut self, now: f64) -> [f32; 3] {
        let target = self.target_coeffs(now);
        let alpha = match self.stepped_at {
            None => 0.0,
            Some(t) => 1.0 - (-((now - t).max(0.0)) / FOLLOW_S).exp() as f32,
        };
        for (c, t) in self.coeffs.iter_mut().zip(target) {
            *c += (t - *c) * alpha;
        }
        self.stepped_at = Some(self.stepped_at.map_or(now, |t| t.max(now)));
        self.coeffs
    }

    /// The eased coefficients as of the last [`step`](Self::step).
    pub fn coeffs(&self) -> [f32; 3] {
        self.coeffs
    }

    /// Display offset from the centroid in `frame`: `Σ_v c_v · vertex_v`.
    pub fn offset(&self, frame: &TriangleFrame) -> Vec3 {
        let mut out = [0.0f32; 3];
        for v in Vertex::ALL {
            let p = frame.vertex(v);
            let c = self.coeffs[v.index()];
            for k in 0..3 {
                out[k] += c * p[k];
            }
        }
        out
    }

    /// True once both the credit and the eased coefficients are negligible.
    pub fn is_settled(&self, now: f64) -> bool {
        self.weights_at(now).iter().sum::<f32>() < SETTLED_EPSILON
            && self.coeffs.iter().sum::<f32>() < SETTLED_EPSILON
    }
}

/// Activity of every agent, keyed by the agent's node index.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DriftField {
    agents: BTreeMap<u32, AgentActivity>,
}

impl DriftField {
    /// An agent acted on a node of body `target` at `now`.
    pub fn record_action(&mut self, agent: u32, target: Vertex, now: f64) {
        self.agents
            .entry(agent)
            .or_insert_with(|| AgentActivity::new(now))
            .record(target, 1.0, now);
    }

    /// A memory access at `now`. When the producer named the agent it gets
    /// full credit; an anonymous flash is shared equally by `all_agents`
    /// (the agents present), pulling the swarm as a whole towards the cloud.
    pub fn record_memory(&mut self, agent: Option<u32>, all_agents: &[u32], now: f64) {
        match agent {
            Some(a) => self.record_one(a, 1.0, now),
            None if !all_agents.is_empty() => {
                let credit = 1.0 / all_agents.len() as f32;
                for &a in all_agents {
                    self.record_one(a, credit, now);
                }
            }
            None => {}
        }
    }

    fn record_one(&mut self, agent: u32, credit: f32, now: f64) {
        self.agents
            .entry(agent)
            .or_insert_with(|| AgentActivity::new(now))
            .record(Vertex::Memory, credit, now);
    }

    /// Ease every agent to `now` and drop those that are home again.
    pub fn step(&mut self, now: f64) {
        for a in self.agents.values_mut() {
            a.step(now);
        }
        self.agents.retain(|_, a| !a.is_settled(now));
    }

    /// Keep only agents for which `keep` is true (e.g. after a graph reload
    /// renumbers nodes).
    pub fn retain(&mut self, mut keep: impl FnMut(u32) -> bool) {
        self.agents.retain(|&k, _| keep(k));
    }

    /// Display offset of `agent` from the centroid; zero when it has no
    /// recent activity.
    pub fn offset(&self, agent: u32, frame: &TriangleFrame) -> Vec3 {
        self.agents
            .get(&agent)
            .map_or([0.0; 3], |a| a.offset(frame))
    }

    /// Activity state of one agent, if it has any.
    pub fn get(&self, agent: u32) -> Option<&AgentActivity> {
        self.agents.get(&agent)
    }

    /// Number of agents with live activity.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// True when no agent has live activity.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(field: &mut DriftField, from: f64, to: f64, dt: f64) {
        let mut t = from;
        while t < to - 1e-9 {
            t += dt;
            field.step(t);
        }
    }

    /// Barycentric test: p inside the (scaled) triangle spanned by the frame's vertices.
    fn inside_triangle(p: Vec3, f: &TriangleFrame) -> bool {
        let [a, b, c] = f.vertices;
        let s = |p1: Vec3, p2: Vec3, p3: Vec3| {
            (p1[0] - p3[0]) * (p2[2] - p3[2]) - (p2[0] - p3[0]) * (p1[2] - p3[2])
        };
        let d1 = s(p, a, b);
        let d2 = s(p, b, c);
        let d3 = s(p, c, a);
        let neg = d1 < -1e-3 || d2 < -1e-3 || d3 < -1e-3;
        let pos = d1 > 1e-3 || d2 > 1e-3 || d3 > 1e-3;
        !(neg && pos)
    }

    #[test]
    fn a_new_agent_starts_at_the_centroid() {
        let f = TriangleFrame::new(300.0);
        let mut field = DriftField::default();
        field.record_action(1, Vertex::Knowledge, 10.0);
        field.step(10.0);
        assert_eq!(field.offset(1, &f), [0.0; 3]);
    }

    #[test]
    fn activity_pulls_towards_the_touched_vertex() {
        let f = TriangleFrame::new(300.0);
        for v in Vertex::ALL {
            let mut field = DriftField::default();
            field.step(0.0);
            field.record_action(1, v, 0.0);
            run(&mut field, 0.0, 6.0, 0.05);
            let p = field.offset(1, &f);
            let target = f.vertex(v);
            let along = (p[0] * target[0] + p[2] * target[2]) / (f.radius * f.radius);
            assert!(along > 0.1 && along < REACH, "{v:?}: fraction {along}");
        }
    }

    #[test]
    fn idle_agents_return_home_and_are_dropped() {
        let f = TriangleFrame::new(300.0);
        let mut field = DriftField::default();
        field.record_action(3, Vertex::Ontology, 0.0);
        run(&mut field, 0.0, 3.0, 0.1);
        assert!(field.offset(3, &f)[0] > 1.0);
        run(&mut field, 3.0, 200.0, 0.5);
        assert!(field.is_empty(), "settled agent pruned");
        assert_eq!(field.offset(3, &f), [0.0; 3]);
    }

    #[test]
    fn one_action_rises_then_falls_without_overshoot() {
        let f = TriangleFrame::new(400.0);
        let mut field = DriftField::default();
        field.step(0.0);
        field.record_action(9, Vertex::Knowledge, 0.0);
        // The target peaks at the action (REACH / 2) and only decays after it;
        // the eased coefficient must rise to one peak and then fall, never
        // above that target peak: no ringing, no overshoot.
        let peak_target = REACH / (1.0 + IDLE_WEIGHT);
        let mut series = Vec::new();
        let mut t = 0.0;
        while t < 300.0 {
            t += 0.1;
            field.step(t);
            match field.get(9) {
                Some(a) => series.push(a.coeffs()[0]),
                None => break,
            }
        }
        let turns = series
            .windows(3)
            .filter(|w| (w[1] - w[0]) * (w[2] - w[1]) < 0.0)
            .count();
        assert!(turns <= 1, "{turns} turning points");
        assert!(series.iter().all(|&c| c <= peak_target + 1e-6));
        assert!(series.len() > 10);
        assert!(inside_triangle(field.offset(9, &f), &f));
    }

    #[test]
    fn offsets_stay_inside_the_triangle_under_mixed_activity() {
        let f = TriangleFrame::new(250.0);
        let mut field = DriftField::default();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut t = 0.0;
        for _ in 0..2000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let agent = (seed % 5) as u32;
            match (seed >> 8) % 4 {
                0 => field.record_action(agent, Vertex::Knowledge, t),
                1 => field.record_action(agent, Vertex::Ontology, t),
                2 => field.record_memory(Some(agent), &[], t),
                _ => field.record_memory(None, &[0, 1, 2, 3, 4], t),
            }
            t += 0.05;
            field.step(t);
            for a in 0..5 {
                let p = field.offset(a, &f);
                assert!(inside_triangle(p, &f), "agent {a} at {p:?}");
                let c = field.get(a).map_or([0.0; 3], |x| x.coeffs());
                assert!(c.iter().sum::<f32>() <= REACH + 1e-5);
            }
        }
    }

    #[test]
    fn anonymous_memory_flashes_are_shared_by_all_agents() {
        let mut field = DriftField::default();
        field.record_memory(None, &[1, 2, 3, 4], 0.0);
        for a in 1..=4 {
            let w = field.get(a).unwrap().weights_at(0.0);
            assert_eq!(w, [0.0, 0.0, 0.25]);
        }
        field.record_memory(None, &[], 0.0);
        assert_eq!(field.len(), 4, "no agents present: nothing recorded");
    }

    #[test]
    fn credit_halves_every_half_life() {
        let mut a = AgentActivity::new(0.0);
        a.record(Vertex::Memory, 1.0, 0.0);
        let w = a.weights_at(HALF_LIFE_S)[2];
        assert!((w - 0.5).abs() < 1e-6);
        assert_eq!(
            a.weights_at(-5.0)[2],
            1.0,
            "earlier now does not grow credit"
        );
    }

    #[test]
    fn replay_is_deterministic() {
        let script = |field: &mut DriftField| {
            field.record_action(1, Vertex::Knowledge, 0.0);
            field.record_memory(None, &[1, 2], 0.4);
            field.record_action(2, Vertex::Ontology, 1.1);
            run(field, 0.0, 5.0, 1.0 / 60.0);
        };
        let mut a = DriftField::default();
        let mut b = DriftField::default();
        script(&mut a);
        script(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn slider_changes_carry_agents_with_the_triangle() {
        let mut field = DriftField::default();
        field.step(0.0);
        field.record_action(5, Vertex::Ontology, 0.0);
        run(&mut field, 0.0, 4.0, 0.1);
        let near = field.offset(5, &TriangleFrame::new(100.0));
        let far = field.offset(5, &TriangleFrame::new(400.0));
        assert!(
            (far[0] / near[0] - 4.0).abs() < 1e-3,
            "scales with the radius"
        );
        assert_eq!(
            field.offset(5, &TriangleFrame::new(0.0)),
            [0.0; 3],
            "merged: home"
        );
    }

    #[test]
    fn retain_drops_unknown_agents() {
        let mut field = DriftField::default();
        field.record_action(1, Vertex::Knowledge, 0.0);
        field.record_action(2, Vertex::Knowledge, 0.0);
        field.retain(|a| a == 2);
        assert!(field.get(1).is_none() && field.get(2).is_some());
    }
}
