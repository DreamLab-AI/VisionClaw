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
pub use visionclaw_domain::ports::gpu_semantic_analyzer::PathfindingResult;

// =============================================================================
// Re-export everything from each submodule
// =============================================================================

// --- graph_messages ---
pub use graph_messages::{
    AddEdge, AddNode, AddNodesFromMetadata, ArchiveWorkspace, AutoBalanceNotification,
    BuildGraphFromMetadata, CreateWorkspace, DeleteWorkspace, GetAutoBalanceNotifications,
    GetGraphData, GetGraphStateActor, GetMetadata, GetNodeIdMapping, GetNodeMap, GetNodePositions,
    GetNodeTypeArrays, GetPositionFrameSnapshot, GetWorkspace, GetWorkspaceCount, GetWorkspaces,
    InitializeActor, LoadWorkspaces, NodeIdMapping, NodeTypeArrays, PositionFrameSnapshot,
    PositionRow, ReloadGraphFromDatabase, RemoveEdge, RemoveNode, RemoveNodeByMetadata,
    RequestGraphUpdate, SaveWorkspaces, ToggleFavoriteWorkspace, UpdateGraphData, UpdateMetadata,
    UpdateNodeFromMetadata, UpdateNodePosition, UpdateNodePositions, UpdateNodeTypeArrays,
    UpdateWorkspace, WorkspaceChangeType, WorkspaceStateChanged,
};

// --- physics_messages ---
pub use physics_messages::{
    AddIsolationLayer,
    AdjustConstraintWeights,
    ApplyConstraintsToNodes,
    // GPU position snapshot (REST API)
    BoundingBox,
    BroadcastPerformanceStats,
    // Phase 5 (ADR-01 D9): event emission only
    ClampKind,
    ComputeForces,
    ConfigureBroadcastOptimization,
    ConfigureCollision,
    ConfigureDAG,
    ConfigureStressMajorization,
    ConfigureTypeClustering,
    CurrentPositionsSnapshot,
    EmitPhysicsEvent,
    ForceResumePhysics,
    GPUInitFailed,
    GPUInitialized,
    GPUStatus,
    GetActiveConstraints,
    GetBroadcastStats,
    GetConstraintBuffer,
    GetConstraints,
    GetCurrentPositions,
    GetEquilibriumStatus,
    GetForceComputeActor,
    GetGPUMetrics,
    GetGPUStatus,
    GetHierarchyLevels,
    GetKernelMode,
    GetNodeData,
    GetPhysicsOrchestratorActor,
    GetPhysicsStats,
    GetSemanticConfig,
    // Settlement telemetry (honest convergence readout)
    GetSettlementState,
    GetStressMajorizationConfig,
    GetStressMajorizationStats,
    InitializeGPU,
    InitializeGPUConnection,
    InitializeVisualAnalytics,
    NodeInteractionMessage,
    NodeInteractionType,
    PhysicsEvent,
    PhysicsPauseMessage,
    // Sequential pipeline (Step 5)
    PhysicsStepCompleted,
    PinNodePositions,
    PositionBroadcastAck,
    PositionSnapshot,
    RecalculateHierarchy,
    RegenerateSemanticConstraints,
    ReloadRelationshipBuffer,
    RemoveConstraints,
    RemoveIsolationLayer,
    RequestPositionSnapshot,
    ResetGPUInitFlag,
    // Layout reset
    ResetPositions,
    ResetStressMajorizationSafety,
    SetAdvancedGPUContext,
    SetAppGpuComputeAddr,
    SetComputeMode,
    SetForceComputeAddr,
    SetGpuComputeAddress,
    SetLayoutMode,
    SetPhysicsOrchestratorAddr,
    SetPhysicsSettled,
    SetRadialLayout,
    SetSharedGPUContext,
    SettlementSnapshot,
    SimulationStep,
    StartSimulation,
    StopSimulation,
    StoreAdvancedGPUContext,
    StoreGPUComputeAddress,
    StressMajorizationConfig,
    TriggerStressMajorization,
    UpdateAdvancedParams,
    UpdateCameraFrustum,
    UpdateClusteringParams,
    UpdateConstraintData,
    UpdateConstraints,
    UpdateForceParams,
    UpdateGPUGraphData,
    UpdateGPUPositions,
    UpdateOntologyConstraintBuffer,
    UpdateSimulationParams,
    UpdateStressMajorizationParams,
    UpdateVisualAnalyticsParams,
    UploadConstraintsToGPU,
    UploadPositions,
};

// --- settings_messages ---
pub use settings_messages::{
    GetSettingByPath, GetSettings, GetSettingsByPaths, MergeSettingsUpdate, PartialSettingsUpdate,
    PriorityUpdate, ReloadSettings, SetSettingByPath, SetSettingsByPaths,
    UpdatePhysicsFromAutoBalance, UpdatePriority, UpdateSettings,
};

// --- ontology_messages ---
pub use ontology_messages::{
    ApplyInferences, ApplyMaterializedAxioms, ApplyOntologyConstraints, CachedOntologyInfo,
    ClearOntologyCaches, ConstraintMergeMode, ConstraintStats, GetCachedOntologies,
    GetConstraintStats, GetOntologyConstraintStats, GetOntologyHealth, GetOntologyHealthLegacy,
    GetOntologyReport, GetValidationReport, LoadOntologyAxioms, OntologyConstraintStats,
    OntologyHealth, SetConstraintGroupActive, UpdateOntologyMapping, ValidateGraph,
    ValidateOntology, ValidationMode,
};

// --- client_messages ---
pub use client_messages::{
    AuthenticateClient, BroadcastAgentActionFrame, BroadcastMessage, BroadcastNodePositions,
    BroadcastPositions, ClientBroadcastAck, ClientRecipients, ForcePositionBroadcast,
    GetClientCount, InitialClientSync, RegisterClient, SendInitialGraphLoad, SendPositionUpdate,
    SendToClientBinary, SendToClientText, SetGraphServiceAddress, UnregisterClient,
    UpdateClientFilter,
};

// --- analytics_messages ---
pub use analytics_messages::{
    AnomalyDetectionMethod, AnomalyDetectionParams, AnomalyDetectionStats, AnomalyMethod,
    AnomalyParams, AnomalyResult, ClearPageRankCache, CommunityDetectionAlgorithm,
    CommunityDetectionParams, CommunityDetectionResult, ComputeAllPairsShortestPaths,
    ComputePageRank, ComputeSSSP, ComputeShortestPaths, DBSCANParams, DBSCANResult, DBSCANStats,
    ExportClusterAssignments, GetClusteringResults, GetClusteringStatus, GetPageRankResult,
    KMeansParams, KMeansResult, PerformGPUClustering, RunAnomalyDetection, RunCommunityDetection,
    RunDBSCAN, RunKMeans, SetNodeAnalytics, SetNodeSSSP, StartGPUClustering, WriteClusterAnalytics,
};

// --- agent_messages ---
pub use agent_messages::{
    AgentMetrics,
    AgentUpdate,
    Bottleneck,
    BottleneckAnalyze,
    CloseTcpConnection,
    ConnectionFailed,
    CoordinationPattern,
    CoordinationSync,
    EstablishTcpConnection,
    GetAgentMetrics,
    GetBotsGraphData,
    GetCachedAgentStatuses,
    GetNeuralStatus,
    GetPerformanceReport,
    GetSwarmStatus,
    InitializeJsonRpc,
    InitializeSwarm,
    LoadBalance,
    MemoryPersist,
    MemorySearch,
    MessageFlowEvent,
    MetricsCollect,
    NeuralPredict,
    NeuralStatus,
    NeuralTrain,
    PerformanceReport,
    PollAgentStatuses,
    PollSwarmData,
    PollSystemMetrics,
    RecordPollFailure,
    RecordPollSuccess,
    RetryMCPConnection,
    // ADR-031: Orchestration improvements
    SetAgentMonitorAddr,
    SpawnAgent,
    SpawnAgentCommand,
    StateSnapshot,
    SwarmDestroy,
    SwarmMonitor,
    SwarmMonitorData,
    SwarmScale,
    SwarmStatus,
    SystemMetrics,
    TaskOrchestrate,
    TaskStatusChanged,
    TopologyOptimize,
    UpdateAgentCache,
    UpdateBotsGraph,
};
