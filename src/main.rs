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
        validation_handler,
        workspace_handler,
    },
    services::speech_service::SpeechService,
    services::briefing_service::BriefingService,
    services::management_api_client::ManagementApiClient,
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
    // NOTE: The runtime case-sensitive `APP_ENV=production` guard previously here
    // (audited as a T2 anti-pattern — `APP_ENV=Production` defeated it) has been
    // removed. Per ADR-06 §D1 and §D11, dev-bypass env vars are now handled at
    // the binary level: the release build's `enforce_release_env_hygiene()` boot
    // hook (see `main()`) refuses to start if any dev env var is present.
    //
    // `is_production` is still derived for the "missing required env vars" check
    // below, but it is NOT a security toggle — it only controls whether missing
    // required vars warn-and-default (dev) or hard-fail (prod).
    let is_production = std::env::var("APP_ENV").map(|v| v == "production").unwrap_or(false);
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

/// ADR-06 §D11 — Startup refusal of dev-mode env vars and argv flags in release.
///
/// In release builds (no `debug_assertions`, no `--features dev-auth`) the binary
/// physically cannot honour `SETTINGS_AUTH_BYPASS`, `VISIONFLOW_DEV_MODE`,
/// `ALLOW_INSECURE_DEFAULTS`, or `--allow-skip-auth` — the codepaths are
/// `#[cfg]`-stripped. But the presence of those vars/flags at deploy time is a
/// strong signal that someone promoted a dev configuration to production. This
/// hook surfaces that as an immediate hard-fail at boot, with status code 2
/// (V3 verification — see docs/migration-sprint/_resolutions/T2-auth-gating.md).
///
/// The dev-build counterpart is a no-op stub.
#[cfg(not(any(debug_assertions, feature = "dev-auth")))]
fn enforce_release_env_hygiene() {
    // 1) Argv refusal — `--allow-skip-auth` (ADR-02 §D8 + ADR-06 §D2 V2).
    if std::env::args().any(|a| a == "--allow-skip-auth") {
        eprintln!(
            "FATAL: --allow-skip-auth is not available in release builds. \
             Rebuild with `--features dev-auth` for development."
        );
        std::process::exit(1);
    }

    // 2) Env-var refusal — D11 SUSPECT_ENVS (V3).
    const SUSPECT_ENVS: &[&str] = &[
        "SETTINGS_AUTH_BYPASS",
        "ALLOW_INSECURE_DEFAULTS",
        "VISIONFLOW_DEV_MODE",
    ];
    let mut offending: Vec<&str> = Vec::new();
    for var in SUSPECT_ENVS {
        if std::env::var(var).is_ok() {
            offending.push(var);
        }
    }
    // Special case: NODE_ENV=development AND DOCKER_ENV both set (T2 §D11).
    let node_env_dev = std::env::var("NODE_ENV")
        .map(|v| v.eq_ignore_ascii_case("development"))
        .unwrap_or(false);
    let docker_env_set = std::env::var("DOCKER_ENV").is_ok();
    if node_env_dev && docker_env_set {
        offending.push("NODE_ENV=development+DOCKER_ENV");
    }
    if !offending.is_empty() {
        for var in &offending {
            eprintln!(
                "FATAL: dev env var '{}' set in release build. \
                 Refusing to start (ADR-06 §D11).",
                var
            );
        }
        eprintln!(
            "These env vars cannot enable bypass (release binary has no \
             corresponding codepath) but their presence indicates a dev \
             configuration was promoted to production. Remove them and retry."
        );
        std::process::exit(2);
    }
}

/// Dev-build stub — `enforce_release_env_hygiene` is a no-op when bypass code
/// is actually present in the binary.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
#[inline(always)]
fn enforce_release_env_hygiene() {}

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

    // ADR-06 §D11 — Before any other startup work, in a release build, refuse
    // to start if dev-mode env vars or argv flags are present. This runs BEFORE
    // tracing/logging init so the message reaches stderr unconditionally even
    // if logging fails. In dev builds this is a no-op.
    enforce_release_env_hygiene();

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
    let github_sync_service = Arc::new(GitHubSyncService::new(
        enhanced_content_api,
        app_state.neo4j_adapter.clone(),
        app_state.ontology_repository.clone(),
    ));
    info!("[main] GitHub Sync Service initialized");

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
        std::env::var("MANAGEMENT_API_HOST").unwrap_or_else(|_| "agentic-workstation".to_string()),
        std::env::var("MANAGEMENT_API_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(9090),
        std::env::var("MANAGEMENT_API_KEY").unwrap_or_default(),
    );
    let briefing_service = web::Data::new(BriefingService::new(management_api_client));

    let nostr_publisher = web::Data::new(
        NostrBeadPublisher::from_env()
            .map(|p| p.with_neo4j(app_state.neo4j_adapter.graph().clone())),
    );

    // Spawn bridge as background task (no-op if FORUM_RELAY_URL is not set).
    if let Some(bridge) = NostrBridge::from_env() {
        tokio::spawn(bridge.run());
        info!("[main] NostrBridge spawned");
    } else {
        info!("[main] NostrBridge not started (VISIONCLAW_NOSTR_PRIVKEY or FORUM_RELAY_URL not set)");
    }

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

    info!("Starting HTTP server on {}", bind_address);

    info!("main: All services and actors initialized. Configuring HTTP server.");
    let server =
        HttpServer::new(move || {
            // CORS configuration with security-aware origin handling
            // Production: Uses CORS_ALLOWED_ORIGINS environment variable
            // Development: Falls back to localhost origins with ALLOW_INSECURE_DEFAULTS.
            // ADR-06 §D1: the `ALLOW_INSECURE_DEFAULTS` env-var read is compile-time
            // gated. Release binaries cannot widen CORS via env vars — they must set
            // `CORS_ALLOWED_ORIGINS` explicitly. The fallback below produces the
            // restrictive single-origin default.
            let cors = {
                let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
                    .unwrap_or_else(|_| {
                        #[cfg(any(debug_assertions, feature = "dev-auth"))]
                        {
                            if std::env::var("ALLOW_INSECURE_DEFAULTS").is_ok() {
                                // Dev mode: allow common local origins.
                                return "http://localhost:3000,http://localhost:3001,http://127.0.0.1:3000,http://localhost:5173".to_string();
                            }
                        }
                        // Release (or dev without ALLOW_INSECURE_DEFAULTS):
                        // restrictive defaults; explicit configuration required.
                        log::warn!("⚠️  CORS_ALLOWED_ORIGINS not set - using restrictive defaults");
                        "http://localhost:3000".to_string()
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
            .app_data(app_state_data.feature_access.clone())
            .app_data(web::Data::new(github_sync_service.clone()))
            .app_data(web::Data::new(ontology_query_service.clone()))
            .app_data(web::Data::new(ontology_mutation_service.clone()))
            .app_data(neo4j_repo_data.clone())
            .app_data(briefing_service.clone())
            .app_data(nostr_publisher.clone())
            .app_data(validation_service.clone())
            .app_data(physics_service.clone())
            
            
            .route("/wss", web::get().to(socket_flow_handler)) 
            .route("/ws/speech", web::get().to(speech_socket_handler))
            .route("/ws/mcp-relay", web::get().to(mcp_relay_handler)) 
            
            .route("/ws/client-messages", web::get().to(client_messages_handler::websocket_client_messages))
            // OpenAPI/Swagger documentation
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", webxr::openapi::ApiDoc::openapi())
            )
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

                    // JavaScript Solid Server (JSS) integration
                    .configure(webxr::handlers::configure_solid_routes)

                    // Image generation via ComfyUI (Flux2)
                    .configure(webxr::handlers::configure_image_gen_routes)

                    // Briefing workflow (voice → brief → role agents → debrief)
                    .configure(webxr::handlers::configure_briefing_routes)

                    // Memory flash events (RuVector access → WS broadcast to all clients)
                    .configure(webxr::handlers::configure_memory_flash_routes)

                    // Layout mode system (ADR-031)
                    .configure(webxr::handlers::configure_layout_routes)

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
