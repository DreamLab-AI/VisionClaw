// src/adapters/physics_orchestrator_adapter.rs
//! Physics Orchestrator Adapter
//!
//! Wraps the existing PhysicsOrchestratorActor to implement the PhysicsSimulator port.
//! This adapter provides backward compatibility while enabling hexagonal architecture.

use actix::Addr;
use async_trait::async_trait;
use tracing::{debug, error, info, instrument};

use crate::actors::messages::{
    ApplyOntologyConstraints, ConstraintMergeMode, StartSimulation, StopSimulation,
    UpdateSimulationParams,
};
use crate::actors::physics_orchestrator_actor::{
    GetPhysicsStatus, PhysicsOrchestratorActor, UpdateGraphData,
};
use crate::models::simulation_params::SimulationParams as ActorSimulationParams;
use crate::ports::physics_simulator::{
    BinaryNodeData, Constraint as PortConstraint, ConstraintType, PhysicsSimulator,
    PhysicsSimulatorError, Result, SimulationParams,
};
use std::sync::Arc;
use std::time::Duration;
use visionclaw_domain::models::constraints::ConstraintSet;
use visionclaw_domain::models::graph::GraphData;

pub struct PhysicsOrchestratorAdapter {
    actor_addr: Addr<PhysicsOrchestratorActor>,
    timeout: Duration,
}

impl PhysicsOrchestratorAdapter {
    pub fn new(actor_addr: Addr<PhysicsOrchestratorActor>) -> Self {
        info!("Initializing PhysicsOrchestratorAdapter");
        Self {
            actor_addr,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn convert_constraint_to_actor(
        constraint: &PortConstraint,
    ) -> visionclaw_domain::models::constraints::Constraint {
        use visionclaw_domain::models::constraints::Constraint as ActorConstraint;

        match constraint.constraint_type {
            ConstraintType::Fixed => {
                if let Some((x, y, z)) = constraint.target_position {
                    ActorConstraint::fixed_position(constraint.node_id, x, y, z)
                } else {
                    ActorConstraint::fixed_position(constraint.node_id, 0.0, 0.0, 0.0)
                }
            }
            ConstraintType::Spring => {
                ActorConstraint::separation(constraint.node_id, constraint.node_id + 1, 100.0)
            }
            ConstraintType::Boundary => {
                ActorConstraint::fixed_position(constraint.node_id, 0.0, 0.0, 0.0)
            }
        }
    }

    fn convert_params_to_actor(params: &SimulationParams) -> ActorSimulationParams {
        ActorSimulationParams {
            repel_k: params.settings.repel_k,
            spring_k: params.settings.spring_k,
            damping: params.settings.damping,
            max_velocity: params.settings.max_velocity,
            enabled: params.settings.enabled,
            ..Default::default()
        }
    }

    fn convert_position_to_port(
        pos: &crate::utils::socket_flow_messages::BinaryNodeData,
    ) -> BinaryNodeData {
        (pos.x, pos.y, pos.z)
    }
}

#[async_trait]
impl PhysicsSimulator for PhysicsOrchestratorAdapter {
    #[instrument(skip(self, graph), fields(node_count = graph.nodes.len()), level = "debug")]
    async fn run_simulation_step(&self, graph: &GraphData) -> Result<Vec<(u32, BinaryNodeData)>> {
        debug!("Running physics simulation step via adapter");

        let graph_arc = Arc::new(graph.clone());
        self.actor_addr.do_send(UpdateGraphData {
            graph_data: graph_arc,
        });

        let _status = tokio::time::timeout(self.timeout, self.actor_addr.send(GetPhysicsStatus))
            .await
            .map_err(|_| {
                error!("Timeout getting physics status");
                PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
            })?
            .map_err(|e| {
                error!("Failed to get physics status: {}", e);
                PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
            })?;

        let positions: Vec<(u32, BinaryNodeData)> = graph
            .nodes
            .iter()
            .map(|node| {
                let client_data: crate::utils::socket_flow_messages::BinaryNodeDataClient =
                    node.data.into();
                (node.id, Self::convert_position_to_port(&client_data))
            })
            .collect();

        debug!("Retrieved {} node positions", positions.len());
        Ok(positions)
    }

    #[instrument(skip(self, params), level = "debug")]
    async fn update_params(&self, params: SimulationParams) -> Result<()> {
        debug!("Updating simulation parameters via adapter");

        let actor_params = Self::convert_params_to_actor(&params);

        let result = tokio::time::timeout(
            self.timeout,
            self.actor_addr.send(UpdateSimulationParams {
                params: actor_params,
            }),
        )
        .await
        .map_err(|_| {
            error!("Timeout updating simulation params");
            PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
        })?
        .map_err(|e| {
            error!("Failed to update simulation params: {}", e);
            PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
        })?;

        match result {
            Ok(_) => {}
            Err(e) => {
                error!("Actor returned error: {}", e);
                return Err(PhysicsSimulatorError::InvalidParameters(e));
            }
        }

        info!("Successfully updated simulation parameters");
        Ok(())
    }

    #[instrument(skip(self, constraints), fields(constraint_count = constraints.len()), level = "debug")]
    async fn apply_constraints(&self, constraints: Vec<PortConstraint>) -> Result<()> {
        debug!("Applying {} constraints via adapter", constraints.len());

        let actor_constraints: Vec<visionclaw_domain::models::constraints::Constraint> =
            constraints
                .iter()
                .map(Self::convert_constraint_to_actor)
                .collect();

        let mut constraint_set = ConstraintSet::default();
        for constraint in actor_constraints {
            constraint_set.constraints.push(constraint);
        }

        let result = tokio::time::timeout(
            self.timeout,
            self.actor_addr.send(ApplyOntologyConstraints {
                constraint_set,
                merge_mode: ConstraintMergeMode::Merge,
                graph_id: 0,
            }),
        )
        .await
        .map_err(|_| {
            error!("Timeout applying constraints");
            PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
        })?
        .map_err(|e| {
            error!("Failed to apply constraints: {}", e);
            PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
        })?;

        match result {
            Ok(_) => {}
            Err(e) => {
                error!("Actor returned error: {}", e);
                return Err(PhysicsSimulatorError::InvalidParameters(e));
            }
        }

        info!("Successfully applied {} constraints", constraints.len());
        Ok(())
    }

    #[instrument(skip(self), level = "info")]
    async fn start_simulation(&self) -> Result<()> {
        info!("Starting physics simulation via adapter");

        let result = tokio::time::timeout(self.timeout, self.actor_addr.send(StartSimulation))
            .await
            .map_err(|_| {
                error!("Timeout starting simulation");
                PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
            })?
            .map_err(|e| {
                error!("Failed to start simulation: {}", e);
                PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
            })?;

        match result {
            Ok(_) => {}
            Err(e) => {
                error!("Actor returned error: {}", e);
                return Err(PhysicsSimulatorError::SimulationError(e));
            }
        }

        info!("Physics simulation started successfully");
        Ok(())
    }

    #[instrument(skip(self), level = "info")]
    async fn stop_simulation(&self) -> Result<()> {
        info!("Stopping physics simulation via adapter");

        let result = tokio::time::timeout(self.timeout, self.actor_addr.send(StopSimulation))
            .await
            .map_err(|_| {
                error!("Timeout stopping simulation");
                PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
            })?
            .map_err(|e| {
                error!("Failed to stop simulation: {}", e);
                PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
            })?;

        match result {
            Ok(_) => {}
            Err(e) => {
                error!("Actor returned error: {}", e);
                return Err(PhysicsSimulatorError::SimulationError(e));
            }
        }

        info!("Physics simulation stopped successfully");
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn is_running(&self) -> Result<bool> {
        debug!("Checking if physics simulation is running");

        let status = tokio::time::timeout(self.timeout, self.actor_addr.send(GetPhysicsStatus))
            .await
            .map_err(|_| {
                error!("Timeout getting physics status");
                PhysicsSimulatorError::SimulationError("Actor communication timeout".to_string())
            })?
            .map_err(|e| {
                error!("Failed to get physics status: {}", e);
                PhysicsSimulatorError::SimulationError(format!("Actor communication failed: {}", e))
            })?;

        let is_running = status.simulation_running && !status.is_paused;
        debug!("Physics simulation running: {}", is_running);
        Ok(is_running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PhysicsSettings;

    #[test]
    fn test_constraint_conversion() {
        let port_constraint = PortConstraint {
            node_id: 1,
            constraint_type: ConstraintType::Fixed,
            target_position: Some((1.0, 2.0, 3.0)),
            strength: 1.0,
        };

        let actor_constraint =
            PhysicsOrchestratorAdapter::convert_constraint_to_actor(&port_constraint);

        assert_eq!(actor_constraint.node_indices.len(), 1);
        assert_eq!(actor_constraint.node_indices[0], 1);
    }

    #[test]
    fn test_params_conversion() {
        let port_params = SimulationParams {
            settings: PhysicsSettings {
                enabled: true,
                repel_k: 100.0,
                spring_k: 0.1,
                damping: 0.9,
                max_velocity: 50.0,
                ..Default::default()
            },
            graph_name: "test".to_string(),
        };

        let actor_params = PhysicsOrchestratorAdapter::convert_params_to_actor(&port_params);

        assert_eq!(actor_params.repel_k, 100.0);
        assert_eq!(actor_params.spring_k, 0.1);
        assert_eq!(actor_params.damping, 0.9);
        assert_eq!(actor_params.max_velocity, 50.0);
        assert!(actor_params.enabled);
    }
}
