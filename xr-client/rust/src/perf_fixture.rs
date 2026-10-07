//! Deterministic production-density scene for CPU measurements of the per-frame
//! pack (`examples/lod_pack_profile.rs`, `tests/pack_alloc.rs`). Not used at run
//! time. 13 164 nodes / 20 000 edges is the measured production density
//! (README, VIVE hardening) at the scene's `EDGE_SAFETY_CEILING`.

use crate::render_store::RenderStore;
use crate::settings_sync::FilterInputs;

/// Production density: nodes in the live graph, and drawn-edge ceiling.
pub const PROD_NODES: usize = 13_164;
pub const PROD_EDGES: usize = 20_000;

/// Small deterministic generator (no `rand` dependency in the library).
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next_f32() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

/// A production-shaped scene plus the per-node analytics a real V3 frame
/// repeats every broadcast.
pub struct Fixture {
    pub store: RenderStore,
    pub ids: Vec<i32>,
    pub pairs: Vec<i32>,
    targets: Vec<[f32; 3]>,
    community: Vec<u32>,
    anomaly: Vec<f32>,
    centrality: Vec<f32>,
}

/// A store shaped like the live graph: positions in a ±400-unit volume,
/// communities, clusters, domains, degrees, filter inputs and edge styles.
pub fn production_store(nodes: usize, edges: usize, seed: u64) -> Fixture {
    let mut rng = Lcg(seed);
    let mut s = RenderStore::new();
    let domains = ["robotics", "AI", "blockchain", "spatial-computing", "infrastructure", ""];
    let mut ids = Vec::with_capacity(nodes);
    let (mut targets, mut community, mut anomaly, mut centrality) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..nodes {
        let id = (i + 1) as u32;
        let p = [rng.next_f32() * 800.0 - 400.0, rng.next_f32() * 800.0 - 400.0, rng.next_f32() * 800.0 - 400.0];
        let (c, a, k) = ((i % 97) as u32 + 1, if i % 50 == 0 { 0.9 } else { 0.1 }, rng.next_f32());
        s.upsert(id, p, c, a, k);
        s.set_cluster(id, (i % 40) as u32);
        s.set_node_domain(id, domains[i % domains.len()]);
        s.set_filter_inputs(id, FilterInputs { quality: Some(rng.next_f32()), ..Default::default() });
        ids.push(id as i32);
        targets.push(p);
        community.push(c);
        anomaly.push(a);
        centrality.push(k);
    }
    let mut pairs = Vec::with_capacity(edges * 2);
    for _ in 0..edges {
        let a = rng.below(nodes);
        let b = (a + 1 + rng.below(nodes - 1)) % nodes;
        pairs.push((a + 1) as i32);
        pairs.push((b + 1) as i32);
    }
    s.compute_degrees(&pairs);
    let codes: Vec<u8> = (0..edges).map(|i| (i % 4) as u8).collect();
    s.set_edge_styles(&pairs, &codes);
    Fixture { store: s, ids, pairs, targets, community, anomaly, centrality }
}

impl Fixture {
    /// One broadcast + render tick: every node's server target drifts a little
    /// (continuous settle streams every position) with its analytics repeated
    /// unchanged, then render positions hunt toward the targets.
    pub fn advance(&mut self, frame: u32) {
        let t = frame as f32 * 0.05;
        for (k, id) in self.ids.iter().enumerate() {
            let w = (t + k as f32 * 0.37).sin() * 0.4;
            let b = self.targets[k];
            self.store.upsert(
                *id as u32,
                [b[0] + w, b[1] - w * 0.5, b[2] + w * 0.25],
                self.community[k],
                self.anomaly[k],
                self.centrality[k],
            );
        }
        self.store.hunt(0.06, None, [0.0; 3]);
    }
}
