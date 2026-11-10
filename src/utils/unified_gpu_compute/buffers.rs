//! The engine's graph-sized device buffers, declared once (rework stage S1a, finding L1).
//!
//! Every device buffer whose length follows the node or edge count is listed in the
//! `graph_sized_buffers!` table below. That one table generates:
//!
//! - [`GraphSizedBuffers::allocate`], used by both `UnifiedGPUCompute::new` and
//!   `resize_buffers`, so construction and resize cannot disagree about a buffer;
//! - `UnifiedGPUCompute::install_graph_sized`, which `resize_buffers` calls to replace every
//!   registered buffer at once;
//! - `UnifiedGPUCompute::graph_sized_lengths`, the registry the analytics paths consult for
//!   bounds before they launch, and `GRAPH_SIZED`, the same list for the registry test.
//!
//! Before S1a, `resize_buffers` reassigned buffers one by one and missed eight of them (the
//! SSSP working set and the label-propagation set). Those stayed at the construction size of
//! 1000 while kernels were launched over the live node count.
//!
//! Device buffers that do *not* follow the graph are listed in `FIXED_EXTENT` with the
//! reason. The test at the bottom of this file reads the engine's struct definition and fails
//! if any `DeviceBuffer` field is in neither list, so a new per-node or per-edge buffer cannot
//! be added without registering it. Two kinds of state hold graph-sized device memory outside
//! these lists, each with its own lifecycle: `semantic` (`SemanticGpu`, rebuilt by
//! `refresh_semantic` after every graph upload) and `sssp_device_distances` (derived output,
//! discarded on every graph change; see `invalidate_graph_derived_state`).

use super::construction::UnifiedGPUCompute;
use super::types::{curandState, Aabb};
use anyhow::{anyhow, Context as _, Result};
use cust::memory::DeviceBuffer;

/// How a registered buffer's length follows the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Extent {
    /// One element per node.
    Nodes,
    /// One element per node plus one (CSR row offsets).
    NodesPlusOne,
    /// One element per CSR slot (directed edge entry).
    Edges,
    /// One element per 256-thread block over the nodes (reduction partials).
    NodeBlocks,
}

/// Threads per block the per-block reduction buffers are sized for.
pub(crate) const REDUCTION_BLOCK: usize = 256;

impl Extent {
    /// Elements a buffer must hold for a graph of `nodes` nodes and `edges` CSR slots.
    pub(crate) fn required(self, nodes: usize, edges: usize) -> usize {
        match self {
            Extent::Nodes => nodes,
            Extent::NodesPlusOne => nodes + 1,
            Extent::Edges => edges,
            Extent::NodeBlocks => nodes.div_ceil(REDUCTION_BLOCK),
        }
    }

    /// Elements allocated for a capacity of `nodes` nodes and `edges` CSR slots. Never zero, so
    /// every registered buffer has a valid device address.
    pub(crate) fn capacity(self, nodes: usize, edges: usize) -> usize {
        self.required(nodes, edges).max(1)
    }
}

/// A buffer of `len` copies of `value`.
fn filled<T: cust::memory::DeviceCopy + Clone>(value: T, len: usize) -> Result<DeviceBuffer<T>> {
    Ok(DeviceBuffer::from_slice(&vec![value; len])?)
}

/// A buffer holding `0, 1, …, len - 1`.
fn iota(len: usize) -> Result<DeviceBuffer<i32>> {
    let values: Vec<i32> = (0..len as i32).collect();
    Ok(DeviceBuffer::from_slice(&values)?)
}

macro_rules! graph_sized_buffers {
    (@alloc zero, $ty:ty, $len:expr) => { DeviceBuffer::<$ty>::zeroed($len).map_err(anyhow::Error::from) };
    (@alloc iota, $ty:ty, $len:expr) => { iota($len) };
    (@alloc fill($value:expr), $ty:ty, $len:expr) => { filled::<$ty>($value, $len) };
    ($( $name:ident : $ty:ty, $extent:ident, $fill:ident $(($value:expr))? ; )*) => {
        /// Every graph-sized device buffer, freshly allocated with its default contents.
        ///
        /// Fields match the `UnifiedGPUCompute` fields of the same name.
        pub(crate) struct GraphSizedBuffers {
            $( pub(crate) $name: DeviceBuffer<$ty>, )*
        }

        impl GraphSizedBuffers {
            /// Allocates every registered buffer for a capacity of `nodes` nodes and `edges`
            /// CSR slots, each filled with its default.
            pub(crate) fn allocate(nodes: usize, edges: usize) -> Result<Self> {
                Ok(Self {
                    $( $name: graph_sized_buffers!(
                        @alloc $fill $(($value))?, $ty, Extent::$extent.capacity(nodes, edges)
                    ).with_context(|| format!(
                        "allocating {} ({} elements)",
                        stringify!($name),
                        Extent::$extent.capacity(nodes, edges)
                    ))?, )*
                })
            }
        }

        /// Name and extent of every registered buffer, in declaration order (read by the
        /// registry test; the engine itself uses `graph_sized_lengths`).
        #[cfg(test)]
        pub(crate) const GRAPH_SIZED: &[(&str, Extent)] = &[ $( (stringify!($name), Extent::$extent), )* ];

        impl UnifiedGPUCompute {
            /// Replaces every registered buffer with the freshly allocated set.
            pub(crate) fn install_graph_sized(&mut self, fresh: GraphSizedBuffers) {
                $( self.$name = fresh.$name; )*
            }

            /// Name, extent and current length of every registered buffer.
            pub(crate) fn graph_sized_lengths(&self) -> Vec<(&'static str, Extent, usize)> {
                vec![ $( (stringify!($name), Extent::$extent, self.$name.len()), )* ]
            }
        }
    };
}

graph_sized_buffers! {
    // Integration state. Positions and velocities are carried across a resize by the caller.
    pos_in_x: f32, Nodes, zero;
    pos_in_y: f32, Nodes, zero;
    pos_in_z: f32, Nodes, zero;
    vel_in_x: f32, Nodes, zero;
    vel_in_y: f32, Nodes, zero;
    vel_in_z: f32, Nodes, zero;
    pos_out_x: f32, Nodes, zero;
    pos_out_y: f32, Nodes, zero;
    pos_out_z: f32, Nodes, zero;
    vel_out_x: f32, Nodes, zero;
    vel_out_y: f32, Nodes, zero;
    vel_out_z: f32, Nodes, zero;
    force_x: f32, Nodes, zero;
    force_y: f32, Nodes, zero;
    force_z: f32, Nodes, zero;
    prev_force_x: f32, Nodes, zero;
    prev_force_y: f32, Nodes, zero;
    prev_force_z: f32, Nodes, zero;

    // Per-node inputs the actor uploads after a graph upload; defaults leave each term inert.
    mass: f32, Nodes, fill(1.0);
    node_graph_id: i32, Nodes, zero;
    class_id: i32, Nodes, zero;
    class_charge: f32, Nodes, fill(1.0);
    class_mass: f32, Nodes, fill(1.0);
    spring_scale: f32, Nodes, fill(1.0);
    pinned_mask: i32, Nodes, zero;
    node_rank: f32, Nodes, fill(-1.0);
    node_plane: f32, Nodes, fill(f32::NAN);
    degree_weight: f32, Nodes, fill(1.0);
    node_constraint_force: f32, Nodes, zero;
    compute_mask: i32, Nodes, zero;

    // CSR topology and per-slot state.
    edge_row_offsets: i32, NodesPlusOne, zero;
    edge_col_indices: i32, Edges, zero;
    edge_weights: f32, Edges, zero;
    edge_rest_snapshot: f32, Edges, zero;

    // Spatial grid sort.
    cell_keys: i32, Nodes, zero;
    sorted_node_indices: i32, Nodes, iota;
    sort_keys_out: i32, Nodes, zero;
    sort_values_out: i32, Nodes, zero;

    // SSSP working set (missed by resize before S1a).
    dist: f32, Nodes, fill(f32::INFINITY);
    current_frontier: i32, Nodes, zero;
    next_frontier_flags: i32, Nodes, zero;

    // Clustering, community detection and cohesion.
    cluster_assignments: i32, Nodes, zero;
    distances_to_centroid: f32, Nodes, zero;
    community_centroids_x: f32, Nodes, zero;
    community_centroids_y: f32, Nodes, zero;
    community_centroids_z: f32, Nodes, zero;
    community_sizes: i32, Nodes, zero;
    min_distances: f32, Nodes, zero;
    node_degrees: f32, Nodes, zero;

    // Label propagation (missed by resize before S1a).
    labels_current: i32, Nodes, zero;
    labels_next: i32, Nodes, zero;
    label_counts: i32, Nodes, zero;
    label_mapping: i32, Nodes, zero;
    rand_states: curandState, Nodes, fill(curandState { _private: [0u8; 48] });

    // Anomaly detection.
    lof_scores: f32, Nodes, zero;
    local_densities: f32, Nodes, zero;
    zscore_values: f32, Nodes, zero;
    feature_values: f32, Nodes, zero;

    // Per-block reduction partials.
    partial_inertia: f32, NodeBlocks, zero;
    partial_sums: f32, NodeBlocks, zero;
    partial_sq_sums: f32, NodeBlocks, zero;
    partial_kinetic_energy: f32, NodeBlocks, zero;
    aabb_block_results: Aabb, NodeBlocks, zero;
}

/// Device buffers whose length does not follow the graph, with the reason. The registry test
/// requires every `DeviceBuffer` field of the engine to appear here or in `GRAPH_SIZED`.
#[cfg(test)]
pub(crate) const FIXED_EXTENT: &[(&str, &str)] = &[
    (
        "cell_start",
        "spatial grid cells; resized by resize_cell_buffers from the grid dims",
    ),
    (
        "cell_end",
        "spatial grid cells; resized by resize_cell_buffers from the grid dims",
    ),
    ("centroids_x", "k-means centroids; max_clusters"),
    ("centroids_y", "k-means centroids; max_clusters"),
    ("centroids_z", "k-means centroids; max_clusters"),
    ("cluster_sizes", "k-means cluster sizes; max_clusters"),
    ("selected_nodes", "k-means++ seeds; max_clusters"),
    ("convergence_flag", "single flag"),
    ("active_node_count", "single counter"),
    ("should_skip_physics", "single flag"),
    ("system_kinetic_energy", "single scalar"),
    (
        "constraint_data",
        "one element per constraint; set_constraints",
    ),
    (
        "constraint_node_offsets",
        "constraint index; rebuilt by resize_buffers from the host copy",
    ),
    (
        "constraint_node_ids",
        "constraint index; rebuilt by resize_buffers from the host copy",
    ),
    (
        "sssp_device_distances",
        "derived SSSP output; discarded on every graph change",
    ),
];

impl UnifiedGPUCompute {
    /// Fails, without launching anything, unless each named registered buffer holds at least
    /// the elements its extent requires for the live graph. Analytics paths call this before
    /// their kernels so a buffer of the wrong length is an error, never an out-of-bounds launch.
    pub(crate) fn require_graph_capacity(&self, names: &[&str]) -> Result<()> {
        let lengths = self.graph_sized_lengths();
        for &wanted in names {
            let (name, extent, len) = lengths
                .iter()
                .copied()
                .find(|(name, _, _)| *name == wanted)
                .ok_or_else(|| anyhow!("{wanted} is not a registered graph-sized buffer"))?;
            let need = extent.required(self.num_nodes, self.num_edges);
            if len < need {
                return Err(anyhow!(
                    "{name} holds {len} elements but the graph needs {need} \
                     ({} nodes, {} edges); refusing to launch",
                    self.num_nodes,
                    self.num_edges
                ));
            }
        }
        Ok(())
    }

    /// Discards everything computed from the previous graph: the SSSP distances the force pass
    /// may read. Called on every graph change (any resize, including a shrink that reuses the
    /// allocation, and every CSR upload), because node indices and edges may have changed.
    pub(crate) fn invalidate_graph_derived_state(&mut self) {
        self.sssp_available = false;
        self.sssp_device_distances = None;
    }

    /// Discards every piece of host-tracked state that is indexed by node and was set for the
    /// previous graph, so nothing pushes forces onto whichever nodes now hold those indices.
    /// Called on every `resize_buffers` call (growth or shrink) and by `initialize_graph`
    /// whenever the node count, edge count or CSR topology changes. The actor re-uploads each
    /// of these after `initialize_graph`; until it does, the term is off, never stale:
    ///
    /// - SSSP distances (`invalidate_graph_derived_state`);
    /// - the sparse compute mask (all nodes computed until `set_compute_mask`);
    /// - community labels and their count (the next cohesion step re-runs detection);
    /// - degree weights (unweighted until `upload_degree_weights`);
    /// - the hold overlay's per-slot rest lengths (the next hold captures fresh ones);
    /// - semantic metadata (`SemanticGpu`, off until `enable_semantic`).
    ///
    /// Not covered: the constraint index. Constraints name nodes by index and are owned by the
    /// constraint actors, which re-send them on their own schedule; see ADR-2191.
    pub(crate) fn invalidate_node_indexed_state(&mut self) {
        self.invalidate_graph_derived_state();
        self.compute_mask_len = 0;
        self.community_count_active = 0;
        self.last_cohesion_refresh_iter = 0;
        self.degree_weights_available = false;
        self.hold_overlay.invalidate();
        self.semantic = None;
    }
}

/// Engine fields that own device memory through another type, with that memory's lifecycle.
/// These are outside the graph-sized table on purpose; the registry test requires every such
/// field to be listed here.
#[cfg(test)]
pub(crate) const NESTED_DEVICE_STATE: &[(&str, &str)] = &[(
    "semantic",
    "SemanticGpu: per-node metadata and per-edge arrays sized from its own SemanticInputs by \
     enable_semantic; dropped on every graph change (invalidate_node_indexed_state) and rebuilt \
     by refresh_semantic after the actor's upload",
)];

#[cfg(test)]
mod registry_tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    /// Types that are device memory in themselves.
    const DEVICE_TYPES: &[&str] = &["DeviceBuffer", "DeviceBox", "DeviceVariable", "DeviceSlice"];

    /// Source with `//` and `/* */` comments removed (doc comments mention `DeviceBuffer`).
    fn strip_comments(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let b = src.as_bytes();
        let (mut i, mut in_str) = (0, false);
        while i < b.len() {
            if in_str {
                if b[i] == b'\\' && i + 1 < b.len() {
                    out.push(b[i] as char);
                    out.push(b[i + 1] as char);
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    in_str = false;
                }
                out.push(b[i] as char);
                i += 1;
            } else if b[i] == b'"' {
                in_str = true;
                out.push('"');
                i += 1;
            } else if b[i..].starts_with(b"//") {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            } else if b[i..].starts_with(b"/*") {
                i += 2;
                while i + 1 < b.len() && !b[i..].starts_with(b"*/") {
                    i += 1;
                }
                i += 2;
            } else {
                out.push(b[i] as char);
                i += 1;
            }
        }
        out
    }

    fn identifiers(text: &str) -> impl Iterator<Item = &str> {
        text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|t| !t.is_empty())
    }

    /// `(name, type)` for each field of every braced struct in `src`, keyed by struct name.
    fn structs(src: &str) -> BTreeMap<String, Vec<(String, String)>> {
        let mut found = BTreeMap::new();
        let mut rest = src;
        while let Some(at) = rest.find("struct ") {
            let after = &rest[at + 7..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let Some(open) = after.find(['{', ';', '(']) else {
                break;
            };
            rest = &after[open..];
            if name.is_empty() || !after[open..].starts_with('{') {
                continue;
            }
            let (mut depth, mut end) = (0i32, 0usize);
            for (k, c) in rest.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &rest[1..end];
            rest = &rest[end..];
            found.insert(name, fields(body));
        }
        found
    }

    /// Splits a struct body at top-level commas and reads `name: Type` from each field.
    fn fields(body: &str) -> Vec<(String, String)> {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0i32, 0usize);
        for (k, c) in body.char_indices() {
            match c {
                '<' | '(' | '[' | '{' => depth += 1,
                '>' | ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(&body[start..k]);
                    start = k + 1;
                }
                _ => {}
            }
        }
        parts.push(&body[start..]);
        parts
            .into_iter()
            .filter_map(|field| {
                let mut f = field.trim();
                while f.starts_with("#[") {
                    f = f[f.find(']')? + 1..].trim_start();
                }
                for vis in ["pub(crate)", "pub(super)", "pub"] {
                    if let Some(r) = f.strip_prefix(vis) {
                        f = r.trim_start();
                        break;
                    }
                }
                let (name, ty) = f.split_once(':')?;
                Some((
                    name.trim().to_string(),
                    ty.split_whitespace().collect::<String>(),
                ))
            })
            .collect()
    }

    /// `type Name = ...;` aliases whose right-hand side mentions device memory.
    fn device_aliases(src: &str) -> BTreeSet<String> {
        src.split("type ")
            .skip(1)
            .filter_map(|t| {
                let (name, rhs) = t.split_once('=')?;
                let rhs = rhs.split(';').next()?;
                let name = name.split('<').next()?.trim();
                let valid = !name.is_empty() && identifiers(name).count() == 1;
                (valid && identifiers(rhs).any(|i| DEVICE_TYPES.contains(&i)))
                    .then(|| name.to_string())
            })
            .collect()
    }

    /// The engine fields that hold device memory directly, and those that hold it through
    /// another struct, from the given sources (comments already stripped).
    fn engine_device_fields(
        module_sources: &[String],
        alias_sources: &[String],
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut direct_words: BTreeSet<String> =
            DEVICE_TYPES.iter().map(|s| s.to_string()).collect();
        for src in alias_sources {
            direct_words.extend(device_aliases(src));
        }
        let mut all = BTreeMap::new();
        for src in module_sources {
            all.extend(structs(src));
        }
        let engine = all
            .remove("UnifiedGPUCompute")
            .expect("UnifiedGPUCompute struct");
        // Fixpoint: a struct holds device memory if a field's type names device memory or
        // another struct that does.
        let mut holders: BTreeSet<String> = BTreeSet::new();
        loop {
            let before = holders.len();
            for (name, fs) in &all {
                if fs.iter().any(|(_, ty)| {
                    identifiers(ty).any(|i| direct_words.contains(i) || holders.contains(i))
                }) {
                    holders.insert(name.clone());
                }
            }
            if holders.len() == before {
                break;
            }
        }
        let (mut direct, mut nested) = (BTreeSet::new(), BTreeSet::new());
        for (name, ty) in engine {
            if identifiers(&ty).any(|i| direct_words.contains(i)) {
                direct.insert(name);
            } else if identifiers(&ty).any(|i| holders.contains(i)) {
                nested.insert(name);
            }
        }
        (direct, nested)
    }

    fn rust_sources(dir: &Path, recurse: bool, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() && recurse {
                rust_sources(&path, true, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(strip_comments(
                    &std::fs::read_to_string(&path).expect("read"),
                ));
            }
        }
    }

    #[test]
    fn the_scanner_sees_split_lines_aliases_and_nested_structs() {
        let fixture = strip_comments(
            r#"
            type Buf = DeviceBuffer<f32>;
            struct Inner { a: DeviceBuffer<i32>, b: usize }
            struct Outer { inner: Inner }
            struct HostOnly { v: Vec<f32> }
            pub struct UnifiedGPUCompute {
                pub multi_line:
                    DeviceBuffer<f32>,
                pub(crate) aliased: Buf,
                #[cfg(test)]
                pub nested: Option<Outer>,
                /// A doc comment naming DeviceBuffer<f32> is not a field type.
                pub plain: usize,
                host: HostOnly,
                pair: (Vec<f32>, Vec<f32>),
            }
            "#,
        );
        let (direct, nested) = engine_device_fields(&[fixture.clone()], &[fixture]);
        assert_eq!(
            direct,
            BTreeSet::from(["aliased".into(), "multi_line".into()])
        );
        assert_eq!(nested, BTreeSet::from(["nested".to_string()]));
    }

    #[test]
    fn every_device_buffer_field_is_registered_fixed_extent_or_nested() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut module = Vec::new();
        rust_sources(&root.join("utils/unified_gpu_compute"), false, &mut module);
        let mut crate_wide = Vec::new();
        rust_sources(&root, true, &mut crate_wide);
        let (direct, nested) = engine_device_fields(&module, &crate_wide);
        assert!(
            direct.len() > 60,
            "scan found only {} direct fields",
            direct.len()
        );

        let registered: BTreeSet<&str> = GRAPH_SIZED.iter().map(|(n, _)| *n).collect();
        let fixed: BTreeSet<&str> = FIXED_EXTENT.iter().map(|(n, _)| *n).collect();
        let listed_nested: BTreeSet<&str> = NESTED_DEVICE_STATE.iter().map(|(n, _)| *n).collect();
        let missing: Vec<&String> = direct
            .iter()
            .filter(|f| !registered.contains(f.as_str()) && !fixed.contains(f.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "device buffers on the engine absent from the registry: {missing:?}. Add each to \
             graph_sized_buffers! (if its length follows the graph) or FIXED_EXTENT."
        );
        let unlisted: Vec<&String> = nested
            .iter()
            .filter(|f| !listed_nested.contains(f.as_str()))
            .collect();
        assert!(
            unlisted.is_empty(),
            "engine fields holding device memory through another type, not in \
             NESTED_DEVICE_STATE: {unlisted:?}"
        );
        for name in &registered {
            assert!(
                direct.contains(*name),
                "{name} is registered but is not a device field"
            );
            assert!(!fixed.contains(name), "{name} is listed twice");
        }
        for name in &fixed {
            assert!(
                direct.contains(*name),
                "{name} is fixed-extent but is not a device field"
            );
        }
        for name in &listed_nested {
            assert!(
                nested.contains(*name),
                "{name} is listed as nested but holds no device memory"
            );
        }
    }

    #[test]
    fn extents_cover_the_live_graph() {
        for &(nodes, edges) in &[(0usize, 0usize), (1, 0), (1000, 1000), (1501, 4003)] {
            for &(name, extent) in GRAPH_SIZED {
                assert!(
                    extent.capacity(nodes, edges) >= extent.required(nodes, edges),
                    "{name} at {nodes}/{edges}"
                );
                assert!(extent.capacity(nodes, edges) >= 1, "{name} would be empty");
            }
        }
    }
}
