// Rebuild: KE velocity fix applied
use visionclaw_server::ports::ontology_repository::OntologyRepository;
use visionclaw_server::services::nostr_service::NostrService;
// SettingsActor removed - OptimizedSettingsActor in AppState is the single source of truth
use visionclaw_server::actors::messages::ReloadGraphFromDatabase;
use visionclaw_server::{
    config::AppFullSettings,
    handlers::{
        admin_rbac_handler, admin_sync_handler, api_handler, bots_visualization_handler,
        client_log_handler, client_messages_handler, consolidated_health_handler,
        graph_export_handler,
        mcp_relay_handler::mcp_relay_handler,
        metrics_handler, multi_mcp_websocket_handler, nostr_handler, pages_handler,
        presence_handler::{
            new_room_registry, new_seen_nonce_cache, ws_presence, PresenceHandlerState,
        },
        socket_flow_handler::{socket_flow_handler, PreReadSocketSettings},
        speech_socket_handler::speech_socket_handler,
        validation_handler, workspace_handler,
    },
    services::briefing_service::BriefingService,
    services::management_api_client::ManagementApiClient,
    services::nostr_bead_publisher::NostrBeadPublisher,
    services::nostr_bridge::NostrBridge,
    services::speech_service::SpeechService,
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
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use visionclaw_server::middleware::{RateLimit, TimeoutMiddleware};
use visionclaw_server::telemetry::agent_telemetry::init_telemetry_logger;
use visionclaw_server::utils::advanced_logging::init_advanced_logging;
use visionclaw_server::utils::json::to_json;
// REMOVED: use visionclaw_server::utils::logging::init_logging; - legacy logging superseded by advanced_logging

// DEPRECATED: ErrorRecoveryMiddleware removed - NetworkRecoveryManager deleted

/// Validate required and recommended environment variables at startup.
/// In production (APP_ENV=production), missing required vars cause a hard failure.
/// In dev mode, missing required vars emit warnings and the server continues with defaults.
fn validate_required_env_vars() -> Result<(), String> {
    let mut missing = Vec::new();
    let required = ["SYSTEM_NETWORK_PORT"];
    for var in &required {
        if std::env::var(var).is_err() {
            missing.push(*var);
        }
    }
    // Warn about optional but recommended vars
    let recommended = ["MANAGEMENT_API_KEY", "JWT_SECRET", "CORS_ALLOWED_ORIGINS"];
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
    let is_production = std::env::var("APP_ENV")
        .map(|v| v == "production")
        .unwrap_or(false);
    if missing.is_empty() {
        Ok(())
    } else {
        // In non-production, warn but continue. In production, fail hard.
        if is_production {
            Err(format!("Missing required env vars: {}", missing.join(", ")))
        } else {
            for var in &missing {
                log::warn!(
                    "Required env var {} is not set — using defaults (dev mode)",
                    var
                );
            }
            Ok(())
        }
    }
}

/// ADR-06 §D11 — Startup refusal of dev-mode env vars and argv flags in release.
///
/// In release builds (no `debug_assertions`, no `--features dev-auth`) the binary
/// physically cannot honour `SETTINGS_AUTH_BYPASS`, `VISIONCLAW_DEV_MODE`,
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
        "VISIONCLAW_DEV_MODE",
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
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(
                "info,\
             actix_web=warn,\
             actix_server=warn,\
             actix_http=warn,\
             h2=warn,\
             hyper=warn,\
             rustls=warn,\
             reqwest=warn,\
             oxigraph=warn,\
             horned_owl=warn,\
             whelk=warn,\
             solid_pod_rs=warn,\
             visionclaw_server::actors::gpu::force_compute_actor=warn,\
             visionclaw_server::actors::physics_orchestrator_actor=info,\
             visionclaw_server::actors::client_coordinator_actor=info,\
             visionclaw_server::handlers::socket_flow_handler=warn",
            )
        }))
        .with(fmt::layer().with_target(true).with_thread_ids(true))
        .init();

    // Validate required environment variables (after tracing init so log macros work)
    if let Err(e) = validate_required_env_vars() {
        error!("Environment validation failed: {}", e);
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
    }

    // Record process start time for uptime reporting via /api/metrics
    let process_start_time = Instant::now();

    info!("--- Configuration Verification ---");
    info!(
        "MARKDOWN_DIR: {}",
        visionclaw_server::services::file_service::MARKDOWN_DIR
    );
    info!(
        "METADATA_PATH: {}",
        "/workspace/ext/data/metadata/metadata.json"
    );
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

    // Dev-mode banner: if the LAN-local full auth bypass is armed, say so LOUDLY
    // and repeatedly — an unauthenticated write door must never be silent. This
    // codepath exists only in dev/`dev-auth` builds; release stubs it out and
    // `enforce_release_env_hygiene()` already hard-failed the boot above if the
    // env var was present.
    #[cfg(any(debug_assertions, feature = "dev-auth"))]
    if visionclaw_server::utils::auth::dev_full_bypass_active() {
        warn!("╔══════════════════════════════════════════════════════════════════╗");
        warn!("║  VISIONCLAW_DEV_MODE=1 — LAN-LOCAL AUTH BYPASS ACTIVE             ║");
        warn!(
            "║  Every request is granted dev-admin ({}).      ║",
            visionclaw_server::utils::auth::DEV_MODE_PUBKEY
        );
        warn!("║  NO NIP-98 / token / peer check. Dev headset only — NEVER expose  ║");
        warn!("║  this port beyond the trusted LAN. Absent from release binaries.  ║");
        warn!("╚══════════════════════════════════════════════════════════════════╝");
    }

    let settings = match AppFullSettings::new() {
        Ok(s) => {
            info!(
                "AppFullSettings loaded successfully from: {}",
                std::env::var("SETTINGS_FILE_PATH")
                    .unwrap_or_else(|_| "/app/settings.yaml".to_string())
            );

            match to_json(&s.visualisation.rendering) {
                Ok(json_output) => {
                    info!(
                        "SERDE ALIAS FIX WORKS! JSON serialization (camelCase): {}",
                        json_output
                    );

                    if json_output.contains("ambientLightIntensity")
                        && !json_output.contains("ambient_light_intensity")
                    {
                        debug!(
                            "CONFIRMED: JSON uses camelCase field names for REST API compatibility"
                        );
                    }

                    debug!("CONFIRMED: Values loaded from snake_case YAML:");
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
                    debug!("SERDE ALIAS FIX IS WORKING: YAML (snake_case) loads successfully, JSON serializes as camelCase!");
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

    // SQLite settings repository for routes (ADR-11 §D5).
    // SettingsActor removed: OptimizedSettingsActor in AppState is the single source of truth.
    info!("Initializing SQLite settings repository for routes");
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let persistence_membership =
        visionclaw_server::services::data_reconciliation::StoreManifest::from_process();
    info!(
        "Selected persistence membership: {}",
        serde_json::to_string(&persistence_membership).expect("manifest serialises")
    );
    let settings_db_path = std::path::Path::new(&data_dir).join("settings.sqlite3");
    let settings_repository = match visionclaw_server::adapters::SqliteSettingsRepository::open(
        &settings_db_path,
    )
    .await
    {
        Ok(repo) => Arc::new(repo),
        Err(e) => {
            error!("Failed to open SQLite settings repository: {}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to open SQLite settings repository: {}", e),
            ));
        }
    };
    let settings_repo_data = web::Data::new(settings_repository.clone());
    info!("SQLite settings repository initialized successfully");

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
        debug!("[main] ragflow_service_option is Some after RAGFlowService::new attempt.");
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
    debug!("[main] Nostr service initialized");

    info!("[main] Initializing GitHub Sync Service...");
    let enhanced_content_api = Arc::new(EnhancedContentAPI::new(github_client.clone()));
    let github_sync_service = Arc::new(GitHubSyncService::new(
        enhanced_content_api,
        app_state.graph_adapter.clone()
            as Arc<
                dyn visionclaw_server::ports::knowledge_graph_repository::KnowledgeGraphRepository,
            >,
        app_state.ontology_repository.clone(),
        app_state.sqlite_settings_repository.clone(),
    ));
    // PRD-018 WS-3 / ADR-098 D1: this is the instance registered as web::Data and
    // used by POST /api/admin/sync. Wire it to the GPUManagerActor so a manual
    // re-sync re-dispatches materialised OWL axioms as GPU constraints. Without
    // this, admin/sync logged "GPUManagerActor address not registered" and the
    // 30k+ axioms were reasoned but never pushed to the live kernel.
    if let Some(gpu_manager) = app_state.gpu_manager_addr.clone() {
        github_sync_service.set_gpu_manager_addr(gpu_manager);
        info!("[main] Wired GPUManagerActor to admin/sync GitHubSyncService for semantic constraint dispatch");
    } else {
        warn!("[main] GPUManagerActor unavailable — admin/sync will not push GPU constraints");
    }
    info!("[main] GitHub Sync Service initialized");

    // Initialize SchemaService for natural language query support
    info!("[main] Initializing Schema Service...");
    let schema_service =
        Arc::new(visionclaw_server::services::schema_service::SchemaService::new());
    debug!("[main] Schema Service initialized");
    // Initialize Natural Language Query Service
    info!("[main] Initializing Natural Language Query Service...");
    let perplexity_service =
        Arc::new(visionclaw_server::services::perplexity_service::PerplexityService::new());
    let nl_query_service = Arc::new(visionclaw_server::services::natural_language_query_service::NaturalLanguageQueryService::new(
        schema_service.clone(),
        perplexity_service.clone(),
    ));
    info!("[main] Natural Language Query Service initialized");

    // Initialize Semantic Pathfinding Service
    info!("[main] Initializing Semantic Pathfinding Service...");
    let pathfinding_service = Arc::new(visionclaw_server::services::semantic_pathfinding_service::SemanticPathfindingService::default());
    info!("[main] Semantic Pathfinding Service initialized");

    // Initialize Ontology Agent Services (query + mutation + GitHub PR)
    info!("[main] Initializing Ontology Agent Services...");
    let whelk_engine = Arc::new(tokio::sync::RwLock::new(
        visionclaw_server::adapters::whelk_inference_engine::WhelkInferenceEngine::new(),
    ));
    let github_pr_service =
        Arc::new(visionclaw_server::services::github_pr_service::GitHubPRService::new());
    let ontology_query_service = Arc::new(visionclaw_server::services::ontology_query_service::OntologyQueryService::new(
        app_state.ontology_repository.clone(),
        app_state.graph_adapter.clone() as Arc<dyn visionclaw_server::ports::knowledge_graph_repository::KnowledgeGraphRepository>,
        whelk_engine.clone(),
        schema_service.clone(),
    ));
    // W-E transaction spine (ADR-049): idempotency store + write-ahead intent log.
    // In-memory by default (real, thread-safe); a durable SQLite-backed impl can
    // drop in behind the same traits without touching the propose pipeline.
    let proposal_idempotency =
        Arc::new(visionclaw_server::services::proposal_spine::InMemoryIdempotencyStore::new());
    let proposal_intents =
        Arc::new(visionclaw_server::services::proposal_spine::InMemoryIntentLog::new());
    let ontology_mutation_service = Arc::new(
        visionclaw_server::services::ontology_mutation_service::OntologyMutationService::new(
            app_state.ontology_repository.clone(),
            whelk_engine.clone(),
            github_pr_service.clone(),
            proposal_idempotency,
            proposal_intents,
        )
        // PRD-022 WS-2: share the Oxigraph store so committed proposals reify
        // queryable PROV-O provenance (single-writer, ADR-11 §D1).
        .with_provenance_store(app_state.ontology_repository.store().clone()),
    );

    // W-B (PRD-022 / ADR-048/049): governed decision write door. Shares the same
    // Oxigraph store as the ontology adapter (single-writer, ADR-11 §D1) and its
    // own spine idempotency + intent log; decision writes ride the spine's single
    // transaction (asserted decision quads + T3 PROV-O provenance quads, atomically).
    // ADR-050 write-half: the decision-elevation actor (env-gated like the
    // ElevationActor — dev/staging default ON, production opt-in, requires
    // FORUM_RELAY_URL + a panel secret). When live it receives a SIGNIFICANT
    // recorded decision via the fire-and-forget sink and opens a broker case for
    // corpus publication; `None` keeps decisions runtime-only. The sink is
    // fail-open: it can never fail the governed decision write.
    let decision_elevation_sink: Option<
        Arc<dyn visionclaw_server::services::decision_elevation::DecisionElevationSink>,
    > = match visionclaw_server::actors::decision_elevation_actor::DecisionElevationActor::new() {
        Some(actor) => {
            let addr = actix::Actor::start(actor);
            info!("[main] DecisionElevationActor started (ADR-050 decision-elevation panel live)");
            Some(Arc::new(
                visionclaw_server::actors::decision_elevation_actor::ActorElevationSink::new(addr),
            ))
        }
        None => {
            info!("[main] DecisionElevationActor disabled (set DECISION_ELEVATION_ENABLED=1 + FORUM_RELAY_URL + ACSP_PANEL_NOSTR_PRIVKEY to enable)");
            None
        }
    };

    let decision_service = Arc::new({
        let svc = visionclaw_server::services::decision_service::DecisionService::new(
            app_state.ontology_repository.clone(),
            app_state.ontology_repository.store().clone(),
            Arc::new(visionclaw_server::services::proposal_spine::InMemoryIdempotencyStore::new()),
            Arc::new(visionclaw_server::services::proposal_spine::InMemoryIntentLog::new()),
        );
        match decision_elevation_sink {
            Some(sink) => svc.with_elevation_sink(sink),
            None => svc,
        }
    });
    info!("[main] Ontology Agent Services initialized");

    info!("--- Starting Data Orchestration Sequence ---");

    // Step 1: Sync Files from GitHub.
    info!("[Startup] Step 1: Syncing files from GitHub to local storage...");
    let github_sync_failed = if let Err(e) =
        visionclaw_server::services::file_service::FileService::initialize_local_storage(
            settings.clone(),
        )
        .await
    {
        error!(
            "[Startup] FAILED to sync from GitHub: {}. Will try local files.",
            e
        );
        true
    } else {
        info!("[Startup] SUCCESS: Local file storage is synchronized with GitHub.");
        false
    };

    // Step 1b: If GitHub sync failed or metadata is empty, scan local files
    let metadata =
        visionclaw_server::services::file_service::FileService::load_or_create_metadata()
            .unwrap_or_default();
    if github_sync_failed || metadata.is_empty() {
        info!("[Startup] Step 1b: Scanning local markdown files as fallback...");
        match visionclaw_server::services::file_service::FileService::scan_local_files_to_metadata()
        {
            Ok(local_metadata) => {
                info!(
                    "[Startup] SUCCESS: Scanned {} public files from local storage.",
                    local_metadata.len()
                );
            }
            Err(e) => {
                error!("[Startup] FAILED to scan local files: {}", e);
            }
        }
    }

    // Step 2: Load Files into Oxigraph store (ADR-11).
    info!("[Startup] Step 2: Populating Oxigraph store from local files...");
    {
        use visionclaw_server::ports::knowledge_graph_repository::KnowledgeGraphRepository;
        let kg_repo: Arc<dyn KnowledgeGraphRepository> =
            app_state.graph_adapter.clone() as Arc<dyn KnowledgeGraphRepository>;
        if let Err(e) =
            visionclaw_server::services::file_service::FileService::load_graph_from_files(&kg_repo)
                .await
        {
            error!("[Startup] FATAL: Failed to populate Oxigraph store: {}. Application is in DEGRADED state.", e);
            app_state.set_degraded(format!("Oxigraph init failed: {}", e));
        } else {
            info!("[Startup] SUCCESS: Oxigraph store is populated and ready.");
        }
    }

    // Step 3: Notify Actors.
    info!("[Startup] Step 3: Notifying actors to reload graph state from database...");
    app_state
        .graph_service_addr
        .do_send(ReloadGraphFromDatabase);
    info!("[Startup] SUCCESS: Actors notified.");
    info!("--- Data Orchestration Sequence Complete ---");

    info!("Skipping bots orchestrator connection during startup (will connect on-demand)");

    info!("Loading ontology graph from Oxigraph store (ADR-11)...");

    let graph_data_option = match app_state.ontology_repository.load_ontology_graph().await {
        Ok(graph_arc) => {
            let graph = graph_arc.as_ref();
            if !graph.nodes.is_empty() {
                info!(
                    "Loaded ontology graph from database: {} nodes, {} edges",
                    graph.nodes.len(),
                    graph.edges.len()
                );
                info!("Ontology classes loaded but NOT sent to actor (KG nodes will be loaded via ReloadGraphFromDatabase)");
                Some((*graph_arc).clone())
            } else {
                info!("📂 Ontology database is empty - waiting for GitHub sync to populate");
                info!("Ontology classes will be loaded after sync extracts OWL data");
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
        info!("Ontology classes are available via API endpoints but nodes come from KG sync");
    } else {
        info!("⏳ GraphServiceActor will be populated by ReloadGraphFromDatabase from existing KG nodes");
        info!("If no KG nodes exist, you can trigger GitHub sync via /api/admin/sync endpoint");
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

    // Oxigraph provenance for NostrBeadPublisher deferred to Phase 2 (ADR-11).
    // todo!("Phase 2: wire OxigraphOntologyRepository into NostrBeadPublisher for provenance triples")
    let nostr_publisher = web::Data::new(NostrBeadPublisher::from_env());

    // Spawn bridge as background task (no-op if FORUM_RELAY_URL is not set).
    if let Some(bridge) = NostrBridge::from_env() {
        tokio::spawn(bridge.run());
        info!("[main] NostrBridge spawned");
    } else {
        info!(
            "[main] NostrBridge not started (VISIONCLAW_NOSTR_PRIVKEY or FORUM_RELAY_URL not set)"
        );
    }

    let app_state_data = web::Data::new(app_state);
    // Opt-in checkpoint uses the open writer's consistent RocksDB snapshot.
    // It never opens a second writer or copies live WAL/SST files by hand.
    if let Ok(backup_root) = std::env::var("ONTOLOGY_BACKUP_DIR") {
        let store = Arc::clone(app_state_data.ontology_repository.store());
        let source = std::path::PathBuf::from(&data_dir);
        let destination = std::path::PathBuf::from(backup_root);
        match tokio::task::spawn_blocking(move || {
            visionclaw_server::services::data_reconciliation::checkpoint_oxigraph(
                &store,
                &source,
                &destination,
            )
        })
        .await
        {
            Ok(Ok(path)) => info!("Ontology checkpoint created at {}", path.display()),
            result => {
                return Err(std::io::Error::other(format!(
                    "configured ontology checkpoint failed: {result:?}"
                )))
            }
        }
    }

    // ADR-142 multi-user RBAC: build the pubkey→role store over the settings
    // SQLite connection, bootstrap the Owner from RBAC_OWNER_PUBKEY, and install
    // it as the process-global that `verify_access` / the `/api` RbacGate
    // consult. Non-fatal on failure — auth degrades to the legacy power-user
    // mapping rather than taking the server down.
    {
        use visionclaw_server::services::role_store::{
            set_global_role_store, RoleStore, RBAC_ALLOW_OWNERLESS_ENV,
        };
        let conn = app_state_data
            .sqlite_settings_repository
            .connection()
            .clone();
        match RoleStore::new(conn).await {
            Ok(store) => {
                if let Err(e) = store.bootstrap_owner_from_env().await {
                    warn!("[rbac] Owner bootstrap failed: {e}");
                }
                // Fail closed on an owner-less store (Codex review [MED]): with no
                // Owner, nobody can ever grant Admin/Owner — a permanent lockout.
                // Legacy single-user deployments (where the POWER_USER_PUBKEYS →
                // Admin fallback is the intended reality) opt out explicitly with
                // RBAC_ALLOW_OWNERLESS=1.
                let allow_ownerless = std::env::var(RBAC_ALLOW_OWNERLESS_ENV)
                    .map(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                match store.has_owner().await {
                    Ok(true) => {}
                    Ok(false) if allow_ownerless => {
                        warn!(
                            "[rbac] no Owner assigned but {RBAC_ALLOW_OWNERLESS_ENV}=1 — \
                             running owner-less; role management (grant Admin/Owner) is \
                             unavailable, only the POWER_USER_PUBKEYS→Admin fallback applies"
                        );
                    }
                    Ok(false) => {
                        error!(
                            "[rbac] FATAL: no Owner is assigned. Set RBAC_OWNER_PUBKEY=<64-hex \
                             pubkey> to bootstrap one, or RBAC_ALLOW_OWNERLESS=1 to permit a \
                             legacy owner-less deployment. Refusing to start (fail-closed)."
                        );
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "RBAC: no Owner assigned and RBAC_ALLOW_OWNERLESS not set",
                        ));
                    }
                    Err(e) => {
                        error!("[rbac] FATAL: could not determine Owner presence: {e}");
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("RBAC: owner check failed: {e}"),
                        ));
                    }
                }
                if set_global_role_store(std::sync::Arc::new(store)).is_err() {
                    warn!("[rbac] global role store already initialised");
                } else {
                    info!("[rbac] multi-user role store initialised (ADR-142)");
                }
            }
            Err(e) => {
                // Fail closed (Codex round-2): a failed store init previously
                // continued WITHOUT the store, so `verify_access` silently fell
                // back to the legacy power-user→Admin mapping — a fail-OPEN
                // downgrade. Refuse to start instead.
                error!("[rbac] FATAL: failed to initialise role store: {e}");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("RBAC: role store initialisation failed: {e}"),
                ));
            }
        }
    }

    let validation_service = web::Data::new(validation_handler::ValidationService::new());

    // Initialize PhysicsService so POST /api/physics/reset and related endpoints work.
    // ActixPhysicsAdapter wraps PhysicsOrchestratorActor; the actor addr is populated
    // lazily by the adapter's own initialization path.
    let physics_service = {
        use visionclaw_server::adapters::actix_physics_adapter::ActixPhysicsAdapter;
        use visionclaw_server::application::PhysicsService;
        use visionclaw_server::events::EventBus;
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

    // PRD-008 §5.3 — XR presence room registry + Schnorr identity verifier.
    // Shared across all Actix workers via web::Data (created before the
    // HttpServer::new move closure so each worker clones the same registry).
    let presence_registry = new_room_registry();
    let presence_verifier: std::sync::Arc<dyn visionclaw_xr_presence::ports::IdentityVerifier> =
        std::sync::Arc::new(
            visionclaw_server::services::nostr_identity_verifier::NostrIdentityVerifier::new(),
        );
    let presence_handler_state = web::Data::new(PresenceHandlerState {
        registry: presence_registry,
        identity_verifier: presence_verifier,
        seen_nonces: new_seen_nonce_cache(),
    });
    info!("[ws-presence] XR presence registry + verifier initialised at /ws/presence");

    // Pre-initialise Solid pod state in the main async context (FsBackend::new
    // is async). The state is injected via app_data so Actix workers don't need
    // to run async init inside their sync configure closure.
    #[cfg(feature = "solid-pod-embed")]
    let solid_state = visionclaw_server::handlers::init_solid_state().await;

    // ADR-2106: the published ontology (ontology-publish.yml, release
    // `ontology-latest`) is pulled INTO the pod at boot and re-checked on an
    // interval, because a hosted runner cannot reach this in-process pod.
    // Fail-open: the pod keeps what it has if the release is unreachable.
    #[cfg(feature = "solid-pod-embed")]
    visionclaw_server::services::ontology_pull::spawn_boot_pull(Arc::clone(&solid_state.storage));

    // HTTP 402 payments + exchange state (Web Ledgers + AMM). Default-disabled via
    // PAY_ENABLED=false; ledger and order book persist to PAY_LEDGER_DIR. Built once
    // here and cloned into each Actix worker via app_data, matching presence/solid.
    #[cfg(feature = "solid-pod-embed")]
    let (pay_config_data, pay_ledger_data, pay_exchange_data) = {
        use visionclaw_server::handlers::pay_handler::{
            FsExchangeStore, FsPaymentStore, VcPayConfig,
        };
        let cfg = VcPayConfig::from_env();
        let ledger_dir = cfg.ledger_dir.clone();
        let ledger = Arc::new(FsPaymentStore::new(&ledger_dir).expect("init payment ledger store"));
        let exchange = Arc::new(FsExchangeStore::new(&ledger_dir).expect("init exchange store"));
        (
            web::Data::new(cfg),
            web::Data::new(ledger),
            web::Data::new(exchange),
        )
    };

    // ADR-2038 — boot-time effective-profile assertion, evaluated BEFORE the
    // listener binds. ADR-2026's `enforce_release_env_hygiene()` above covers
    // three suspect variables and the argv flag; this covers the *whole*
    // effective profile: the dev-token opt-in (ADR-2012), the RBAC report mode
    // that a dated acknowledgement could otherwise activate in a release build,
    // the dev-auth artefact identity (ADR-2037) and drift from the declared
    // ADR-2027 profile. In a production artefact any finding exits(2) here, so
    // a mis-promoted image never accepts a request; a development build logs
    // and continues.
    let boot_profile = {
        use visionclaw_server::config::security_profile::{
            assert_effective_profile_or_exit, BuildIdentity, EnvSnapshot,
        };
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_effective_profile_or_exit(
            &EnvSnapshot::from_process(),
            BuildIdentity::current(),
            &today,
        )
    };
    info!(
        "main: effective security profile — {} (observed flags: {:?})",
        boot_profile.summary(),
        boot_profile.observed_flags
    );

    info!("main: All services and actors initialized. Configuring HTTP server.");
    // RES-a: capture the harness handle before `app_state_data` moves into the
    // server factory closure, so the KG watchdog spawned below can reach it.
    let watchdog_harness = app_state_data.liveness_harness.clone();
    // REC-4: likewise capture the KPI store handle for the agent-event volume
    // tap spawned below, before `app_state_data` moves into the closure.
    let tap_kpi_repo = app_state_data.sqlite_kpi_repository.clone();
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
            .wrap(TimeoutMiddleware::with_config(
                visionclaw_server::middleware::TimeoutConfig::new(Duration::from_secs(30))
                    .with_override("/api/admin/sync", Duration::from_secs(600))
            ))


            .app_data(settings_data.clone())
            .app_data(web::Data::new(github_client.clone()))
            .app_data(web::Data::new(content_api.clone()))
            .app_data(app_state_data.clone())
            // RES-a: the LivenessHarness backs /api/canary/* and the KG watchdog.
            .app_data(web::Data::from(app_state_data.liveness_harness.clone()))
            // REC-4: the KpiComputeService backs /api/kpi/{summary,lineage}.
            .app_data(web::Data::from(app_state_data.kpi_compute_service.clone()))
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
            .app_data(web::Data::new(decision_service.clone()))
            .app_data(settings_repo_data.clone())
            .app_data(briefing_service.clone())
            .app_data(nostr_publisher.clone())
            .app_data(validation_service.clone())
            .app_data(physics_service.clone())
            // PRD-008 — XR presence handler (Quest 3 native APK)
            .app_data(presence_handler_state.clone());

            // Inject pre-initialised Solid pod state (avoids async init in worker threads)
            #[cfg(feature = "solid-pod-embed")]
            let app = app.app_data(solid_state.clone());

            // HTTP 402 payment + exchange state and `/pay/*` routes.
            // Endpoints are inert until PAY_ENABLED=true (`.info` reports disabled,
            // gated routes return 403), so this is safe to mount unconditionally.
            #[cfg(feature = "solid-pod-embed")]
            let app = app
                .app_data(pay_config_data.clone())
                .app_data(pay_ledger_data.clone())
                .app_data(pay_exchange_data.clone())
                .configure(visionclaw_server::handlers::pay_handler::configure_pay_routes);

            let app = app
            // Root-level k8s/Docker probes (the /api/* variants below are kept for back-compat)
            .route("/healthz", web::get().to(consolidated_health_handler::liveness_probe))
            .route("/readyz", web::get().to(consolidated_health_handler::readiness_probe))
            .route("/wss", web::get().to(socket_flow_handler))
            // ADR-059 §1: authenticated inbound agent_action ingest (agentbox → VisionClaw)
            .route("/wss/agent-events", web::get().to(visionclaw_server::agent_events::agent_events_ws))
            .route("/ws/speech", web::get().to(speech_socket_handler))
            .route("/ws/mcp-relay", web::get().to(mcp_relay_handler))

            .route("/ws/client-messages", web::get().to(client_messages_handler::websocket_client_messages))
            // PRD-008 §5.3: XR presence WebSocket — Quest 3 native APK multi-user sync
            .route("/ws/presence", web::get().to(ws_presence))
            // OpenAPI/Swagger documentation
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", visionclaw_server::openapi::ApiDoc::openapi())
            )
            .service(
                web::scope("/api")
                    // Pre-demo security fix #1 (audit 2026-08-21): when
                    // PUBLIC_DEMO=read-only, reject every mutating method on the
                    // whole /api scope with 403. Inert (passthrough) by default.
                    .wrap(visionclaw_server::middleware::PublicDemoGuard::from_env())
                    // ADR-142 multi-user RBAC: central gate over the whole /api
                    // scope. Public reads pass through; mutations require an
                    // authenticated Editor+, /api/settings writes require
                    // WriteSettings, and /api/admin/* requires Admin. Auth
                    // endpoints (/api/auth/*) and client-logs are allowlisted.
                    // Mode via RBAC_GATE_MODE (enforce|report; default enforce).
                    .wrap(visionclaw_server::middleware::RbacGate::from_env())
                    // Client logs route - registered early to avoid scope conflicts
                    .route("/client-logs", web::post().to(client_log_handler::handle_client_logs))
                    // ADR-142: RBAC admin management surface (/api/admin/rbac/*)
                    .configure(admin_rbac_handler::configure_routes)
                    .service(
                        web::scope("/settings")
                            .wrap(RateLimit::per_minute(60))
                            .configure(visionclaw_server::settings::api::configure_routes)
                    )
                    // WS-9: register /ontology/derived BEFORE api_handler::config.
                    // actix scopes match in registration order by path segment, and
                    // the /ontology scope (inside api_handler::config) would otherwise
                    // shadow /ontology/derived → 404. Registering the more-specific
                    // scope first lets /api/ontology/derived match correctly while
                    // /api/ontology/* still falls through to the /ontology scope.
                    .configure(visionclaw_server::handlers::configure_ontology_derived_routes)
                    // RES-d fix: /ontology/class-count MUST also be registered BEFORE
                    // api_handler::config for the SAME reason — the broad /ontology
                    // scope inside api_handler::ontology::config matches the
                    // /ontology/class-count prefix and, finding no inner route, 404s
                    // without falling through (actix does not fall through a matched
                    // scope prefix). Registered here as the more-specific scope, the
                    // canon DriftCounter's GET /api/ontology/class-count is reachable.
                    // (Guarded by tests/resd_class_count_route.rs.)
                    .configure(visionclaw_server::handlers::configure_ontology_class_count_routes)
                    .configure(api_handler::config)
                    .configure(workspace_handler::config)
                    .configure(admin_sync_handler::configure_routes)
                    .configure(validation_handler::config)

                    // Pipeline admin routes removed (SQLite-specific handlers deleted in Oxigraph migration, ADR-11)
                    // Cypher query endpoint removed (handler deleted in Oxigraph migration, ADR-11)

                    // Phase 5: Hexagonal architecture handlers
                    .configure(visionclaw_server::handlers::configure_physics_routes)
                    .configure(visionclaw_server::handlers::configure_schema_routes)
                    .configure(visionclaw_server::handlers::configure_nl_query_routes)
                    .configure(visionclaw_server::handlers::configure_pathfinding_routes)
                    .configure(visionclaw_server::handlers::configure_semantic_routes)
                    // ADR-2066: `/api/inference/*` removed. Every handler in the
                    // Phase 7 inference stack extracted an `InferenceService` that
                    // was never registered as app data, so each route 500'd at the
                    // extractor. The live reasoning path is
                    // `GitHubSyncService::run_post_sync_reasoning`, untouched.

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
                    .configure(visionclaw_server::handlers::configure_ontology_agent_routes)

                    // Governed decision records + bounded trace (PRD-022 W-B /
                    // ADR-048): /api/decisions/record is authed like /propose.
                    .configure(visionclaw_server::handlers::configure_decision_routes)

                    // Solid Pod (embedded solid-pod-rs)
                    .configure(visionclaw_server::handlers::configure_solid_routes)

                    // Image generation via ComfyUI (Flux2)
                    .configure(visionclaw_server::handlers::configure_image_gen_routes)

                    // Briefing workflow (voice → brief → role agents → debrief)
                    .configure(visionclaw_server::handlers::configure_briefing_routes)

                    // Memory flash events (RuVector access → WS broadcast to all clients)
                    .configure(visionclaw_server::handlers::configure_memory_flash_routes)

                    // Enrichment-proposals broker write-back (governance decisions)
                    .configure(visionclaw_server::handlers::configure_enrichment_proposals_routes)

                    // (WS-9 derived ontology graphs registered earlier — before
                    // api_handler::config — to avoid the /ontology scope shadowing it.)

                    // Broker inbox read surface (WS-12) — agentbox broker-bridge
                    .configure(visionclaw_server::handlers::configure_broker_inbox_routes)

                    // GOV-4: git-ingest write-back (`/api/ingest/writeback`) —
                    // adapts the agentbox git-bridge WriteBackSaga POST onto the
                    // shared enrichment-decide core (was unregistered → 404).
                    .configure(visionclaw_server::handlers::configure_ingest_writeback_routes)

                    // RES-a: LivenessHarness — /api/canary/{register,observe,status}
                    .configure(visionclaw_server::handlers::configure_liveness_routes)

                    // REC-4 (ADR-043 resurrection): four-KPI dashboard —
                    // /api/kpi/{summary,lineage}
                    .configure(visionclaw_server::handlers::configure_kpi_routes)

                    // REC-10 (PRD-023 WP-12): Insight Ingestion Loop v1 —
                    // /api/insight-loop/trace[/{case_id}] (Mesh Velocity source)
                    .configure(visionclaw_server::handlers::configure_insight_loop_routes)

                    // REC-11 (PRD-023 WP-12): data-moat unified provenance trace —
                    // /api/trace (joins agent-events + broker decisions on did:nostr)
                    .configure(visionclaw_server::handlers::configure_trace_routes)

                    // (RES-d /ontology/class-count registered EARLIER — before
                    // api_handler::config — so the broad /ontology scope does not
                    // shadow it to a 404. See the WS-9/RES-d note above.)

                    // Layout mode system (ADR-031)
                    .configure(visionclaw_server::handlers::configure_layout_routes)

            );

            app
        })
        .bind(&bind_address)?
        .workers(4)
        .run();

    let server_handle = server.handle();

    // RES-a: spawn the KG-backend watchdog once the HTTP server is live. It
    // self-polls /api/health (this server IS the KG backend) and drives the
    // kg_backend_up gauge, firing CANARY-VC-RESA-KG on every transition.
    {
        let harness = watchdog_harness.clone();
        let self_url = std::env::var("VISIONCLAW_SELF_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{}", port));
        let watchdog_secs = std::env::var("VISIONCLAW_KG_WATCHDOG_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);
        tokio::spawn(
            visionclaw_server::services::liveness_harness::run_kg_watchdog(
                harness,
                self_url,
                std::time::Duration::from_secs(watchdog_secs),
            ),
        );
    }

    // REC-4 (ADR-130 D5): the agent-event volume tap. Subscribes to the
    // process-global /wss/agent-events hub (the same seam the render actor uses)
    // and records one volume row per envelope into the KPI store — the
    // Augmentation Ratio numerator source, read without new emit-site
    // instrumentation. Detached; fail-open on a lagged/closed channel.
    {
        let kpi_repo = tap_kpi_repo.clone();
        tokio::spawn(visionclaw_server::services::kpi_compute::run_agent_event_tap(kpi_repo));
        info!("[main] KPI agent-event volume tap spawned");
    }

    // RES-a / WP-11 AC3 / ADR-130 D3: the Nostr-relay tap lets Nostr-only
    // repositories (nostr-rust-forum, solid-pod-rs) fire canaries they cannot
    // POST over HTTP. Feeds the SAME LivenessHarness.observe path. Disabled
    // unless CANARY_TAP_RELAY_URL is set; fail-open (its own detached task).
    if let Some(tap) = visionclaw_server::services::canary_nostr_tap::CanaryNostrTap::from_env(
        watchdog_harness.clone(),
    ) {
        tokio::spawn(tap.run());
        info!("[main] canary Nostr tap spawned");
    } else {
        info!("[main] canary Nostr tap not started (CANARY_TAP_RELAY_URL not set)");
    }

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
