//! Actor system modules for replacing Arc<RwLock<T>> patterns with Actix actors

pub mod agent_beam_actor;
// ADR-110 — flagship ACSP agentic actor (knowledge elevation via forum cases)
pub mod elevation_actor;
pub mod elevation_voice;
// ADR-050 — decision elevation (inverse corpus path): the write-half actor that
// mirrors ElevationActor for dl:DecisionRecord instances.
pub mod decision_elevation_actor;
// ADR-110 — voice → settings-assistant bridge (Control Center configuration agent)
pub mod agent_monitor_actor;
pub mod client_coordinator_actor;
pub mod client_filter;
pub mod gpu;
pub mod graph_state_actor;
pub mod voice_interface_actor;
pub mod graph_actor {
    // Re-export graph_state_actor types for backward compatibility
    pub use super::graph_state_actor::GraphStateActor;
    pub use super::messages::AutoBalanceNotification;

    // PhysicsState type alias - represents the state of physics simulation
    // Contains simulation parameters and running status
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    pub struct PhysicsState {
        pub is_running: bool,
        pub params: crate::models::simulation_params::SimulationParams,
    }
}
pub mod metadata_actor;
pub mod optimized_settings_actor;
pub mod physics_orchestrator_actor;
pub mod protected_settings_actor;
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
// PRD-008 §5.3 — per-room XR presence broadcast actor
pub mod presence_actor;

pub use agent_beam_actor::AgentBeamActor;
pub use agent_monitor_actor::AgentMonitorActor;
pub use client_coordinator_actor::{
    ClientCoordinatorActor, ClientCoordinatorStats, ClientManager, ClientState,
};
pub use gpu::GPUManagerActor;
pub use graph_service_supervisor::{
    ActorHealth, ActorHeartbeat, ActorType, BackoffStrategy, GetSupervisorStatus,
    GraphServiceSupervisor, GraphSupervisionStrategy, RestartActor, RestartAllActors,
    RestartPolicy, SupervisorMessage, SupervisorStatus,
};
pub use graph_state_actor::GraphStateActor;
pub use messages::*;
pub use messaging::{
    AckStatus, MessageAck, MessageId, MessageKind, MessageMetrics, MessageTracker,
};
pub use metadata_actor::MetadataActor;
pub use multi_mcp_visualization_actor::MultiMcpVisualizationActor;
pub use ontology_actor::{
    ActorStatistics as OntologyActorStatistics, JobPriority, JobStatus, OntologyActor,
    OntologyActorConfig, ValidationJob,
};
pub use optimized_settings_actor::OptimizedSettingsActor;
pub use physics_orchestrator_actor::{
    PhysicsOrchestratorActor, SetClientCoordinator, UserNodeInteraction,
};
pub use protected_settings_actor::ProtectedSettingsActor;
pub use semantic_processor_actor::{
    AISemanticFeatures, SemanticProcessorActor, SemanticProcessorConfig, SemanticStats,
};
pub use task_orchestrator_actor::{
    CreateTask, GetSystemStatus, GetTaskStatus, InterruptAgentTask, InterruptError,
    ListActiveTasks, StopTask, SystemStatusInfo, TaskOrchestratorActor, TaskState,
};
pub use voice_commands::{SwarmIntent, SwarmVoiceResponse, VoiceCommand, VoicePreamble};
pub use workspace_actor::WorkspaceActor;

// Phase 5: Actor lifecycle management and coordination
pub mod event_coordination;
pub use event_coordination::{initialize_event_coordinator, EventCoordinator};
// ADR-2045: `src/actors/lifecycle.rs` (`ActorLifecycleManager`,
// `initialize_actor_system`, `shutdown_actor_system`, its own
// `SupervisionStrategy`/`SupervisionDecision` pair) was dead code — it was
// re-exported here but `initialize_actor_system`/`shutdown_actor_system` were
// never called from anywhere in `src/`, and `ACTOR_SYSTEM` had no other
// reader. It duplicated a second, unrelated `PhysicsOrchestratorActor` /
// `SemanticProcessorActor` pair and its own health monitor alongside the
// live `GraphServiceSupervisor` (`src/actors/graph_service_supervisor.rs`),
// which is the actor system's real supervision path. Removed, not stubbed.
//
// ADR-2045: `src/actors/supervisor.rs` (the generic `SupervisorActor`,
// `ActorFactory`, `SupervisedActorTrait`, `SupervisionStrategy`,
// `ActorFailed`, `InitiateGracefulShutdown`) went the same way. Its
// `SupervisorActor::new` was called only from its own `#[cfg(test)]` module
// and `InitiateGracefulShutdown` was never sent. Its one non-test coupling was
// `GraphServiceSupervisor`'s `parent_supervisor` field and the
// `SetParentSupervisor` message — but that message was never SENT by anything,
// so the field was permanently `None` and the `Escalate` branch always took
// the stop path. Both were removed with it, and `Escalate` now says plainly
// that it is the top of the tree.
