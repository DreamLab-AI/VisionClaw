// src/application/mod.rs
//! CQRS Application Layer
//!
//! This module implements the application layer using hexser's CQRS patterns.
//! It contains all Directives (write operations) and Queries (read operations)
//! organized by domain.
//!
//! ## Architecture
//!
//! Following hexagonal architecture principles:
//! - **Directives**: Write operations that modify state (commands)
//! - **Queries**: Read operations that retrieve state (queries)
//! - **Handlers**: Process directives and queries using ports (repositories, adapters)
//! - **Services**: High-level orchestration of commands, queries, and events
//!
//! ## Domains
//!
//! - **Settings**: Application configuration and user preferences
//! - **Knowledge Graph**: Main graph structure from local markdown files
//! - **Ontology**: OWL-based ontology graph from GitHub markdown
//! - **Physics**: GPU-accelerated physics simulation

pub mod events;
pub mod graph;
pub mod knowledge_graph;
pub mod ontology;
pub mod physics;
pub mod settings;

// Re-export settings domain
pub use settings::{
    ClearSettingsCache, ClearSettingsCacheHandler, DeletePhysicsProfile,
    DeletePhysicsProfileHandler, GetPhysicsSettings, GetPhysicsSettingsHandler, GetSetting,
    GetSettingHandler, GetSettingsBatch, GetSettingsBatchHandler, ListPhysicsProfiles,
    ListPhysicsProfilesHandler, LoadAllSettings, LoadAllSettingsHandler, SaveAllSettings,
    SaveAllSettingsHandler, UpdatePhysicsSettings, UpdatePhysicsSettingsHandler, UpdateSetting,
    UpdateSettingHandler, UpdateSettingsBatch, UpdateSettingsBatchHandler,
};

// Re-export knowledge graph domain
pub use knowledge_graph::{
    AddEdge, AddEdgeHandler, AddNode, AddNodeHandler, BatchUpdatePositions,
    BatchUpdatePositionsHandler, GetGraphStatistics, GetGraphStatisticsHandler, GetNode,
    GetNodeEdges, GetNodeEdgesHandler, GetNodeHandler, GetNodesByMetadataId,
    GetNodesByMetadataIdHandler, LoadGraph, LoadGraphHandler, QueryNodes, QueryNodesHandler,
    QueryResult, RemoveEdge, RemoveEdgeHandler, RemoveNode, RemoveNodeHandler, SaveGraph,
    SaveGraphHandler, UpdateEdge, UpdateEdgeHandler, UpdateNode, UpdateNodeHandler,
};

// Re-export ontology domain
pub use ontology::{
    AddAxiom, AddAxiomHandler, AddOwlClass, AddOwlClassHandler, AddOwlProperty,
    AddOwlPropertyHandler, GetClassAxioms, GetClassAxiomsHandler, GetInferenceResults,
    GetInferenceResultsHandler, GetOntologyMetrics, GetOntologyMetricsHandler, GetOwlClass,
    GetOwlClassHandler, GetOwlProperty, GetOwlPropertyHandler, ListOwlClasses,
    ListOwlClassesHandler, ListOwlProperties, ListOwlPropertiesHandler, LoadOntologyGraph,
    LoadOntologyGraphHandler, QueryOntology, QueryOntologyHandler, RemoveAxiom, RemoveAxiomHandler,
    RemoveOwlClass, RemoveOwlClassHandler, SaveOntologyGraph, SaveOntologyGraphHandler,
    StoreInferenceResults, StoreInferenceResultsHandler, UpdateOwlClass, UpdateOwlClassHandler,
    UpdateOwlProperty, UpdateOwlPropertyHandler, ValidateOntology, ValidateOntologyHandler,
};

// Re-export graph domain
pub use graph::{
    ComputeShortestPaths, ComputeShortestPathsHandler, GetAutoBalanceNotifications,
    GetAutoBalanceNotificationsHandler, GetConstraints,
    GetConstraintsHandler, GetEquilibriumStatus, GetEquilibriumStatusHandler, GetGraphData,
    GetGraphDataHandler, GetNodeMap, GetNodeMapHandler, GetPhysicsState, GetPhysicsStateHandler,
};

// Application services removed - handlers use actors directly via CQRS or direct messaging

// Phase 5: New hexagonal architecture services
pub mod physics_service;
pub mod semantic_service;

pub use physics_service::PhysicsService;
pub use semantic_service::SemanticService;

// ADR-2066: the Phase 7 `InferenceService` (formerly `inference_service`) was
// never constructed anywhere — dead code alongside `src/handlers/inference_handler.rs`
// and `src/events/inference_triggers.rs`. Removed.

// Re-export events
pub use events::DomainEvent;
