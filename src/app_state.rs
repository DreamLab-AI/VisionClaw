use actix::prelude::*;
use actix_web::web;
use log::{debug, error, info, warn};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::RwLock;

// Oxigraph adapters — canonical persistence layer (ADR-11)
use crate::adapters::{
    OxigraphGraphRepository, OxigraphOntologyRepository, SqliteEnrichmentRepository,
    SqliteKpiRepository, SqliteSettingsRepository,
};

// CQRS Phase 1D: Graph domain imports
use crate::adapters::actor_graph_repository::ActorGraphRepository;
use crate::application::graph::*;

// Event bus and store (events module — unrelated to the removed CQRS bus)
use crate::events::{EventBus, EventStore};

use crate::actors::gpu;
use crate::actors::gpu::GPUContextBus;
use crate::actors::graph_service_supervisor::GraphServiceSupervisor;
use crate::actors::ontology_actor::OntologyActor;
use crate::actors::GPUManagerActor;
use crate::actors::{
    AgentBeamActor, AgentMonitorActor, ClientCoordinatorActor, MetadataActor,
    OptimizedSettingsActor, ProtectedSettingsActor, TaskOrchestratorActor, WorkspaceActor,
};
use crate::config::feature_access::FeatureAccess;
use crate::config::AppFullSettings;
use crate::models::protected_settings::{ApiKeys, NostrUser, ProtectedSettings};
use crate::services::bots_client::BotsClient;
use crate::services::github::content_enhanced::EnhancedContentAPI;
use crate::services::github::{ContentAPI, GitHubClient};
use crate::services::github_sync_service::GitHubSyncService;
use crate::services::management_api_client::ManagementApiClient;
use crate::services::nostr_service::NostrService;
use crate::services::perplexity_service::PerplexityService;
use crate::services::ragflow_service::RAGFlowService;
use crate::services::speech_service::SpeechService;
use crate::utils::client_message_extractor::ClientMessage;
use tokio::sync::mpsc;
use tokio::time::Duration;
use visionclaw_domain::models::metadata::MetadataStore;

// Repository trait imports for hexagonal architecture
use crate::ports::settings_repository::SettingsRepository;

/// SECURITY: List of known insecure default values that must be rejected
/// Note: Do NOT include empty string - use separate length check instead
const INSECURE_DEFAULT_KEYS: &[&str] = &[
    "change-this-secret-key",
    "changeme",
    "secret",
    "password",
    "admin",
    "test",
    "default",
    "your-secret-key",
    "your-api-key",
    "replace-me",
    "xxx",
];

/// Validates all security-critical environment variables at startup.
/// This function enforces strict security requirements:
/// - MANAGEMENT_API_KEY must be set and not contain insecure default values
/// - JWT_SECRET (if used) must be set and not contain insecure default values
/// # Panics
/// Panics if any security-critical environment variable is missing or insecure.
/// This is intentional to prevent the application from starting in an insecure state.
/// # Returns
/// The validated MANAGEMENT_API_KEY value on success.
///
/// ADR-2094: `pub(crate)` so `AgentMonitorActor` shares this one policy rather
/// than carrying a laxer copy of it. Any crate-internal consumer of
/// `MANAGEMENT_API_KEY` must validate through here.
pub(crate) fn validate_security_env_vars(
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut errors: Vec<String> = Vec::new();

    // Validate MANAGEMENT_API_KEY
    let mgmt_api_key = match std::env::var("MANAGEMENT_API_KEY") {
        Ok(key) => {
            let key_lower = key.to_lowercase();
            if INSECURE_DEFAULT_KEYS.iter().any(|&insecure| {
                !insecure.is_empty() && (key_lower == insecure || key_lower.contains(insecure))
            }) {
                errors.push(format!(
                    "MANAGEMENT_API_KEY contains an insecure default value. \
                     Please set a strong, unique API key (minimum 32 characters recommended)."
                ));
                None
            } else if key.len() < 16 {
                errors.push(format!(
                    "MANAGEMENT_API_KEY is too short ({} chars). \
                     Please use a key with at least 16 characters (32+ recommended).",
                    key.len()
                ));
                None
            } else {
                Some(key)
            }
        }
        Err(_) => {
            errors.push(
                "MANAGEMENT_API_KEY environment variable is not set. \
                 This is required for secure API authentication. \
                 Please set MANAGEMENT_API_KEY to a strong, unique value."
                    .to_string(),
            );
            None
        }
    };

    // Validate JWT_SECRET if it exists (optional but must be secure if set)
    if let Ok(jwt_secret) = std::env::var("JWT_SECRET") {
        let jwt_lower = jwt_secret.to_lowercase();
        if INSECURE_DEFAULT_KEYS.iter().any(|&insecure| {
            !insecure.is_empty() && (jwt_lower == insecure || jwt_lower.contains(insecure))
        }) {
            errors.push(format!(
                "JWT_SECRET contains an insecure default value. \
                 Please set a strong, unique secret (minimum 32 characters recommended)."
            ));
        } else if jwt_secret.len() < 32 {
            warn!(
                "[Security] JWT_SECRET is shorter than recommended ({} chars). \
                 Consider using at least 32 characters for production.",
                jwt_secret.len()
            );
        }
    }

    // If there are any security errors, log them clearly and panic
    if !errors.is_empty() {
        log::error!("=========================================================");
        log::error!("  SECURITY CONFIGURATION ERROR - APPLICATION CANNOT START");
        log::error!("=========================================================");
        for (i, error) in errors.iter().enumerate() {
            log::error!("  {}. {}", i + 1, error);
        }
        log::error!("---------------------------------------------------------");
        log::error!("  Required environment variables:");
        log::error!("    - MANAGEMENT_API_KEY: Strong unique API key (16+ chars)");
        log::error!("    - JWT_SECRET (optional): Strong unique secret (32+ chars)");
        log::error!("---------------------------------------------------------");
        log::error!("  Example secure configuration:");
        log::error!("    export MANAGEMENT_API_KEY=$(openssl rand -hex 32)");
        log::error!("    export JWT_SECRET=$(openssl rand -hex 32)");
        log::error!("=========================================================");

        return Err(format!(
            "Security configuration failed: {} error(s). See logs above for details.",
            errors.len()
        )
        .into());
    }

    // Return the validated key
    let key = mgmt_api_key.expect("Key validated but None - logic error");
    info!(
        "[Security] MANAGEMENT_API_KEY validated successfully ({}*** chars)",
        key.len()
    );

    Ok(key)
}

// CQRS Phase 1D: Graph query handlers struct
#[derive(Clone)]
pub struct GraphQueryHandlers {
    pub get_graph_data: Arc<GetGraphDataHandler>,
    pub get_node_map: Arc<GetNodeMapHandler>,
    pub get_physics_state: Arc<GetPhysicsStateHandler>,
    pub get_auto_balance_notifications: Arc<GetAutoBalanceNotificationsHandler>,
    pub get_bots_graph_data: Arc<GetBotsGraphDataHandler>,
    pub get_constraints: Arc<GetConstraintsHandler>,
    pub get_equilibrium_status: Arc<GetEquilibriumStatusHandler>,
    pub compute_shortest_paths: Arc<ComputeShortestPathsHandler>,
}

// Phase 7: GPU Subsystem Decomposition
// Independent subsystems that receive GPU context via event bus

/// Physics simulation subsystem actors
#[derive(Clone)]
pub struct PhysicsSubsystem {
    pub force_compute: Option<Addr<gpu::ForceComputeActor>>,
    pub stress_major: Option<Addr<gpu::StressMajorizationActor>>,
    pub constraint: Option<Addr<gpu::ConstraintActor>>,
}

/// Analytics and ML subsystem actors
#[derive(Clone)]
pub struct AnalyticsSubsystem {
    pub clustering: Option<Addr<gpu::ClusteringActor>>,
    pub anomaly: Option<Addr<gpu::AnomalyDetectionActor>>,
    pub pagerank: Option<Addr<gpu::PageRankActor>>,
}

/// Graph algorithm subsystem actors
#[derive(Clone)]
pub struct GraphSubsystem {
    pub shortest_path: Option<Addr<gpu::ShortestPathActor>>,
    pub components: Option<Addr<gpu::ConnectedComponentsActor>>,
}

impl Default for PhysicsSubsystem {
    fn default() -> Self {
        Self {
            force_compute: None,
            stress_major: None,
            constraint: None,
        }
    }
}

impl Default for AnalyticsSubsystem {
    fn default() -> Self {
        Self {
            clustering: None,
            anomaly: None,
            pagerank: None,
        }
    }
}

impl Default for GraphSubsystem {
    fn default() -> Self {
        Self {
            shortest_path: None,
            components: None,
        }
    }
}

impl PhysicsSubsystem {
    /// Check if any physics actors are initialized
    pub fn is_initialized(&self) -> bool {
        self.force_compute.is_some() || self.stress_major.is_some() || self.constraint.is_some()
    }

    /// Get count of active actors
    pub fn active_count(&self) -> usize {
        [
            self.force_compute.is_some(),
            self.stress_major.is_some(),
            self.constraint.is_some(),
        ]
        .iter()
        .filter(|&&x| x)
        .count()
    }
}

impl AnalyticsSubsystem {
    /// Check if any analytics actors are initialized
    pub fn is_initialized(&self) -> bool {
        self.clustering.is_some() || self.anomaly.is_some() || self.pagerank.is_some()
    }

    /// Get count of active actors
    pub fn active_count(&self) -> usize {
        [
            self.clustering.is_some(),
            self.anomaly.is_some(),
            self.pagerank.is_some(),
        ]
        .iter()
        .filter(|&&x| x)
        .count()
    }
}

impl GraphSubsystem {
    /// Check if any graph algorithm actors are initialized
    pub fn is_initialized(&self) -> bool {
        self.shortest_path.is_some() || self.components.is_some()
    }

    /// Get count of active actors
    pub fn active_count(&self) -> usize {
        [self.shortest_path.is_some(), self.components.is_some()]
            .iter()
            .filter(|&&x| x)
            .count()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub graph_service_addr: Addr<GraphServiceSupervisor>,
    pub gpu_manager_addr: Option<Addr<GPUManagerActor>>,
    /// ForceComputeActor address - populated asynchronously after GPU initialization
    /// Use `get_gpu_compute_addr().await` to access this safely
    pub gpu_compute_addr: Arc<RwLock<Option<Addr<gpu::ForceComputeActor>>>>,
    pub stress_majorization_addr: Option<Addr<gpu::StressMajorizationActor>>,

    // Phase 7: Decomposed GPU subsystems (direct access, bypassing GPUManagerActor)
    pub physics: PhysicsSubsystem,
    pub analytics: AnalyticsSubsystem,
    pub graph_ops: GraphSubsystem,

    // Event bus for GPU context distribution to independent subsystems
    pub gpu_context_bus: Arc<GPUContextBus>,

    pub settings_repository: Arc<dyn SettingsRepository>,

    // Concrete SQLite settings repository for user-specific operations (filters, etc.) — ADR-11
    pub sqlite_settings_repository: Arc<SqliteSettingsRepository>,

    // WS-9: durable EnrichmentProposal lifecycle store (data/enrichment.sqlite3).
    // Separate DB file from settings.sqlite3 to isolate the single-writer posture
    // and keep WS-9 migrations independent.
    pub sqlite_enrichment_repository: Arc<SqliteEnrichmentRepository>,

    // gap-close item 2 (ADR-130 Decision 2): optional shared ACSP client that
    // projects operator/bridge REST decisions back to the forum as kind-31403
    // ActionResponse events, so `apply_decision` is visible in the forum's
    // `broker_decisions`. `None` when FORUM_RELAY_URL + a panel secret are
    // unconfigured (or the connect failed) — the decide path then records
    // `forum_projection=skipped` and logs loudly (degraded must be visible).
    pub acsp_client: Option<Arc<crate::services::acsp::AcspClient>>,

    // RES-a: sprint-wide live-traffic observer + KG-backend watchdog gauge
    // (ADR-130 Decision 3). Backs /api/canary/{register,observe,status} and the
    // tokio watchdog spawned in main.rs. Own SQLite file (data/liveness.sqlite3).
    pub liveness_harness: Arc<crate::services::liveness_harness::LivenessHarness>,

    // REC-4 (ADR-130 Decision 5): durable KPI snapshot + lineage store
    // (data/kpi.sqlite3) and the compute engine over it. The store is fed by the
    // agent-event volume tap spawned in main.rs; the service backs /api/kpi/*.
    pub sqlite_kpi_repository: Arc<SqliteKpiRepository>,
    pub kpi_compute_service: Arc<crate::services::kpi_compute::KpiComputeService>,

    // Oxigraph graph repository — canonical knowledge graph store (ADR-11)
    pub graph_adapter: Arc<OxigraphGraphRepository>,

    // Oxigraph ontology repository (ADR-11)
    pub ontology_repository: Arc<OxigraphOntologyRepository>,

    pub graph_repository: Arc<ActorGraphRepository>,
    pub graph_query_handlers: GraphQueryHandlers,

    pub event_bus: Arc<RwLock<EventBus>>,
    pub event_store: Arc<EventStore>,

    pub settings_addr: Addr<OptimizedSettingsActor>,
    pub protected_settings_addr: Addr<ProtectedSettingsActor>,
    pub metadata_addr: Addr<MetadataActor>,
    pub client_manager_addr: Addr<ClientCoordinatorActor>,
    pub agent_monitor_addr: Addr<AgentMonitorActor>,
    pub workspace_addr: Addr<WorkspaceActor>,
    pub ontology_actor_addr: Option<Addr<OntologyActor>>,
    pub github_client: Arc<GitHubClient>,
    pub content_api: Arc<ContentAPI>,
    pub perplexity_service: Option<Arc<PerplexityService>>,
    pub ragflow_service: Option<Arc<RAGFlowService>>,
    pub speech_service: Option<Arc<SpeechService>>,
    /// COM-15 / D6: per-user PTT sessions with the selected-agent `did:nostr`
    /// binding (was defined but consumerless; now the governed voice loop's
    /// binding store).
    pub audio_router: Arc<crate::services::audio_router::AudioRouter>,
    /// COM-15 / V1: the governed-voice-loop consumer (signs a 31402 targeted at
    /// the bound agent and POSTs to agentbox `/v1/voice-intent`). `None` when the
    /// loop is unconfigured for this profile.
    pub voice_intent_client: Option<Arc<crate::services::voice_intent_client::VoiceIntentClient>>,
    /// V3 (PRD-023 WP-10): per-session voice conversation state — the store that
    /// carries a pending clarification across turns (the governed-voice repair
    /// merge reads/writes `ConversationContext.pending_clarification` here).
    pub voice_context_manager: Arc<crate::services::voice_context_manager::VoiceContextManager>,
    /// V3 (PRD-023 WP-10): the configurable STT/intent confidence gate. Below
    /// threshold (`VISIONCLAW_VOICE_CONFIDENCE_THRESHOLD`, default 0.55) a spoken
    /// command is held for a clarification turn instead of dispatched.
    pub clarification_gate: crate::services::voice_clarification::ClarificationGate,
    pub nostr_service: Option<web::Data<NostrService>>,
    pub feature_access: web::Data<FeatureAccess>,
    pub ragflow_session_id: String,
    pub active_connections: Arc<AtomicUsize>,
    pub bots_client: Arc<BotsClient>,
    pub task_orchestrator_addr: Addr<TaskOrchestratorActor>,
    pub debug_enabled: bool,
    pub client_message_tx: mpsc::UnboundedSender<ClientMessage>,
    pub client_message_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ClientMessage>>>,
    pub ontology_pipeline_service:
        Option<Arc<crate::services::ontology_pipeline_service::OntologyPipelineService>>,
    /// Health degradation reason. `None` means healthy; `Some(reason)` means degraded.
    /// Uses `std::sync::RwLock` (not tokio) so it can be read synchronously in health checks.
    pub degraded_reason: Arc<std::sync::RwLock<Option<String>>>,

    /// Shared per-node analytics data populated by GPU analytics actors.
    /// Maps node_id -> NodeAnalytics{cluster_id, community_id, anomaly, centrality}.
    /// Read by the binary broadcast path to fill V3 analytics fields.
    pub node_analytics: Arc<
        std::sync::RwLock<
            std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
        >,
    >,

    /// Shared per-node SSSP distances populated by ShortestPathActor after a
    /// ComputeSSP run. Maps compact node_id -> (distance, parent_id). Read by the
    /// binary broadcast path to fill V3 wire slot 28 (sssp_distance@28). Absent
    /// nodes default to (INFINITY, -1).
    pub node_sssp: Arc<std::sync::RwLock<std::collections::HashMap<u32, (f32, i32)>>>,
}

impl AppState {
    pub async fn new(
        settings: AppFullSettings,
        github_client: Arc<GitHubClient>,
        content_api: Arc<ContentAPI>,
        perplexity_service: Option<Arc<PerplexityService>>,
        ragflow_service: Option<Arc<RAGFlowService>>,
        speech_service: Option<Arc<SpeechService>>,
        ragflow_session_id: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        info!("[AppState::new] Initializing actor system");
        tokio::time::sleep(Duration::from_millis(50)).await;

        // PRD-022 WS-1: seed the process-wide SHACL ingest-gate mode from
        // config (`ontology_agent.shacl_mode`, default "enforcing"). This is the
        // single source of truth read by both the ingest pipeline and the
        // trust-status endpoint. Enforcing = a `sh:Violation` finding rejects
        // the write; advisory = findings are logged but the write proceeds.
        {
            use visionclaw_ontology::services::jsonld_ingest::shacl_gate::{
                set_global_gate_mode, GateMode,
            };
            let configured = settings
                .ontology_agent
                .as_ref()
                .map(|o| o.shacl_mode.as_str())
                .unwrap_or("enforcing");
            let mode = GateMode::from_config_str(configured);
            set_global_gate_mode(mode);
            info!(
                "[AppState::new] SHACL ingest gate mode: {} (from ontology_agent.shacl_mode={:?})",
                mode.as_str(),
                configured
            );
        }

        info!("[AppState::new] Creating repository adapters for hexagonal architecture (ADR-11 Oxigraph)");

        // Open Oxigraph store — shared across ontology + graph repositories (ADR-11 §D1)
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        let oxigraph_path = std::path::Path::new(&data_dir).join("oxigraph");
        let onto_repo = Arc::new(
            OxigraphOntologyRepository::open(&oxigraph_path)
                .await
                .map_err(|e| format!("Failed to open Oxigraph store: {}", e))?,
        );
        let oxigraph_store = onto_repo.store().clone();
        let graph_repo = Arc::new(OxigraphGraphRepository::from_store(oxigraph_store));

        // SQLite settings repository (ADR-11 §D5)
        let settings_db_path = std::path::Path::new(&data_dir).join("settings.sqlite3");
        let sqlite_settings_repo = Arc::new(
            SqliteSettingsRepository::open(&settings_db_path)
                .await
                .map_err(|e| format!("Failed to open SQLite settings: {}", e))?,
        );
        let settings_repository: Arc<dyn SettingsRepository> = sqlite_settings_repo.clone();
        let sqlite_settings_repository = sqlite_settings_repo;
        let ontology_repository = onto_repo;
        let graph_adapter = graph_repo;

        // WS-9: durable EnrichmentProposal store — SEPARATE db file so its
        // single-writer posture + migrations stay isolated from settings.sqlite3.
        let enrichment_db_path = std::path::Path::new(&data_dir).join("enrichment.sqlite3");
        let sqlite_enrichment_repository = Arc::new(
            SqliteEnrichmentRepository::open(&enrichment_db_path)
                .await
                .map_err(|e| format!("Failed to open SQLite enrichment store: {}", e))?,
        );
        // Install the process-global handle so the WS-12 broker read accessors
        // (`enrichment_proposals_handler::store::{all,get}`) reach the SAME store.
        crate::handlers::enrichment_proposals_handler::store::init(
            sqlite_enrichment_repository.clone(),
        );

        // RES-a: durable LivenessCanary store — SEPARATE db file so its
        // single-writer posture stays isolated. The harness seeds the P0-wave
        // canaries at boot (idempotent) so the KG watchdog's target exists.
        let liveness_db_path = std::path::Path::new(&data_dir).join("liveness.sqlite3");
        let canary_repository = Arc::new(
            crate::adapters::SqliteCanaryRepository::open(&liveness_db_path)
                .await
                .map_err(|e| format!("Failed to open SQLite liveness store: {}", e))?,
        );
        let liveness_harness = Arc::new(crate::services::liveness_harness::LivenessHarness::new(
            canary_repository,
        ));
        if let Err(e) = liveness_harness.seed_p0_canaries().await {
            warn!("[AppState::new] failed to seed P0 liveness canaries: {}", e);
        }
        // COM-15 (PRD-023 WP-5): register the standing governed-voice-loop canary
        // so `GET /api/canary/status` carries CANARY-VC-COM15-PTT from boot.
        if let Err(e) = liveness_harness.seed_p1_canaries().await {
            warn!("[AppState::new] failed to seed P1 liveness canaries: {}", e);
        }
        // P2 rows (PRD-023 WP-9/WP-10/WP-12): the MR copresence canaries
        // (CANARY-VC-M4-RAY, CANARY-VC-COM18-INTERV — fired by the xr-runtime
        // sidecar session over the shared observe path; M1-HUD rides P0), the
        // one-shot voice-repair canary (CANARY-VC-V3-REPAIR), and the loop/trace
        // canaries (CANARY-VC-REC10-LOOP, CANARY-VC-REC11-TRACE).
        if let Err(e) = liveness_harness.seed_p2_canaries().await {
            warn!("[AppState::new] failed to seed P2 liveness canaries: {}", e);
        }

        // REC-4 (ADR-130 Decision 5): durable KPI snapshot + lineage store —
        // SEPARATE db file so its single-writer posture stays isolated. The
        // compute service reads the enrichment decision store + the agent-event
        // volume (tapped from the hub in main.rs) and fires CANARY-VC-REC4-KPI.
        let kpi_db_path = std::path::Path::new(&data_dir).join("kpi.sqlite3");
        let sqlite_kpi_repository = Arc::new(
            SqliteKpiRepository::open(&kpi_db_path)
                .await
                .map_err(|e| format!("Failed to open SQLite KPI store: {}", e))?,
        );
        let kpi_compute_service = Arc::new(crate::services::kpi_compute::KpiComputeService::new(
            sqlite_kpi_repository.clone(),
            sqlite_enrichment_repository.clone(),
            liveness_harness.clone(),
        ));

        // COM-15 / D6 / M5: the previously consumerless AudioRouter now gets a
        // consumer — it holds the per-user selected-agent PTT binding the
        // governed voice loop reads (superseding the global-toggle-only path).
        let audio_router = Arc::new(crate::services::audio_router::AudioRouter::new());
        // COM-15 / V1: the governed-voice-loop consumer. `None` when unconfigured
        // (no AGENTBOX_VOICE_INTENT_URL/ACSP_PANEL_NOSTR_PRIVKEY) — the loop stays
        // off and a spoken command falls back to the settings assistant, honestly.
        let voice_intent_client =
            crate::services::voice_intent_client::VoiceIntentClient::from_env();
        if voice_intent_client.is_some() {
            info!("[AppState::new] governed voice loop enabled (VoiceIntentClient configured)");
        } else {
            info!("[AppState::new] governed voice loop off (VoiceIntentClient unconfigured) — spoken commands use the settings assistant");
        }
        // V3 (PRD-023 WP-10): the conversational-repair store + confidence gate.
        let voice_context_manager =
            Arc::new(crate::services::voice_context_manager::VoiceContextManager::new());
        let clarification_gate =
            crate::services::voice_clarification::ClarificationGate::from_env();
        info!(
            "[AppState::new] voice confidence gate threshold = {:.2}",
            clarification_gate.threshold()
        );

        info!("[AppState::new] Oxigraph + SQLite repositories initialized successfully");
        info!("[AppState::new] Database and settings service initialized successfully");

        // Create ontology pipeline service with semantic physics
        info!("[AppState::new] Creating ontology pipeline service");
        let mut pipeline_service =
            crate::services::ontology_pipeline_service::OntologyPipelineService::new(
                crate::services::ontology_pipeline_service::SemanticPhysicsConfig::default(),
            );

        // CRITICAL: Set graph repository for IRI → node ID resolution (Oxigraph, ADR-11)
        pipeline_service.set_graph_repository(graph_adapter.clone()
            as Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>);

        let ontology_pipeline_service = Some(Arc::new(pipeline_service));

        info!("[AppState::new] Initializing GitHubSyncService for data ingestion");

        let enhanced_content_api = Arc::new(EnhancedContentAPI::new(github_client.clone()));
        // ADR-2114: ingest reads through the CorpusSource port — the mounted
        // vault by default, GitHub when CORPUS_SOURCE=github.
        let corpus_source =
            crate::services::corpus_source::source_from_env_with_github(enhanced_content_api);
        let github_sync_service = Arc::new(GitHubSyncService::new(
            corpus_source,
            graph_adapter.clone()
                as Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>,
            ontology_repository.clone(),
            sqlite_settings_repository.clone(),
        ));

        info!("[AppState::new] Starting GitHub data sync in background (non-blocking)...");

        let sync_service_clone = github_sync_service.clone();

        // Will be initialized before spawn
        let graph_service_addr_ref: std::sync::Arc<
            tokio::sync::Mutex<Option<Addr<GraphServiceSupervisor>>>,
        > = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let graph_service_addr_clone_for_sync = graph_service_addr_ref.clone();

        // Signals that the post-sync Oxigraph reload of GraphStateActor has been
        // requested. The reload resets GraphStateActor positions to the stored
        // layout, clobbering the dual-graph projection applied at boot. A
        // dedicated task (below) waits on this flag and re-emits a projected
        // ForceFullBroadcast so a fresh boot WITHOUT a connected client still
        // serves separated/compressed positions on the REST polling path.
        let sync_reloaded = Arc::new(AtomicBool::new(false));
        let sync_reloaded_for_task = sync_reloaded.clone();

        let sync_handle = tokio::spawn(async move {
            info!("Background GitHub sync task spawned successfully");
            debug!("Task ID: {:?}", std::thread::current().id());
            info!("Starting sync_graphs() execution...");

            info!("Calling sync_service.sync_graphs()...");
            let sync_start = std::time::Instant::now();

            match sync_service_clone.sync_graphs().await {
                Ok(stats) => {
                    let elapsed = sync_start.elapsed();
                    info!("GitHub sync complete! (elapsed: {:?})", elapsed);
                    debug!("  Total files scanned: {}", stats.total_files);
                    debug!("  Knowledge graph files: {}", stats.kg_files_processed);
                    debug!("  Ontology files: {}", stats.ontology_files_processed);
                    debug!("  Duration: {:?}", stats.duration);
                    if !stats.errors.is_empty() {
                        warn!("  ⚠️  Errors encountered: {}", stats.errors.len());
                        for (i, error) in stats.errors.iter().enumerate().take(5) {
                            warn!("    {}. {}", i + 1, error);
                        }
                        if stats.errors.len() > 5 {
                            warn!("    ... and {} more errors", stats.errors.len() - 5);
                        }
                    }

                    // Load synced data into graph actor (if it's ready)
                    if let Some(graph_addr) = &*graph_service_addr_clone_for_sync.lock().await {
                        info!("[GitHub Sync] Notifying GraphServiceActor to reload synced data...");
                        graph_addr.do_send(crate::actors::messages::ReloadGraphFromDatabase);
                        info!("[GitHub Sync] Reload notification sent to GraphServiceActor");
                        sync_reloaded.store(true, Ordering::SeqCst);
                    } else {
                        info!("[GitHub Sync] Graph service not yet initialized - will load on startup");
                    }
                }
                Err(e) => {
                    let elapsed = sync_start.elapsed();
                    log::error!(
                        "❌ Background GitHub sync failed after {:?}: {}",
                        elapsed,
                        e
                    );
                    log::error!("❌ Error details: {:?}", e);
                    log::error!(
                        "⚠️  Databases may have partial data - use manual import API if needed"
                    );
                }
            }
        });

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            debug!("GitHub sync monitor: Checking task status...");

            // A full re-sync fetches content for every file (no SHA1 skip):
            // ~5.5 min for the ~5.5k dual-source corpus, and longer when the
            // working graph grows. 300s tripped a false "deadlock" on every
            // full sync. The task is detached, so this monitor only logs — it
            // never aborts the sync — but the alarm was misleading.
            let timeout_duration = Duration::from_secs(1200);
            match tokio::time::timeout(timeout_duration, sync_handle).await {
                Ok(join_result) => match join_result {
                    Ok(_sync_result) => {
                        debug!("GitHub sync monitor: Task completed successfully");
                    }
                    Err(join_error) => {
                        if join_error.is_cancelled() {
                            log::error!("GitHub sync monitor: Task was CANCELLED");
                        } else if join_error.is_panic() {
                            log::error!("GitHub sync monitor: Task PANICKED");
                            log::error!("JoinError details: {:?}", join_error);
                        } else {
                            log::error!("GitHub sync monitor: Task failed with unknown error");
                            log::error!("JoinError: {:?}", join_error);
                        }
                    }
                },
                Err(_timeout_error) => {
                    log::error!(
                        "GitHub sync monitor: Task TIMED OUT after {:?}",
                        timeout_duration
                    );
                    log::error!(
                        "This likely indicates a deadlock or infinite loop in sync_graphs()"
                    );
                }
            }

            debug!("GitHub sync monitor: Monitoring complete");
        });

        info!("[AppState::new] GitHub sync running in background with enhanced monitoring, proceeding with actor initialization");

        // Create shared node analytics map early so it can be shared with ClientCoordinatorActor
        let node_analytics: Arc<
            std::sync::RwLock<
                std::collections::HashMap<u32, crate::utils::binary_protocol::NodeAnalytics>,
            >,
        > = Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));

        // ADR-031 D2b: shared SSSP map fed by ShortestPathActor, read by the
        // binary broadcast path to fill wire slot 28.
        let node_sssp: Arc<std::sync::RwLock<std::collections::HashMap<u32, (f32, i32)>>> =
            Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));

        info!("[AppState::new] Starting ClientCoordinatorActor");
        let mut client_coordinator = ClientCoordinatorActor::new();
        client_coordinator.set_settings_repository(sqlite_settings_repository.clone());
        client_coordinator.set_node_analytics(node_analytics.clone());
        let client_manager_addr = client_coordinator.start();

        // ADR-059 Phase 2b: spawn the agent-embodiment beam actor once at startup.
        // It subscribes to the process-global agent-events hub and fans encoded
        // 0x23 frames out through the ClientCoordinatorActor's existing binary
        // dispatch. Mirrors the fire-once child-actor spawn pattern used by the
        // physics supervisor (physics_supervisor.rs spawn_child_actors). The
        // returned Addr is intentionally dropped: the actor is kept alive by its
        // hub stream subscription, not by an external handle.
        info!("[AppState::new] Starting AgentBeamActor (ADR-059 Phase 2b)");
        AgentBeamActor::new(client_manager_addr.clone()).start();

        // Read persisted physics from SQLite (same source the API GET uses) so a fresh
        // boot applies runtime-persisted controls (graph_separation_x, enable_dual_disc_layout,
        // adaptive_speed, etc). Fall back to YAML/defaults when nothing is persisted yet.
        let physics_settings = {
            use crate::ports::settings_repository::SettingValue;
            // When SQLite has no usable "physics" row, seed it from the canonical
            // PhysicsSettings::default() and write it back, so the persisted store
            // and the live GPU actor share one source of truth (graph_separation_x=100,
            // enable_dual_disc_layout=false → natural 3D). Without this, a
            // fresh boot served defaults to the actor but left SQLite empty/stale,
            // so GET /api/settings/physics could report a different separation than
            // the running simulation.
            let seed_canonical = |reason: &str| -> crate::config::PhysicsSettings {
                let default_physics = crate::config::PhysicsSettings::default();
                info!(
                    "[AppState::new] {} — seeding SQLite physics from canonical default (graph_separation_x={}, enable_dual_disc_layout={})",
                    reason, default_physics.graph_separation_x, default_physics.enable_dual_disc_layout
                );
                default_physics
            };

            let resolved = match sqlite_settings_repository.get_setting("physics").await {
                Ok(Some(SettingValue::Json(json))) => {
                    match serde_json::from_value::<crate::config::PhysicsSettings>(json) {
                        Ok(persisted) => {
                            info!("[AppState::new] Loaded persisted physics from SQLite (graph_separation_x={}, enable_dual_disc_layout={}, adaptive_speed={})", persisted.graph_separation_x, persisted.enable_dual_disc_layout, persisted.adaptive_speed);
                            (persisted, false)
                        }
                        Err(e) => (
                            seed_canonical(&format!(
                                "Persisted physics JSON failed to deserialize ({})",
                                e
                            )),
                            true,
                        ),
                    }
                }
                Ok(Some(_)) => (
                    seed_canonical("Persisted physics row had a non-JSON value"),
                    true,
                ),
                Ok(None) => (seed_canonical("No persisted physics row in SQLite"), true),
                Err(e) => (
                    seed_canonical(&format!(
                        "Failed to read persisted physics from SQLite ({})",
                        e
                    )),
                    true,
                ),
            };

            let (physics, needs_seed) = resolved;
            if needs_seed {
                match serde_json::to_value(&physics) {
                    Ok(physics_json) => {
                        if let Err(e) = sqlite_settings_repository
                            .set_setting(
                                "physics",
                                SettingValue::Json(physics_json),
                                Some("Physics simulation settings (canonical default, boot seed)"),
                            )
                            .await
                        {
                            warn!(
                                "[AppState::new] Failed to seed canonical physics into SQLite: {}",
                                e
                            );
                        } else {
                            info!("[AppState::new] Seeded canonical physics default into SQLite (first boot / stale row)");
                        }
                    }
                    Err(e) => warn!(
                        "[AppState::new] Failed to serialize canonical physics for boot seed: {}",
                        e
                    ),
                }
            }
            physics
        };

        info!("[AppState::new] Starting MetadataActor");
        let metadata_addr = MetadataActor::new(MetadataStore::new()).start();

        info!("[AppState::new] Starting GraphServiceSupervisor (refactored architecture)");

        // GraphServiceSupervisor uses the Oxigraph-backed graph repository (ADR-11)
        let graph_service_addr = GraphServiceSupervisor::new(graph_adapter.clone()
            as Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>)
        .start();

        // Live linkage: rebind the supervisor's client coordinator to THIS
        // AppState's canonical instance (the one SocketFlowServer registers
        // clients with). The supervisor's own ClientCoordinator child has an
        // empty registry — broadcasts through it (graphUpdated, forwarded
        // BroadcastMessage) would reach nobody. Mailbox ordering guarantees
        // this lands after the supervisor's initialize_actors self-wiring.
        graph_service_addr.do_send(
            crate::actors::graph_service_supervisor::SetClientCoordinatorAddr {
                addr: client_manager_addr.clone(),
            },
        );

        // Store graph service address in Arc for GitHub sync task to use
        let graph_service_addr_clone = graph_service_addr.clone();
        tokio::spawn(async move {
            let mut addr_guard = graph_service_addr_ref.lock().await;
            *addr_guard = Some(graph_service_addr_clone);
            info!("[AppState::new] GitHub sync task notified - graph service address available");
        });

        info!("[AppState::new] Retrieving GraphStateActor from GraphServiceSupervisor for CQRS");
        let graph_actor_addr = graph_service_addr
            .send(crate::actors::messages::GetGraphStateActor)
            .await
            .map_err(|e| format!("Failed to send GetGraphStateActor message: {}", e))?
            .ok_or_else(|| "GraphStateActor not initialized in supervisor".to_string())?;

        // Create ActorGraphRepository using the graph actor (Oxigraph-backed, ADR-11)
        let graph_repository = Arc::new(crate::adapters::ActorGraphRepository::new(
            graph_actor_addr.clone(),
        ));

        // Load existing data from Oxigraph into repository cache on startup
        info!("[AppState::new] Loading graph data from Oxigraph store into repository cache...");
        // Migrated to SPARQL — see OxigraphGraphRepository (ADR-11)
        {
            use crate::ports::graph_repository::GraphRepository;
            match graph_adapter.get_graph().await {
                Ok(g) => info!("[AppState::new] Oxigraph store has {} nodes", g.nodes.len()),
                Err(e) => warn!(
                    "[AppState::new] Could not count Oxigraph nodes at startup: {}",
                    e
                ),
            }
        }

        // Get node count via actor graph repository
        let node_count = {
            use crate::ports::graph_repository::GraphRepository;
            graph_repository
                .get_graph()
                .await
                .map(|g| g.nodes.len())
                .unwrap_or(0)
        };
        info!(
            "[AppState::new] Graph data loaded from Oxigraph store ({} nodes)",
            node_count
        );

        info!("[AppState::new] Initializing CQRS query handlers for graph domain");
        let graph_query_handlers = GraphQueryHandlers {
            get_graph_data: Arc::new(GetGraphDataHandler::new(graph_repository.clone())),
            get_node_map: Arc::new(GetNodeMapHandler::new(graph_repository.clone())),
            get_physics_state: Arc::new(GetPhysicsStateHandler::new(graph_repository.clone())),
            get_auto_balance_notifications: Arc::new(GetAutoBalanceNotificationsHandler::new(
                graph_repository.clone(),
            )),
            get_bots_graph_data: Arc::new(GetBotsGraphDataHandler::new(graph_repository.clone())),
            get_constraints: Arc::new(GetConstraintsHandler::new(graph_repository.clone())),
            get_equilibrium_status: Arc::new(GetEquilibriumStatusHandler::new(
                graph_repository.clone(),
            )),
            compute_shortest_paths: Arc::new(ComputeShortestPathsHandler::new(
                graph_repository.clone(),
            )),
        };

        let event_bus = Arc::new(RwLock::new(EventBus::new()));

        // Initialize EventStore with file-backed repository (configurable via env)
        let event_store_path = std::env::var("EVENT_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/visionclaw-events"));
        let event_store = Arc::new(EventStore::with_file_backend(event_store_path.clone()));
        info!(
            "[AppState::new] EventStore initialized with file backend at {:?}",
            event_store_path
        );

        // Register event handlers on the EventBus
        {
            use crate::events::handlers::{
                AuditEventHandler, GraphEventHandler, NotificationEventHandler,
                OntologyEventHandler,
            };

            let bus = event_bus.write().await;
            let audit_handler = Arc::new(AuditEventHandler::new("global-audit"));
            let notification_handler =
                Arc::new(NotificationEventHandler::new("global-notifications"));
            let graph_handler = Arc::new(GraphEventHandler::new("global-graph"));
            let ontology_handler = Arc::new(OntologyEventHandler::new("global-ontology"));
            bus.subscribe(audit_handler).await;
            bus.subscribe(notification_handler).await;
            bus.subscribe(graph_handler).await;
            bus.subscribe(ontology_handler).await;
            info!("[AppState::new] EventBus handlers registered: AuditEventHandler, NotificationEventHandler, GraphEventHandler, OntologyEventHandler");
        }

        // Log warnings for settings that are present in config but not yet wired
        {
            let net = &settings.system.network;
            let sec = &settings.system.security;

            if net.enable_tls {
                warn!("[Settings] system.network.enableTls is true but TLS termination is not implemented server-side. Use a reverse proxy for TLS.");
            }
            if net.enable_http2 {
                warn!("[Settings] system.network.enableHttp2 is true but HTTP/2 is not implemented server-side. Use a reverse proxy for HTTP/2.");
            }
            if net.enable_rate_limiting {
                warn!("[Settings] system.network.enableRateLimiting is true but rate limiting middleware is not yet wired. These values have no effect: rateLimitRequests={}, rateLimitWindow={}s",
                    net.rate_limit_requests, net.rate_limit_window);
            }
            if net.enable_metrics {
                warn!("[Settings] system.network.enableMetrics is true but metrics endpoint on port {} is not yet implemented.", net.metrics_port);
            }
            if sec.enable_request_validation {
                warn!("[Settings] system.security.enableRequestValidation is true but request validation middleware is not yet wired.");
            }
        }

        info!("[AppState::new] Linking ClientCoordinatorActor to GraphServiceSupervisor for settling fix");

        let graph_supervisor_clone = graph_service_addr.clone();
        let client_manager_clone = client_manager_addr.clone();
        actix::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Set the GraphServiceSupervisor address in ClientManagerActor
            info!("Setting GraphServiceSupervisor address in ClientManagerActor");
            client_manager_clone.do_send(crate::actors::messages::SetGraphServiceAddress {
                addr: graph_supervisor_clone.clone(),
            });
        });

        let gpu_manager_addr = {
            info!("[AppState::new] Starting GPUManagerActor (modular architecture)");
            let gpu_manager = GPUManagerActor::new().start();

            // ADR-2053: the shared SSSP map goes to the SUPERVISED ShortestPathActor,
            // via GPUManagerActor -> GraphAnalyticsSupervisor. This used to start a
            // second, standalone ShortestPathActor/ConnectedComponentsActor pair here
            // and send the map to that one — but ResourceSupervisor distributes
            // SharedGPUContext only to the subsystem supervisors, so the standalone
            // pair never received a context and always took the GPU-absent arm. Every
            // /api/analytics pathfinding route addressed that GPU-blind pair.
            // ADR-031 D2b: without this map, wire slot 28 stops publishing per-node
            // SSSP distances.
            gpu_manager.do_send(crate::actors::messages::SetNodeSSSP {
                node_sssp: node_sssp.clone(),
            });
            info!("[AppState::new] Sent SetNodeSSSP to GPUManagerActor for the supervised ShortestPathActor");

            // ADR-014 DL4 fix: Send shared node_analytics to GPUManagerActor so it reaches
            // ClusteringActor and AnomalyDetectionActor via AnalyticsSupervisor.
            gpu_manager.do_send(crate::actors::messages::SetNodeAnalytics {
                node_analytics: node_analytics.clone(),
            });
            info!("[AppState::new] Sent SetNodeAnalytics to GPUManagerActor");

            // PRD-018 WS-3 / ADR-098 D1: register the GPUManagerActor with the
            // GitHubSyncService so post-sync reasoning can push materialised OWL
            // axioms to the live-kernel constraint mapper. The background sync
            // (spawned above) takes minutes; this address is set well before its
            // post-reasoning phase fires, so semantic constraints reach the GPU.
            github_sync_service.set_gpu_manager_addr(gpu_manager.clone());
            info!("[AppState::new] Registered GPUManagerActor with GitHubSyncService for semantic constraint dispatch");

            Some(gpu_manager)
        };

        // Create shared gpu_compute_addr that will be populated asynchronously
        let gpu_compute_addr: Arc<RwLock<Option<Addr<gpu::ForceComputeActor>>>> =
            Arc::new(RwLock::new(None));

        {
            use crate::actors::messages::{GetForceComputeActor, InitializeGPUConnection};

            info!("[AppState] Initializing GPU connection with GPUManagerActor for proper message delegation");
            if let Some(ref gpu_manager) = gpu_manager_addr {
                // Use a delayed spawn to avoid mailbox contention with ontology sync.
                // The ontology pipeline floods GraphServiceSupervisor's mailbox at startup;
                // do_send silently drops InitializeGPUConnection when the mailbox is full.
                // We retry with .send() (which awaits capacity) after a short delay.
                let graph_service_for_gpu = graph_service_addr.clone();
                let gpu_manager_for_conn = gpu_manager.clone();
                actix::spawn(async move {
                    // Wait for initial ontology sync to drain
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    info!(
                        "[AppState] Sending InitializeGPUConnection (delayed, post-ontology-sync)"
                    );
                    match graph_service_for_gpu
                        .send(InitializeGPUConnection {
                            gpu_manager: Some(gpu_manager_for_conn),
                        })
                        .await
                    {
                        Ok(_) => info!("[AppState] InitializeGPUConnection delivered successfully"),
                        Err(e) => error!(
                            "[AppState] Failed to deliver InitializeGPUConnection: {}",
                            e
                        ),
                    }
                });

                // Spawn async task to get ForceComputeActor address after actors are ready,
                // then register deferred CQRS physics handlers.
                let gpu_manager_clone = gpu_manager.clone();
                let gpu_compute_addr_clone = gpu_compute_addr.clone();
                let gpu_compute_addr_for_rebroadcast = gpu_compute_addr.clone();
                // Persisted physics params must reach ForceComputeActor on boot.
                // The actor initialises with default SimulationParams (graph_separation_x=0,
                // enable_dual_disc_layout=false, adaptive_speed off); without this push the persisted
                // values only take effect after a live PUT that differs from the defaults,
                // making the separation/compression/adaptive-speed controls appear dead.
                let startup_sim_params =
                    crate::models::simulation_params::SimulationParams::from(&physics_settings);

                // Re-project after the post-sync Oxigraph reload. The reload resets
                // GraphStateActor to the stored (un-separated) layout AFTER the boot
                // projection broadcast; without this, a fresh boot with no connected
                // client serves un-separated positions on the REST polling path until
                // a client PUTs settings. The GPU still holds the converged+projected
                // state, so a ForceFullBroadcast re-emits projected UpdateNodePositions
                // and re-syncs GraphStateActor.
                actix::spawn(async move {
                    // Wait for the sync task to request the reload (bounded).
                    let mut waited_ms = 0u64;
                    while !sync_reloaded_for_task.load(Ordering::SeqCst) && waited_ms < 180_000 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        waited_ms += 500;
                    }
                    if !sync_reloaded_for_task.load(Ordering::SeqCst) {
                        debug!("[AppState] Post-sync re-broadcast skipped — sync reload not signalled within timeout");
                        return;
                    }
                    // Settle margin: let the GraphStateActor reload + any GPU re-upload
                    // churn finish (reload observed ~4s after the notification).
                    tokio::time::sleep(tokio::time::Duration::from_secs(8)).await;
                    let addr = { gpu_compute_addr_for_rebroadcast.read().await.clone() };
                    if let Some(addr) = addr {
                        addr.do_send(crate::actors::gpu::force_compute_actor::ForceFullBroadcast);
                        info!("[AppState] Sent post-sync ForceFullBroadcast to re-project GraphStateActor positions");
                    } else {
                        warn!("[AppState] Post-sync re-broadcast skipped — ForceComputeActor address not available");
                    }
                });

                actix::spawn(async move {
                    // Wait for GPUManagerActor and PhysicsSupervisor to fully initialize
                    // This delay allows the actor hierarchy to be established:
                    // GPUManagerActor -> ResourceSupervisor -> GPU init -> PhysicsSupervisor -> ForceComputeActor
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

                    // Retry loop to get ForceComputeActor address
                    let max_retries = 10;
                    let retry_delay = tokio::time::Duration::from_millis(500);
                    let mut gpu_ready = false;

                    for attempt in 1..=max_retries {
                        debug!("[AppState] Querying GPUManagerActor for ForceComputeActor (attempt {}/{})",
                               attempt, max_retries);

                        match gpu_manager_clone.send(GetForceComputeActor).await {
                            Ok(Ok(force_compute_actor)) => {
                                info!("[AppState] Successfully obtained ForceComputeActor address on attempt {}", attempt);
                                // Push persisted physics params so layout controls
                                // (graph_separation_x, enable_dual_disc_layout, adaptive_speed)
                                // are live from boot, not just after the first live PUT.
                                force_compute_actor.do_send(
                                    crate::actors::messages::UpdateSimulationParams {
                                        params: startup_sim_params.clone(),
                                    },
                                );
                                info!(
                                    "[AppState] Pushed persisted SimulationParams to ForceComputeActor (graph_separation_x={}, enable_dual_disc_layout={}, adaptive_speed={})",
                                    startup_sim_params.graph_separation_x,
                                    startup_sim_params.enable_dual_disc_layout,
                                    startup_sim_params.adaptive_speed
                                );
                                let mut guard = gpu_compute_addr_clone.write().await;
                                *guard = Some(force_compute_actor);
                                info!("[AppState] ForceComputeActor address stored - GPU physics now available via AppState");
                                gpu_ready = true;
                                break;
                            }
                            Ok(Err(e)) => {
                                debug!("[AppState] ForceComputeActor not ready yet: {} (attempt {}/{})",
                                       e, attempt, max_retries);
                            }
                            Err(e) => {
                                warn!("[AppState] Failed to communicate with GPUManagerActor: {} (attempt {}/{})",
                                      e, attempt, max_retries);
                            }
                        }

                        if attempt < max_retries {
                            tokio::time::sleep(retry_delay).await;
                        }
                    }

                    if !gpu_ready {
                        warn!("[AppState] Failed to obtain ForceComputeActor after {} attempts - HTTP handlers will use fallback paths",
                              max_retries);
                    }
                });
            } else {
                warn!("[AppState] GPUManagerActor not available - GPU physics will be disabled");
            }
        }

        // Register AppState's gpu_compute_addr with GraphServiceSupervisor
        // so its 10s periodic refresh also updates AppState when ForceComputeActor is respawned.
        {
            let supervisor_addr = graph_service_addr.clone();
            let gpu_addr_for_supervisor = gpu_compute_addr.clone();
            supervisor_addr.do_send(crate::actors::messages::SetAppGpuComputeAddr {
                addr: gpu_addr_for_supervisor,
            });
            info!("[AppState] Registered gpu_compute_addr with GraphServiceSupervisor for periodic refresh");
        }

        info!("[AppState::new] Starting OptimizedSettingsActor with repository injection (hexagonal architecture, ADR-11)");

        // Reuse the already-opened SQLite settings repository (ADR-11 §D5)
        let settings_actor = OptimizedSettingsActor::with_actors(
            sqlite_settings_repository.clone() as Arc<dyn SettingsRepository>,
            Some(graph_service_addr.clone()),
            None,
        )
        .map_err(|e| {
            log::error!("Failed to create OptimizedSettingsActor: {}", e);
            e
        })?;
        let settings_addr = settings_actor.start();

        info!("[AppState::new] Starting settings hot-reload watcher");

        info!(
            "[AppState::new] Settings hot-reload watcher DISABLED (was causing database deadlocks)"
        );

        info!("[AppState::new] Starting AgentMonitorActor for MCP monitoring");
        let mcp_host =
            std::env::var("MCP_HOST").unwrap_or_else(|_| "agentic-workstation".to_string());
        let mcp_port = std::env::var("MCP_TCP_PORT")
            .unwrap_or_else(|_| "9500".to_string())
            .parse::<u16>()
            .unwrap_or(9500);

        info!(
            "[AppState::new] AgentMonitorActor will poll MCP at {}:{}",
            mcp_host, mcp_port
        );
        let claude_flow_client =
            visionclaw_domain::types::claude_flow::ClaudeFlowClient::new(mcp_host, mcp_port);
        let agent_monitor_addr =
            AgentMonitorActor::new(claude_flow_client, graph_service_addr.clone()).start();

        let sim_params =
            crate::models::simulation_params::SimulationParams::from(&physics_settings);

        let update_msg = crate::actors::messages::UpdateSimulationParams { params: sim_params };

        graph_service_addr.do_send(update_msg.clone());

        if let Some(ref _gpu_addr) = gpu_manager_addr {}

        info!("[AppState::new] Starting ProtectedSettingsActor");
        let protected_settings_addr =
            ProtectedSettingsActor::new(ProtectedSettings::default()).start();

        info!("[AppState::new] Starting WorkspaceActor");
        let workspace_addr = WorkspaceActor::new().start();

        info!("[AppState::new] Starting OntologyActor");
        let ontology_actor_addr = {
            let mut ontology_actor = OntologyActor::new();
            // Wire GPU manager for constraint pipeline (Fix #2)
            if let Some(ref gpu_mgr) = gpu_manager_addr {
                ontology_actor.set_gpu_manager_addr(gpu_mgr.clone());
                info!("[AppState] OntologyActor wired to GPUManagerActor for constraint pipeline");
            }
            // Wire client coordinator for WebSocket broadcasts (Fix #8)
            ontology_actor.set_client_manager_addr(client_manager_addr.clone());
            info!("[AppState] OntologyActor initialized successfully");
            Some(ontology_actor.start())
        };

        info!("[AppState::new] Initializing BotsClient with graph service");
        let bots_client = Arc::new(BotsClient::with_graph_service(graph_service_addr.clone()));

        // D1 (PRD-023 WP-2): start the agent-list poll at boot. connect() tests MCP
        // reachability, initialises the session, and spawns the 2s polling loop that
        // populates the agents cache behind /api/bots/agents — the beam and avatar
        // layers starve without it. Fail-open with backoff: an unreachable MCP server
        // must never block boot; CANARY-VC-D1-BEAM remains the proof that data flows.
        {
            let bots_client_boot = bots_client.clone();
            tokio::spawn(async move {
                let delays = [5u64, 15, 60, 300];
                let mut attempt = 0usize;
                loop {
                    match bots_client_boot.connect("").await {
                        Ok(()) => {
                            info!(
                                "[AppState] BotsClient poll started at boot (attempt {})",
                                attempt + 1
                            );
                            break;
                        }
                        Err(e) => {
                            let delay = delays[attempt.min(delays.len() - 1)];
                            warn!(
                                "[AppState] BotsClient connect failed (attempt {}): {}; retrying in {}s",
                                attempt + 1, e, delay
                            );
                            tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                            attempt += 1;
                        }
                    }
                }
            });
        }

        info!("[AppState::new] Initializing TaskOrchestratorActor with Management API");
        let mgmt_api_host = std::env::var("MANAGEMENT_API_HOST")
            .unwrap_or_else(|_| "agentic-workstation".to_string());
        let mgmt_api_port = std::env::var("MANAGEMENT_API_PORT")
            .unwrap_or_else(|_| "9090".to_string())
            .parse::<u16>()
            .unwrap_or(9090);
        // SECURITY: Validate all security-critical environment variables at startup
        let mgmt_api_key = validate_security_env_vars()?;

        let mgmt_client = ManagementApiClient::new(mgmt_api_host, mgmt_api_port, mgmt_api_key);
        let task_orchestrator_addr = TaskOrchestratorActor::new(mgmt_client).start();

        // ADR-110: flagship ACSP agentic actor — knowledge elevation through
        // forum governance cases, voice-guided when the local speech stack
        // (Whisper STT / PocketTts TTS) is up. ADR-130 Decision 2: the gate now
        // defaults ON in dev/staging (opt-in in production) and still requires
        // FORUM_RELAY_URL + a panel secret to publish; None means the gate is
        // closed for this profile/config.
        match crate::actors::elevation_actor::ElevationActor::new(
            graph_adapter.clone()
                as Arc<dyn crate::ports::knowledge_graph_repository::KnowledgeGraphRepository>,
            sqlite_enrichment_repository.clone(),
            speech_service.clone(),
            // GOV-7 (ADR-130): base ontology source for the EL++ consistency gate.
            // Always available here, so approvals are gated; a None here would
            // fail the gate closed (block approvals) rather than wave them through.
            Some(ontology_repository.clone()
                as Arc<dyn crate::ports::ontology_repository::OntologyRepository>),
        ) {
            Some(actor) => {
                let _ = actor.start();
                info!("[AppState] ElevationActor started (ACSP knowledge-elevation panel live)");
            }
            None => info!(
                "[AppState] ElevationActor disabled (dev/staging default ON — set ELEVATION_ACTOR_ENABLED=0 to force off, or in production set ELEVATION_ACTOR_ENABLED=1; also requires FORUM_RELAY_URL + ACSP_PANEL_NOSTR_PRIVKEY)"
            ),
        }

        // ADR-110: voice → settings-assistant bridge. Spoken configuration
        // requests reach the same agent the Control Center command box drives;
        // active whenever the local speech stack (Whisper/PocketTts) is up.
        match crate::actors::voice_interface_actor::VoiceInterfaceActor::new(
            task_orchestrator_addr.clone(),
            speech_service.clone(),
        ) {
            Some(actor) => {
                let _ = actor.start();
                info!(
                    "[AppState] VoiceInterfaceActor started (spoken interface configuration live)"
                );
            }
            None => info!("[AppState] VoiceInterfaceActor disabled (no speech service)"),
        }

        info!("[AppState] GPU manager will self-initialize when needed");

        info!("[AppState::new] Actor system initialization complete (GPU initialization sent earlier)");

        let debug_enabled = crate::utils::logging::is_debug_enabled();

        info!("[AppState::new] Debug mode enabled: {}", debug_enabled);

        let (client_message_tx, client_message_rx) = mpsc::unbounded_channel::<ClientMessage>();
        info!("[AppState::new] Client message channel created");

        // Phase 7: Initialize GPU context bus for event-based distribution
        info!("[AppState::new] Creating GPU context bus for subsystem distribution");
        let gpu_context_bus = Arc::new(GPUContextBus::new());

        // Phase 7: Initialize decomposed GPU subsystems
        // These will receive GPU context via the event bus when GPUManagerActor initializes
        let physics = PhysicsSubsystem {
            force_compute: None,
            stress_major: None,
            constraint: None,
        };

        let analytics = AnalyticsSubsystem {
            clustering: None,
            anomaly: None,
            pagerank: None,
        };

        // ADR-2053: the graph-ops children (ShortestPathActor,
        // ConnectedComponentsActor) are owned by GraphAnalyticsSupervisor, which
        // spawns them and gives them a SharedGPUContext. AppState no longer holds
        // their addresses — it previously held a SECOND, standalone pair that never
        // received a context. This inventory is therefore empty by construction, and
        // `GraphAnalyticsSupervisor`'s `GetSubsystemHealth` is the live health source.
        let graph_ops = GraphSubsystem::default();

        info!(
            "[AppState::new] GPU subsystems initialized (physics={}, analytics={}, graph_ops={} — graph-ops children are supervisor-owned, see ADR-2053)",
            physics.active_count(),
            analytics.active_count(),
            graph_ops.active_count()
        );

        // gap-close item 2 (ADR-130 Decision 2): connect the shared ACSP client
        // that projects REST/bridge decisions back to the forum as kind-31403.
        // Same identity + relay as ElevationActor (ACSP_PANEL_NOSTR_PRIVKEY /
        // VISIONCLAW_NOSTR_PRIVKEY + FORUM_RELAY_URL). Unconfigured or failed
        // connect ⇒ None, and the decide path records forum_projection=skipped.
        let acsp_client = {
            let relay = std::env::var("FORUM_RELAY_URL").ok();
            let secret = std::env::var("ACSP_PANEL_NOSTR_PRIVKEY")
                .or_else(|_| std::env::var("VISIONCLAW_NOSTR_PRIVKEY"))
                .ok();
            match (relay, secret) {
                (Some(relay), Some(secret)) => {
                    match crate::services::acsp::AcspClient::connect(&secret, &relay).await {
                        Ok(c) => {
                            info!("[AppState] ACSP decision-projection client connected to {} — REST/bridge decisions project to the forum as kind-31403", relay);
                            Some(Arc::new(c))
                        }
                        Err(e) => {
                            warn!("[AppState] ACSP decision-projection client FAILED to connect ({}) — operator/bridge decisions will record forum_projection=skipped (degraded, visible)", e);
                            None
                        }
                    }
                }
                _ => {
                    info!("[AppState] ACSP decision-projection client OFF (FORUM_RELAY_URL + ACSP_PANEL_NOSTR_PRIVKEY/VISIONCLAW_NOSTR_PRIVKEY unset) — REST/bridge decisions record forum_projection=skipped");
                    None
                }
            }
        };

        let state = Self {
            graph_service_addr,
            gpu_manager_addr,
            gpu_compute_addr, // Now Arc<RwLock<Option<...>>>, populated asynchronously
            // ADR-2053: always None. This was the second slot of the tuple removed
            // above and was already a literal `None` before that change — a comment
            // there claimed the actor "will be available after GPU initialization",
            // which never happened because nothing ever assigned it. It is kept as a
            // field only because the readiness inventory below reports on it; the
            // real StressMajorizationActor is a PhysicsSupervisor child.
            stress_majorization_addr: None,

            // Phase 7: Decomposed subsystems
            physics,
            analytics,
            graph_ops,
            gpu_context_bus,

            settings_repository,
            sqlite_settings_repository,
            sqlite_enrichment_repository,
            acsp_client,
            liveness_harness,
            sqlite_kpi_repository,
            kpi_compute_service,

            graph_adapter,

            ontology_repository,

            graph_repository,
            graph_query_handlers,

            event_bus,
            event_store,

            settings_addr,
            protected_settings_addr,
            metadata_addr,
            client_manager_addr,
            agent_monitor_addr,
            workspace_addr,
            ontology_actor_addr,
            github_client,
            content_api,
            perplexity_service,
            ragflow_service,
            speech_service,
            audio_router,
            voice_intent_client,
            voice_context_manager,
            clarification_gate,
            nostr_service: None,
            feature_access: web::Data::new(FeatureAccess::from_env()),
            ragflow_session_id,
            active_connections: Arc::new(AtomicUsize::new(0)),
            bots_client,
            task_orchestrator_addr,
            debug_enabled,
            client_message_tx,
            client_message_rx: Arc::new(tokio::sync::Mutex::new(client_message_rx)),
            ontology_pipeline_service,
            degraded_reason: Arc::new(std::sync::RwLock::new(None)),
            node_analytics,
            node_sssp,
        };

        // Validate optional actor addresses
        info!("[AppState::new] Validating actor initialization");
        let validation_report = state.validate();
        validation_report.log();

        if !validation_report.is_valid() {
            return Err(
                format!("AppState validation failed: {:?}", validation_report.errors).into(),
            );
        }

        info!("[AppState::new] All validation checks passed");

        Ok(state)
    }

    /// Validate that all optional actors and services are properly initialized
    /// based on feature flags and environment configuration.
    pub fn validate(&self) -> crate::validation::ValidationReport {
        use crate::validation::*;
        let mut report = ValidationReport::new();

        // GPU-related actors
        {
            report.add(ValidationItem {
                name: "GPUManagerActor".to_string(),
                expected: true,
                present: self.gpu_manager_addr.is_some(),
                severity: Severity::Warning,
                reason: "GPU feature is enabled".to_string(),
            });

            // gpu_compute_addr is populated asynchronously - check via try_read
            let gpu_compute_present = self
                .gpu_compute_addr
                .try_read()
                .map(|guard| guard.is_some())
                .unwrap_or(false);
            report.add(ValidationItem {
                name: "gpu_compute_addr".to_string(),
                expected: false,
                present: gpu_compute_present,
                severity: Severity::Info,
                reason: "Initialized asynchronously after GPU manager starts".to_string(),
            });

            report.add(ValidationItem {
                name: "stress_majorization_addr".to_string(),
                expected: false,
                present: self.stress_majorization_addr.is_some(),
                severity: Severity::Info,
                reason: "Initialized after GPU manager starts".to_string(),
            });
        }

        // Ontology actor
        {
            let present = self.ontology_actor_addr.is_some();
            report.add(ValidationItem {
                name: "OntologyActor".to_string(),
                expected: true,
                present,
                severity: Severity::Warning,
                reason: "Ontology feature is enabled".to_string(),
            });
        }

        // Perplexity service (environment-dependent)
        let perplexity_expected = env_is_set("PERPLEXITY_API_KEY");
        report.add(ValidationItem {
            name: "PerplexityService".to_string(),
            expected: perplexity_expected,
            present: self.perplexity_service.is_some(),
            severity: if perplexity_expected {
                Severity::Warning
            } else {
                Severity::Info
            },
            reason: if perplexity_expected {
                "PERPLEXITY_API_KEY is set".to_string()
            } else {
                "PERPLEXITY_API_KEY not set".to_string()
            },
        });

        // RAGFlow service (environment-dependent)
        let ragflow_expected = env_is_set("RAGFLOW_API_KEY");
        report.add(ValidationItem {
            name: "RAGFlowService".to_string(),
            expected: ragflow_expected,
            present: self.ragflow_service.is_some(),
            severity: if ragflow_expected {
                Severity::Warning
            } else {
                Severity::Info
            },
            reason: if ragflow_expected {
                "RAGFLOW_API_KEY is set".to_string()
            } else {
                "RAGFLOW_API_KEY not set".to_string()
            },
        });

        // Speech service (environment-dependent)
        let speech_expected = env_is_set("SPEECH_SERVICE_ENABLED");
        report.add(ValidationItem {
            name: "SpeechService".to_string(),
            expected: speech_expected,
            present: self.speech_service.is_some(),
            severity: if speech_expected {
                Severity::Warning
            } else {
                Severity::Info
            },
            reason: if speech_expected {
                "SPEECH_SERVICE_ENABLED is set".to_string()
            } else {
                "SPEECH_SERVICE_ENABLED not set".to_string()
            },
        });

        // Nostr service (set later via set_nostr_service)
        report.add(ValidationItem {
            name: "NostrService".to_string(),
            expected: false,
            present: self.nostr_service.is_some(),
            severity: Severity::Info,
            reason: "Set later via set_nostr_service()".to_string(),
        });

        // Ontology pipeline service
        report.add(ValidationItem {
            name: "OntologyPipelineService".to_string(),
            expected: true,
            present: self.ontology_pipeline_service.is_some(),
            severity: Severity::Warning,
            reason: "Required for semantic physics".to_string(),
        });

        report
    }

    pub fn increment_connections(&self) -> usize {
        self.active_connections.fetch_add(1, Ordering::SeqCst)
    }

    pub fn decrement_connections(&self) -> usize {
        self.active_connections
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                if current > 0 {
                    Some(current - 1)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    }

    pub async fn get_api_keys(&self, pubkey: &str) -> ApiKeys {
        use crate::actors::protected_settings_actor::GetApiKeys;
        self.protected_settings_addr
            .send(GetApiKeys {
                pubkey: pubkey.to_string(),
            })
            .await
            .unwrap_or_else(|_| ApiKeys::default())
    }

    pub async fn get_nostr_user(&self, pubkey: &str) -> Option<NostrUser> {
        if let Some(nostr_service) = &self.nostr_service {
            nostr_service.get_user(pubkey).await
        } else {
            None
        }
    }

    pub async fn validate_nostr_session(&self, pubkey: &str, token: &str) -> bool {
        if let Some(nostr_service) = &self.nostr_service {
            nostr_service.validate_session(pubkey, token).await
        } else {
            false
        }
    }

    pub async fn update_nostr_user_api_keys(
        &self,
        pubkey: &str,
        api_keys: ApiKeys,
    ) -> Result<NostrUser, String> {
        if let Some(nostr_service) = &self.nostr_service {
            nostr_service
                .update_user_api_keys(pubkey, api_keys)
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Nostr service not initialized".to_string())
        }
    }

    pub fn set_nostr_service(&mut self, service: NostrService) {
        self.nostr_service = Some(web::Data::new(service));
    }

    pub fn is_power_user(&self, pubkey: &str) -> bool {
        self.feature_access.is_power_user(pubkey)
    }

    pub fn can_sync_settings(&self, pubkey: &str) -> bool {
        self.feature_access.can_sync_settings(pubkey)
    }

    pub fn has_feature_access(&self, pubkey: &str, feature: &str) -> bool {
        self.feature_access.has_feature_access(pubkey, feature)
    }

    pub fn get_available_features(&self, pubkey: &str) -> Vec<String> {
        self.feature_access.get_available_features(pubkey)
    }

    pub fn get_client_manager_addr(&self) -> &Addr<ClientCoordinatorActor> {
        &self.client_manager_addr
    }

    pub fn get_graph_service_addr(&self) -> &Addr<GraphServiceSupervisor> {
        &self.graph_service_addr
    }

    pub fn get_settings_addr(&self) -> &Addr<OptimizedSettingsActor> {
        &self.settings_addr
    }

    pub fn get_metadata_addr(&self) -> &Addr<MetadataActor> {
        &self.metadata_addr
    }

    pub fn get_workspace_addr(&self) -> &Addr<WorkspaceActor> {
        &self.workspace_addr
    }

    pub fn get_ontology_actor_addr(&self) -> Option<&Addr<OntologyActor>> {
        self.ontology_actor_addr.as_ref()
    }

    pub fn get_task_orchestrator_addr(&self) -> &Addr<TaskOrchestratorActor> {
        &self.task_orchestrator_addr
    }

    /// Get the ForceComputeActor address asynchronously.
    /// Returns None if the address hasn't been initialized yet or GPU is not available.
    pub async fn get_gpu_compute_addr(&self) -> Option<Addr<gpu::ForceComputeActor>> {
        self.gpu_compute_addr.read().await.clone()
    }

    /// Mark the application as degraded with a reason string.
    /// This is checked by the health endpoint to report degraded state.
    pub fn set_degraded(&self, reason: String) {
        if let Ok(mut guard) = self.degraded_reason.write() {
            *guard = Some(reason);
        }
    }

    /// Returns `true` if no degradation reason has been set.
    pub fn is_healthy(&self) -> bool {
        self.degraded_reason
            .read()
            .map(|g| g.is_none())
            .unwrap_or(false)
    }

    /// Returns the current degradation reason, if any.
    pub fn get_degraded_reason(&self) -> Option<String> {
        self.degraded_reason.read().ok().and_then(|g| g.clone())
    }

    /// Try to get the ForceComputeActor address synchronously (non-blocking).
    /// Returns None if the lock is held or the address isn't available.
    pub fn try_get_gpu_compute_addr(&self) -> Option<Addr<gpu::ForceComputeActor>> {
        self.gpu_compute_addr
            .try_read()
            .ok()
            .and_then(|guard| guard.clone())
    }
}
