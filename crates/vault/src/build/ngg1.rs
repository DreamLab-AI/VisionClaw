//! The NGG1 binary graph format, and the two layout functions that feed it.
//!
//! NGG1 is the **frozen** wire format the explorer's physics worker memory-maps
//! straight out of the published site (`FORMAT-NGG1.md`,
//! `publishing-tools/WasmVOWL`). This writer and the Rust/TS readers must agree
//! byte for byte, so nothing here is a judgement call:
//!
//! ```text
//! magic    "NGG1"                                  4 bytes
//! header   version:u16 pad:u16 nodes:u32 edges:u32
//!          off_nodes:u32 off_adjacency:u32 off_edge_types:u32 off_strings:u32
//! nodes    id:u32 x:f32 y:f32 domain:u16 category:u16 flags:u8 pad:3 degree:u32
//!          (24-byte stride — the brief's "20" is arithmetically impossible)
//! adjacency row_ptr:[u32; nodes+1] then col_idx:[u32; edges]   (CSR)
//! types    edge_type:[u8; edges], zero-padded to a 4-byte boundary
//! strings  count:u32 blob_len:u32 offsets:[u32; count] blob
//!          (offsets pair label,iri per node, so count == 2 × nodes)
//! ```
//!
//! # The layout functions are parity-critical
//!
//! `overview.json` bakes positions from a Fruchterman-Reingold layout seeded by
//! **Python's** `random.Random(42)`. Reproducing those coordinates means
//! reproducing MT19937 and `random.uniform` exactly, which is why [`PyRandom`]
//! exists rather than a Rust RNG: a different stream is a different picture.

// `clippy::suboptimal_flops` would fold `a + b * c` into `mul_add` here.
// It must not: FMA rounds once where CPython rounds twice, and over 200 layout
// iterations that moved a coordinate across a `round(x, 3)` boundary.
#![allow(clippy::suboptimal_flops)]
// Every `usize as u32` below is a field of the frozen NGG1 record, whose widths
// are u32 by specification. A corpus with more than 4 billion nodes or bytes is
// not a truncation risk, it is a different format.
#![allow(clippy::cast_possible_truncation)]

use std::f64::consts::PI;

/// `NGG1` as a little-endian `u32` — `0x3147_474E`.
pub const MAGIC: &[u8; 4] = b"NGG1";
/// Format version.
pub const VERSION: u16 = 1;
/// Header length in bytes.
pub const HEADER_SIZE: usize = 32;
/// Node record stride in bytes.
pub const NODE_STRIDE: usize = 24;

/// A `subClassOf` edge — the taxonomy backbone.
pub const EDGE_SUBCLASS: u8 = 0;
/// An `objectProperty` edge.
pub const EDGE_RELATION: u8 = 1;

/// Node is one of the six domain roots.
pub const FLAG_DOMAIN_ROOT: u8 = 0x01;
/// Node is one of the 34 taxonomy category roots.
pub const FLAG_CATEGORY_ROOT: u8 = 0x02;
/// Node has an authored page behind it.
pub const FLAG_HAS_PAGE: u8 = 0x04;
/// Node ships at least one `bridges-to` edge.
pub const FLAG_BRIDGE: u8 = 0x08;
/// Node is an OWL individual, not a class.
pub const FLAG_INDIVIDUAL: u8 = 0x10;

/// Sentinel for a node with no resolvable domain.
pub const DOMAIN_NONE: u16 = 0xFFFF;
/// Sentinel for an uncategorised node.
pub const CATEGORY_NONE: u16 = 0xFFFF;

/// One graph node, in local-index order within its tier.
#[derive(Debug, Clone)]
pub struct Node {
    /// Stable external id, shared across tiers.
    pub gid: u32,
    /// Display label.
    pub label: String,
    /// Canonical (HTTP-projected) IRI.
    pub iri: String,
    /// `true` for an OWL individual.
    pub individual: bool,
    /// Index into the domain list, or [`DOMAIN_NONE`].
    pub domain_id: u16,
    /// Index into the category list, or [`CATEGORY_NONE`].
    pub category_id: u16,
    /// Bit flags.
    pub flags: u8,
    /// Full-graph incident degree — the ranking key.
    pub degree: u32,
    /// Baked x position.
    pub x: f64,
    /// Baked y position.
    pub y: f64,
    /// Node is a domain root.
    pub is_domain_root: bool,
    /// Node is a category root.
    pub is_category_root: bool,
}

/// Compressed sparse row adjacency.
#[derive(Debug, Clone, Default)]
pub struct Csr {
    /// `nodes + 1` offsets into [`Csr::col_idx`].
    pub row_ptr: Vec<u32>,
    /// Target local indices.
    pub col_idx: Vec<u32>,
    /// One edge type per entry of [`Csr::col_idx`].
    pub edge_type: Vec<u8>,
}

/// Build CSR from directed `(src_local, tgt_local, edge_type)` triples.
///
/// Edges arrive pre-ordered per source (backbone before relations); this only
/// groups them, preserving arrival order within a source.
#[must_use]
pub fn build_csr(num_nodes: usize, edges: &[(u32, u32, u8)]) -> Csr {
    let mut counts = vec![0u32; num_nodes];
    for (src, _, _) in edges {
        counts[*src as usize] += 1;
    }
    let mut row_ptr = vec![0u32; num_nodes + 1];
    for i in 0..num_nodes {
        row_ptr[i + 1] = row_ptr[i] + counts[i];
    }
    let mut col_idx = vec![0u32; edges.len()];
    let mut edge_type = vec![0u8; edges.len()];
    let mut cursor: Vec<u32> = row_ptr[..num_nodes].to_vec();
    for (src, tgt, t) in edges {
        let p = cursor[*src as usize] as usize;
        col_idx[p] = *tgt;
        edge_type[p] = *t;
        cursor[*src as usize] += 1;
    }
    Csr {
        row_ptr,
        col_idx,
        edge_type,
    }
}

const fn pad4(n: usize) -> usize {
    (4 - (n & 3)) & 3
}

/// Serialise one tier to NGG1 bytes.
///
/// `nodes` are in local-index order and `csr.col_idx` holds local indices.
///
/// # Panics
/// When `csr` is inconsistent with `nodes` — a programming error, not input.
#[must_use]
#[allow(clippy::too_many_lines)] // One frozen binary layout, written once.
pub fn pack(nodes: &[Node], csr: &Csr) -> Vec<u8> {
    let node_count = nodes.len();
    let edge_count = csr.col_idx.len();
    assert_eq!(csr.row_ptr.len(), node_count + 1, "row_ptr length");
    assert_eq!(csr.edge_type.len(), edge_count, "edge_type length");
    assert_eq!(csr.row_ptr[node_count] as usize, edge_count, "row_ptr tail");

    let off_nodes = HEADER_SIZE;
    let off_adjacency = off_nodes + node_count * NODE_STRIDE;
    let off_edge_types = off_adjacency + (node_count + 1) * 4 + edge_count * 4;
    let off_strings = off_edge_types + edge_count + pad4(edge_count);

    let mut out: Vec<u8> = Vec::with_capacity(off_strings + 8 + node_count * 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // pad
    for v in [
        node_count as u32,
        edge_count as u32,
        off_nodes as u32,
        off_adjacency as u32,
        off_edge_types as u32,
        off_strings as u32,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }

    for n in nodes {
        out.extend_from_slice(&n.gid.to_le_bytes());
        out.extend_from_slice(&(n.x as f32).to_le_bytes());
        out.extend_from_slice(&(n.y as f32).to_le_bytes());
        out.extend_from_slice(&n.domain_id.to_le_bytes());
        out.extend_from_slice(&n.category_id.to_le_bytes());
        out.push(n.flags);
        out.extend_from_slice(&[0u8; 3]); // pad
        out.extend_from_slice(&n.degree.to_le_bytes());
    }

    for v in &csr.row_ptr {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &csr.col_idx {
        out.extend_from_slice(&v.to_le_bytes());
    }

    out.extend_from_slice(&csr.edge_type);
    out.extend(std::iter::repeat_n(0u8, pad4(edge_count)));

    let mut blob: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(node_count * 2);
    for n in nodes {
        offsets.push(blob.len() as u32);
        blob.extend_from_slice(n.label.as_bytes());
        offsets.push(blob.len() as u32);
        blob.extend_from_slice(n.iri.as_bytes());
    }
    out.extend_from_slice(&(offsets.len() as u32).to_le_bytes());
    out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    for v in &offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&blob);

    debug_assert_eq!(out.len(), off_strings + 8 + offsets.len() * 4 + blob.len());
    out
}

// ── Python's Mersenne Twister ────────────────────────────────────────────

/// MT19937 with `CPython`'s `random.Random` seeding and `random()` derivation.
///
/// Not a general-purpose RNG and deliberately not swappable: the baked overview
/// positions in the published `overview.json` came out of this exact stream, so
/// anything else silently redraws the picture.
pub struct PyRandom {
    state: [u32; 624],
    index: usize,
}

impl PyRandom {
    /// Seed as `random.Random(n)` does for a non-negative `int`: `init_by_array`
    /// over the seed's 32-bit little-endian limbs.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let mut r = Self {
            state: [0; 624],
            index: 624,
        };
        r.init_genrand(19_650_218);
        let key = [seed];
        let mut i: usize = 1;
        let mut j: usize = 0;
        for _ in 0..624.max(key.len()) {
            let prev = r.state[i - 1] ^ (r.state[i - 1] >> 30);
            r.state[i] = (r.state[i] ^ prev.wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                r.state[0] = r.state[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            let prev = r.state[i - 1] ^ (r.state[i - 1] >> 30);
            r.state[i] = (r.state[i] ^ prev.wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                r.state[0] = r.state[623];
                i = 1;
            }
        }
        r.state[0] = 0x8000_0000;
        r
    }

    fn init_genrand(&mut self, s: u32) {
        self.state[0] = s;
        for i in 1..624 {
            let prev = self.state[i - 1];
            self.state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.index = 624;
    }

    fn genrand_u32(&mut self) -> u32 {
        const MATRIX: [u32; 2] = [0, 0x9908_b0df];
        if self.index >= 624 {
            for k in 0..624 {
                let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % 624] & 0x7fff_ffff);
                self.state[k] = self.state[(k + 397) % 624] ^ (y >> 1) ^ MATRIX[(y & 1) as usize];
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `random.random()` — `genrand_res53`, 53 bits of mantissa from two draws.
    pub fn random(&mut self) -> f64 {
        let a = f64::from(self.genrand_u32() >> 5);
        let b = f64::from(self.genrand_u32() >> 6);
        (a * 67_108_864.0 + b) / 9_007_199_254_740_992.0
    }

    /// `random.uniform(a, b)` — `a + (b - a) * random()`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }
}

/// `math.hypot`, with the callers folding in Python's `or 0.01` guard.
///
/// `f64::hypot` and `CPython`'s `math.hypot` agree: both are correctly rounded.
/// Hand-rolling a scaled `hi * sqrt(1 + t²)` instead moved one of the
/// overview's eighty coordinates across a `round(x, 3)` boundary, so the
/// platform function is the right one and the naive form is not.
fn hypot(dx: f64, dy: f64) -> f64 {
    dx.hypot(dy)
}

/// Deterministic Fruchterman-Reingold spring-electrical layout.
///
/// Pure O(n²) per iteration, used only for the ~40-node overview graph. The
/// result is rounded to three decimals, which is what `overview.json` carries.
#[must_use]
#[allow(clippy::many_single_char_names)] // dx/dy/d/f/k/t are the paper's names.
pub fn force_layout(
    n: usize,
    edges: &[(usize, usize)],
    iters: usize,
    seed: u32,
) -> Vec<(f64, f64)> {
    const AREA: f64 = 1_000_000.0;
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![(0.0, 0.0)];
    }
    let mut rng = PyRandom::new(seed);
    let r0 = AREA.sqrt() / 4.0;
    #[allow(clippy::cast_precision_loss)] // n is ~40 here
    let mut pos: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let angle = 2.0 * PI * i as f64 / n as f64;
            (
                r0 * angle.cos() + rng.uniform(-2.0, 2.0),
                r0 * angle.sin() + rng.uniform(-2.0, 2.0),
            )
        })
        .collect();

    #[allow(clippy::cast_precision_loss)]
    let k = (AREA / n as f64).sqrt();
    let mut t = AREA.sqrt() / 10.0;

    for _ in 0..iters {
        let mut disp = vec![(0.0f64, 0.0f64); n];
        for i in 0..n {
            let (xi, yi) = pos[i];
            for j in (i + 1)..n {
                let dx = xi - pos[j].0;
                let dy = yi - pos[j].1;
                let mut d = hypot(dx, dy);
                if d == 0.0 {
                    d = 0.01;
                }
                let f = (k * k) / d;
                let (ux, uy) = (dx / d, dy / d);
                disp[i].0 += ux * f;
                disp[i].1 += uy * f;
                disp[j].0 -= ux * f;
                disp[j].1 -= uy * f;
            }
        }
        for (a, b) in edges {
            let dx = pos[*a].0 - pos[*b].0;
            let dy = pos[*a].1 - pos[*b].1;
            let mut d = hypot(dx, dy);
            if d == 0.0 {
                d = 0.01;
            }
            let f = (d * d) / k;
            let (ux, uy) = (dx / d, dy / d);
            disp[*a].0 -= ux * f;
            disp[*a].1 -= uy * f;
            disp[*b].0 += ux * f;
            disp[*b].1 += uy * f;
        }
        for i in 0..n {
            let mut dl = hypot(disp[i].0, disp[i].1);
            if dl == 0.0 {
                dl = 0.01;
            }
            let lim = dl.min(t);
            pos[i] = (
                pos[i].0 + disp[i].0 / dl * lim,
                pos[i].1 + disp[i].1 / dl * lim,
            );
        }
        t *= 0.95;
    }
    pos.into_iter()
        .map(|(x, y)| (round3(x), round3(y)))
        .collect()
}

/// Python's `round(v, 3)`.
///
/// **Not** `(v * 1000.0).round() / 1000.0`: multiplying by 1000 introduces its
/// own rounding error, and on the overview layout that flipped one coordinate
/// in eighty (`1967.0735` to `.074` where Python gives `.073`). Both `round()`
/// in `CPython` and `{:.3}` in Rust are *correctly rounded* on the true decimal
/// value of the f64, half-to-even, so formatting and re-parsing reproduces
/// `CPython` exactly.
///
/// # Panics
/// Never: `{:.3}` always produces a parseable decimal.
#[must_use]
pub fn round3(v: f64) -> f64 {
    format!("{v:.3}")
        .parse()
        .expect("a formatted f64 always re-parses")
}

/// Radii of the deterministic radial-cluster seed layout.
const DOMAIN_RING_R: f64 = 900.0;
const CAT_RING_R: f64 = 240.0;
const LEAF_RING_R: f64 = 95.0;

/// Assign deterministic seed positions in place: domains on an outer ring,
/// categories ringed around their domain centre, leaves around their category.
///
/// A single-domain tier places its domain at the origin. The physics worker
/// refines these — they only need to be a sane, reproducible warm start.
#[allow(clippy::cast_precision_loss)] // counts are small
pub fn bake_positions(nodes: &mut [Node]) {
    use std::collections::BTreeMap;

    let mut by_domain: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        by_domain.entry(n.domain_id).or_default().push(i);
    }
    let ndoms = by_domain.len();
    for (di, (_domain, members)) in by_domain.iter().enumerate() {
        let (cx, cy) = if ndoms == 1 {
            (0.0, 0.0)
        } else {
            let ang = 2.0 * PI * di as f64 / ndoms as f64;
            (DOMAIN_RING_R * ang.cos(), DOMAIN_RING_R * ang.sin())
        };
        let mut by_cat: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for &i in members {
            if nodes[i].is_domain_root {
                nodes[i].x = cx;
                nodes[i].y = cy;
            } else {
                by_cat.entry(nodes[i].category_id).or_default().push(i);
            }
        }
        // Python sorts categories with `(c == CATEGORY_NONE, c)`, so the
        // uncategorised bucket is always last however low its numeric value.
        let mut cats: Vec<u16> = by_cat.keys().copied().collect();
        cats.sort_by_key(|c| (*c == CATEGORY_NONE, *c));
        let ncats = cats.len().max(1);
        for (ci, cat) in cats.iter().enumerate() {
            let cang = 2.0 * PI * ci as f64 / ncats as f64;
            let ccx = cx + CAT_RING_R * cang.cos();
            let ccy = cy + CAT_RING_R * cang.sin();
            let mut group = by_cat[cat].clone();
            group.sort_by_key(|&i| nodes[i].gid);
            let m = group.len();
            for (mi, &i) in group.iter().enumerate() {
                if nodes[i].is_category_root {
                    nodes[i].x = ccx;
                    nodes[i].y = ccy;
                } else {
                    let mang = 2.0 * PI * mi as f64 / m.max(1) as f64;
                    nodes[i].x = ccx + LEAF_RING_R * mang.cos();
                    nodes[i].y = ccy + LEAF_RING_R * mang.sin();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(gid: u32, label: &str, iri: &str) -> Node {
        Node {
            gid,
            label: label.to_owned(),
            iri: iri.to_owned(),
            individual: false,
            domain_id: 0,
            category_id: CATEGORY_NONE,
            flags: FLAG_HAS_PAGE,
            degree: 0,
            x: 0.0,
            y: 0.0,
            is_domain_root: false,
            is_category_root: false,
        }
    }

    #[test]
    fn the_python_rng_stream_matches_cpython() {
        // Captured from CPython 3.12: `random.Random(42)` then `.random()` ×10.
        let expected = [
            0.639_426_798_457_883_7,
            0.025_010_755_222_666_936,
            0.275_029_318_369_119_26,
            0.223_210_738_148_822_75,
            0.736_471_214_164_012_4,
            0.676_699_487_422_911_3,
            0.892_179_567_704_845_4,
            0.086_938_832_629_416_2,
            0.421_921_819_685_270_4,
            0.029_797_219_438_070_344,
        ];
        let mut rng = PyRandom::new(42);
        for (i, want) in expected.iter().enumerate() {
            let got = rng.random();
            assert!(
                (got - want).abs() < 1e-15,
                "draw {i}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn uniform_matches_cpython() {
        let expected = [
            0.557_707_193_831_535,
            -1.899_956_979_109_332_3,
            -0.899_882_726_523_523,
            -1.107_157_047_404_709,
            0.945_884_856_656_049_6,
        ];
        let mut rng = PyRandom::new(42);
        for (i, want) in expected.iter().enumerate() {
            let got = rng.uniform(-2.0, 2.0);
            assert!((got - want).abs() < 1e-15, "draw {i}: {got} != {want}");
        }
    }

    #[test]
    fn force_layout_matches_the_python_reference() {
        // `force_layout(3, [(0,1),(1,2)], iters=5, seed=42)` in CPython.
        let got = force_layout(3, &[(0, 1), (1, 2)], 5, 42);
        let want = [(582.014, 255.676), (-65.823, 86.1), (-516.499, -373.407)];
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g.0 - w.0).abs() < 1e-3 && (g.1 - w.1).abs() < 1e-3,
                "node {i}: {g:?} != {w:?}"
            );
        }
    }

    #[test]
    fn force_layout_is_stable_for_the_overview_sized_graph() {
        let a = force_layout(40, &[(6, 0), (7, 0), (8, 1)], 200, 42);
        let b = force_layout(40, &[(6, 0), (7, 0), (8, 1)], 200, 42);
        assert_eq!(a, b);
        assert_eq!(a.len(), 40);
    }

    #[test]
    fn degenerate_layouts_are_handled() {
        assert!(force_layout(0, &[], 10, 42).is_empty());
        assert_eq!(force_layout(1, &[], 10, 42), vec![(0.0, 0.0)]);
    }

    #[test]
    fn csr_groups_by_source_and_preserves_arrival_order() {
        let csr = build_csr(3, &[(0, 1, 0), (0, 2, 1), (2, 0, 0)]);
        assert_eq!(csr.row_ptr, vec![0, 2, 2, 3]);
        assert_eq!(csr.col_idx, vec![1, 2, 0]);
        assert_eq!(csr.edge_type, vec![0, 1, 0]);
    }

    #[test]
    fn the_header_is_the_frozen_shape() {
        let nodes = vec![node(0, "A", "https://x/a"), node(1, "B", "https://x/b")];
        let csr = build_csr(2, &[(0, 1, EDGE_SUBCLASS)]);
        let bytes = pack(&nodes, &csr);

        assert_eq!(&bytes[0..4], MAGIC);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION);
        let rd = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!(rd(8), 2, "node count");
        assert_eq!(rd(12), 1, "edge count");
        assert_eq!(rd(16), HEADER_SIZE as u32, "off_nodes");
        assert_eq!(
            rd(20),
            (HEADER_SIZE + 2 * NODE_STRIDE) as u32,
            "off_adjacency"
        );
    }

    #[test]
    fn the_node_stride_is_twenty_four_bytes() {
        let nodes = vec![node(7, "A", "https://x/a")];
        let csr = build_csr(1, &[]);
        let bytes = pack(&nodes, &csr);
        let off_adjacency = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        assert_eq!(off_adjacency - HEADER_SIZE, NODE_STRIDE);
        // id, then x/y as f32, then domain/category/flags/pad/degree.
        assert_eq!(u32::from_le_bytes(bytes[32..36].try_into().unwrap()), 7);
    }

    #[test]
    fn edge_types_are_padded_to_a_four_byte_boundary() {
        let nodes: Vec<Node> = (0..3)
            .map(|i| node(i, &format!("N{i}"), &format!("https://x/{i}")))
            .collect();
        // 3 edges => 1 byte of padding.
        let csr = build_csr(3, &[(0, 1, 0), (0, 2, 1), (1, 2, 1)]);
        let bytes = pack(&nodes, &csr);
        let off_edge_types = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        let off_strings = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
        assert_eq!(off_strings - off_edge_types, 4, "3 types + 1 pad");
        assert_eq!(&bytes[off_edge_types..off_edge_types + 3], &[0, 1, 1]);
        assert_eq!(bytes[off_edge_types + 3], 0, "pad byte is zero");
    }

    #[test]
    fn the_string_table_pairs_label_and_iri_per_node() {
        let nodes = vec![
            node(0, "Alpha", "https://x/a"),
            node(1, "Beta", "https://x/b"),
        ];
        let csr = build_csr(2, &[]);
        let bytes = pack(&nodes, &csr);
        let off = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let blob_len = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()) as usize;
        assert_eq!(count, 4, "two strings per node");
        let blob_start = off + 8 + count * 4;
        let blob = &bytes[blob_start..blob_start + blob_len];
        assert_eq!(
            std::str::from_utf8(blob).unwrap(),
            "Alphahttps://x/aBetahttps://x/b"
        );
    }

    #[test]
    fn packing_is_byte_stable() {
        let nodes = vec![node(0, "A", "https://x/a"), node(1, "B", "https://x/b")];
        let csr = build_csr(2, &[(0, 1, 0)]);
        assert_eq!(pack(&nodes, &csr), pack(&nodes, &csr));
    }

    #[test]
    fn bake_positions_puts_a_lone_domain_at_the_origin() {
        let mut nodes = vec![node(0, "Root", "https://x/root")];
        nodes[0].is_domain_root = true;
        bake_positions(&mut nodes);
        assert!((nodes[0].x).abs() < f64::EPSILON);
        assert!((nodes[0].y).abs() < f64::EPSILON);
    }

    #[test]
    fn bake_positions_rings_leaves_around_their_category() {
        let mut nodes: Vec<Node> = (0..4)
            .map(|i| {
                let mut n = node(i, &format!("N{i}"), &format!("https://x/{i}"));
                n.category_id = 3;
                n
            })
            .collect();
        nodes[0].is_category_root = true;
        bake_positions(&mut nodes);
        let (cx, cy) = (nodes[0].x, nodes[0].y);
        for n in &nodes[1..] {
            let d = (n.x - cx).hypot(n.y - cy);
            assert!(
                (d - LEAF_RING_R).abs() < 1e-6,
                "leaf at {d} from the centre"
            );
        }
    }

    #[test]
    fn the_uncategorised_bucket_sorts_last() {
        let mut nodes: Vec<Node> = (0..2)
            .map(|i| node(i, &format!("N{i}"), &format!("https://x/{i}")))
            .collect();
        nodes[0].category_id = CATEGORY_NONE;
        nodes[1].category_id = 9;
        nodes[1].is_category_root = true;
        bake_positions(&mut nodes);
        // Category 9 takes ring slot 0 (angle 0), so it sits on the +x axis.
        assert!(nodes[1].y.abs() < 1e-9, "the real category takes slot 0");
    }

    #[test]
    fn hypot_is_the_platform_function() {
        assert!((hypot(3.0, 4.0) - 5.0).abs() < 1e-12);
        assert!(hypot(0.0, 0.0).abs() < f64::EPSILON);
        assert!((hypot(-3.0, 4.0) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn rounding_matches_cpython_round_to_three_places() {
        // Captured from CPython: these are the cases where a naive
        // multiply-round-divide disagrees.
        assert!((round3(1967.0735) - 1967.073).abs() < 1e-12, "half-to-even");
        assert!((round3(582.0136) - 582.014).abs() < 1e-12);
        assert!((round3(1.0005) - 1.0).abs() < 1e-12);
        assert!((round3(-1967.0735) + 1967.073).abs() < 1e-12);
    }
}
