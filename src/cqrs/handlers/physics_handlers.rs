// src/cqrs/handlers/physics_handlers.rs
//! GPU Physics Command and Query Handlers

use crate::cqrs::commands::*;
use crate::cqrs::queries::*;
use crate::cqrs::types::{Command, CommandHandler, QueryHandler, Result};
use crate::ports::GpuPhysicsAdapter;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PhysicsCommandHandler {
    adapter: Arc<Mutex<dyn GpuPhysicsAdapter>>,
}

impl PhysicsCommandHandler {
    pub fn new(adapter: Arc<Mutex<dyn GpuPhysicsAdapter>>) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl CommandHandler<InitializePhysicsCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: InitializePhysicsCommand) -> Result<()> {
        command.validate()?;
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.initialize(command.graph, command.params).await?)
    }
}

#[async_trait]
impl CommandHandler<UpdatePhysicsParametersCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: UpdatePhysicsParametersCommand) -> Result<()> {
        command.validate()?;
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.update_parameters(command.params).await?)
    }
}

#[async_trait]
impl CommandHandler<UpdateGraphDataCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: UpdateGraphDataCommand) -> Result<()> {
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.update_graph_data(command.graph).await?)
    }
}

#[async_trait]
impl CommandHandler<ApplyExternalForcesCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: ApplyExternalForcesCommand) -> Result<()> {
        command.validate()?;
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.apply_external_forces(command.forces).await?)
    }
}

#[async_trait]
impl CommandHandler<PinNodesCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: PinNodesCommand) -> Result<()> {
        command.validate()?;
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.pin_nodes(command.nodes).await?)
    }
}

#[async_trait]
impl CommandHandler<UnpinNodesCommand> for PhysicsCommandHandler {
    async fn handle(&self, command: UnpinNodesCommand) -> Result<()> {
        command.validate()?;
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.unpin_nodes(command.node_ids).await?)
    }
}

#[async_trait]
impl CommandHandler<ResetPhysicsCommand> for PhysicsCommandHandler {
    async fn handle(&self, _command: ResetPhysicsCommand) -> Result<()> {
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.reset().await?)
    }
}

#[async_trait]
impl CommandHandler<CleanupPhysicsCommand> for PhysicsCommandHandler {
    async fn handle(&self, _command: CleanupPhysicsCommand) -> Result<()> {
        let mut adapter = self.adapter.lock().await;
        Ok(adapter.cleanup().await?)
    }
}

pub struct PhysicsQueryHandler {
    adapter: Arc<Mutex<dyn GpuPhysicsAdapter>>,
}

impl PhysicsQueryHandler {
    pub fn new(adapter: Arc<Mutex<dyn GpuPhysicsAdapter>>) -> Self {
        Self { adapter }
    }
}

#[async_trait]
impl QueryHandler<GetGpuStatusQuery> for PhysicsQueryHandler {
    async fn handle(
        &self,
        _query: GetGpuStatusQuery,
    ) -> Result<crate::ports::gpu_physics_adapter::GpuDeviceInfo> {
        let adapter = self.adapter.lock().await;
        Ok(adapter.get_gpu_status().await?)
    }
}

#[async_trait]
impl QueryHandler<GetPhysicsStatisticsQuery> for PhysicsQueryHandler {
    async fn handle(
        &self,
        _query: GetPhysicsStatisticsQuery,
    ) -> Result<crate::ports::gpu_physics_adapter::PhysicsStatistics> {
        let adapter = self.adapter.lock().await;
        Ok(adapter.get_statistics().await?)
    }
}

#[async_trait]
impl QueryHandler<ListGpuDevicesQuery> for PhysicsQueryHandler {
    async fn handle(
        &self,
        _query: ListGpuDevicesQuery,
    ) -> Result<Vec<crate::ports::gpu_physics_adapter::GpuDeviceInfo>> {
        
        let adapter = self.adapter.lock().await;
        let device = adapter.get_gpu_status().await?;
        Ok(vec![device])
    }
}

#[async_trait]
impl QueryHandler<GetPerformanceMetricsQuery> for PhysicsQueryHandler {
    async fn handle(&self, query: GetPerformanceMetricsQuery) -> Result<PerformanceMetrics> {
        let adapter = self.adapter.lock().await;
        let stats = adapter.get_statistics().await?;

        match query.metric_type {
            PerformanceMetricType::StepTime => Ok(PerformanceMetrics {
                average_step_time_ms: Some(stats.average_step_time_ms),
                average_energy: None,
                gpu_memory_used_mb: None,
                cache_hit_rate: None,
            }),
            PerformanceMetricType::Energy => Ok(PerformanceMetrics {
                average_step_time_ms: None,
                average_energy: Some(stats.average_energy),
                gpu_memory_used_mb: None,
                cache_hit_rate: None,
            }),
            PerformanceMetricType::MemoryUsage => Ok(PerformanceMetrics {
                average_step_time_ms: None,
                average_energy: None,
                gpu_memory_used_mb: Some(stats.gpu_memory_used_mb),
                cache_hit_rate: None,
            }),
            PerformanceMetricType::CacheHitRate => Ok(PerformanceMetrics {
                average_step_time_ms: None,
                average_energy: None,
                gpu_memory_used_mb: None,
                cache_hit_rate: Some(stats.cache_hit_rate),
            }),
            PerformanceMetricType::All => Ok(PerformanceMetrics {
                average_step_time_ms: Some(stats.average_step_time_ms),
                average_energy: Some(stats.average_energy),
                gpu_memory_used_mb: Some(stats.gpu_memory_used_mb),
                cache_hit_rate: Some(stats.cache_hit_rate),
            }),
        }
    }
}

#[async_trait]
impl QueryHandler<IsGpuAvailableQuery> for PhysicsQueryHandler {
    async fn handle(&self, _query: IsGpuAvailableQuery) -> Result<bool> {
        let adapter = self.adapter.lock().await;
        match adapter.get_gpu_status().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}
