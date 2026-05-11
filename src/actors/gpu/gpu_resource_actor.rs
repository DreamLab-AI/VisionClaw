//! GPU Resource Actor - Handles GPU initialization, memory management, and device status

use actix::prelude::*;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use std::time::Instant;

use cudarc::driver::sys::CUdevice_attribute_enum;
use cudarc::driver::{CudaDevice, CudaStream};

use super::shared::GPUState;
use crate::actors::messages::*;
use crate::models::graph::GraphData;
use crate::utils::socket_flow_messages::BinaryNodeData;
use crate::utils::unified_gpu_compute::UnifiedGPUCompute;

#[allow(dead_code)]
const MAX_NODES: u32 = 1_000_000;
#[allow(dead_code)]
const MAX_GPU_FAILURES: u32 = 5;

pub struct GPUResourceActor {
    
    device: Option<Arc<CudaDevice>>,

    
    cuda_stream: Option<CudaStream>,

    
    unified_compute: Option<UnifiedGPUCompute>,

    
    gpu_state: GPUState,

    
    #[allow(dead_code)]
    last_failure_reset: Instant,
}

impl GPUResourceActor {
    pub fn new() -> Self {
        debug!("GPUResourceActor::new() - Creating new instance");
        Self {
            device: None,
            cuda_stream: None,
            unified_compute: None,
            gpu_state: GPUState::default(),
            last_failure_reset: Instant::now(),
        }
    }

    
    async fn perform_gpu_initialization(
        &mut self,
        graph_data: Arc<GraphData>,
    ) -> Result<(), String> {
        info!(
            "GPUResourceActor: Starting GPU initialization with {} nodes",
            graph_data.nodes.len()
        );
        debug!(
            "GPUResourceActor - Graph has {} nodes and {} edges",
            graph_data.nodes.len(),
            graph_data.edges.len()
        );

        
        debug!("GPUResourceActor - Testing GPU capabilities...");
        Self::static_test_gpu_capabilities()
            .await
            .map_err(|e| format!("GPU capabilities test failed: {}", e))?;

        
        debug!("GPUResourceActor - Creating CUDA device 0...");
        let device = CudaDevice::new(0).map_err(|e| {
            error!("Failed to create CUDA device: {}", e);
            format!("Failed to create CUDA device: {}", e)
        })?;
        info!("CUDA device initialized successfully");

        
        debug!("GPUResourceActor - Creating CUDA stream...");
        let cuda_stream = device.fork_default_stream().map_err(|e| {
            error!("Failed to create CUDA stream: {}", e);
            format!("Failed to create CUDA stream: {}", e)
        })?;
        info!("CUDA stream created successfully");

        
        let max_threads_per_block = device
            .attribute(CUdevice_attribute_enum::CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK)
            .map_err(|e| format!("Failed to get device attributes: {}", e))?
            as u32;

        let compute_capability_major = device
            .attribute(CUdevice_attribute_enum::CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR)
            .map_err(|e| format!("Failed to get compute capability: {}", e))?;

        info!(
            "GPU Capabilities - Max threads per block: {}, Compute capability: {}.x",
            max_threads_per_block, compute_capability_major
        );

        
        
        debug!("Loading PTX content using ptx utility module");
        let ptx_content = crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::VisionflowUnified,
        )
        .map_err(|e| {
            error!("Failed to load PTX content: {}", e);
            format!("Failed to load PTX content: {}", e)
        })?;
        debug!(
            "Main PTX content loaded successfully, size: {} bytes",
            ptx_content.len()
        );

        
        let clustering_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::GpuClusteringKernels,
        ) {
            Ok(content) => {
                debug!(
                    "Clustering PTX content loaded successfully, size: {} bytes",
                    content.len()
                );
                Some(content)
            }
            Err(e) => {
                warn!("Failed to load clustering PTX (will use fallback): {}", e);
                None
            }
        };

        // Load ontology constraints PTX for OWL axiom -> physics force pipeline
        let _ontology_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::OntologyConstraints,
        ) {
            Ok(content) => {
                info!(
                    "Ontology constraints PTX loaded successfully, size: {} bytes",
                    content.len()
                );
                Some(content)
            }
            Err(e) => {
                warn!("Failed to load ontology constraints PTX (will use CPU fallback): {}", e);
                None
            }
        };

        // Load APSP PTX for GPU-accelerated landmark distance assembly
        let apsp_ptx = match crate::utils::ptx::load_ptx_module_sync(
            crate::utils::ptx::PTXModule::GpuLandmarkApsp,
        ) {
            Ok(content) => {
                info!(
                    "APSP PTX loaded successfully, size: {} bytes",
                    content.len()
                );
                Some(content)
            }
            Err(e) => {
                warn!("Failed to load APSP PTX (will use CPU fallback): {}", e);
                None
            }
        };

        debug!("Creating UnifiedGPUCompute with initial capacity: nodes=8192, edges=16384");
        let unified_compute = UnifiedGPUCompute::new_with_modules(
            8192,
            16384,
            &ptx_content,
            clustering_ptx.as_deref(),
            apsp_ptx.as_deref(),
        )
        .map_err(|e| {
            error!("Failed to create unified compute: {}", e);
            format!("Failed to create unified compute: {}", e)
        })?;

        info!("UnifiedGPUCompute engine initialized successfully");

        // Store device/stream/compute BEFORE CSR creation so SharedGPUContext
        // can be created even when graph data is empty (race condition at startup).
        self.device = Some(device);
        self.cuda_stream = Some(cuda_stream);
        self.unified_compute = Some(unified_compute);

        // CSR creation and graph upload — tolerate empty graphs
        match self.create_csr_from_graph_data(&graph_data) {
            Ok(csr_result) => {
                if let Some(ref mut compute) = self.unified_compute {
                    compute
                        .initialize_graph(
                            csr_result.row_offsets.iter().map(|&x| x as i32).collect(),
                            csr_result.col_indices.iter().map(|&x| x as i32).collect(),
                            csr_result.edge_weights,
                            csr_result.positions_x,
                            csr_result.positions_y,
                            csr_result.positions_z,
                            csr_result.num_nodes as usize,
                            csr_result.num_edges as usize,
                        )
                        .map_err(|e| format!("Failed to initialize graph in unified compute: {}", e))?;

                    // Upload buffer_index → graph_node_id mapping so clustering
                    // actors can translate GPU indices back to real node IDs.
                    // Pad to allocated_nodes since device buffer may be overallocated.
                    let mut graph_ids = Self::build_node_graph_id_vec(&csr_result.node_indices, csr_result.num_nodes as usize);
                    use cust::memory::CopyDestination;
                    if graph_ids.len() < compute.node_graph_id.len() {
                        graph_ids.resize(compute.node_graph_id.len(), 0);
                    }
                    compute.node_graph_id.copy_from(&graph_ids)
                        .map_err(|e| format!("Failed to upload node_graph_id: {}", e))?;
                }

                // Upload domain-based class_id and class_charge for domain clustering.
                // Same-domain nodes get lower charge (less mutual repulsion) so edges
                // pull them together, creating natural domain clusters.
                if let Some(ref mut uc) = self.unified_compute {
                    let num = csr_result.num_nodes as usize;
                    let mut class_ids = Vec::with_capacity(num);
                    let mut class_charges = Vec::with_capacity(num);
                    let class_masses = vec![1.0f32; num];

                    for node in &graph_data.nodes {
                        let domain = node.metadata.get("source_domain")
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        // Charge modulates repulsion: charge[i] * charge[j] multiplies base repelK.
                        // Same-domain pairs: 0.3 * 0.3 = 0.09 (91% less repulsion — cluster tight)
                        // Cross-domain pairs: 0.3 * 2.5 = 0.75 (25% less — moderate)
                        // Unknown pairs: 2.5 * 2.5 = 6.25 (525% more — push to periphery)
                        let (id, charge) = match domain {
                            "ai" => (1i32, 0.3f32),
                            "bc" => (2, 0.3),
                            "mv" => (3, 0.3),
                            "rb" => (4, 0.3),
                            "ngm" => (5, 0.3),
                            "tc" => (6, 0.3),
                            _ => (0, 2.5),
                        };
                        class_ids.push(id);
                        class_charges.push(charge);
                    }

                    match uc.upload_class_metadata(&class_ids, &class_charges, &class_masses) {
                        Ok(_) => {
                            let classified = class_ids.iter().filter(|&&id| id > 0).count();
                            info!("Uploaded domain class metadata: {}/{} nodes classified", classified, num);
                        }
                        Err(e) => warn!("Failed to upload class metadata: {}", e),
                    }
                }

                info!("Graph data uploaded to GPU successfully");

                self.gpu_state.num_nodes = csr_result.num_nodes;
                self.gpu_state.num_edges = csr_result.num_edges;
                self.gpu_state.node_indices = csr_result.node_indices;
                self.gpu_state.graph_structure_hash = Self::calculate_graph_structure_hash(&graph_data);
                self.gpu_state.positions_hash = Self::calculate_positions_hash(&graph_data);
                self.gpu_state.csr_structure_uploaded = true;
            }
            Err(e) => {
                // Empty graph at startup is expected — graph data loads from Neo4j asynchronously.
                // SharedGPUContext will still be created so ForceComputeActor can receive it.
                // Graph data will be uploaded later via UpdateGPUGraphData or reinitialize_with_graph.
                warn!(
                    "CSR creation deferred ({}). GPU device/compute ready — graph data will be uploaded when available.",
                    e
                );
            }
        }

        info!("GPU initialization completed successfully");
        Ok(())
    }

    
    async fn static_test_gpu_capabilities() -> Result<(), Error> {
        info!("Testing CUDA capabilities");
        match CudaDevice::count() {
            Ok(count) => {
                info!("Found {} CUDA device(s)", count);
                if count == 0 {
                    error!("No CUDA devices found");
                    Err(Error::new(ErrorKind::NotFound, "No CUDA devices found"))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                error!("Failed to get CUDA device count: {}", e);
                Err(Error::new(ErrorKind::Other, format!("CUDA error: {}", e)))
            }
        }
    }

    
    fn create_csr_from_graph_data(&self, graph_data: &GraphData) -> Result<CsrResult, String> {
        let num_nodes = graph_data.nodes.len() as u32;
        let num_edges = graph_data.edges.len() as u32;

        if num_nodes == 0 {
            return Err("Cannot create CSR from empty graph".to_string());
        }

        info!(
            "Creating CSR representation: {} nodes, {} edges",
            num_nodes, num_edges
        );

        
        let mut node_indices = HashMap::new();
        for (i, node) in graph_data.nodes.iter().enumerate() {
            node_indices.insert(node.id, i);
        }

        
        let mut row_offsets = vec![0u32; (num_nodes + 1) as usize];
        let mut col_indices = Vec::new();
        let mut edge_weights = Vec::new();

        
        let positions_x: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.x).collect();
        let positions_y: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.y).collect();
        let positions_z: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.z).collect();

        
        let mut adjacency_lists: Vec<Vec<(u32, f32)>> = vec![Vec::new(); num_nodes as usize];

        for edge in &graph_data.edges {
            if let (Some(&source_idx), Some(&target_idx)) = (
                node_indices.get(&edge.source),
                node_indices.get(&edge.target),
            ) {
                let weight = edge.weight;
                adjacency_lists[source_idx].push((target_idx as u32, weight));

                
                if source_idx != target_idx {
                    adjacency_lists[target_idx].push((source_idx as u32, weight));
                }
            }
        }

        
        let mut edge_count = 0;
        for (i, adj_list) in adjacency_lists.iter().enumerate() {
            row_offsets[i] = edge_count;

            for &(target, weight) in adj_list {
                col_indices.push(target);
                edge_weights.push(weight);
                edge_count += 1;
            }
        }
        row_offsets[num_nodes as usize] = edge_count;

        info!(
            "CSR conversion complete: {} total edges (including reverse edges)",
            edge_count
        );

        Ok(CsrResult {
            row_offsets,
            col_indices,
            edge_weights,
            positions_x,
            positions_y,
            positions_z,
            num_nodes,
            num_edges: edge_count,
            node_indices,
        })
    }

    /// Build a Vec<i32> mapping GPU buffer index → graph node ID.
    /// Inverts the `node_indices` HashMap (graph_id → buffer_index).
    fn build_node_graph_id_vec(node_indices: &HashMap<u32, usize>, num_nodes: usize) -> Vec<i32> {
        let mut graph_ids = vec![0i32; num_nodes];
        for (&graph_id, &buffer_idx) in node_indices.iter() {
            if buffer_idx < num_nodes {
                graph_ids[buffer_idx] = graph_id as i32;
            }
        }
        graph_ids
    }

    /// Compute a stable graph structure hash using BLAKE3 (P1-24).
    fn calculate_graph_structure_hash(graph_data: &GraphData) -> u64 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&graph_data.nodes.len().to_le_bytes());
        hasher.update(&graph_data.edges.len().to_le_bytes());
        for edge in &graph_data.edges {
            hasher.update(&edge.source.to_le_bytes());
            hasher.update(&edge.target.to_le_bytes());
            hasher.update(&edge.weight.to_bits().to_le_bytes());
        }
        let hash = hasher.finalize();
        let bytes = hash.as_bytes();
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    /// Compute a stable positions hash using BLAKE3 (P1-24).
    fn calculate_positions_hash(graph_data: &GraphData) -> u64 {
        let mut hasher = blake3::Hasher::new();
        for node in &graph_data.nodes {
            hasher.update(&node.data.x.to_bits().to_le_bytes());
            hasher.update(&node.data.y.to_bits().to_le_bytes());
            hasher.update(&node.data.z.to_bits().to_le_bytes());
        }
        let hash = hasher.finalize();
        let bytes = hash.as_bytes();
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

struct CsrResult {
    row_offsets: Vec<u32>,
    col_indices: Vec<u32>,
    edge_weights: Vec<f32>,
    positions_x: Vec<f32>,
    positions_y: Vec<f32>,
    positions_z: Vec<f32>,
    num_nodes: u32,
    num_edges: u32,
    node_indices: HashMap<u32, usize>,
}

impl Actor for GPUResourceActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        debug!(
            "GPUResourceActor::started() - Actor lifecycle started, address: {:?}",
            ctx.address()
        );
        debug!(
            "GPUResourceActor - Initial state: device={}, cuda_stream={}, unified_compute={}",
            self.device.is_some(),
            self.cuda_stream.is_some(),
            self.unified_compute.is_some()
        );
        info!("GPU Resource Actor started successfully");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        error!("GPUResourceActor::stopped() - Actor lifecycle stopped!");
        error!(
            "GPUResourceActor - Final state: device={}, cuda_stream={}, unified_compute={}",
            self.device.is_some(),
            self.cuda_stream.is_some(),
            self.unified_compute.is_some()
        );
        error!(
            "GPUResourceActor - Failure count: {}",
            self.gpu_state.gpu_failure_count
        );
    }
}

// === Message Handlers ===

impl Handler<InitializeGPU> for GPUResourceActor {
    type Result = ResponseActFuture<Self, Result<(), String>>;

    fn handle(&mut self, msg: InitializeGPU, _ctx: &mut Self::Context) -> Self::Result {
        debug!("GPUResourceActor::handle(InitializeGPU) - Message received");
        info!(
            "GPUResourceActor: InitializeGPU received with {} nodes",
            msg.graph.nodes.len()
        );
        debug!(
            "Graph service address present: {}",
            msg.graph_service_addr.is_some()
        );
        debug!(
            "GPU manager address present: {}",
            msg.gpu_manager_addr.is_some()
        );

        let graph_data = msg.graph;
        let graph_service_addr = msg.graph_service_addr;
        let physics_orchestrator_addr = msg.physics_orchestrator_addr;
        let gpu_manager_addr = msg.gpu_manager_addr;

        
        debug!("Starting async GPU initialization");
        Box::pin(async move {
            
            Ok::<(), ()>(())
        }.into_actor(self).map(move |result, actor, _ctx| {
            match result {
                Ok(_) => {
                    debug!("Async initialization started, performing GPU initialization...");
                    
                    let initialization_result = futures::executor::block_on(
                        actor.perform_gpu_initialization(graph_data)
                    );

                    match initialization_result {
                        Ok(_) => {
                            info!("GPU initialization completed successfully");

                            
                            if let (Some(device), Some(stream), Some(compute)) = (
                                actor.device.as_ref().cloned(),
                                actor.cuda_stream.take(),
                                actor.unified_compute.take(),
                            ) {
                                let safe_stream = super::cuda_stream_wrapper::SafeCudaStream::new(stream);

                                // Initialize GpuMemoryManager with 80% of GPU memory or 6GB default
                                let mem_limit = cudarc::driver::result::mem_get_info()
                                    .map(|(_free, total)| (total as f64 * 0.8) as usize)
                                    .unwrap_or(6 * 1024 * 1024 * 1024);
                                let memory_manager = crate::gpu::memory_manager::GpuMemoryManager::with_limit(mem_limit)
                                    .or_else(|_| crate::gpu::memory_manager::GpuMemoryManager::new())
                                    .map(|mgr| Arc::new(std::sync::Mutex::new(mgr)))
                                    .unwrap_or_else(|e| {
                                        warn!("GPUResourceActor: GpuMemoryManager init failed: {} — using dummy", e);
                                        Arc::new(std::sync::Mutex::new(
                                            crate::gpu::memory_manager::GpuMemoryManager::with_limit(6 * 1024 * 1024 * 1024)
                                                .expect("fallback GpuMemoryManager must succeed")
                                        ))
                                    });

                                let shared_context = Arc::new(super::shared::SharedGPUContext {
                                    device: device.clone(),
                                    stream: Arc::new(std::sync::Mutex::new(safe_stream)),
                                    unified_compute: Arc::new(std::sync::Mutex::new(compute)),
                                    memory_manager,
                                    gpu_access_lock: Arc::new(tokio::sync::RwLock::new(())),
                                    resource_metrics: Arc::new(std::sync::Mutex::new(super::shared::GPUResourceMetrics::default())),
                                    operation_batch: Arc::new(std::sync::Mutex::new(Vec::new())),
                                    batch_timeout: std::time::Duration::from_millis(10),
                                });

                                info!("Created SharedGPUContext - distributing to GPU actors");

                                if let Some(manager_addr) = gpu_manager_addr {
                                    if let Err(e) = manager_addr.try_send(SetSharedGPUContext {
                                        context: shared_context.clone(),
                                        graph_service_addr: graph_service_addr.clone(),
                                        correlation_id: None,
                                    }) {
                                        error!("Failed to send SharedGPUContext to GPUManagerActor: {}", e);
                                    } else {
                                        info!("SharedGPUContext sent to GPUManagerActor for distribution with GraphServiceActor address");
                                    }
                                }

                                info!("SharedGPUContext ownership transferred to shared actors");
                            } else {
                                error!("Failed to create SharedGPUContext - missing components");
                            }


                            // Log GPU initialization completion
                            if graph_service_addr.is_some() {
                                info!("GPU initialized - GraphServiceActor notification");
                            }

                            if physics_orchestrator_addr.is_some() {
                                info!("GPU initialized - PhysicsOrchestratorActor notification");
                            }
                            Ok(())
                        },
                        Err(e) => {
                            error!("GPU initialization failed: {}", e);
                            Err(e)
                        }
                    }
                },
                Err(_) => Err("Failed to start GPU initialization".to_string())
            }
        }))
    }
}

impl Handler<UpdateGPUGraphData> for GPUResourceActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: UpdateGPUGraphData, _ctx: &mut Self::Context) -> Self::Result {
        if self.device.is_none() {
            error!("GPU not initialized! Cannot update graph data");
            return Err("GPU not initialized".to_string());
        }

        
        self.update_graph_data_internal_optimized(&msg.graph)
    }
}

impl Handler<GetNodeData> for GPUResourceActor {
    type Result = Result<Vec<BinaryNodeData>, String>;

    fn handle(&mut self, _msg: GetNodeData, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(ref mut unified_compute) = self.unified_compute {
            match unified_compute.get_node_positions() {
                Ok((positions_x, positions_y, positions_z)) => {
                    let mut node_data = Vec::new();

                    for i in 0..positions_x
                        .len()
                        .min(positions_y.len())
                        .min(positions_z.len())
                    {
                        node_data.push(BinaryNodeData {
                            node_id: i as u32,
                            x: positions_x[i],
                            y: positions_y[i],
                            z: positions_z[i],
                            vx: 0.0,
                            vy: 0.0,
                            vz: 0.0,
                        });
                    }

                    Ok(node_data)
                }
                Err(e) => {
                    error!("Failed to get node positions from GPU: {}", e);
                    Err(format!("Failed to get node positions: {}", e))
                }
            }
        } else {
            Err("GPU not initialized".to_string())
        }
    }
}

impl GPUResourceActor {
    
    fn update_graph_data_internal_optimized(
        &mut self,
        graph_data: &Arc<GraphData>,
    ) -> Result<(), String> {
        let new_structure_hash = Self::calculate_graph_structure_hash(graph_data);
        let new_positions_hash = Self::calculate_positions_hash(graph_data);

        let structure_changed = new_structure_hash != self.gpu_state.graph_structure_hash;
        let positions_changed = new_positions_hash != self.gpu_state.positions_hash;

        info!(
            "GPU upload optimization - structure_changed: {}, positions_changed: {}",
            structure_changed, positions_changed
        );

        
        if !structure_changed && !positions_changed {
            trace!("GPU upload skipped - no changes detected");
            return Ok(());
        }

        if structure_changed {
            
            info!("GPU: Full structure update required");

            let csr_result = self.create_csr_from_graph_data(graph_data)?;

            let unified_compute = self
                .unified_compute
                .as_mut()
                .ok_or_else(|| "Unified compute not initialized".to_string())?;

            unified_compute
                .initialize_graph(
                    csr_result.row_offsets.iter().map(|&x| x as i32).collect(),
                    csr_result.col_indices.iter().map(|&x| x as i32).collect(),
                    csr_result.edge_weights,
                    csr_result.positions_x,
                    csr_result.positions_y,
                    csr_result.positions_z,
                    csr_result.num_nodes as usize,
                    csr_result.num_edges as usize,
                )
                .map_err(|e| format!("Failed to upload full graph structure: {}", e))?;

            // Upload buffer_index → graph_node_id mapping for clustering actors.
            // Pad to allocated_nodes since device buffer may be overallocated.
            let mut graph_ids = Self::build_node_graph_id_vec(&csr_result.node_indices, csr_result.num_nodes as usize);
            use cust::memory::CopyDestination;
            if graph_ids.len() < unified_compute.node_graph_id.len() {
                graph_ids.resize(unified_compute.node_graph_id.len(), 0);
            }
            unified_compute.node_graph_id.copy_from(&graph_ids)
                .map_err(|e| format!("Failed to upload node_graph_id: {}", e))?;


            self.gpu_state.num_nodes = csr_result.num_nodes;
            self.gpu_state.num_edges = csr_result.num_edges;
            self.gpu_state.node_indices = csr_result.node_indices;
            self.gpu_state.graph_structure_hash = new_structure_hash;
            self.gpu_state.positions_hash = new_positions_hash;
            self.gpu_state.csr_structure_uploaded = true;
        } else if positions_changed {
            
            info!("GPU: Position-only update");

            let positions_x: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.x).collect();
            let positions_y: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.y).collect();
            let positions_z: Vec<f32> = graph_data.nodes.iter().map(|n| n.data.z).collect();

            let unified_compute = self
                .unified_compute
                .as_mut()
                .ok_or_else(|| "Unified compute not initialized".to_string())?;

            unified_compute
                .update_positions_only(&positions_x, &positions_y, &positions_z)
                .map_err(|e| format!("Failed to update positions: {}", e))?;

            self.gpu_state.positions_hash = new_positions_hash;
        }

        Ok(())
    }
}
