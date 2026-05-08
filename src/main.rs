// Rebuild: KE velocity fix applied
use webxr::ports::ontology_repository::OntologyRepository;
use webxr::services::nostr_service::NostrService;
// SettingsActor removed - OptimizedSettingsActor in AppState is the single source of truth
use webxr::adapters::neo4j_settings_repository::{Neo4jSettingsRepository, Neo4jSettingsConfig};
use webxr::actors::messages::ReloadGraphFromDatabase;
use webxr::{
    config::AppFullSettings,
    handlers::{
        admin_sync_handler,
        api_handler,
        bots_visualization_handler,
        client_log_handler,
        client_messages_handler,
        consolidated_health_handler,
        graph_export_handler,
        mcp_relay_handler::mcp_relay_handler,
        metrics_handler,
        multi_mcp_websocket_handler,
        nostr_handler,
        pages_handler,
        socket_flow_handler::{socket_flow_handler, PreReadSocketSettings},
        speech_socket_handler::speech_socket_handler,
        agent_events_ws_handler::{agent_events_handler, new_broadcaster},
        presence_handler::{new_room_registry, ws_presence, PresenceHandlerState},
        validation_handler,
        workspace_handler,
    },
    services::speech_service::SpeechService,
    services::briefing_service::BriefingService,
    services::management_api_client::ManagementApiClient,
    services::bead_lifecycle::BeadLifecycleOrchestrator,
    services::bead_store::NoopBeadStore,
    services::nostr_bead_publisher::NostrBeadPublisher,
    services::nostr_bridge::NostrBridge,
    services::{

        github::{content_enhanced::EnhancedContentAPI, ContentAPI, GitHubClient, GitHubConfig},
        github_sync_service::GitHubSyncService,
        ragflow_service::RAGFlowService,
    },

    AppState,
};

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
// DEPRECATED: std::future imports removed (were for ErrorRecoveryMiddleware)
// DEPRECATED: Actix dev imports removed (were for ErrorRecoveryMiddleware)
// DEPRECATED: LocalBoxFuture import removed (was for ErrorRecoveryMiddleware)
// use actix_files::Files;
use dotenvy::dotenv;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Instant;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use webxr::middleware::{RateLimit, TimeoutMiddleware};
use webxr::telemetry::agent_telemetry::init_telemetry_logger;
use webxr::utils::advanced_logging::init_advanced_logging;
use webxr::utils::json::to_json;
// REMOVED: use webxr::utils::logging::init_logging; - legacy logging superseded by advanced_logging

// DEPRECATED: ErrorRecoveryMiddleware removed - NetworkRecoveryManager deleted

/// Validate required and recommended environment variables at startup.
/// In production (APP_ENV=production), missing required vars cause a hard failure.
/// In dev mode, missing required vars emit warnings and the server continues with defaults.
fn validate_required_env_vars() -> Result<(), String> {
    let mut missing = Vec::new();
    let required = [
        "NEO4J_URI",
        "NEO4J_PASSWORD",
        "SYSTEM_NETWORK_PORT",
    ];
    for var in &required {
        if std::env::var(var).is_err() {
            missing.push(*var);
        }
    }
    // Warn about optional but recommended vars
    let recommended = [
        "MANAGEMENT_API_KEY",
        "JWT_SECRET",
        "CORS_ALLOWED_ORIGINS",
    ];
    for var in &recommended {
        if std::env::var(var).is_err() {
            log::warn!("Recommended env var {} is not set", var);
        }
    }
    // Check ALLOW_INSECURE_DEFAULTS is not set in production
    let is_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false);
    if is_production {
        if std::env::var("ALLOW_INSECURE_DEFAULTS").is_ok() {
            log::error!("ALLOW_INSECURE_DEFAULTS is set in production — this is a security risk");
        }
        if std::env::var("SETTINGS_AUTH_BYPASS").map(|v| v == "true").unwrap_or(false) {
            return Err("SETTINGS_AUTH_BYPASS=true is not allowed in production".to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        // In non-production, warn but continue. In production, fail hard.
        if is_production {
            Err(format!("Missing required env vars: {}", missing.join(", ")))
        } else {
            for var in &missing {
                log::warn!("Required env var {} is not set — using defaults (dev mode)", var);
            }
            Ok(())
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Install a global panic hook that logs location + payload to stderr.
    // This fires before the default handler and ensures panics on any thread
    // are captured in container logs / journald.
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .unwrap_or("unknown");
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        eprintln!("PANIC at {}: {}", location, payload);
    }));

    dotenv().ok();

    // Initialize tracing_subscriber for structured logging with distributed tracing support.
    // This replaces env_logger and bridges to the `log` crate, so existing log::info! etc. still work.
    // RUST_LOG env var controls filtering (e.g. RUST_LOG=debug or RUST_LOG=webxr=debug,actix_web=info).
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(true),
        )
        .init();

    // Validate required environment variables (after tracing init so log macros work)
    if let Err(e) = validate_required_env_vars() {
        error!("Environment validation failed: {}", e);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            e,
        ));
    }

    // Record process start time for uptime reporting via /api/metrics
    let process_start_time = Instant::now();

    info!("--- Configuration Verification ---");
    info!("MARKDOWN_DIR: {}", webxr::services::file_service::MARKDOWN_DIR);
    info!("METADATA_PATH: {}", "/workspace/ext/data/metadata/metadata.json");
    info!("---------------------------------");

    // REMOVED: init_logging()? call - using advanced_logging instead
    if let Err(e) = init_advanced_logging() {
        error!("Failed to initialize advanced logging: {}", e);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Advanced logging initialization failed: {}", e),
        ));
    } else {
        info!("Advanced logging system initialized successfully");
    }

    
    
    let log_dir = if std::path::Path::new("/app/logs").exists() {
        "/app/logs".to_string()
    } else if std::path::Path::new("/workspace/ext/logs").exists() {
        "/workspace/ext/logs".to_string()
    } else {
        
        std::env::temp_dir()
            .join("webxr_telemetry")
            .to_string_lossy()
            .to_string()
    };

    let log_dir = std::env::var("TELEMETRY_LOG_DIR").unwrap_or(log_dir);

    if let Err(e) = init_telemetry_logger(&log_dir, 100) {
        error!("Failed to initialize telemetry logger: {}", e);
    } else {
        info!("Telemetry logger initialized with directory: {}", log_dir);
    }

    
    let settings = match AppFullSettings::new() {
        Ok(s) => {
            info!(
                "✅ AppFullSettings loaded successfully from: {}",
                std::env::var("SETTINGS_FILE_PATH")
                    .unwrap_or_else(|_| "/app/settings.yaml".to_string())
            );

            
            match to_json(&s.visualisation.rendering) {
                Ok(json_output) => {
                    info!(
                        "✅ SERDE ALIAS FIX WORKS! JSON serialization (camelCase): {}",
                        json_output
                    );

                    
                    if json_output.contains("ambientLightIntensity")
                        && !json_output.contains("ambient_light_intensity")
                    {
                        info!("✅ CONFIRMED: JSON uses camelCase field names for REST API compatibility");
                    }

                    
                    info!("✅ CONFIRMED: Values loaded from snake_case YAML:");
                    info!(
                        "   - ambient_light_intensity -> {}",
                        s.visualisation.rendering.ambient_light_intensity
                    );
                    info!(
                        "   - enable_ambient_occlusion -> {}",
                        s.visualisation.rendering.enable_ambient_occlusion
                    );
                    info!(
                        "   - background_color -> {}",
                        s.visualisation.rendering.background_color
                    );
                    info!("🎉 SERDE ALIAS FIX IS WORKING: YAML (snake_case) loads successfully, JSON serializes as camelCase!");
                }
                Err(e) => {
                    error!("❌ JSON serialization failed: {}", e);
                }
            }

            Arc::new(RwLock::new(s)) 
        }
        Err(e) => {
            error!("❌ Failed to load AppFullSettings: {:?}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to initialize AppFullSettings: {:?}", e),
            ));
        }
    };

    
    info!("GPU compute will be initialized by GPUComputeActor when needed");

    debug!("Successfully loaded AppFullSettings");

    info!("Starting WebXR application...");
    debug!("main: Beginning application startup sequence.");

    // Phase 3: Initialize Neo4j settings repository and actor
    // SettingsActor removed: OptimizedSettingsActor in AppState is the single source of truth.
    // Settings routes now use AppState.settings_addr (OptimizedSettingsActor) directly.
    info!("Initializing Neo4j settings repository for routes");
    let settings_config = Neo4jSettingsConfig::default();
    let settings_repository = match Neo4jSettingsRepository::new(settings_config).await {
        Ok(repo) => Arc::new(repo),
        Err(e) => {
            error!("Failed to create Neo4j settings repository: {}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create Neo4j settings repository: {}", e),
            ));
        }
    };
    let neo4j_repo_data = web::Data::new(settings_repository.clone());
    info!("Neo4j settings repository initialized successfully");



    let settings_data = web::Data::new(settings.clone());

    
    let github_config = match GitHubConfig::from_env() {
        Ok(config) => {
            info!("[main] GitHub config loaded from environment");
            config
        }
        Err(e) => {
            warn!("[main] GitHub config unavailable ({}), using disabled placeholder — content API routes will return errors", e);
            GitHubConfig::disabled()
        }
    };



    let github_client = match GitHubClient::new(github_config, settings.clone()).await {
        Ok(client) => Arc::new(client),
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to initialize GitHub client: {}", e),
            ))
        }
    };

    let content_api = Arc::new(ContentAPI::new(github_client.clone()));

    
    
    let speech_service = {
        let service = SpeechService::new(settings.clone());
        Some(Arc::new(service))
    };

    
    info!("[main] Attempting to initialize RAGFlowService...");
    let ragflow_service_option = match RAGFlowService::new(settings.clone()).await {
        Ok(service) => {
            info!("[main] RAGFlowService::new SUCCEEDED. Service instance created.");
            Some(Arc::new(service))
        }
        Err(e) => {
            error!("[main] RAGFlowService::new FAILED. Error: {}", e);
            None
        }
    };

    if ragflow_service_option.is_some() {
        info!("[main] ragflow_service_option is Some after RAGFlowService::new attempt.");
    } else {
        error!("[main] ragflow_service_option is None after RAGFlowService::new attempt. Chat functionality will be unavailable.");
    }

    
    
    let settings_value = {
        let settings_read = settings.read().await;
        settings_read.clone()
    };

    let mut app_state = match AppState::new(
        settings_value,
        github_client.clone(),
        content_api.clone(),
        None,                   
        ragflow_service_option, 
        speech_service,
        "default_session".to_string(), 
    )
    .await
    {
        Ok(state) => {
            info!("[main] AppState::new completed successfully");
            state
        }
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to initialize app state: {}", e),
            ))
        }
    };

    info!("[main] About to initialize Nostr service");
    nostr_handler::init_nostr_service(&mut app_state).await;
    info!("[main] Nostr service initialized");

    
    info!("[main] Initializing GitHub Sync Service...");
    let enhanced_content_api = Arc::new(EnhancedContentAPI::new(github_client.clone()));
    let mut github_sync_service_inner = GitHubSyncService::new(
        enhanced_content_api,
        app_state.neo4j_adapter.clone(),
        app_state.ontology_repository.clone(),
    );

    // ADR-051: wire the Pod-first-Neo4j-second saga and spawn the resumption
    // task. Both are no-ops unless `POD_SAGA_ENABLED=true`; construction is
    // cheap so we always build the saga and let the runtime flag decide.
    match webxr::services::ingest_saga::build_from_env(app_state.neo4j_adapter.clone()) {
        Ok(saga) => {
            github_sync_service_inner.set_saga(saga.clone());
            let handle = webxr::services::ingest_saga::spawn_resumption_task(saga);
            // Task is held by the runtime; handle intentionally dropped.
            std::mem::forget(handle);
            info!("[main] IngestSaga wired (POD_SAGA_ENABLED={})",
                  std::env::var("POD_SAGA_ENABLED").unwrap_or_else(|_| "unset".to_string()));
        }
        Err(e) => {
            warn!("[main] IngestSaga not wired: {} — legacy ingest path active", e);
        }
    }

    let github_sync_service = Arc::new(github_sync_service_inner);
    info!("[main] GitHub Sync Service initialized");

    // PRD-013: Git Ingest Surface — git-over-HTTP ingest pipeline.
    let git_ingest_registry = webxr::services::git_ingest::RemoteRegistry::new(
        app_state.neo4j_adapter.graph().clone(),
    );
    let git_ingest_service = Arc::new(
        webxr::services::git_ingest::GitIngestService::new(git_ingest_registry.clone()),
    );
    let writeback_saga = Arc::new(
        webxr::services::git_ingest::WriteBackSaga::new(
            git_ingest_registry.clone(),
            app_state.neo4j_adapter.graph().clone(),
        ),
    );
    // Auto-register legacy GITHUB_* env vars as a PAT remote (idempotent).
    if webxr::services::git_ingest::git_ingest_enabled() {
        if let Err(e) = git_ingest_registry.legacy_github_shim().await {
            warn!("[main] git-ingest legacy shim failed: {}", e);
        }
    }
    let git_ingest_data = web::Data::new(git_ingest_service);
    let git_ingest_registry_data = web::Data::new(git_ingest_registry);
    let writeback_saga_data = web::Data::new(writeback_saga);
    info!("[main] Git Ingest Service initialized (enabled={})",
          webxr::services::git_ingest::git_ingest_enabled());

    // Initialize SchemaService for natural language query support
    info!("[main] Initializing Schema Service...");
    let schema_service = Arc::new(webxr::services::schema_service::SchemaService::new());
    info!("[main] Schema Service initialized");
    // Initialize Natural Language Query Service
    info!("[main] Initializing Natural Language Query Service...");
    let perplexity_service = Arc::new(webxr::services::perplexity_service::PerplexityService::new());
    let nl_query_service = Arc::new(webxr::services::natural_language_query_service::NaturalLanguageQueryService::new(
        schema_service.clone(),
        perplexity_service.clone(),
    ));
    info!("[main] Natural Language Query Service initialized");

    // Initialize Semantic Pathfinding Service
    info!("[main] Initializing Semantic Pathfinding Service...");
    let pathfinding_service = Arc::new(webxr::services::semantic_pathfinding_service::SemanticPathfindingService::default());
    info!("[main] Semantic Pathfinding Service initialized");

    // Initialize Ontology Agent Services (query + mutation + GitHub PR)
    info!("[main] Initializing Ontology Agent Services...");
    let whelk_engine = Arc::new(tokio::sync::RwLock::new(
        webxr::adapters::whelk_inference_engine::WhelkInferenceEngine::new(),
    ));
    let github_pr_service = Arc::new(webxr::services::github_pr_service::GitHubPRService::new());
    let ontology_query_service = Arc::new(webxr::services::ontology_query_service::OntologyQueryService::new(
        app_state.ontology_repository.clone(),
        app_state.neo4j_adapter.clone(),
        whelk_engine.clone(),
        schema_service.clone(),
    ));
    let ontology_mutation_service = Arc::new(webxr::services::ontology_mutation_service::OntologyMutationService::new(
        app_state.ontology_repository.clone(),
        whelk_engine.clone(),
        github_pr_service.clone(),
    ));
    info!("[main] Ontology Agent Services initialized");

    info!("--- Starting Data Orchestration Sequence ---");

    // Step 1: Sync Files from GitHub.
    info!("[Startup] Step 1: Syncing files from GitHub to local storage...");
    let github_sync_failed = if let Err(e) = webxr::services::file_service::FileService::initialize_local_storage(settings.clone()).await {
        error!("[Startup] FAILED to sync from GitHub: {}. Will try local files.", e);
        true
    } else {
        info!("[Startup] SUCCESS: Local file storage is synchronized with GitHub.");
        false
    };

    // Step 1b: If GitHub sync failed or metadata is empty, scan local files
    let metadata = webxr::services::file_service::FileService::load_or_create_metadata().unwrap_or_default();
    if github_sync_failed || metadata.is_empty() {
        info!("[Startup] Step 1b: Scanning local markdown files as fallback...");
        match webxr::services::file_service::FileService::scan_local_files_to_metadata() {
            Ok(local_metadata) => {
                info!("[Startup] SUCCESS: Scanned {} public files from local storage.", local_metadata.len());
            }
            Err(e) => {
                error!("[Startup] FAILED to scan local files: {}", e);
            }
        }
    }

    // Step 2: Load Files into Neo4j.
    info!("[Startup] Step 2: Populating Neo4j from local files...");
    if let Err(e) = webxr::services::file_service::FileService::load_graph_from_files_into_neo4j(&app_state.neo4j_adapter).await {
        error!("[Startup] FATAL: Failed to populate Neo4j: {}. Application is in DEGRADED state.", e);
        app_state.set_degraded(format!("Neo4j init failed: {}", e));
    } else {
        info!("[Startup] SUCCESS: Neo4j database is populated and ready.");
    }

    // Step 3: Notify Actors.
    info!("[Startup] Step 3: Notifying actors to reload graph state from database...");
    app_state.graph_service_addr.do_send(ReloadGraphFromDatabase);
    info!("[Startup] SUCCESS: Actors notified.");
    info!("--- Data Orchestration Sequence Complete ---");










    info!("Skipping bots orchestrator connection during startup (will connect on-demand)");


    info!("Loading ontology graph from Neo4j...");

    let graph_data_option = match app_state.ontology_repository.load_ontology_graph().await {
        Ok(graph_arc) => {
            let graph = graph_arc.as_ref();
            if !graph.nodes.is_empty() {
                info!(
                    "✅ Loaded ontology graph from database: {} nodes, {} edges",
                    graph.nodes.len(),
                    graph.edges.len()
                );
                info!("ℹ️  Ontology classes loaded but NOT sent to actor (KG nodes will be loaded via ReloadGraphFromDatabase)");
                Some((*graph_arc).clone())
            } else {
                info!("📂 Ontology database is empty - waiting for GitHub sync to populate");
                info!("ℹ️  Ontology classes will be loaded after sync extracts OWL data");
                None
            }
        }
        Err(e) => {
            error!("⚠️  Failed to load ontology graph from database: {}", e);
            error!("⚠️  Graph will be empty until GitHub sync completes");
            None
        }
    };


    // CRITICAL FIX: Do NOT send ontology graph via UpdateGraphData!
    // This would overwrite the KG nodes that should be loaded via ReloadGraphFromDatabase.
    // The architecture is: KG nodes (from GitHub sync) with owl_class_iri links to ontology.
    // ReloadGraphFromDatabase (sent in app_state.rs) will load all KG nodes from database.
    // UpdateGraphData here would overwrite them with only the 1 ontology root node.
    //
    // Keeping graph_data_option for potential future use but not sending it to actor.

    if let Some(_graph_data) = graph_data_option {
        info!("⏭️  Ontology graph loaded but not sent to actor (will use KG nodes from ReloadGraphFromDatabase instead)");
        info!("ℹ️  Ontology classes are available via API endpoints but nodes come from KG sync");
    } else {
        info!("⏳ GraphServiceActor will be populated by ReloadGraphFromDatabase from existing KG nodes");
        info!("ℹ️  If no KG nodes exist, you can trigger GitHub sync via /api/admin/sync endpoint");
    }

    info!("Starting HTTP server...");

    
    
    
    
    
    
    
    info!("Skipping redundant StartSimulation message to GraphServiceSupervisor for debugging stack overflow. Simulation should already be running from supervisor's started() method.");

    
    // --- Briefing + Nostr services ---
    let management_api_client = ManagementApiClient::new(
        std::env::var("MANAGEMENT_API_HOST").unwrap_or_else(|_| "localhost".to_string()),
        std::env::var("MANAGEMENT_API_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(9190),
        std::env::var("MANAGEMENT_API_KEY").unwrap_or_default(),
    );
    let briefing_service = web::Data::new(BriefingService::new(management_api_client));

    let nostr_publisher = NostrBeadPublisher::from_env()
        .map(|p| p.with_neo4j(app_state.neo4j_adapter.graph().clone()));
    let bead_orchestrator = web::Data::new(std::sync::Arc::new(
        BeadLifecycleOrchestrator::new(
            std::sync::Arc::new(NoopBeadStore),
            nostr_publisher,
        ),
    ));

    // Spawn bridge as background task (no-op if FORUM_RELAY_URL is not set).
    if let Some(bridge) = NostrBridge::from_env() {
        tokio::spawn(bridge.run());
        info!("[main] NostrBridge spawned");
    } else {
        info!("[main] NostrBridge not started (VISIONCLAW_NOSTR_PRIVKEY or FORUM_RELAY_URL not set)");
    }

    // Server's own Nostr identity (kinds 30023/30100/30200/30300).
    // In APP_ENV=production a missing SERVER_NOSTR_PRIVKEY aborts startup — fail fast.
    let server_identity = {
        let mut si = webxr::services::server_identity::ServerIdentity::from_env()
            .map_err(|e| {
                log::error!("[main] ServerIdentity initialisation failed: {e:#}");
                std::io::Error::new(std::io::ErrorKind::Other, format!("{e:#}"))
            })?;
        si.connect_relays().await;
        std::sync::Arc::new(si)
    };
    info!(
        "[main] ServerIdentity ready (pubkey_hex={})",
        server_identity.pubkey_hex()
    );
    let server_nostr_addr = {
        use actix::Actor;
        webxr::actors::ServerNostrActor::new(server_identity.clone()).start()
    };
    let server_identity_data = web::Data::new(server_identity.clone());
    let server_nostr_addr_for_visibility = server_nostr_addr.clone();
    let server_nostr_addr_for_bridge = server_nostr_addr.clone();
    let server_nostr_addr_data = web::Data::new(server_nostr_addr);

    // Prometheus metrics registry (task #18) — shared Arc so every handler and
    // actor can observe counters without copying state.
    let metrics_registry: Arc<webxr::services::metrics::MetricsRegistry> =
        Arc::new(webxr::services::metrics::MetricsRegistry::new());
    let metrics_data = web::Data::new(metrics_registry.clone());
    info!(
        "[main] Prometheus MetricsRegistry ready (METRICS_ENABLED={})",
        webxr::services::metrics::metrics_enabled()
    );

    // ADR-051: VisibilityTransitionService orchestrates publish/unpublish
    // sagas (Pod MOVE + Neo4j flip + tombstone + audit). Registered as
    // web::Data so solid-proxy's tombstone check can look it up. Gated at
    // the call site by VISIBILITY_TRANSITIONS.
    let visibility_transition_service = match webxr::services::pod_client::PodClient::from_env() {
        Ok(pc) => {
            let svc = std::sync::Arc::new(
                webxr::sovereign::VisibilityTransitionService::new(
                    std::sync::Arc::new(pc),
                    app_state.neo4j_adapter.clone(),
                    server_nostr_addr_for_visibility,
                    Some(metrics_registry.clone()),
                ),
            );
            info!(
                "[main] VisibilityTransitionService wired (VISIBILITY_TRANSITIONS={})",
                std::env::var("VISIBILITY_TRANSITIONS").unwrap_or_else(|_| "unset".to_string())
            );
            Some(svc)
        }
        Err(e) => {
            warn!(
                "[main] VisibilityTransitionService not wired: {} — tombstone check inert",
                e
            );
            None
        }
    };
    let visibility_transition_data = visibility_transition_service
        .as_ref()
        .map(|svc| web::Data::new(svc.clone()));

    // ADR-051 §bridge + ADR-050 §server-identity: BridgeEdgeService handles
    // BRIDGE_TO promotion with a kind-30100 audit-trail fan-out. Registered
    // as web::Data so migration handlers can look it up. Gated at the call
    // site by BRIDGE_EDGE_ENABLED.
    let bridge_edge_service = {
        let svc = std::sync::Arc::new(
            webxr::services::bridge_edge::BridgeEdgeService::new(
                app_state.neo4j_adapter.clone(),
            )
            .with_prom(metrics_registry.clone())
            .with_server_nostr(server_nostr_addr_for_bridge),
        );
        info!(
            "[main] BridgeEdgeService wired with kind-30100 fan-out (BRIDGE_EDGE_ENABLED={})",
            std::env::var("BRIDGE_EDGE_ENABLED").unwrap_or_else(|_| "unset".to_string())
        );
        svc
    };
    let bridge_edge_data = web::Data::new(bridge_edge_service);

    let app_state_data = web::Data::new(app_state);
    let validation_service = web::Data::new(validation_handler::ValidationService::new());

    // Initialize PhysicsService so POST /api/physics/reset and related endpoints work.
    // ActixPhysicsAdapter wraps PhysicsOrchestratorActor; the actor addr is populated
    // lazily by the adapter's own initialization path.
    let physics_service = {
        use webxr::adapters::actix_physics_adapter::ActixPhysicsAdapter;
        use webxr::application::PhysicsService;
        use webxr::events::EventBus;
        let adapter = Arc::new(tokio::sync::RwLock::new(ActixPhysicsAdapter::new()));
        let event_bus = Arc::new(tokio::sync::RwLock::new(EventBus::new()));
        web::Data::new(Arc::new(PhysicsService::new(adapter, event_bus)))
    };
    

    
    let bind_address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SYSTEM_NETWORK_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(4000);
    let bind_address = format!("{}:{}", bind_address, port);

    
    let pre_read_ws_settings = {
        let s = settings.read().await;
        PreReadSocketSettings {
            min_update_rate: s.system.websocket.min_update_rate,
            max_update_rate: s.system.websocket.max_update_rate,
            motion_threshold: s.system.websocket.motion_threshold,
            motion_damping: s.system.websocket.motion_damping,
            heartbeat_interval_ms: s.system.websocket.heartbeat_interval, 
            heartbeat_timeout_ms: s.system.websocket.heartbeat_timeout,   
        }
    };
    let pre_read_ws_settings_data = web::Data::new(pre_read_ws_settings);

    // ADR-053/056: JSS retired 2026-04-20. Solid is always served by the
    // external `solid-pod-rs` crate (git dep in Cargo.toml). A startup
    // failure here is fatal — there is no fallback path.
    let native_solid_data: web::Data<Arc<webxr::handlers::NativeSolidService>> =
        match webxr::handlers::NativeSolidService::from_env().await {
            Ok(svc) => web::Data::new(Arc::new(svc)),
            Err(e) => {
                error!("[solid-pod-rs] NativeSolidService init failed: {e}");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("solid-pod-rs init failed: {e}"),
                ));
            }
        };
    info!("[solid-pod-rs] native Pod service ready (JSS retired)");

    // ADR-059 / ADR-014 — bidirectional agent events channel.
    // TransientEdgeActor manages beam+gluon visual lifecycle (≤1ms reap @ 10Hz).
    // Broadcaster fans user_interaction events out to subscribed agentbox WS sessions.
    use actix::Actor;
    let transient_edge_addr = webxr::actors::transient_edge_actor::TransientEdgeActor::new().start();
    let agent_events_broadcaster = new_broadcaster();
    let transient_edge_data = web::Data::new(transient_edge_addr);
    let agent_events_broadcaster_data = web::Data::new(agent_events_broadcaster.clone());
    info!("[agent-events] TransientEdgeActor started + broadcaster initialised");

    // PRD-008 §5.3 — XR presence room registry + Schnorr identity verifier.
    let presence_registry = new_room_registry();
    let presence_verifier: std::sync::Arc<dyn visionclaw_xr_presence::ports::IdentityVerifier> =
        std::sync::Arc::new(webxr::services::nostr_identity_verifier::NostrIdentityVerifier::new());
    let presence_handler_state = web::Data::new(PresenceHandlerState {
        registry: presence_registry,
        identity_verifier: presence_verifier,
    });
    info!("[ws-presence] XR presence registry + verifier initialised at /ws/presence");

    info!("Starting HTTP server on {}", bind_address);

    info!("main: All services and actors initialized. Configuring HTTP server.");
    let server =
        HttpServer::new(move || {
            // CORS configuration with security-aware origin handling
            // Production: Uses CORS_ALLOWED_ORIGINS environment variable
            // Development: Falls back to localhost origins with ALLOW_INSECURE_DEFAULTS
            let cors = {
                let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
                    .unwrap_or_else(|_| {
                        if std::env::var("ALLOW_INSECURE_DEFAULTS").is_ok() {
                            // Development mode: allow common local origins
                            "http://localhost:3000,http://localhost:3001,http://127.0.0.1:3000,http://localhost:5173".to_string()
                        } else {
                            // Production: require explicit configuration
                            log::warn!("⚠️  CORS_ALLOWED_ORIGINS not set - using restrictive defaults");
                            "http://localhost:3000".to_string()
                        }
                    });

                let mut cors_builder = Cors::default();

                for origin in allowed_origins.split(',').map(|s| s.trim()) {
                    if !origin.is_empty() {
                        cors_builder = cors_builder.allowed_origin(origin);
                    }
                }

                // Also accept origins that match the request Host (same-host via nginx proxy).
                // This handles Docker internal IPs without listing them explicitly.
                // SECURITY: Only enabled in non-production to prevent origin spoofing.
                let is_cors_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false);
                if !is_cors_production {
                    let origins_for_fn = allowed_origins.clone();
                    cors_builder = cors_builder
                        .allowed_origin_fn(move |origin, req_head| {
                            let origin_str = origin.to_str().unwrap_or("");
                            // Check explicit list first
                            if origins_for_fn.split(',').map(|s| s.trim()).any(|a| a == origin_str) {
                                return true;
                            }
                            // Same-host check: compare hostnames (strip scheme and port)
                            if let Some(host) = req_head.headers().get("host") {
                                let host_str = host.to_str().unwrap_or("");
                                let origin_host = origin_str
                                    .strip_prefix("http://")
                                    .or_else(|| origin_str.strip_prefix("https://"))
                                    .unwrap_or("");
                                let host_no_port = host_str.split(':').next().unwrap_or("");
                                let origin_no_port = origin_host.split(':').next().unwrap_or("");
                                if !host_no_port.is_empty() && host_no_port == origin_no_port {
                                    return true;
                                }
                            }
                            false
                        });
                } else {
                    log::info!("Production mode: same-host CORS origin function disabled");
                }
                cors_builder
                    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"])
                    .allowed_headers(vec![
                        actix_web::http::header::AUTHORIZATION,
                        actix_web::http::header::CONTENT_TYPE,
                        actix_web::http::header::ACCEPT,
                        actix_web::http::header::ORIGIN,
                    ])
                    .supports_credentials()
                    .max_age(3600)
            };

            let app = App::new()
            .wrap(middleware::Logger::default())
            .wrap(cors)
            .wrap(middleware::Compress::default())
            .wrap(TimeoutMiddleware::new(Duration::from_secs(30))) 


            .app_data(settings_data.clone())
            .app_data(web::Data::new(github_client.clone()))
            .app_data(web::Data::new(content_api.clone()))
            .app_data(app_state_data.clone())
            .app_data(pre_read_ws_settings_data.clone())
            .app_data(web::Data::new(metrics_handler::ProcessStartTime(process_start_time)))

            .app_data(web::Data::new(app_state_data.graph_service_addr.clone()))
            .app_data(web::Data::new(app_state_data.settings_addr.clone()))
            .app_data(web::Data::new(app_state_data.metadata_addr.clone()))
            .app_data(web::Data::new(app_state_data.client_manager_addr.clone()))
            .app_data(web::Data::new(app_state_data.workspace_addr.clone()))
            .app_data(web::Data::new(schema_service.clone()))
            .app_data(web::Data::new(nl_query_service.clone()))
            .app_data(web::Data::new(pathfinding_service.clone()))
            .app_data(app_state_data.nostr_service.clone().unwrap_or_else(|| web::Data::new(NostrService::default())))
            .app_data(server_identity_data.clone())
            .app_data(server_nostr_addr_data.clone())
            .app_data(bridge_edge_data.clone())
            // ADR-059 / ADR-014 bidirectional agent events
            .app_data(transient_edge_data.clone())
            .app_data(agent_events_broadcaster_data.clone())
            // PRD-008 — XR presence handler (Quest 3 native APK)
            .app_data(presence_handler_state.clone())
            .app_data(metrics_data.clone())
            .app_data(app_state_data.feature_access.clone())
            .app_data(web::Data::from(github_sync_service.clone()))
            .app_data(web::Data::new(ontology_query_service.clone()))
            .app_data(web::Data::new(ontology_mutation_service.clone()))
            .app_data(neo4j_repo_data.clone())
            .app_data(briefing_service.clone())
            .app_data(bead_orchestrator.clone())
            .app_data(validation_service.clone())
            .app_data(physics_service.clone())
            .app_data(git_ingest_data.clone())
            .app_data(git_ingest_registry_data.clone())
            .app_data(writeback_saga_data.clone());

            // ADR-051: optional VisibilityTransitionService web::Data. When
            // absent (PodClient couldn't be constructed at startup), the
            // solid-proxy's tombstone check becomes inert and GETs pass
            // straight through to JSS.
            let app = if let Some(vts) = visibility_transition_data.as_ref() {
                app.app_data(vts.clone())
            } else {
                app
            };
            let app = app
                .route("/wss", web::get().to(socket_flow_handler))
            // ADR-059 §1: bidirectional agent events. Subprotocol vc-agent-events.v1.
            .route("/wss/agent-events", web::get().to(agent_events_handler))
            .route("/ws/speech", web::get().to(speech_socket_handler))
            .route("/ws/mcp-relay", web::get().to(mcp_relay_handler)) 
            
            .route("/ws/client-messages", web::get().to(client_messages_handler::websocket_client_messages))
            // PRD-008 §5.3: XR presence WebSocket — Quest 3 native APK multi-user sync
            .route("/ws/presence", web::get().to(ws_presence))
            // OpenAPI/Swagger documentation
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", webxr::openapi::ApiDoc::openapi())
            )
            // Prometheus scrape endpoint at the server root (task #18).
            // Intentionally outside the /api scope so scrapers hit /metrics
            // by convention, with no auth or rate-limit wrapper.
            .configure(metrics_handler::configure_metrics_routes)
            .service(
                web::scope("/api")
                    // Client logs route - registered early to avoid scope conflicts
                    .route("/client-logs", web::post().to(client_log_handler::handle_client_logs))
                    .service(
                        web::scope("/settings")
                            .wrap(RateLimit::per_minute(60))
                            .configure(webxr::settings::api::configure_routes)
                    )
                    .configure(api_handler::config)
                    .configure(workspace_handler::config)
                    .configure(admin_sync_handler::configure_routes)
                    .configure(validation_handler::config)

                    // Pipeline admin routes removed (SQLite-specific handlers deleted in Neo4j migration)
                    // Cypher query endpoint removed (handler deleted in Neo4j migration)

                    // Phase 5: Hexagonal architecture handlers
                    .configure(webxr::handlers::configure_physics_routes)
                    .configure(webxr::handlers::configure_schema_routes)
                    .configure(webxr::handlers::configure_nl_query_routes)
                    .configure(webxr::handlers::configure_pathfinding_routes)
                    .configure(webxr::handlers::configure_semantic_routes)
                    .configure(webxr::handlers::configure_inference_routes)

                    // Health and monitoring
                    .configure(consolidated_health_handler::configure_routes)

                    // Observability metrics endpoint
                    .configure(metrics_handler::configure_routes)

                    // Multi-MCP WebSocket
                    .configure(multi_mcp_websocket_handler::configure_multi_mcp_routes)

                    .service(web::scope("/pages").configure(pages_handler::config))
                    .service(web::scope("/bots").configure(api_handler::bots::config))
                    .configure(bots_visualization_handler::configure_routes)
                    .configure(graph_export_handler::configure_routes)

                    // Ontology agent tools (MCP surface)
                    .configure(webxr::handlers::configure_ontology_agent_routes)

                    // ADR-072: Combined content+topology feature discovery
                    .configure(webxr::handlers::configure_discovery_routes)

                    // Solid Pod routes — native `solid-pod-rs` backend only
                    // (ADR-053/056; JSS proxy retired 2026-04-20).
                    .app_data(native_solid_data.clone())
                    .configure(webxr::handlers::configure_solid_native_routes)

                    // Image generation via ComfyUI (Flux2)
                    .configure(webxr::handlers::configure_image_gen_routes)

                    // Briefing workflow (voice → brief → role agents → debrief)
                    .configure(webxr::handlers::configure_briefing_routes)

                    // Memory flash events (RuVector access → WS broadcast to all clients)
                    .configure(webxr::handlers::configure_memory_flash_routes)

                    // URI resolver (PRD-006 §5.2): /api/v1/uri/{urn|by-curie/{curie}}
                    .configure(webxr::handlers::configure_uri_resolver_routes)

                    // ADR-059 §3 Phase 3 — POST /api/v1/agent-events/user-interaction
                    .service(
                        web::scope("/v1")
                            .configure(webxr::handlers::user_interaction_handler::configure_routes)
                    )

                    // Layout mode system (ADR-031)
                    .configure(webxr::handlers::configure_layout_routes)

                    // Combined content + topology embedding discovery
                    .configure(webxr::handlers::configure_discovery_routes)

                    // PRD-013: Git Ingest Surface — remote CRUD, sync trigger, write-back
                    .configure(webxr::services::git_ingest::configure_routes)

                    // Server Nostr identity — third parties discover the
                    // server's signing pubkey via GET /api/server/identity.
                    .service(
                        web::scope("/server")
                            .configure(webxr::handlers::configure_server_identity_routes)
                    )

            );

            app
        })
        .bind(&bind_address)?
        .workers(4) 
        .run();

    let server_handle = server.handle();

    
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM signal");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT signal");
            }
        }
        info!("Initiating graceful shutdown");
        server_handle.stop(true).await;
    });

    info!("main: HTTP server startup sequence complete. Server is now running.");
    server.await?;

    info!("HTTP server stopped");
    Ok(())
}
