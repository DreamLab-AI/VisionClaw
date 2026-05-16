//! Actor system modules for replacing Arc<RwLock<T>> patterns with Actix actors

pub mod agent_monitor_actor;
pub mod broadcast_actor;
pub mod client_coordinator_actor;
pub mod client_filter;
pub mod gpu;
pub mod graph_state_actor;
pub mod graph_actor {
    // Re-export graph_state_actor types for backward compatibility
    pub use super::graph_state_actor::GraphStateActor;
    pub use super::messages::AutoBalanceNotification;

    // PhysicsState type alias - represents the state of physics simulation
    // Contains simulation parameters and running status
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct PhysicsState {
        pub is_running: bool,
        pub params: crate::models::simulation_params::SimulationParams,
    }

    impl Default for PhysicsState {
        fn default() -> Self {
            Self {
                is_running: false,
                params: crate::models::simulation_params::SimulationParams::default(),
            }
        }
    }
}
pub mod metadata_actor;
pub mod optimized_settings_actor;
pub mod physics_orchestrator_actor;
pub mod protected_settings_actor;
pub mod supervisor;
pub mod voice_commands;
// pub mod supervisor_voice; 
// graph_messages module removed - AutoBalanceNotification consolidated into messages.rs
pub mod graph_service_supervisor;
pub mod messages;
pub mod messaging;
pub mod multi_mcp_visualization_actor;
pub mod ontology_actor;
pub mod semantic_processor_actor;
pub mod task_orchestrator_actor;
pub mod workspace_actor;

pub use agent_monitor_actor::AgentMonitorActor;
pub use broadcast_actor::{BroadcastActor, BroadcastConfig};
pub use client_coordinator_actor::{
    ClientCoordinatorActor, ClientCoordinatorStats, ClientManager, ClientState,
};
pub use gpu::GPUManagerActor;
pub use graph_state_actor::GraphStateActor;
pub use graph_service_supervisor::{
    ActorHealth, ActorHeartbeat, ActorType, BackoffStrategy, GetSupervisorStatus,
    GraphServiceSupervisor, GraphSupervisionStrategy, RestartActor, RestartAllActors,
    RestartPolicy, SetParentSupervisor, SupervisorMessage, SupervisorStatus,
};
pub use messages::*;
pub use messaging::{AckStatus, MessageAck, MessageId, MessageKind, MessageMetrics, MessageTracker};
pub use metadata_actor::MetadataActor;
pub use multi_mcp_visualization_actor::MultiMcpVisualizationActor;
pub use ontology_actor::{
    ActorStatistics as OntologyActorStatistics, JobPriority, JobStatus, OntologyActor,
    OntologyActorConfig, ValidationJob,
};
pub use optimized_settings_actor::OptimizedSettingsActor;
pub use physics_orchestrator_actor::{PhysicsOrchestratorActor, SetClientCoordinator, UserNodeInteraction};
pub use protected_settings_actor::ProtectedSettingsActor;
pub use semantic_processor_actor::{
    AISemanticFeatures, SemanticProcessorActor, SemanticProcessorConfig, SemanticStats,
};
pub use supervisor::{
    ActorFactory, SupervisedActorInfo, SupervisedActorTrait, SupervisionStrategy, SupervisorActor,
};
pub use task_orchestrator_actor::{
    CreateTask, GetSystemStatus, GetTaskStatus, ListActiveTasks, StopTask, SystemStatusInfo,
    TaskOrchestratorActor, TaskState,
};
pub use voice_commands::{SwarmIntent, SwarmVoiceResponse, VoiceCommand, VoicePreamble};
pub use workspace_actor::WorkspaceActor;

// Phase 5: Actor lifecycle management and coordination
pub mod event_coordination;
pub mod lifecycle;
pub use event_coordination::{initialize_event_coordinator, EventCoordinator};
pub use lifecycle::{
    initialize_actor_system, shutdown_actor_system, ActorLifecycleManager,
    SupervisionStrategy as Phase5SupervisionStrategy,
};
