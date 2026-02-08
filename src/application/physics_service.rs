// src/application/physics_service.rs
//! Physics Service
//!
//! Application service that integrates actor-based physics simulation
//! through hexagonal architecture ports. Handles GPU-accelerated physics
//! computations and event publishing.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::events::domain_events::{
    LayoutOptimizedEvent, PositionsUpdatedEvent, SimulationStartedEvent, SimulationStoppedEvent,
};
use crate::events::event_bus::EventBus;
use crate::models::graph::GraphData;
use crate::models::node::Node;
use crate::ports::gpu_physics_adapter::{
    GpuDeviceInfo, GpuPhysicsAdapter, PhysicsParameters, PhysicsStatistics,
    PhysicsStepResult, Result as PhysicsResult,
};
use crate::utils::time;

#[derive(Debug, Clone)]
pub struct SimulationParams {
    pub profile_name: String,
    pub physics_params: PhysicsParameters,
    pub auto_stop_on_convergence: bool,
}

impl Default for SimulationParams {
    fn default() -> Self {
        Self {
            profile_name: "default".to_string(),
            physics_params: PhysicsParameters::default(),
            auto_stop_on_convergence: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayoutOptimizationRequest {
    pub algorithm: String,
    pub max_iterations: u32,
    pub target_energy: f32,
}

pub struct PhysicsService {
    physics_adapter: Arc<RwLock<dyn GpuPhysicsAdapter>>,
    event_bus: Arc<RwLock<EventBus>>,
    simulation_id: Arc<RwLock<Option<String>>>,
}

impl PhysicsService {
    
    pub fn new(
        physics_adapter: Arc<RwLock<dyn GpuPhysicsAdapter>>,
        event_bus: Arc<RwLock<EventBus>>,
    ) -> Self {
        Self {
            physics_adapter,
            event_bus,
            simulation_id: Arc::new(RwLock::new(None)),
        }
    }

    
    pub async fn start_simulation(
        &self,
        graph: Arc<GraphData>,
        params: SimulationParams,
    ) -> PhysicsResult<String> {
        
        let mut adapter = self.physics_adapter.write().await;
        adapter
            .initialize(graph.clone(), params.physics_params.clone())
            .await?;

        
        let sim_id = format!("sim-{}", uuid::Uuid::new_v4());
        *self.simulation_id.write().await = Some(sim_id.clone());

        
        let event = SimulationStartedEvent {
            simulation_id: sim_id.clone(),
            physics_profile: params.profile_name,
            node_count: graph.nodes.len(),
            timestamp: time::now(),
        };

        self.event_bus
            .write()
            .await
            .publish(event)
            .await;

        Ok(sim_id)
    }

    
    pub async fn stop_simulation(&self) -> PhysicsResult<()> {
        let sim_id = self.simulation_id.read().await.clone();

        if let Some(simulation_id) = sim_id {
            
            let adapter = self.physics_adapter.read().await;
            let stats = adapter.get_statistics().await?;

            
            let event = SimulationStoppedEvent {
                simulation_id,
                iterations: stats.total_steps as u32,
                final_energy: stats.average_energy as f64,
                timestamp: time::now(),
            };

            self.event_bus
                .write()
                .await
                .publish(event)
                .await;

            
            *self.simulation_id.write().await = None;
        }

        Ok(())
    }

    
    pub async fn compute_layout(
        &self,
        graph: Arc<GraphData>,
        params: PhysicsParameters,
    ) -> PhysicsResult<Vec<Node>> {
        
        let mut adapter = self.physics_adapter.write().await;
        adapter.initialize(graph.clone(), params).await?;

        
        let _result = adapter.simulate_until_convergence().await?;

        
        let forces = adapter.compute_forces().await?;
        let positions = adapter.update_positions(&forces).await?;

        
        let mut nodes = Vec::new();
        for (node_id, x, y, z) in positions {
            if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
                let mut updated_node = node.clone();
                updated_node.set_x(x);
                updated_node.set_y(y);
                updated_node.set_z(z);
                nodes.push(updated_node);
            }
        }

        
        let event = PositionsUpdatedEvent {
            graph_id: "main".to_string(),
            updated_nodes: nodes.iter().map(|n| n.id.to_string()).collect(),
            timestamp: time::now(),
        };

        self.event_bus
            .write()
            .await
            .publish(event)
            .await;

        Ok(nodes)
    }

    
    pub async fn step(&self) -> PhysicsResult<PhysicsStepResult> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.step().await
    }

    
    pub async fn optimize_layout(
        &self,
        graph: Arc<GraphData>,
        request: LayoutOptimizationRequest,
    ) -> PhysicsResult<Vec<Node>> {
        
        let mut params = PhysicsParameters::default();
        params.max_iterations = request.max_iterations;
        params.convergence_threshold = request.target_energy;

        
        let nodes = self.compute_layout(graph, params).await?;

        
        let event = LayoutOptimizedEvent {
            layout_id: format!("layout-{}", uuid::Uuid::new_v4()),
            algorithm: request.algorithm,
            node_count: nodes.len(),
            optimization_score: request.target_energy as f64,
            timestamp: time::now(),
        };

        self.event_bus
            .write()
            .await
            .publish(event)
            .await;

        Ok(nodes)
    }

    
    pub async fn apply_external_forces(
        &self,
        forces: Vec<(u32, f32, f32, f32)>,
    ) -> PhysicsResult<()> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.apply_external_forces(forces).await
    }

    
    pub async fn pin_nodes(&self, nodes: Vec<(u32, f32, f32, f32)>) -> PhysicsResult<()> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.pin_nodes(nodes).await
    }

    
    pub async fn unpin_nodes(&self, node_ids: Vec<u32>) -> PhysicsResult<()> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.unpin_nodes(node_ids).await
    }

    
    pub async fn update_parameters(&self, params: PhysicsParameters) -> PhysicsResult<()> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.update_parameters(params).await
    }

    
    pub async fn get_gpu_status(&self) -> PhysicsResult<GpuDeviceInfo> {
        let adapter = self.physics_adapter.read().await;
        adapter.get_gpu_status().await
    }

    
    pub async fn get_statistics(&self) -> PhysicsResult<PhysicsStatistics> {
        let adapter = self.physics_adapter.read().await;
        adapter.get_statistics().await
    }

    
    pub async fn reset(&self) -> PhysicsResult<()> {
        let mut adapter = self.physics_adapter.write().await;
        adapter.reset().await
    }

    
    pub async fn is_running(&self) -> bool {
        self.simulation_id.read().await.is_some()
    }

    
    pub async fn get_simulation_id(&self) -> Option<String> {
        self.simulation_id.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::events::event_bus::EventBus;
    use crate::ports::gpu_physics_adapter::NodeForce;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    struct MockPhysicsAdapter;

    #[async_trait]
    impl GpuPhysicsAdapter for MockPhysicsAdapter {
        async fn initialize(
            &mut self,
            _graph: Arc<GraphData>,
            _params: PhysicsParameters,
        ) -> PhysicsResult<()> {
            Ok(())
        }

        async fn compute_forces(&mut self) -> PhysicsResult<Vec<NodeForce>> {
            Ok(vec![])
        }

        async fn update_positions(
            &mut self,
            _forces: &[NodeForce],
        ) -> PhysicsResult<Vec<(u32, f32, f32, f32)>> {
            Ok(vec![])
        }

        async fn step(&mut self) -> PhysicsResult<PhysicsStepResult> {
            Ok(PhysicsStepResult {
                nodes_updated: 0,
                total_energy: 0.0,
                max_displacement: 0.0,
                converged: true,
                computation_time_ms: 0.0,
            })
        }

        async fn simulate_until_convergence(&mut self) -> PhysicsResult<PhysicsStepResult> {
            Ok(PhysicsStepResult {
                nodes_updated: 0,
                total_energy: 0.0,
                max_displacement: 0.0,
                converged: true,
                computation_time_ms: 0.0,
            })
        }

        async fn apply_external_forces(
            &mut self,
            _forces: Vec<(u32, f32, f32, f32)>,
        ) -> PhysicsResult<()> {
            Ok(())
        }

        async fn pin_nodes(&mut self, _nodes: Vec<(u32, f32, f32, f32)>) -> PhysicsResult<()> {
            Ok(())
        }

        async fn unpin_nodes(&mut self, _node_ids: Vec<u32>) -> PhysicsResult<()> {
            Ok(())
        }

        async fn update_parameters(&mut self, _params: PhysicsParameters) -> PhysicsResult<()> {
            Ok(())
        }

        async fn update_graph_data(&mut self, _graph: Arc<GraphData>) -> PhysicsResult<()> {
            Ok(())
        }

        async fn get_gpu_status(&self) -> PhysicsResult<GpuDeviceInfo> {
            Ok(GpuDeviceInfo {
                device_id: 0,
                device_name: "Mock GPU".to_string(),
                compute_capability: (7, 5),
                total_memory_mb: 8192,
                free_memory_mb: 4096,
                multiprocessor_count: 40,
                warp_size: 32,
                max_threads_per_block: 1024,
            })
        }

        async fn get_statistics(&self) -> PhysicsResult<PhysicsStatistics> {
            Ok(PhysicsStatistics {
                total_steps: 100,
                average_step_time_ms: 1.5,
                average_energy: 0.5,
                gpu_memory_used_mb: 256.0,
                cache_hit_rate: 0.8,
                last_convergence_iterations: 50,
            })
        }

        async fn reset(&mut self) -> PhysicsResult<()> {
            Ok(())
        }

        async fn cleanup(&mut self) -> PhysicsResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_physics_service_creation() {
        let adapter = Arc::new(RwLock::new(MockPhysicsAdapter));
        let event_bus = Arc::new(RwLock::new(EventBus::new()));
        let service = PhysicsService::new(adapter, event_bus);

        assert!(!service.is_running().await);
    }

    #[tokio::test]
    async fn test_get_gpu_status() {
        let adapter = Arc::new(RwLock::new(MockPhysicsAdapter));
        let event_bus = Arc::new(RwLock::new(EventBus::new()));
        let service = PhysicsService::new(adapter, event_bus);

        let status = service.get_gpu_status().await.unwrap();
        assert_eq!(status.device_name, "Mock GPU");
    }
}
