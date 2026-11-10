//! Construction and initialization of the `UnifiedGPUCompute` struct.

use super::buffers::{Extent, GraphSizedBuffers};
use super::types::{curandState, Aabb, GPUPerformanceMetrics};
use crate::models::constraints::ConstraintData;
pub use crate::models::simulation_params::SimParams;
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::device::Device;
use cust::event::{Event, EventFlags};
use cust::memory::DeviceBuffer;
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};
use log::{error, info};

/// Frames between community-label refreshes (Leiden/Louvain) for force cohesion.
/// Detection is topology-driven (modularity over CSR adjacency) with host-side
/// refinement/aggregation, so cost scales with graph size — ~10s for a 17k-node
/// knowledge graph — and it runs INLINE on the physics execute thread, blocking
/// the step for its duration. Within a session the graph topology is static
/// (identical modularity across refreshes), and the two cheap mechanisms keep
/// cohesion correct between refreshes: the per-frame centroid reduction tracks
/// live positions, and a buffer resize (node add/remove) force-resets the
/// refresh (memory.rs). So labels only need infrequent recompute to catch
/// edge-only background re-syncs. Frame-based, so it self-scales: larger graphs
/// run fewer fps → longer wall-clock interval, exactly where detection is
/// costliest. 3600 frames ≈ 60s at 60fps (small graphs), several minutes at the
/// few-fps rates seen on large graphs.
pub const COHESION_REFRESH_INTERVAL: u32 = 3600;

pub struct UnifiedGPUCompute {
    pub(crate) device: Device,
    pub(crate) _context: Context,
    pub(crate) _module: Module,
    pub(crate) clustering_module: Option<Module>,
    pub(crate) stream: Stream,

    pub(crate) build_grid_kernel_name: &'static str,
    pub(crate) compute_cell_bounds_kernel_name: &'static str,
    pub(crate) force_pass_kernel_name: &'static str,
    pub(crate) integrate_pass_kernel_name: &'static str,

    pub(crate) params: SimParams,

    pub pos_in_x: DeviceBuffer<f32>,
    pub pos_in_y: DeviceBuffer<f32>,
    pub pos_in_z: DeviceBuffer<f32>,
    pub vel_in_x: DeviceBuffer<f32>,
    pub vel_in_y: DeviceBuffer<f32>,
    pub vel_in_z: DeviceBuffer<f32>,

    pub pos_out_x: DeviceBuffer<f32>,
    pub pos_out_y: DeviceBuffer<f32>,
    pub pos_out_z: DeviceBuffer<f32>,
    pub vel_out_x: DeviceBuffer<f32>,
    pub vel_out_y: DeviceBuffer<f32>,
    pub vel_out_z: DeviceBuffer<f32>,

    pub mass: DeviceBuffer<f32>,
    pub node_graph_id: DeviceBuffer<i32>,

    // Ontology class metadata for class-based physics
    pub class_id: DeviceBuffer<i32>, // Maps owl_class_iri to integer class ID
    pub class_charge: DeviceBuffer<f32>, // Class-specific charge modifiers
    pub class_mass: DeviceBuffer<f32>, // Class-specific mass modifiers
    // Per-population spring strength multiplier (Knowledge/Ontology/Agent).
    // Default 1.0 == identity (current LinLog coefficient). Read in both spring paths.
    pub spring_scale: DeviceBuffer<f32>,

    // Per-node pinned mask (0 = free, non-0 = pinned). A pinned node has its
    // integration skipped (held at the host-supplied position) but still exerts
    // repulsion/spring forces on its neighbours. Populated via upload_pinned_mask.
    pub pinned_mask: DeviceBuffer<i32>,

    // Per-node DAG hierarchy rank for the radial bias force (PHASE 2). 0 = root,
    // deeper = larger; -1.0 = not in the hierarchy / unreachable (no bias applied).
    // Populated via upload_node_rank; default -1.0 leaves the term inert.
    pub node_rank: DeviceBuffer<f32>,

    // Per-node centered plane offset for the stratified-plane bias (ADR-141 P2).
    // e.g. -1, 0, +1; NaN = not assigned to any plane (no bias applied).
    // Populated via upload_node_plane; default NaN leaves the term inert.
    pub node_plane: DeviceBuffer<f32>,

    pub edge_row_offsets: DeviceBuffer<i32>,
    pub edge_col_indices: DeviceBuffer<i32>,
    pub edge_weights: DeviceBuffer<f32>,
    /// ADR-2169 amendment (spring mode while held): per-CSR-edge length captured when the
    /// hold begins. Only read while the springs run (bound through `g_edge_rest_snapshot`);
    /// sized to `allocated_edges`.
    pub(crate) edge_rest_snapshot: DeviceBuffer<f32>,
    /// Elastic-overlay state: active flag, snapshot validity, snapshot count.
    pub(crate) hold_overlay: super::hold_overlay::HoldOverlay,
    /// Device address last written to the kernels' `g_edge_rest_snapshot` (0 = overlay off).
    pub(crate) edge_rest_bound: std::sync::atomic::AtomicU64,
    /// Multiplier on `spring_k` and `max_force` while the spring term runs (a hold only).
    pub(crate) hold_spring_gain: f32,

    pub(crate) force_x: DeviceBuffer<f32>,
    pub(crate) force_y: DeviceBuffer<f32>,
    pub(crate) force_z: DeviceBuffer<f32>,

    pub(crate) cell_keys: DeviceBuffer<i32>,
    pub(crate) sorted_node_indices: DeviceBuffer<i32>,
    // Persistent Thrust grid-sort output buffers (sized `allocated_nodes`).
    // Hoisted out of the per-frame hot path so they are allocated once and
    // reused every frame instead of `DeviceBuffer::zeroed(allocated_nodes)` per sort.
    pub(crate) sort_keys_out: DeviceBuffer<i32>,
    pub(crate) sort_values_out: DeviceBuffer<i32>,
    pub(crate) cell_start: DeviceBuffer<i32>,
    pub(crate) cell_end: DeviceBuffer<i32>,

    pub num_nodes: usize,
    pub num_edges: usize,
    pub(crate) allocated_nodes: usize,
    pub(crate) allocated_edges: usize,
    pub max_grid_cells: usize,
    pub(crate) iteration: i32,

    pub(crate) zero_buffer: Vec<i32>,

    pub(crate) cell_buffer_growth_factor: f32,
    pub(crate) max_allowed_grid_cells: usize,
    pub(crate) resize_count: usize,
    pub(crate) total_memory_allocated: usize,

    pub dist: DeviceBuffer<f32>,
    pub current_frontier: DeviceBuffer<i32>,
    pub next_frontier_flags: DeviceBuffer<i32>,

    pub(crate) sssp_stream: Option<Stream>,

    pub(crate) constraint_data: DeviceBuffer<ConstraintData>,
    pub(crate) num_constraints: usize,
    /// Host copy of the resident constraints, kept so the per-node index can be rebuilt when the
    /// node capacity changes (`resize_buffers`).
    pub(crate) constraints_host: Vec<ConstraintData>,
    /// How many times `set_constraints` has committed a set to the device. Tests read it to prove
    /// the actor uploads on change only.
    pub(crate) constraint_uploads: u64,
    /// Per-node constraint index (CSR) the force kernels read through the
    /// `g_constraint_index` device global; see `constraint_index.rs`.
    pub(crate) constraint_node_offsets: DeviceBuffer<i32>,
    pub(crate) constraint_node_ids: DeviceBuffer<i32>,

    pub sssp_available: bool,

    /// Persistent device-side copy of SSSP distances for the force kernel's
    /// `d_sssp_dist` parameter.  Populated after each successful `run_sssp()`.
    pub(crate) sssp_device_distances: Option<DeviceBuffer<f32>>,

    /// Whether the SSSP-spring-adjust feature is enabled (toggled at runtime).
    pub(crate) sssp_spring_adjust_enabled: bool,

    pub(crate) performance_metrics: GPUPerformanceMetrics,

    pub centroids_x: DeviceBuffer<f32>,
    pub centroids_y: DeviceBuffer<f32>,
    pub centroids_z: DeviceBuffer<f32>,
    pub cluster_assignments: DeviceBuffer<i32>,
    pub distances_to_centroid: DeviceBuffer<f32>,
    pub cluster_sizes: DeviceBuffer<i32>,
    pub partial_inertia: DeviceBuffer<f32>,
    pub min_distances: DeviceBuffer<f32>,
    pub selected_nodes: DeviceBuffer<i32>,
    pub max_clusters: usize,

    /// Per-community centroids for Louvain-driven cohesion. Sized to num_nodes
    /// (Louvain can yield up to N communities, unlike K-means' fixed max_clusters).
    /// Recomputed every frame from live positions so cohesion tracks each
    /// community's running center of mass. Reuses cluster_assignments as labels
    /// and community_sizes as the per-community counter.
    pub community_centroids_x: DeviceBuffer<f32>,
    pub community_centroids_y: DeviceBuffer<f32>,
    pub community_centroids_z: DeviceBuffer<f32>,
    /// Number of distinct communities currently labelled in cluster_assignments
    /// (0 until the first Louvain refresh). Passed to the centroid + cohesion
    /// kernels as `num_clusters`; both guard `label < num_clusters`.
    pub community_count_active: usize,
    /// Active clustering algorithm driving force cohesion ("louvain" | "kmeans").
    /// Defaults to PhysicsSettings canonical ("louvain"). Gated by cluster_strength.
    pub clustering_algorithm: String,
    /// Louvain modularity resolution (PhysicsSettings default 1.0).
    pub clustering_resolution: f32,
    /// Louvain max local-pass iterations (PhysicsSettings default 50).
    pub clustering_iterations: u32,
    /// Frames between Louvain label refreshes. Labels are topology-driven and
    /// expensive (host round-trips); centroids recompute every frame regardless.
    pub cohesion_refresh_interval: u32,
    /// Iteration index of the last Louvain label refresh (force-step counter).
    pub last_cohesion_refresh_iter: i32,

    pub lof_scores: DeviceBuffer<f32>,
    pub local_densities: DeviceBuffer<f32>,
    pub zscore_values: DeviceBuffer<f32>,
    pub feature_values: DeviceBuffer<f32>,
    pub partial_sums: DeviceBuffer<f32>,
    pub partial_sq_sums: DeviceBuffer<f32>,

    pub labels_current: DeviceBuffer<i32>,
    pub labels_next: DeviceBuffer<i32>,
    pub label_counts: DeviceBuffer<i32>,
    pub convergence_flag: DeviceBuffer<i32>,
    pub node_degrees: DeviceBuffer<f32>,
    pub community_sizes: DeviceBuffer<i32>,
    pub label_mapping: DeviceBuffer<i32>,
    pub rand_states: DeviceBuffer<curandState>,

    pub partial_kinetic_energy: DeviceBuffer<f32>,
    pub active_node_count: DeviceBuffer<i32>,
    pub should_skip_physics: DeviceBuffer<i32>,
    pub system_kinetic_energy: DeviceBuffer<f32>,

    pub(crate) transfer_stream: Stream,
    pub(crate) transfer_events: [Event; 2],

    pub(crate) host_pos_buffer_a: (Vec<f32>, Vec<f32>, Vec<f32>),
    pub(crate) host_pos_buffer_b: (Vec<f32>, Vec<f32>, Vec<f32>),
    pub(crate) host_vel_buffer_a: (Vec<f32>, Vec<f32>, Vec<f32>),
    pub(crate) host_vel_buffer_b: (Vec<f32>, Vec<f32>, Vec<f32>),

    pub(crate) current_pos_buffer: bool,
    pub(crate) current_vel_buffer: bool,
    pub(crate) pos_transfer_pending: bool,
    pub(crate) vel_transfer_pending: bool,

    pub(crate) aabb_block_results: DeviceBuffer<Aabb>,
    pub(crate) aabb_num_blocks: usize,

    /// Pre-computed degree weights for degree-weighted gravity.
    /// Each element is log(1 + degree) for the corresponding node.
    /// Isolated nodes (degree 0) have weight 0.0.
    pub degree_weight: DeviceBuffer<f32>,
    /// Whether degree weights have been uploaded (graph data available).
    pub(crate) degree_weights_available: bool,

    /// Semantic forces (DAG / type / physicality / role clustering, collision, attribute
    /// springs, maturity, cross-domain). `None` until a graph's derived metadata is uploaded
    /// with `enable_semantic`. Never a hidden fallback: absent means not applied.
    pub semantic: Option<super::semantic::SemanticGpu>,
    /// True when the stored configuration has at least one force enabled; the step skips the
    /// whole pass otherwise. Derived from the store's `enabled` flags, not a separate switch.
    pub semantic_active: bool,

    /// FA2 adaptive speed: previous-step force per node (for swing/traction calculation).
    /// Initialized to zero; updated each integration step when adaptive_speed is enabled.
    pub(crate) prev_force_x: DeviceBuffer<f32>,
    pub(crate) prev_force_y: DeviceBuffer<f32>,
    pub(crate) prev_force_z: DeviceBuffer<f32>,

    /// ADR-070 D2.2: per-node total constraint-force magnitude, written by the
    /// force pass and read by the stability check as the third (constraint)
    /// criterion. Persistent so the stability kernel (which runs *before* the
    /// force pass in a tick) sees the previous tick's magnitudes.
    pub(crate) node_constraint_force: DeviceBuffer<f32>,

    /// ADR-070 D3.1 (P2): sparse compute mask. When `compute_mask_len > 0` the
    /// masked force pass evaluates only these node indices (built host-side by
    /// `visionclaw_gpu::hardening::build_compute_mask_with_neighbors`: visible
    /// persona nodes + their 1-hop neighbours). Capacity is `allocated_nodes`.
    pub(crate) compute_mask: DeviceBuffer<i32>,
    /// Number of valid entries in `compute_mask`; 0 = mask inactive (evaluate all).
    pub(crate) compute_mask_len: usize,
}

impl UnifiedGPUCompute {
    pub fn new(num_nodes: usize, num_edges: usize, ptx_content: &str) -> Result<Self> {
        Self::new_with_modules(num_nodes, num_edges, ptx_content, None, None)
    }

    pub fn new_with_modules(
        num_nodes: usize,
        num_edges: usize,
        ptx_content: &str,
        clustering_ptx: Option<&str>,
        apsp_ptx: Option<&str>,
    ) -> Result<Self> {
        Self::new_with_all_modules(num_nodes, num_edges, ptx_content, clustering_ptx, apsp_ptx)
    }

    // ADR-098 D3: the separate `ontology_ptx` parameter (and the
    // `ontology_constraints.cu` module it loaded) is retired. Ontology
    // constraints now flow through the generic live `force_pass_kernel`
    // constraint loop via the ConstraintData buffer.
    pub fn new_with_all_modules(
        num_nodes: usize,
        num_edges: usize,
        ptx_content: &str,
        clustering_ptx: Option<&str>,
        apsp_ptx: Option<&str>,
    ) -> Result<Self> {
        if let Err(e) = crate::utils::gpu_diagnostics::validate_ptx_content(ptx_content) {
            let diagnosis = crate::utils::gpu_diagnostics::diagnose_ptx_error(&e);
            return Err(anyhow!("PTX validation failed: {}\n{}", e, diagnosis));
        }

        let device = Device::get_device(0)?;
        let _context = Context::new(device)?;

        let module = Module::from_ptx(ptx_content, &[]).map_err(|e| {
            let error_msg = format!("Module::from_ptx() failed: {}", e);
            let diagnosis = crate::utils::gpu_diagnostics::diagnose_ptx_error(&error_msg);
            anyhow!("{}\n{}", error_msg, diagnosis)
        })?;

        // Secondary-module load failures degrade the feature rather than the
        // whole GPU actor, but must be unmissable in logs (ADR-2030 follow-up:
        // a silent None here previously read as "feature absent" rather than
        // "feature broken").
        let clustering_module = if let Some(clustering_ptx_content) = clustering_ptx {
            if let Err(e) =
                crate::utils::gpu_diagnostics::validate_ptx_content(clustering_ptx_content)
            {
                error!(
                    "GPU DEGRADED: clustering PTX validation failed: {}. \
                     Clustering analytics are DISABLED for this process.",
                    e
                );
                None
            } else {
                match Module::from_ptx(clustering_ptx_content, &[]) {
                    Ok(module) => {
                        info!("Successfully loaded clustering module");
                        Some(module)
                    }
                    Err(e) => {
                        error!(
                            "GPU DEGRADED: clustering module load failed: {}. \
                             Clustering analytics are DISABLED for this process.",
                            e
                        );
                        None
                    }
                }
            }
        } else {
            None
        };

        // REMOVED (ADR-2054): the APSP module load. `apsp_module` had zero read
        // sites — nothing ever resolved a kernel from it — so the load cost a
        // startup PTX parse and, worse, logged "GPU DEGRADED: GPU APSP is
        // DISABLED" about a capability NFR-7 forbids permanently (see
        // ShortestPathActor::ComputeAPSP). The `apsp_ptx` parameter is retained
        // in the signature and ignored so callers need not change shape.
        let _ = apsp_ptx;

        let stream = Stream::new(StreamFlags::NON_BLOCKING, None)?;

        // Every graph-sized buffer comes from the one registered allocator (buffers.rs), the
        // same one `resize_buffers` uses. The destructuring names every field, so a buffer
        // added to the table and not to the engine (or the reverse) fails to compile.
        let GraphSizedBuffers {
            pos_in_x,
            pos_in_y,
            pos_in_z,
            vel_in_x,
            vel_in_y,
            vel_in_z,
            pos_out_x,
            pos_out_y,
            pos_out_z,
            vel_out_x,
            vel_out_y,
            vel_out_z,
            force_x,
            force_y,
            force_z,
            prev_force_x,
            prev_force_y,
            prev_force_z,
            mass,
            node_graph_id,
            class_id,
            class_charge,
            class_mass,
            spring_scale,
            pinned_mask,
            node_rank,
            node_plane,
            degree_weight,
            node_constraint_force,
            compute_mask,
            edge_row_offsets,
            edge_col_indices,
            edge_weights,
            edge_rest_snapshot,
            cell_keys,
            sorted_node_indices,
            sort_keys_out,
            sort_values_out,
            dist,
            current_frontier,
            next_frontier_flags,
            cluster_assignments,
            distances_to_centroid,
            community_centroids_x,
            community_centroids_y,
            community_centroids_z,
            community_sizes,
            min_distances,
            node_degrees,
            labels_current,
            labels_next,
            label_counts,
            label_mapping,
            rand_states,
            lof_scores,
            local_densities,
            zscore_values,
            feature_values,
            partial_inertia,
            partial_sums,
            partial_sq_sums,
            partial_kinetic_energy,
            aabb_block_results,
        } = GraphSizedBuffers::allocate(num_nodes, num_edges)?;
        let allocated_nodes = Extent::Nodes.capacity(num_nodes, num_edges);
        let allocated_edges = Extent::Edges.capacity(num_nodes, num_edges);

        let max_grid_cells = 32 * 32 * 32;
        let cell_start = DeviceBuffer::zeroed(max_grid_cells)?;
        let cell_end = DeviceBuffer::zeroed(max_grid_cells)?;

        let sssp_stream = Some(Stream::new(StreamFlags::NON_BLOCKING, None)?);

        let max_clusters = 50;
        let centroids_x = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_y = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_z = DeviceBuffer::zeroed(max_clusters)?;
        let cluster_sizes = DeviceBuffer::zeroed(max_clusters)?;
        let selected_nodes = DeviceBuffer::zeroed(max_clusters)?;
        let convergence_flag = DeviceBuffer::from_slice(&[1i32])?;

        let kernel_module = module;

        let initial_memory = Self::calculate_memory_usage(num_nodes, num_edges, max_grid_cells);

        let gpu_compute = Self {
            device,
            _context,
            _module: kernel_module,
            clustering_module,
            stream,
            build_grid_kernel_name: "build_grid_kernel",
            compute_cell_bounds_kernel_name: "compute_cell_bounds_kernel",
            force_pass_kernel_name: "force_pass_kernel",
            integrate_pass_kernel_name: "integrate_pass_kernel",
            params: SimParams::default(),
            pos_in_x,
            pos_in_y,
            pos_in_z,
            vel_in_x,
            vel_in_y,
            vel_in_z,
            pos_out_x,
            pos_out_y,
            pos_out_z,
            vel_out_x,
            vel_out_y,
            vel_out_z,
            mass,
            node_graph_id,
            pinned_mask,
            node_rank,
            node_plane,
            class_id,
            class_charge,
            class_mass,
            spring_scale,
            edge_row_offsets,
            edge_col_indices,
            edge_weights,
            edge_rest_snapshot,
            hold_overlay: Default::default(),
            edge_rest_bound: std::sync::atomic::AtomicU64::new(0),
            hold_spring_gain: super::HOLD_SPRING_GAIN,
            force_x,
            force_y,
            force_z,
            cell_keys,
            sorted_node_indices,
            sort_keys_out,
            sort_values_out,
            cell_start,
            cell_end,
            num_nodes,
            num_edges,
            allocated_nodes,
            allocated_edges,
            max_grid_cells,
            iteration: 0,
            zero_buffer: vec![0i32; max_grid_cells],

            dist,
            current_frontier,
            next_frontier_flags,
            sssp_stream,

            constraint_data: DeviceBuffer::from_slice(&[])?,
            num_constraints: 0,
            constraints_host: Vec::new(),
            constraint_uploads: 0,
            constraint_node_offsets: DeviceBuffer::from_slice(&[])?,
            constraint_node_ids: DeviceBuffer::from_slice(&[])?,
            sssp_available: false,
            sssp_device_distances: None,
            sssp_spring_adjust_enabled: false,
            performance_metrics: GPUPerformanceMetrics::default(),

            centroids_x,
            centroids_y,
            centroids_z,
            cluster_assignments,
            distances_to_centroid,
            cluster_sizes,
            partial_inertia,
            min_distances,
            selected_nodes,
            max_clusters,

            community_centroids_x,
            community_centroids_y,
            community_centroids_z,
            community_count_active: 0,
            // Leiden is the default discrete community detector — strict upgrade
            // over Louvain (guarantees connected communities; eases the modularity
            // resolution limit). "louvain"/"kmeans" remain selectable. Resolution
            // and iteration bounds match PhysicsSettings::default() (physics_config.rs:372).
            clustering_algorithm: "leiden".to_string(),
            clustering_resolution: 1.0,
            clustering_iterations: 50,
            cohesion_refresh_interval: COHESION_REFRESH_INTERVAL,
            last_cohesion_refresh_iter: 0,

            lof_scores,
            local_densities,
            zscore_values,
            feature_values,
            partial_sums,
            partial_sq_sums,

            labels_current,
            labels_next,
            label_counts,
            convergence_flag,
            node_degrees,
            community_sizes,
            label_mapping,
            rand_states,

            cell_buffer_growth_factor: 1.5,
            max_allowed_grid_cells: 128 * 128 * 128,
            resize_count: 0,
            total_memory_allocated: initial_memory,

            partial_kinetic_energy,
            active_node_count: DeviceBuffer::zeroed(1)?,
            should_skip_physics: DeviceBuffer::zeroed(1)?,
            system_kinetic_energy: DeviceBuffer::zeroed(1)?,

            transfer_stream: Stream::new(StreamFlags::NON_BLOCKING, None)?,
            transfer_events: [
                Event::new(EventFlags::DEFAULT)?,
                Event::new(EventFlags::DEFAULT)?,
            ],

            host_pos_buffer_a: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_pos_buffer_b: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_vel_buffer_a: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),
            host_vel_buffer_b: (
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
                vec![0.0f32; num_nodes],
            ),

            current_pos_buffer: false,
            current_vel_buffer: false,
            pos_transfer_pending: false,
            vel_transfer_pending: false,

            aabb_num_blocks: Extent::NodeBlocks.capacity(num_nodes, num_edges),
            aabb_block_results,

            degree_weight,
            degree_weights_available: false,
            semantic: None,
            semantic_active: false,

            prev_force_x,
            prev_force_y,
            prev_force_z,

            // ADR-070 D2.2 / D3.1 buffers (num_nodes-sized, mask inactive at start).
            node_constraint_force,
            compute_mask,
            compute_mask_len: 0,
        };

        Ok(gpu_compute)
    }

    pub(crate) fn calculate_memory_usage(
        num_nodes: usize,
        num_edges: usize,
        max_grid_cells: usize,
    ) -> usize {
        let node_memory = num_nodes * (12 * 4 + 4 + 4);

        let edge_memory = (num_nodes + 1) * 4 + num_edges * (4 + 4);

        let grid_memory = max_grid_cells * (4 + 4) + num_nodes * (4 + 4);

        let force_memory = num_nodes * 3 * 4;

        let other_memory = num_nodes * 10 * 4;

        node_memory + edge_memory + grid_memory + force_memory + other_memory
    }
}
