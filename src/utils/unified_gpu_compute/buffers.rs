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
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// Field names of `UnifiedGPUCompute` whose type mentions `DeviceBuffer`, read from the
    /// struct definition in `construction.rs`.
    fn device_buffer_fields() -> Vec<String> {
        let source = include_str!("construction.rs");
        let start = source
            .find("pub struct UnifiedGPUCompute {")
            .expect("struct definition");
        let body = &source[start..];
        let end = body.find("\n}\n").expect("end of struct");
        body[..end]
            .lines()
            .filter(|line| line.contains("DeviceBuffer<"))
            .filter_map(|line| {
                let line = line.trim();
                let line = line.strip_prefix("pub(crate) ").unwrap_or(line);
                let line = line.strip_prefix("pub ").unwrap_or(line);
                line.split(':').next().map(|name| name.trim().to_string())
            })
            .collect()
    }

    #[test]
    fn every_device_buffer_field_is_registered_or_fixed_extent() {
        let fields = device_buffer_fields();
        assert!(
            fields.len() > 60,
            "struct scan found only {} fields",
            fields.len()
        );
        let registered: Vec<&str> = GRAPH_SIZED.iter().map(|(n, _)| *n).collect();
        let fixed: Vec<&str> = FIXED_EXTENT.iter().map(|(n, _)| *n).collect();
        let missing: Vec<&String> = fields
            .iter()
            .filter(|f| !registered.contains(&f.as_str()) && !fixed.contains(&f.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "device buffers allocated at construction but absent from the registry: {missing:?}. \
             Add each to graph_sized_buffers! (if its length follows the graph) or FIXED_EXTENT."
        );
        for name in registered.iter().chain(&fixed) {
            assert!(
                fields.iter().any(|f| f == name),
                "{name} is listed but is not a DeviceBuffer field of UnifiedGPUCompute"
            );
        }
        for name in &registered {
            assert!(!fixed.contains(name), "{name} is listed twice");
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
