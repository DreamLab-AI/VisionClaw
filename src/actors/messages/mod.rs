//! Message definitions for actor system communication.
//!
//! Split into domain-specific submodules for maintainability.
//! All types are re-exported here so that `use crate::actors::messages::*`
//! continues to work unchanged.

pub mod agent_messages;
pub mod analytics_messages;
pub mod client_messages;
pub mod graph_messages;
pub mod ontology_messages;
pub mod physics_messages;
pub mod settings_messages;

// Re-export PathfindingResult from the port for convenience
pub use crate::ports::gpu_semantic_analyzer::PathfindingResult;

// =============================================================================
// Re-export everything from each submodule
// =============================================================================

// --- graph_messages ---
pub use graph_messages::{
    AddEdge, AddNode, AddNodesFromMetadata, ArchiveWorkspace, AutoBalanceNotification,
    BuildGraphFromMetadata, CreateWorkspace, DeleteWorkspace, GetAutoBalanceNotifications,
    GetGraphData, GetGraphStateActor, GetMetadata, GetNodeIdMapping, GetNodeMap, GetNodePositions,
    GetNodeTypeArrays, NodeIdMapping, GetWorkspace, GetWorkspaceCount, GetWorkspaces, InitializeActor,
    LoadWorkspaces, NodeTypeArrays, RefreshMetadata, ReloadGraphFromDatabase, RemoveEdge,
    RemoveNode, RemoveNodeByMetadata, RequestGraphUpdate, SaveWorkspaces,
    ToggleFavoriteWorkspace, UpdateGraphData, UpdateMetadata, UpdateNodeFromMetadata,
    UpdateNodePosition, UpdateNodePositions, UpdateNodeTypeArrays, UpdateWorkspace,
    WorkspaceChangeType, WorkspaceStateChanged,
};

// --- physics_messages ---
pub use physics_messages::{
    AddIsolationLayer, AdjustConstraintWeights, ApplyConstraintsToNodes,
    BroadcastPerformanceStats, ComputeForces, ConfigureBroadcastOptimization, ConfigureCollision,
    ConfigureDAG, ConfigureMaturity, ConfigurePhysicality, ConfigureRole,
    ConfigureStressMajorization, ConfigureTypeClustering, ForceResumePhysics,
    GPUInitFailed, GPUInitialized, GPUStatus, GetActiveConstraints, GetBroadcastStats, GetConstraintBuffer,
    GetConstraints, GetEquilibriumStatus, GetForceComputeActor, GetPhysicsOrchestratorActor, GetGPUMetrics, GetGPUStatus,
    GetHierarchyLevels, GetKernelMode, GetNodeData, GetPhysicsStats, GetSemanticConfig,
    GetStressMajorizationConfig, GetStressMajorizationStats, InitializeGPU,
    InitializeGPUConnection, InitializeVisualAnalytics, NodeInteractionMessage, SetAppGpuComputeAddr,
    NodeInteractionType, PhysicsPauseMessage, PositionBroadcastAck, PositionSnapshot,
    RecalculateHierarchy, RegenerateSemanticConstraints, ReloadRelationshipBuffer,
    RemoveConstraints, RemoveIsolationLayer, RequestPositionSnapshot, ResetGPUInitFlag,
    ResetStressMajorizationSafety, SetAdvancedGPUContext, SetComputeMode, SetForceComputeAddr,
    SetGpuComputeAddress, SetSharedGPUContext, SimulationStep, StartSimulation,
    StopSimulation, StoreAdvancedGPUContext, StoreGPUComputeAddress,
    StressMajorizationConfig, TriggerStressMajorization, UpdateAdvancedParams, UpdateCameraFrustum,
    UpdateConstraintData, UpdateConstraints, UpdateForceParams, UpdateGPUGraphData,
    UpdateGPUPositions, UpdateOntologyConstraintBuffer, UpdateSimulationParams,
    UpdateStressMajorizationParams, UpdateVisualAnalyticsParams, UploadConstraintsToGPU,
    UploadPositions,
    // Sequential pipeline (Step 5)
    PhysicsStepCompleted, SetPhysicsOrchestratorAddr,
    // GPU position snapshot (REST API)
    BoundingBox, CurrentPositionsSnapshot, GetCurrentPositions,
    // Layout reset
    ResetPositions,
};

// --- settings_messages ---
pub use settings_messages::{
    GetSettingByPath, GetSettings, GetSettingsByPaths, MergeSettingsUpdate, PartialSettingsUpdate,
    PriorityUpdate, ReloadSettings, SetSettingByPath, SetSettingsByPaths, UpdatePhysicsFromAutoBalance,
    UpdatePriority, UpdateSettings,
};

// --- ontology_messages ---
pub use ontology_messages::{
    ApplyInferences, ApplyOntologyConstraints, CachedOntologyInfo, ClearOntologyCaches,
    ConstraintMergeMode, ConstraintStats, GetCachedOntologies, GetConstraintStats,
    GetOntologyConstraintStats, GetOntologyHealth, GetOntologyHealthLegacy, GetValidationReport,
    LoadOntologyAxioms, OntologyConstraintStats, OntologyHealth, ProcessOntologyData,
    SetConstraintGroupActive, UpdateOntologyMapping, ValidateGraph, ValidateOntology,
    ValidationMode, GetOntologyReport,
};

// --- client_messages ---
pub use client_messages::{
    AuthenticateClient, BroadcastMessage, BroadcastNodePositions, BroadcastPositions,
    ClientBroadcastAck, ForcePositionBroadcast, GetClientCount, InitialClientSync,
    RegisterClient, SendInitialGraphLoad, SendPositionUpdate, SendToClientBinary,
    SendToClientText, SetGraphServiceAddress, UnregisterClient, UpdateClientFilter,
};

// --- analytics_messages ---
pub use analytics_messages::{
    AnomalyDetectionMethod, AnomalyDetectionParams, AnomalyDetectionStats, AnomalyMethod,
    AnomalyParams, AnomalyResult, ClearPageRankCache, CommunityDetectionAlgorithm,
    CommunityDetectionParams, CommunityDetectionResult, ComputeAllPairsShortestPaths,
    ComputePageRank, ComputeSSSP, ComputeShortestPaths, DBSCANParams, DBSCANResult,
    DBSCANStats, ExportClusterAssignments, GetClusteringResults, GetClusteringStatus,
    GetPageRankResult, KMeansParams, KMeansResult, PerformGPUClustering, RunAnomalyDetection,
    RunCommunityDetection, RunDBSCAN, RunKMeans, SetNodeAnalytics, StartGPUClustering,
    UpdateComponentEdges,
};

// --- agent_messages ---
pub use agent_messages::{
    AgentMetrics, AgentUpdate, Bottleneck, BottleneckAnalyze, CloseTcpConnection,
    ConnectionFailed, CoordinationPattern, CoordinationSync, EstablishTcpConnection,
    GetAgentMetrics, GetBotsGraphData, GetCachedAgentStatuses, GetNeuralStatus,
    GetPerformanceReport, GetSwarmStatus, InitializeJsonRpc, InitializeSwarm, LoadBalance,
    MemoryPersist, MemorySearch, MessageFlowEvent, MetricsCollect, NeuralPredict, NeuralStatus,
    NeuralTrain, PerformanceReport, PollAgentStatuses, PollSwarmData, PollSystemMetrics,
    RecordPollFailure, RecordPollSuccess, RetryMCPConnection, SpawnAgent, SpawnAgentCommand,
    StateSnapshot, SwarmDestroy, SwarmMonitor, SwarmMonitorData, SwarmScale, SwarmStatus,
    SystemMetrics, TaskOrchestrate, TopologyOptimize, UpdateAgentCache, UpdateBotsGraph,
    // ADR-031: Orchestration improvements
    SetAgentMonitorAddr, TaskStatusChanged,
};
