// Rebuild: KE velocity fix applied
use actix::Actor;
use webxr::ports::ontology_repository::OntologyRepository;
use webxr::services::nostr_service::NostrService;
use webxr::settings::settings_actor::SettingsActor;
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
        multi_mcp_websocket_handler,
        nostr_handler,
        pages_handler,
        socket_flow_handler::{socket_flow_handler, PreReadSocketSettings}, 
        speech_socket_handler::speech_socket_handler,
        
        
        workspace_handler,
    },
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
use log::{debug, error, info};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use tokio::time::Duration;
use webxr::middleware::TimeoutMiddleware;
use webxr::telemetry::agent_telemetry::init_telemetry_logger;
use webxr::utils::advanced_logging::init_advanced_logging;
use webxr::utils::json::to_json;
// REMOVED: use webxr::utils::logging::init_logging; - legacy logging superseded by advanced_logging

// DEPRECATED: ErrorRecoveryMiddleware removed - NetworkRecoveryManager deleted


#[actix_web::main]
async fn main() -> std::io::Result<()> {

    dotenv().ok();

    // Initialize env_logger for console output - MUST be before any log macros
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

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
    info!("Initializing SettingsActor with Neo4j");
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

    let settings_actor = SettingsActor::new(settings_repository.clone()).start();
    let settings_actor_data = web::Data::new(settings_actor);
    let neo4j_repo_data = web::Data::new(settings_repository.clone());
    info!("SettingsActor initialized successfully");



    let settings_data = web::Data::new(settings.clone());

    
    let github_config = match GitHubConfig::from_env() {
        Ok(config) => config,
        Err(e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to load GitHub config: {}", e),
            ))
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
        error!("[Startup] FATAL: Failed to populate Neo4j: {}. The application may not function correctly.", e);
        // Optional: Exit if this step is critical
        // return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
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

    
    let app_state_data = web::Data::new(app_state);
    

    
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
            .app_data(settings_actor_data.clone())
            .app_data(neo4j_repo_data.clone()) 
            
            
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
                    .service(web::scope("/settings").configure(webxr::settings::api::configure_routes))
                    .configure(api_handler::config)
                    .configure(workspace_handler::config)
                    .configure(admin_sync_handler::configure_routes)

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

                    // Multi-MCP WebSocket
                    .configure(multi_mcp_websocket_handler::configure_multi_mcp_routes)

                    .service(web::scope("/pages").configure(pages_handler::config))
                    .service(web::scope("/bots").configure(api_handler::bots::config))
                    .configure(bots_visualization_handler::configure_routes)
                    .configure(graph_export_handler::configure_routes)

                    // JavaScript Solid Server (JSS) integration
                    .configure(webxr::handlers::configure_solid_routes)

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
