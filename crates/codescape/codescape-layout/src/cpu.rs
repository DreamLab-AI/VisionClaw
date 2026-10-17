//! The CPU engine: Barnes-Hut repulsion, CSR springs, host terms and
//! the GPU kernel's damped semi-implicit Euler, parallel over bodies with rayon.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::time::Instant;

use codescape_model::{GraphDelta, GraphSnapshot};
use rayon::prelude::*;

use crate::model::Model;
use crate::octree::{Octree, Repulsion};
use crate::seed::place;
use crate::{LayoutEngine, LayoutError, LayoutParams, PositionSet, StepStats, SETTLE_STEPS};

/// Softening `ε` in `repulsion / (d² + ε)`, in squared world units.
pub(crate) const REPULSION_SOFTENING: f32 = 1.0;

/// Barnes-Hut force layout on the CPU. It is also the conformance oracle for
/// the CUDA engine.
///
/// ```
/// use codescape_layout::{CpuEngine, LayoutEngine, LayoutParams};
///
/// let engine = CpuEngine::new(LayoutParams::default());
/// assert_eq!(engine.name(), "cpu");
/// assert!(!engine.settled());
/// ```
#[derive(Debug)]
pub struct CpuEngine {
    params: LayoutParams,
    state: State,
    tree: Octree,
    cache: OnceCell<PositionSet>,
}

/// Simulation state, shared in shape with the CUDA engine's host mirror.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub model: Model,
    pub pos: Vec<[f32; 3]>,
    pub vel: Vec<[f32; 3]>,
    pub acc: Vec<[f32; 3]>,
    /// Every node's position: bodies plus aggregate centroids.
    pub node_pos: Vec<[f32; 3]>,
    pub focus_ids: Option<Vec<u32>>,
    /// Bodies being integrated, or `None` for all.
    pub active: Option<Vec<u32>>,
    pub calm: u32,
    pub host: Vec<[f32; 3]>,
    /// `true` once a snapshot has been loaded.
    pub loaded: bool,
}

impl State {
    /// Compile `snapshot` with initial positions by id; velocities by id where known.
    pub fn new(
        snapshot: &GraphSnapshot,
        params: &LayoutParams,
        known_pos: &HashMap<u32, [f32; 3]>,
        known_vel: &HashMap<u32, [f32; 3]>,
        incremental: bool,
    ) -> Self {
        let model = Model::build(snapshot, params);
        let node_pos = place(snapshot, params, known_pos, incremental);
        let pos: Vec<[f32; 3]> = model
            .body_node
            .iter()
            .map(|&ni| node_pos[ni as usize])
            .collect();
        let vel = model
            .body_node
            .iter()
            .map(|&ni| {
                known_vel
                    .get(&model.ids[ni as usize])
                    .copied()
                    .unwrap_or([0.0; 3])
            })
            .collect();
        let mut node_pos = node_pos;
        model.aggregate(&pos, &mut node_pos);
        let nb = model.n_bodies();
        State {
            model,
            pos,
            vel,
            acc: vec![[0.0; 3]; nb],
            node_pos,
            host: vec![[0.0; 3]; nb],
            loaded: true,
            ..Default::default()
        }
    }

    /// Position and velocity of every node by id, for warm starts.
    pub fn by_id(&self) -> (HashMap<u32, [f32; 3]>, HashMap<u32, [f32; 3]>) {
        let pos = self
            .model
            .ids
            .iter()
            .copied()
            .zip(self.node_pos.iter().copied())
            .collect();
        let vel = self
            .model
            .body_node
            .iter()
            .map(|&ni| self.model.ids[ni as usize])
            .zip(self.vel.iter().copied())
            .collect();
        (pos, vel)
    }

    /// Resolve the focus ids against the current model.
    pub fn refocus(&mut self) {
        self.active = self
            .focus_ids
            .as_ref()
            .map(|ids| self.model.focus_bodies(ids));
        self.calm = 0;
    }

    /// The public view.
    pub fn position_set(&self) -> PositionSet {
        let mut vel = vec![[0.0f32; 3]; self.model.n_nodes()];
        for (b, &ni) in self.model.body_node.iter().enumerate() {
            vel[ni as usize] = self.vel[b];
        }
        PositionSet {
            ids: self.model.ids.clone(),
            pos: self.node_pos.clone(),
            vel,
        }
    }

    /// Record a step's energy for [`LayoutEngine::settled`].
    pub fn record_energy(&mut self, energy: f32, params: &LayoutParams) {
        let n = self
            .active
            .as_ref()
            .map_or(self.model.n_bodies(), Vec::len)
            .max(1);
        if energy / n as f32 <= params.settle_energy {
            self.calm = self.calm.saturating_add(1);
        } else {
            self.calm = 0;
        }
    }

    pub fn settled(&self) -> bool {
        self.calm >= SETTLE_STEPS
    }
}

fn snapshot_known(warm: Option<&PositionSet>) -> (HashMap<u32, [f32; 3]>, HashMap<u32, [f32; 3]>) {
    match warm {
        Some(w) => (
            w.ids.iter().copied().zip(w.pos.iter().copied()).collect(),
            w.ids.iter().copied().zip(w.vel.iter().copied()).collect(),
        ),
        None => (HashMap::new(), HashMap::new()),
    }
}

/// Validate a delta against the snapshot that claims to be its result.
pub(crate) fn check_delta(
    loaded: bool,
    snapshot_after: &GraphSnapshot,
    delta: &GraphDelta,
) -> Result<(), LayoutError> {
    if !loaded {
        return Err(LayoutError::DeltaMismatch("no snapshot loaded".into()));
    }
    if delta.to_revision != snapshot_after.revision {
        return Err(LayoutError::DeltaMismatch(format!(
            "delta leads to revision {}, snapshot is at {}",
            delta.to_revision, snapshot_after.revision
        )));
    }
    snapshot_after
        .validate()
        .map_err(LayoutError::InvalidSnapshot)
}

/// Warm state for `snapshot_after` from the current state, minus removed nodes.
pub(crate) fn state_after_delta(
    old: &State,
    snapshot_after: &GraphSnapshot,
    delta: &GraphDelta,
    params: &LayoutParams,
) -> State {
    let (mut pos, mut vel) = old.by_id();
    for id in &delta.removed_nodes {
        pos.remove(id);
        vel.remove(id);
    }
    // Added ids are new even if a stale position lingers under that id.
    for n in &delta.added_nodes {
        pos.remove(&n.id);
        vel.remove(&n.id);
    }
    let mut s = State::new(snapshot_after, params, &pos, &vel, true);
    s.focus_ids = old.focus_ids.clone();
    s.refocus();
    s
}

impl CpuEngine {
    /// A new engine with nothing loaded.
    pub fn new(params: LayoutParams) -> Self {
        Self {
            params,
            state: State::default(),
            tree: Octree::default(),
            cache: OnceCell::new(),
        }
    }

    /// The parameters in use.
    pub fn params(&self) -> &LayoutParams {
        &self.params
    }

    /// Net force on every active body at the current positions, into `acc`.
    fn compute_acc(&mut self) {
        let s = &mut self.state;
        let p = &self.params;
        s.model.aggregate(&s.pos, &mut s.node_pos);
        s.model.host_forces(&s.pos, &s.node_pos, &mut s.host);
        self.tree.rebuild(&s.pos);
        let law = Repulsion {
            k: p.repulsion,
            eps: REPULSION_SOFTENING,
            max_pair: p.max_force(),
            theta2: p.theta * p.theta,
        };
        let max_force = p.max_force();
        let (tree, model, pos, host) = (&self.tree, &s.model, &s.pos, &s.host);
        let force = |b: usize| -> [f32; 3] {
            let x = pos[b];
            let mut f = tree.force_on(b as u32, x, pos, law);
            for k in model.row[b]..model.row[b + 1] {
                let j = model.col[k as usize] as usize;
                let w = p.spring_k * model.weight[k as usize];
                for a in 0..3 {
                    f[a] += w * (pos[j][a] - x[a]);
                }
            }
            for a in 0..3 {
                f[a] += host[b][a] - p.gravity * x[a];
            }
            clamp(f, max_force)
        };
        match &s.active {
            None => s
                .acc
                .par_iter_mut()
                .enumerate()
                .for_each(|(b, a)| *a = force(b)),
            Some(active) => {
                let forces: Vec<[f32; 3]> = active.par_iter().map(|&b| force(b as usize)).collect();
                for (&b, f) in active.iter().zip(forces) {
                    s.acc[b as usize] = f;
                }
            }
        }
    }

    /// One step of the integrator `integrate_pass_kernel` uses, so both engines
    /// follow the same trajectory for the same forces:
    /// `v ← clamp((v + F·dt)·damping, max_velocity)`, then `x ← x + v·dt`.
    /// `F` is already clamped to `max_force` by [`CpuEngine::compute_acc`].
    fn one_step(&mut self) -> (f32, f32) {
        self.compute_acc();
        let (dt, damping, vmax) = (
            self.params.dt,
            self.params.damping,
            self.params.max_velocity,
        );
        let s = &mut self.state;
        let (pos, vel, acc) = (&mut s.pos, &mut s.vel, &s.acc);
        let mut kick = |b: usize| -> (f64, f32) {
            let mut v = [0.0f32; 3];
            for k in 0..3 {
                v[k] = (vel[b][k] + acc[b][k] * dt) * damping;
            }
            let v = clamp(v, vmax);
            let mut d2 = 0.0f32;
            for k in 0..3 {
                let dx = v[k] * dt;
                pos[b][k] += dx;
                d2 += dx * dx;
            }
            vel[b] = v;
            (
                f64::from(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]),
                d2.sqrt(),
            )
        };
        let (mut energy, mut max_disp) = (0.0f64, 0.0f32);
        let mut account = |(e, d): (f64, f32)| {
            energy += e;
            max_disp = max_disp.max(d);
        };
        match &s.active {
            Some(active) => active.iter().for_each(|&b| account(kick(b as usize))),
            None => (0..acc.len()).for_each(|b| account(kick(b))),
        }
        // Aggregates follow the new body positions.
        s.model.aggregate(&s.pos, &mut s.node_pos);
        (energy as f32, max_disp)
    }
}

/// Scale `v` down to length `max` if it is longer; zero it if it is not finite.
pub(crate) fn clamp(v: [f32; 3], max: f32) -> [f32; 3] {
    let n2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if !n2.is_finite() {
        return [0.0; 3];
    }
    if n2 > max * max {
        let s = max / n2.sqrt();
        [v[0] * s, v[1] * s, v[2] * s]
    } else {
        v
    }
}

impl LayoutEngine for CpuEngine {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn load(
        &mut self,
        snapshot: &GraphSnapshot,
        warm: Option<&PositionSet>,
    ) -> Result<(), LayoutError> {
        snapshot.validate().map_err(LayoutError::InvalidSnapshot)?;
        let (pos, vel) = snapshot_known(warm);
        let focus = self.state.focus_ids.take();
        self.state = State::new(snapshot, &self.params, &pos, &vel, false);
        self.state.focus_ids = focus;
        self.state.refocus();
        self.cache = OnceCell::new();
        Ok(())
    }

    fn apply_delta(
        &mut self,
        snapshot_after: &GraphSnapshot,
        delta: &GraphDelta,
    ) -> Result<(), LayoutError> {
        check_delta(self.state.loaded, snapshot_after, delta)?;
        self.state = state_after_delta(&self.state, snapshot_after, delta, &self.params);
        self.cache = OnceCell::new();
        Ok(())
    }

    fn set_focus_mask(&mut self, ids: Option<&[u32]>) {
        self.state.focus_ids = ids.map(<[u32]>::to_vec);
        self.state.refocus();
    }

    fn step(&mut self, n: u32) -> StepStats {
        let t0 = Instant::now();
        let mut stats = StepStats {
            steps: n,
            ..Default::default()
        };
        if self.state.model.n_bodies() > 0 {
            for _ in 0..n {
                let (e, d) = self.one_step();
                stats.energy = e;
                stats.max_displacement = stats.max_displacement.max(d);
                self.state.record_energy(e, &self.params);
            }
            // compute_acc left aggregates at the pre-kick positions of this step's
            // drift, which are the current positions: node_pos is up to date.
        }
        self.cache = OnceCell::new();
        stats.millis = t0.elapsed().as_secs_f32() * 1000.0;
        stats.host_ms = stats.millis;
        stats
    }

    fn positions(&self) -> &PositionSet {
        self.cache.get_or_init(|| self.state.position_set())
    }

    fn settled(&self) -> bool {
        self.state.settled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::test_graphs::*;
    use codescape_model::{NodeKind, Relation, Tier};

    fn centroid(ps: &PositionSet, ids: &[u32]) -> [f32; 3] {
        let mut c = [0.0f32; 3];
        for id in ids {
            let p = ps.position_of(*id).unwrap();
            for k in 0..3 {
                c[k] += p[k] / ids.len() as f32;
            }
        }
        c
    }

    fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn steps_are_deterministic() {
        let g = repo_graph(&Repo {
            dirs: 3,
            files_per_dir: 4,
            symbols_per_file: 6,
            calls_per_symbol: 2,
            seed: 9,
        });
        let run = || {
            let mut e = CpuEngine::new(LayoutParams::default());
            e.load(&g, None).unwrap();
            e.step(30);
            e.positions().clone()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn two_cliques_separate() {
        let g = two_cliques(30);
        let mut e = CpuEngine::new(LayoutParams::default());
        e.load(&g, None).unwrap();
        e.step(400);
        let ps = e.positions();
        let a: Vec<u32> = (1..=30).collect();
        let b: Vec<u32> = (31..=60).collect();
        let (ca, cb) = (centroid(ps, &a), centroid(ps, &b));
        let radius = |ids: &[u32], c: [f32; 3]| {
            ids.iter()
                .map(|&i| dist(ps.position_of(i).unwrap(), c))
                .fold(0.0f32, f32::max)
        };
        let (ra, rb) = (radius(&a, ca), radius(&b, cb));
        let sep = dist(ca, cb);
        assert!(
            sep > ra.max(rb),
            "inter-centroid {sep} vs intra radius {ra} / {rb}"
        );
    }

    #[test]
    fn star_is_roughly_symmetric() {
        let g = star(40);
        let mut e = CpuEngine::new(LayoutParams {
            gravity: 0.0,
            ..LayoutParams::default()
        });
        e.load(&g, None).unwrap();
        e.step(500);
        let ps = e.positions();
        let hub = ps.position_of(1).unwrap();
        let leaves: Vec<u32> = (2..42).collect();
        let c = centroid(ps, &leaves);
        let d: Vec<f32> = leaves
            .iter()
            .map(|&i| dist(ps.position_of(i).unwrap(), hub))
            .collect();
        let mean = d.iter().sum::<f32>() / d.len() as f32;
        let sd = (d.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / d.len() as f32).sqrt();
        assert!(
            dist(c, hub) < 0.25 * mean,
            "leaf centroid {} off the hub (mean radius {mean})",
            dist(c, hub)
        );
        assert!(sd < 0.25 * mean, "leaf radii spread {sd} around {mean}");
    }

    #[test]
    fn warm_start_keeps_unrelated_nodes_exactly() {
        let g = repo_graph(&Repo {
            dirs: 3,
            files_per_dir: 3,
            symbols_per_file: 4,
            calls_per_symbol: 1,
            seed: 11,
        });
        let mut e = CpuEngine::new(LayoutParams::default());
        e.load(&g, None).unwrap();
        e.step(50);
        let before = e.positions().clone();

        // Add a symbol to one file and drop a symbol from another.
        let mut after = g.clone();
        after.revision += 1;
        let host = after
            .nodes
            .iter()
            .find(|n| n.tier == Tier::L1)
            .unwrap()
            .clone();
        let mut new = node(50_000, "new::sym", NodeKind::Function, host.path.as_deref());
        new.parent = host.id;
        new.community = 1;
        after.nodes.push(new);
        after.edges.push(edge(host.id, 50_000, Relation::Contains));
        let victim = after
            .nodes
            .iter()
            .rev()
            .find(|n| n.tier == Tier::L2 && n.parent != host.id)
            .unwrap()
            .id;
        after.nodes.retain(|n| n.id != victim);
        after
            .edges
            .retain(|e| e.source != victim && e.target != victim);
        let delta = GraphDelta::diff(&g, &after);
        e.apply_delta(&after, &delta).unwrap();

        let ps = e.positions();
        assert_eq!(ps.len(), after.nodes.len());
        let host_pos = before.position_of(host.id).unwrap();
        let p_new = ps.position_of(50_000).unwrap();
        assert!(
            dist(p_new, host_pos) < 0.01 * e.params().shell_radius,
            "new node seeded near its file"
        );
        assert!(ps.position_of(victim).is_none());
        let unrelated = after
            .nodes
            .iter()
            .find(|n| n.tier == Tier::L2 && n.parent != host.id && n.id != 50_000)
            .unwrap();
        assert_eq!(
            ps.position_of(unrelated.id),
            before.position_of(unrelated.id),
            "an unrelated symbol does not move on apply_delta"
        );

        // A focused burst on the dirty set leaves unrelated, unconnected bodies exactly in place.
        e.set_focus_mask(Some(&delta.dirty_ids()));
        e.step(20);
        let model = &e.state.model;
        let active = e.state.active.clone().unwrap();
        let ps = e.positions();
        for (b, &ni) in model.body_node.iter().enumerate() {
            if !active.contains(&(b as u32)) {
                let id = model.ids[ni as usize];
                assert_eq!(
                    ps.position_of(id),
                    before.position_of(id),
                    "frozen body {id} moved"
                );
            }
        }
    }

    #[test]
    fn focus_mask_freezes_everything_else_exactly() {
        let g = repo_graph(&Repo {
            dirs: 2,
            files_per_dir: 3,
            symbols_per_file: 5,
            calls_per_symbol: 1,
            seed: 13,
        });
        let mut e = CpuEngine::new(LayoutParams::default());
        e.load(&g, None).unwrap();
        e.step(5);
        let before = e.positions().clone();
        let target = g.nodes.iter().find(|n| n.tier == Tier::L2).unwrap().id;
        e.set_focus_mask(Some(&[target]));
        let active: Vec<u32> = e.state.active.clone().unwrap();
        let active_ids: Vec<u32> = active
            .iter()
            .map(|&b| e.state.model.ids[e.state.model.body_node[b as usize] as usize])
            .collect();
        assert!(active_ids.contains(&target));
        e.step(25);
        let after = e.positions().clone();
        let mut moved = false;
        for (i, id) in after.ids.iter().enumerate() {
            let is_body = e.state.model.node_body[i] != crate::model::NONE;
            if !is_body {
                continue;
            }
            if active_ids.contains(id) {
                moved |= after.pos[i] != before.position_of(*id).unwrap();
            } else {
                assert_eq!(
                    after.pos[i],
                    before.position_of(*id).unwrap(),
                    "body {id} outside the focus moved"
                );
            }
        }
        assert!(moved, "the focused bodies integrate");
        e.set_focus_mask(None);
        assert!(e.state.active.is_none());
    }

    #[test]
    fn settles_on_a_2k_random_graph() {
        let g = repo_graph(&Repo {
            dirs: 8,
            files_per_dir: 10,
            symbols_per_file: 25,
            calls_per_symbol: 2,
            seed: 17,
        });
        assert!(g.tier_counts()[2] >= 2000);
        let mut e = CpuEngine::new(LayoutParams::default());
        e.load(&g, None).unwrap();
        let mut steps = 0;
        while !e.settled() && steps < 3000 {
            e.step(50);
            steps += 50;
        }
        assert!(e.settled(), "not settled after {steps} steps");
    }

    #[test]
    fn delta_for_another_revision_is_refused() {
        let g = star(3);
        let mut e = CpuEngine::new(LayoutParams::default());
        e.load(&g, None).unwrap();
        let mut after = g.clone();
        after.revision = 7;
        let delta = GraphDelta {
            from_revision: 1,
            to_revision: 2,
            ..Default::default()
        };
        assert!(matches!(
            e.apply_delta(&after, &delta),
            Err(LayoutError::DeltaMismatch(_))
        ));
    }

    /// Cold seed → settled, and `apply_delta` latency, at 100k symbols.
    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn bench_cpu_settle_and_delta_100k() {
        let g = repo_graph(&Repo {
            dirs: 50,
            files_per_dir: 40,
            symbols_per_file: 50,
            calls_per_symbol: 3,
            seed: 21,
        });
        let mut e = CpuEngine::new(LayoutParams::default());
        let t = Instant::now();
        e.load(&g, None).unwrap();
        let load_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let mut steps = 0u32;
        while !e.settled() && steps < 3000 {
            let st = e.step(25);
            steps += 25;
            if steps.is_multiple_of(500) {
                let n = e.state.model.n_bodies() as f32;
                println!(
                    "  step {steps}: mean |v|² {:.4}, max displacement {:.2}",
                    st.energy / n,
                    st.max_displacement
                );
            }
        }
        let settle_s = t.elapsed().as_secs_f64();
        let settled = e.settled();
        let (after, delta) = one_symbol_delta(&g);
        let t = Instant::now();
        e.apply_delta(&after, &delta).unwrap();
        let delta_ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "cpu 100k: load {load_ms:.0} ms; settled={settled} after {steps} steps in {settle_s:.1} s; apply_delta {delta_ms:.0} ms"
        );
    }

    /// Diagnostic: where the late-plateau kinetic energy lives.
    #[test]
    #[ignore = "diagnostic: run in release with --ignored --nocapture"]
    fn energy_concentration_on_plateau() {
        let g = repo_graph(&Repo {
            dirs: 80,
            files_per_dir: 10,
            symbols_per_file: 25,
            calls_per_symbol: 2,
            seed: 1,
        });
        let mut e = CpuEngine::new(LayoutParams {
            seed: 1,
            ..LayoutParams::default()
        });
        e.load(&g, None).unwrap();
        for chunk in 1..=8 {
            e.step(500);
            let m = &e.state.model;
            let mut v2: Vec<(f32, usize)> = e
                .state
                .vel
                .iter()
                .enumerate()
                .map(|(b, v)| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2], b))
                .collect();
            v2.sort_by(|a, b| b.0.total_cmp(&a.0));
            let total: f32 = v2.iter().map(|x| x.0).sum();
            let top = |f: f32| {
                v2[..((v2.len() as f32 * f) as usize).max(1)]
                    .iter()
                    .map(|x| x.0)
                    .sum::<f32>()
                    / total
            };
            let median = v2[v2.len() / 2].0;
            // Energy in the bulk motion of whole files vs motion within them.
            let mut fsum: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
            for (b, v) in e.state.vel.iter().enumerate() {
                let s = fsum.entry(m.body_group[b]).or_insert(([0.0; 3], 0.0));
                for k in 0..3 {
                    s.0[k] += v[k];
                }
                s.1 += 1.0;
            }
            let bulk: f32 = (0..e.state.vel.len())
                .map(|b| {
                    let (sv, c) = fsum[&m.body_group[b]];
                    (0..3).map(|k| (sv[k] / c).powi(2)).sum::<f32>()
                })
                .sum();
            let top_ids: Vec<&str> = v2[..3]
                .iter()
                .map(|x| g.nodes[m.body_node[x.1] as usize].key.as_ref())
                .collect();
            println!("step {}: mean {:.4} median {:.5} top1% {:.2} top10% {:.2} file-bulk share {:.2} fastest {top_ids:?}", chunk * 500, total / v2.len() as f32, median, top(0.01), top(0.1), bulk / total);
        }
    }

    #[test]
    #[ignore = "benchmark: run in release with --ignored --nocapture"]
    fn bench_cpu_step() {
        for (dirs, files, syms) in [(10, 20, 50), (25, 40, 50), (50, 40, 50)] {
            let g = repo_graph(&Repo {
                dirs,
                files_per_dir: files,
                symbols_per_file: syms,
                calls_per_symbol: 3,
                seed: 21,
            });
            let mut e = CpuEngine::new(LayoutParams::default());
            e.load(&g, None).unwrap();
            e.step(3); // warm up
            let s = e.step(20);
            println!(
                "cpu step: {:>6} bodies, {:>6} edges, {:>7.2} ms/step (threads {})",
                e.state.model.n_bodies(),
                g.edges.len(),
                s.millis / 20.0,
                rayon::current_num_threads()
            );
        }
    }
}
