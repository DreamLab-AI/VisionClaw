//! Construction and initialization of the `UnifiedGPUCompute` struct.

use super::types::{curandState, GPUPerformanceMetrics, AABB};
use crate::models::constraints::ConstraintData;
pub use crate::models::simulation_params::SimParams;
use anyhow::{anyhow, Result};
use cust::context::Context;
use cust::device::Device;
use cust::event::{Event, EventFlags};
use cust::memory::{CopyDestination, DeviceBuffer};
use cust::module::Module;
use cust::stream::{Stream, StreamFlags};
use log::{info, warn};

#[allow(dead_code)]
pub struct UnifiedGPUCompute {
    pub(crate) device: Device,
    pub(crate) _context: Context,
    pub(crate) _module: Module,
    pub(crate) clustering_module: Option<Module>,
    pub(crate) apsp_module: Option<Module>,
    pub(crate) ontology_module: Option<Module>,
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
    pub class_id: DeviceBuffer<i32>,        // Maps owl_class_iri to integer class ID
    pub class_charge: DeviceBuffer<f32>,    // Class-specific charge modifiers
    pub class_mass: DeviceBuffer<f32>,      // Class-specific mass modifiers

    // Semantic metadata per node — drives semantic_forces.cu kernels.
    // All default to 0 (None/Unknown) so graphs without semantic data run kernels harmlessly.
    pub physicality_code: DeviceBuffer<i32>,   // PhysicalityCode enum: 0=None,1=Abstract,2=Virtual,3=Conceptual,255=Unknown
    pub role_code: DeviceBuffer<i32>,          // RoleCode enum: 0=None,1=Concept,2=Object,3=Process,4=Domain,5=Method,6=Agent,255=Unknown
    pub maturity_level: DeviceBuffer<i32>,     // MaturityLevel enum: 0=None,1=Emerging,2=Mature,3=Declining,255=Unknown

    // Centroid accumulator buffers written by semantic centroid kernels.
    pub physicality_centroids: DeviceBuffer<f32>, // 4 entries × 3 components = 12 floats (float3 × 4)
    pub physicality_counts: DeviceBuffer<i32>,    // 4 entries (one per physicality bucket)
    pub role_centroids: DeviceBuffer<f32>,        // 7 entries × 3 components = 21 floats (float3 × 7)
    pub role_counts: DeviceBuffer<i32>,           // 7 entries (one per role bucket)


    pub edge_row_offsets: DeviceBuffer<i32>,
    pub edge_col_indices: DeviceBuffer<i32>,
    pub edge_weights: DeviceBuffer<f32>,


    pub(crate) force_x: DeviceBuffer<f32>,
    pub(crate) force_y: DeviceBuffer<f32>,
    pub(crate) force_z: DeviceBuffer<f32>,


    pub(crate) cell_keys: DeviceBuffer<i32>,
    pub(crate) sorted_node_indices: DeviceBuffer<i32>,
    pub(crate) cell_start: DeviceBuffer<i32>,
    pub(crate) cell_end: DeviceBuffer<i32>,


    pub(crate) cub_temp_storage: DeviceBuffer<u8>,


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
    pub parents: Option<DeviceBuffer<i32>>,


    pub(crate) sssp_stream: Option<Stream>,


    pub(crate) constraint_data: DeviceBuffer<ConstraintData>,
    pub(crate) num_constraints: usize,


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
    pub modularity_contributions: DeviceBuffer<f32>,
    pub community_sizes: DeviceBuffer<i32>,
    pub label_mapping: DeviceBuffer<i32>,
    pub rand_states: DeviceBuffer<curandState>,
    pub max_labels: usize,


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


    pub(crate) aabb_block_results: DeviceBuffer<AABB>,
    pub(crate) aabb_num_blocks: usize,

    /// Pre-computed degree weights for degree-weighted gravity.
    /// Each element is log(1 + degree) for the corresponding node.
    /// Isolated nodes (degree 0) have weight 0.0.
    pub degree_weight: DeviceBuffer<f32>,
    /// Whether degree weights have been uploaded (graph data available).
    pub(crate) degree_weights_available: bool,

    /// FA2 adaptive speed: previous-step force per node (for swing/traction calculation).
    /// Initialized to zero; updated each integration step when adaptive_speed is enabled.
    pub(crate) prev_force_x: DeviceBuffer<f32>,
    pub(crate) prev_force_y: DeviceBuffer<f32>,
    pub(crate) prev_force_z: DeviceBuffer<f32>,
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
        Self::new_with_all_modules(num_nodes, num_edges, ptx_content, clustering_ptx, apsp_ptx, None)
    }

    pub fn new_with_all_modules(
        num_nodes: usize,
        num_edges: usize,
        ptx_content: &str,
        clustering_ptx: Option<&str>,
        apsp_ptx: Option<&str>,
        ontology_ptx: Option<&str>,
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


        let clustering_module = if let Some(clustering_ptx_content) = clustering_ptx {
            if let Err(e) =
                crate::utils::gpu_diagnostics::validate_ptx_content(clustering_ptx_content)
            {
                warn!(
                    "Clustering PTX validation failed: {}. Continuing without clustering support.",
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
                        warn!("Failed to load clustering module: {}. Continuing without clustering support.", e);
                        None
                    }
                }
            }
        } else {
            None
        };

        let apsp_module = if let Some(apsp_ptx_content) = apsp_ptx {
            if let Err(e) =
                crate::utils::gpu_diagnostics::validate_ptx_content(apsp_ptx_content)
            {
                warn!(
                    "APSP PTX validation failed: {}. Continuing without GPU APSP support.",
                    e
                );
                None
            } else {
                match Module::from_ptx(apsp_ptx_content, &[]) {
                    Ok(module) => {
                        info!("Successfully loaded APSP module");
                        Some(module)
                    }
                    Err(e) => {
                        warn!("Failed to load APSP module: {}. Continuing without GPU APSP support.", e);
                        None
                    }
                }
            }
        } else {
            None
        };

        let ontology_module = if let Some(ontology_ptx_content) = ontology_ptx {
            if let Err(e) =
                crate::utils::gpu_diagnostics::validate_ptx_content(ontology_ptx_content)
            {
                warn!(
                    "Ontology PTX validation failed: {}. Continuing without specialized ontology kernels.",
                    e
                );
                None
            } else {
                match Module::from_ptx(ontology_ptx_content, &[]) {
                    Ok(module) => {
                        info!("Successfully loaded ontology constraints module with specialized kernels");
                        Some(module)
                    }
                    Err(e) => {
                        warn!("Failed to load ontology module: {}. Falling back to generic constraint path.", e);
                        None
                    }
                }
            }
        } else {
            None
        };

        let stream = Stream::new(StreamFlags::NON_BLOCKING, None)?;


        let pos_in_x = DeviceBuffer::zeroed(num_nodes)?;
        let pos_in_y = DeviceBuffer::zeroed(num_nodes)?;
        let pos_in_z = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_x = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_y = DeviceBuffer::zeroed(num_nodes)?;
        let vel_in_z = DeviceBuffer::zeroed(num_nodes)?;

        let pos_out_x = DeviceBuffer::zeroed(num_nodes)?;
        let pos_out_y = DeviceBuffer::zeroed(num_nodes)?;
        let pos_out_z = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_x = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_y = DeviceBuffer::zeroed(num_nodes)?;
        let vel_out_z = DeviceBuffer::zeroed(num_nodes)?;


        let mass = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;
        let node_graph_id = DeviceBuffer::zeroed(num_nodes)?;

        // Initialize ontology class metadata buffers
        let class_id = DeviceBuffer::zeroed(num_nodes)?;           // Default class ID = 0 (unknown)
        let class_charge = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;  // Default charge = 1.0
        let class_mass = DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?;    // Default mass = 1.0

        // Semantic metadata buffers — zeroed so absent metadata maps to None/Unknown (0)
        let physicality_code = DeviceBuffer::zeroed(num_nodes)?;
        let role_code = DeviceBuffer::zeroed(num_nodes)?;
        let maturity_level = DeviceBuffer::zeroed(num_nodes)?;

        // Centroid accumulators — zeroed; reset at the start of each physics tick
        let physicality_centroids = DeviceBuffer::zeroed(12usize)?; // 4 × float3
        let physicality_counts = DeviceBuffer::zeroed(4usize)?;
        let role_centroids = DeviceBuffer::zeroed(21usize)?;        // 7 × float3
        let role_counts = DeviceBuffer::zeroed(7usize)?;

        let edge_row_offsets = DeviceBuffer::zeroed(num_nodes + 1)?;
        let edge_col_indices = DeviceBuffer::zeroed(num_edges)?;
        let edge_weights = DeviceBuffer::zeroed(num_edges)?;
        let force_x = DeviceBuffer::zeroed(num_nodes)?;
        let force_y = DeviceBuffer::zeroed(num_nodes)?;
        let force_z = DeviceBuffer::zeroed(num_nodes)?;


        let cell_keys = DeviceBuffer::zeroed(num_nodes)?;
        let mut sorted_node_indices = DeviceBuffer::zeroed(num_nodes)?;

        let initial_indices: Vec<i32> = (0..num_nodes as i32).collect();
        sorted_node_indices.copy_from(&initial_indices)?;



        let max_grid_cells = 32 * 32 * 32;
        let cell_start = DeviceBuffer::from_slice(&vec![-1i32; max_grid_cells])?;
        let cell_end = DeviceBuffer::from_slice(&vec![-1i32; max_grid_cells])?;


        let cub_temp_storage = Self::calculate_cub_temp_storage(num_nodes, max_grid_cells)?;


        let dist = DeviceBuffer::from_slice(&vec![f32::INFINITY; num_nodes])?;
        let current_frontier = DeviceBuffer::zeroed(num_nodes)?;
        let next_frontier_flags = DeviceBuffer::zeroed(num_nodes)?;
        let sssp_stream = Some(Stream::new(StreamFlags::NON_BLOCKING, None)?);


        let max_clusters = 50;
        let centroids_x = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_y = DeviceBuffer::zeroed(max_clusters)?;
        let centroids_z = DeviceBuffer::zeroed(max_clusters)?;
        let cluster_assignments = DeviceBuffer::zeroed(num_nodes)?;
        let distances_to_centroid = DeviceBuffer::zeroed(num_nodes)?;
        let cluster_sizes = DeviceBuffer::zeroed(max_clusters)?;

        let num_blocks = (num_nodes + 255) / 256;
        let partial_inertia = DeviceBuffer::zeroed(num_blocks)?;
        let min_distances = DeviceBuffer::zeroed(num_nodes)?;
        let selected_nodes = DeviceBuffer::zeroed(max_clusters)?;


        let lof_scores = DeviceBuffer::zeroed(num_nodes)?;
        let local_densities = DeviceBuffer::zeroed(num_nodes)?;
        let zscore_values = DeviceBuffer::zeroed(num_nodes)?;
        let feature_values = DeviceBuffer::zeroed(num_nodes)?;
        let partial_sums = DeviceBuffer::zeroed(num_blocks)?;
        let partial_sq_sums = DeviceBuffer::zeroed(num_blocks)?;


        let labels_current = DeviceBuffer::zeroed(num_nodes)?;
        let labels_next = DeviceBuffer::zeroed(num_nodes)?;
        let label_counts = DeviceBuffer::zeroed(num_nodes)?;
        let convergence_flag = DeviceBuffer::from_slice(&[1i32])?;
        let node_degrees = DeviceBuffer::zeroed(num_nodes)?;
        let modularity_contributions = DeviceBuffer::zeroed(num_nodes)?;
        let community_sizes = DeviceBuffer::zeroed(num_nodes)?;
        let label_mapping = DeviceBuffer::zeroed(num_nodes)?;

        let rand_states = DeviceBuffer::from_slice(&vec![
            curandState {
                _private: [0u8; 48]
            };
            num_nodes
        ])?;
        let max_labels = num_nodes;


        let kernel_module = module;


        let initial_memory = Self::calculate_memory_usage(num_nodes, num_edges, max_grid_cells);

        let gpu_compute = Self {
            device,
            _context,
            _module: kernel_module,
            clustering_module,
            apsp_module,
            ontology_module,
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
            class_id,
            class_charge,
            class_mass,
            physicality_code,
            role_code,
            maturity_level,
            physicality_centroids,
            physicality_counts,
            role_centroids,
            role_counts,
            edge_row_offsets,
            edge_col_indices,
            edge_weights,
            force_x,
            force_y,
            force_z,
            cell_keys,
            sorted_node_indices,
            cell_start,
            cell_end,
            cub_temp_storage,
            num_nodes,
            num_edges,
            allocated_nodes: num_nodes,
            allocated_edges: num_edges,
            max_grid_cells,
            iteration: 0,
            zero_buffer: vec![-1i32; max_grid_cells],

            dist,
            current_frontier,
            next_frontier_flags,
            parents: None,
            sssp_stream,

            constraint_data: DeviceBuffer::from_slice(&vec![])?,
            num_constraints: 0,
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
            modularity_contributions,
            community_sizes,
            label_mapping,
            rand_states,
            max_labels,

            cell_buffer_growth_factor: 1.5,
            max_allowed_grid_cells: 128 * 128 * 128,
            resize_count: 0,
            total_memory_allocated: initial_memory,

            partial_kinetic_energy: DeviceBuffer::zeroed((num_nodes + 255) / 256)?,
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


            aabb_num_blocks: (num_nodes + 255) / 256,
            aabb_block_results: DeviceBuffer::zeroed((num_nodes + 255) / 256)?,

            degree_weight: DeviceBuffer::from_slice(&vec![1.0f32; num_nodes])?,
            degree_weights_available: false,

            prev_force_x: DeviceBuffer::zeroed(num_nodes)?,
            prev_force_y: DeviceBuffer::zeroed(num_nodes)?,
            prev_force_z: DeviceBuffer::zeroed(num_nodes)?,
        };



        Ok(gpu_compute)
    }

    pub(crate) fn calculate_cub_temp_storage(
        num_nodes: usize,
        num_cells: usize,
    ) -> Result<DeviceBuffer<u8>> {
        // CUB DeviceRadixSort and DeviceScan require temporary workspace whose
        // exact size depends on the input length.  Ideally we would call the CUB
        // API with a nullptr output to query the required size, but there is no
        // Rust FFI wrapper for that today.  Instead we use a conservative
        // heuristic derived from CUB internals:
        //
        //   RadixSort: ~2 * num_items * sizeof(key+value) + fixed overhead
        //   ExclusiveSum: ~num_items * sizeof(value) + fixed overhead
        //
        // We take the maximum and add a generous safety margin.

        let num_items = num_nodes.max(num_cells);

        // Sort temp: each key-value pair is (i32, i32) = 8 bytes.
        // CUB double-buffers internally, so ~2x the data plus per-bin counters.
        let sort_bytes = num_items * 2 * std::mem::size_of::<i32>() * 2 + 2048;

        // Scan temp: one pass over i32 values plus block-level partial sums.
        let scan_bytes = num_items * std::mem::size_of::<i32>() + 2048;

        // Use the larger of the two so the buffer can serve both operations.
        let total_bytes = sort_bytes.max(scan_bytes).max(4096);

        info!(
            "CUB temp storage: sort={} bytes, scan={} bytes, allocated={} bytes (num_items={})",
            sort_bytes, scan_bytes, total_bytes, num_items
        );

        DeviceBuffer::zeroed(total_bytes)
            .map_err(|e| anyhow!("Failed to allocate CUB temp storage ({} bytes): {}", total_bytes, e))
    }

    pub(crate) fn calculate_memory_usage(num_nodes: usize, num_edges: usize, max_grid_cells: usize) -> usize {

        let node_memory = num_nodes * (12 * 4 + 1 * 4 + 1 * 4);

        let edge_memory = (num_nodes + 1) * 4 + num_edges * (4 + 4);

        let grid_memory = max_grid_cells * (4 + 4) + num_nodes * (4 + 4);

        let force_memory = num_nodes * 3 * 4;

        let other_memory = num_nodes * 10 * 4;

        node_memory + edge_memory + grid_memory + force_memory + other_memory
    }
}
