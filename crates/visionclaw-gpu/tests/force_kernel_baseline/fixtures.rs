//! Deterministic fixture graphs.
//!
//! Coordinates are dyadic (multiples of 1/8, or of 1/2048 for the one
//! near-coincident pair), so every coordinate difference is exact in `f32`
//! and the two- and three-node cases can be checked by hand. The larger
//! fixture comes from an inline SplitMix64 generator with a fixed seed, so it
//! is identical on every host without a dependency or a committed input file.
//! Each fixture's [`Fixture::fingerprint`] goes into the golden header, so an
//! edited fixture is reported as such rather than as a kernel regression.

/// One undirected edge as the graph stores it.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub src: u32,
    pub dst: u32,
    pub weight: f32,
    /// Relationship type id (semantic module); ignored by the unified pass.
    pub kind: i32,
}

/// Both-direction CSR as the server builds it.
pub struct Csr {
    pub offsets: Vec<i32>,
    pub cols: Vec<i32>,
    pub weights: Vec<f32>,
}

/// A graph plus every per-node attribute any force kernel reads.
#[derive(Clone, Debug)]
pub struct Fixture {
    pub name: &'static str,
    pub pos: Vec<[f32; 3]>,
    pub edges: Vec<Edge>,
    pub class_id: Vec<i32>,
    pub class_charge: Vec<f32>,
    pub spring_scale: Vec<f32>,
    pub sssp: Vec<f32>,
    /// DAG rank / radial shell key; `< 0` = unranked.
    pub rank: Vec<f32>,
    /// Stratified plane offset; NaN = unassigned.
    pub plane: Vec<f32>,
    pub node_type: Vec<i32>,
    pub hierarchy_level: Vec<i32>,
    pub physicality: Vec<i32>,
    pub role: Vec<i32>,
    pub maturity: Vec<i32>,
    pub cross_domain: Vec<i32>,
}

impl Fixture {
    pub fn n(&self) -> usize {
        self.pos.len()
    }

    /// The CSR exactly as `GPUResourceActor::create_csr_from_graph_data`
    /// builds it: each edge appended to its source's list and, unless it is a
    /// self-loop, to its target's list; duplicates kept; rows in node order.
    pub fn csr(&self) -> Csr {
        let mut adj: Vec<Vec<(i32, f32)>> = vec![Vec::new(); self.n()];
        for e in &self.edges {
            adj[e.src as usize].push((e.dst as i32, e.weight));
            if e.src != e.dst {
                adj[e.dst as usize].push((e.src as i32, e.weight));
            }
        }
        let mut offsets = vec![0i32];
        let (mut cols, mut weights) = (Vec::new(), Vec::new());
        for row in &adj {
            for &(c, w) in row {
                cols.push(c);
                weights.push(w);
            }
            offsets.push(cols.len() as i32);
        }
        Csr {
            offsets,
            cols,
            weights,
        }
    }

    /// `node_degrees` as the server fills it for FA2: the sum of incident
    /// CSR edge weights per node.
    pub fn degrees(&self) -> Vec<f32> {
        let csr = self.csr();
        (0..self.n())
            .map(|i| {
                let (a, b) = (csr.offsets[i] as usize, csr.offsets[i + 1] as usize);
                csr.weights[a..b].iter().sum()
            })
            .collect()
    }

    /// FNV-1a 64 over every input field, in a fixed order.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv::new();
        h.str(self.name);
        for p in &self.pos {
            p.iter().for_each(|v| h.f32(*v));
        }
        for e in &self.edges {
            h.u32(e.src);
            h.u32(e.dst);
            h.f32(e.weight);
            h.u32(e.kind as u32);
        }
        for v in [
            &self.class_charge,
            &self.spring_scale,
            &self.sssp,
            &self.rank,
            &self.plane,
        ] {
            v.iter().for_each(|x| h.f32(*x));
        }
        for v in [
            &self.class_id,
            &self.node_type,
            &self.hierarchy_level,
            &self.physicality,
            &self.role,
            &self.maturity,
            &self.cross_domain,
        ] {
            v.iter().for_each(|x| h.u32(*x as u32));
        }
        h.0
    }

    /// The same graph with every node displaced by a fixed, node-dependent
    /// offset: the "after the drag" positions for hold-overlay scenarios.
    pub fn displaced(&self) -> Fixture {
        let mut out = self.clone();
        for (i, p) in out.pos.iter_mut().enumerate() {
            let k = (i % 7) as f32;
            p[0] += 0.5 + 0.25 * k;
            p[1] -= 0.375 * ((i % 3) as f32);
            p[2] += 0.125 * ((i % 5) as f32);
        }
        out
    }
}

/// FNV-1a 64-bit, for fingerprints only (not a security primitive).
pub struct Fnv(pub u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    pub fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
    pub fn u32(&mut self, v: u32) {
        v.to_le_bytes().iter().for_each(|b| self.byte(*b));
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub fn str(&mut self, s: &str) {
        s.bytes().for_each(|b| self.byte(b));
        self.byte(0);
    }
}

/// SplitMix64: a fixed, well-known 64-bit generator.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range_i(&mut self, lo: i32, hi_inclusive: i32) -> i32 {
        lo + self.below((hi_inclusive - lo + 1) as u64) as i32
    }
    /// A multiple of 1/8 in [-half, half].
    fn coord(&mut self, half: i32) -> f32 {
        self.range_i(-half * 8, half * 8) as f32 / 8.0
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

fn e(src: u32, dst: u32, weight: f32, kind: i32) -> Edge {
    Edge {
        src,
        dst,
        weight,
        kind,
    }
}

/// Two nodes 5 apart (a 3-4-5 triangle in the xy plane), one unit edge.
pub fn pair() -> Fixture {
    Fixture {
        name: "pair",
        pos: vec![[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]],
        edges: vec![e(0, 1, 1.0, 0)],
        class_id: vec![1, 1],
        class_charge: vec![1.0, 2.0],
        spring_scale: vec![1.0, 0.5],
        sssp: vec![0.0, 2.0],
        rank: vec![0.0, 1.0],
        plane: vec![0.0, 1.0],
        node_type: vec![0, 1],
        hierarchy_level: vec![1, 1],
        physicality: vec![1, 2],
        role: vec![1, 2],
        maturity: vec![1, 3],
        cross_domain: vec![0, 2],
    }
}

/// A 3-4-5 right triangle with three different edge weights and types.
pub fn triangle() -> Fixture {
    Fixture {
        name: "triangle",
        pos: vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 3.0, 0.0]],
        edges: vec![e(0, 1, 1.0, 0), e(1, 2, 0.5, 1), e(0, 2, 2.0, 2)],
        class_id: vec![1, 2, 0],
        class_charge: vec![1.0, 1.5, 0.5],
        spring_scale: vec![1.0, 2.0, 0.5],
        sssp: vec![0.0, 1.0, f32::INFINITY],
        rank: vec![0.0, 1.0, -1.0],
        plane: vec![-1.0, f32::NAN, 1.0],
        node_type: vec![0, 0, 1],
        hierarchy_level: vec![0, 1, 1],
        physicality: vec![1, 1, 3],
        role: vec![1, 3, 0],
        maturity: vec![2, 1, 0],
        cross_domain: vec![1, 0, 3],
    }
}

/// 256 nodes in a 120-unit cube, a 100-edge hub at node 0 (it makes edge
/// accumulation order matter for the atomic kernels) and 400 random edges
/// with mixed weights and types, duplicates allowed.
pub fn mixed256() -> Fixture {
    const N: usize = 256;
    let mut r = SplitMix(0x5eed_f0ce_2026_1110);
    let pos = (0..N)
        .map(|_| [r.coord(60), r.coord(60), r.coord(60)])
        .collect();
    let weights = [0.25f32, 0.5, 1.0, 1.5, 2.0, 3.0];
    let mut edges = Vec::new();
    for j in 1..=100u32 {
        edges.push(e(0, j, weights[j as usize % weights.len()], (j % 5) as i32));
    }
    while edges.len() < 500 {
        let (u, v) = (r.below(N as u64) as u32, r.below(N as u64) as u32);
        if u == v {
            continue;
        }
        edges.push(e(u, v, r.pick(&weights), r.range_i(0, 4)));
    }
    let class_id = (0..N).map(|_| r.range_i(0, 3)).collect();
    let class_charge = (0..N).map(|_| r.pick(&[0.5f32, 1.0, 1.5, 2.0])).collect();
    let spring_scale = (0..N).map(|_| r.pick(&[0.5f32, 1.0, 2.0])).collect();
    let sssp = (0..N)
        .map(|_| {
            if r.below(10) == 0 {
                f32::INFINITY
            } else {
                r.range_i(0, 64) as f32 / 4.0
            }
        })
        .collect();
    let rank = (0..N).map(|_| r.range_i(-1, 4) as f32).collect();
    let plane = (0..N)
        .map(|_| {
            if r.below(5) == 0 {
                f32::NAN
            } else {
                r.range_i(-2, 2) as f32
            }
        })
        .collect();
    let node_type = (0..N).map(|_| r.range_i(0, 5)).collect();
    let hierarchy_level = (0..N).map(|_| r.range_i(-1, 3)).collect();
    let physicality = (0..N).map(|_| r.range_i(0, 3)).collect();
    let role = (0..N).map(|_| r.range_i(0, 4)).collect();
    let maturity = (0..N).map(|_| r.range_i(0, 3)).collect();
    let cross_domain = (0..N).map(|_| r.range_i(0, 5)).collect();
    Fixture {
        name: "mixed256",
        pos,
        edges,
        class_id,
        class_charge,
        spring_scale,
        sssp,
        rank,
        plane,
        node_type,
        hierarchy_level,
        physicality,
        role,
        maturity,
        cross_domain,
    }
}

/// The degenerate cases the loaders admit: two coincident nodes joined by an
/// edge, a near-coincident pair (separation 1/2048, below the kernels'
/// `dist_sq > 1e-6` repulsion floor but above the `dist > 1e-6` spring floor),
/// a zero-weight edge, a self-loop, a duplicated edge, an isolated node, and
/// an edge type outside the relationship table.
pub fn degenerate() -> Fixture {
    Fixture {
        name: "degenerate",
        pos: vec![
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 1.0 / 2048.0, 0.0],
            [-20.0, 5.0, 5.0],
            [5.0, -5.0, 2.0],
            [5.0, -5.0, 2.5],
        ],
        edges: vec![
            e(0, 1, 1.0, 0),
            e(2, 3, 1.0, 0),
            e(2, 5, 0.0, 1),
            e(5, 5, 1.0, 0),
            e(5, 6, 1.0, 2),
            e(5, 6, 1.0, 2),
            e(0, 2, 2.0, 7),
        ],
        class_id: vec![1, 1, 2, 2, 0, 3, 3],
        class_charge: vec![1.0; 7],
        spring_scale: vec![1.0; 7],
        sssp: vec![0.0, 0.0, 2.0, 3.0, f32::INFINITY, 1.0, 2.0],
        rank: vec![0.0, 0.0, 1.0, 1.0, -1.0, 2.0, 2.0],
        plane: vec![0.0, 0.0, 1.0, 1.0, f32::NAN, -1.0, -1.0],
        node_type: vec![0, 0, 1, 1, 2, 3, 3],
        hierarchy_level: vec![0, 0, 1, 1, -1, 2, 2],
        physicality: vec![1, 1, 2, 2, 0, 3, 3],
        role: vec![1, 1, 2, 2, 0, 4, 4],
        maturity: vec![1, 1, 2, 2, 0, 3, 3],
        cross_domain: vec![0, 0, 1, 1, 0, 2, 2],
    }
}

/// Every fixture, small to large.
pub fn all() -> Vec<Fixture> {
    vec![pair(), triangle(), degenerate(), mixed256()]
}
