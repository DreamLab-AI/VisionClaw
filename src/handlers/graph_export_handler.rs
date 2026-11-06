use crate::adapters::sqlite_share_repository::{AccessClaim, SqliteShareRepository};
use crate::middleware::RequireAuth;
use crate::models::graph_export::*;
use crate::services::graph_serialization::GraphSerializationService;
use crate::AppState;
use crate::{
    bad_request, error_json, forbidden, not_found, ok_json, too_many_requests, unauthorized,
};
use actix_web::{http::header::HeaderValue, web, HttpRequest, HttpResponse, Result as ActixResult};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use visionclaw_domain::models::graph::GraphData;

#[derive(Debug, Clone, Default)]
pub struct RateLimitState {
    pub requests: Vec<DateTime<Utc>>,
    pub daily_count: u32,
    pub hourly_count: u32,
}

type RateLimitStorage = Arc<RwLock<HashMap<String, RateLimitState>>>;

/// The `/api/graph-export` state. Built once in `main` and shared by every actix worker, so a
/// share and a rate-limit count are the same on every worker.
pub struct GraphExportHandler {
    serialization_service: GraphSerializationService,
    shares: Arc<SqliteShareRepository>,
    rate_limits: RateLimitStorage,
    daily_export_limit: u32,
    hourly_export_limit: u32,
}

impl GraphExportHandler {
    /// A handler over `storage_path` (export and share files) and the share store.
    pub fn new(storage_path: std::path::PathBuf, shares: Arc<SqliteShareRepository>) -> Self {
        Self {
            serialization_service: GraphSerializationService::new(storage_path),
            shares,
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            daily_export_limit: 100,
            hourly_export_limit: 20,
        }
    }

    async fn check_rate_limit(&self, client_ip: &str) -> Result<RateLimitInfo> {
        let mut rate_limits = self.rate_limits.write().await;
        let now = Utc::now();

        let state = rate_limits
            .entry(client_ip.to_string())
            .or_insert_with(RateLimitState::default);

        state
            .requests
            .retain(|&timestamp| now.signed_duration_since(timestamp).num_hours() < 24);

        let hourly_count = state
            .requests
            .iter()
            .filter(|&&timestamp| now.signed_duration_since(timestamp).num_hours() < 1)
            .count() as u32;

        let daily_count = state.requests.len() as u32;

        if daily_count >= self.daily_export_limit {
            return Ok(RateLimitInfo {
                remaining_exports: 0,
                reset_time: now + chrono::Duration::days(1),
                daily_limit: self.daily_export_limit,
                hourly_limit: self.hourly_export_limit,
            });
        }

        if hourly_count >= self.hourly_export_limit {
            return Ok(RateLimitInfo {
                remaining_exports: 0,
                reset_time: now + chrono::Duration::hours(1),
                daily_limit: self.daily_export_limit,
                hourly_limit: self.hourly_export_limit,
            });
        }

        state.requests.push(now);
        state.daily_count = daily_count + 1;
        state.hourly_count = hourly_count + 1;

        Ok(RateLimitInfo {
            remaining_exports: self.daily_export_limit - daily_count - 1,
            reset_time: now + chrono::Duration::days(1),
            daily_limit: self.daily_export_limit,
            hourly_limit: self.hourly_export_limit,
        })
    }

    /// The caller's view of the current graph (ADR-060): an export or share never carries
    /// another owner's private nodes.
    async fn get_current_graph(
        &self,
        app_state: &AppState,
        caller: &crate::settings::OptionalAuth,
    ) -> Result<GraphData> {
        use crate::actors::messages::GetGraphData;

        match app_state.graph_service_addr.send(GetGraphData).await {
            Ok(Ok(graph_data)) => {
                Ok((*crate::handlers::graph_access::caller_view(&graph_data, caller)).clone())
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Graph service error: {}", e)),
            Err(e) => Err(anyhow::anyhow!("Graph service actor not responding: {}", e)),
        }
    }
}

pub async fn export_graph(
    app_state: web::Data<AppState>,
    handler: web::Data<GraphExportHandler>,
    request: web::Json<ExportRequest>,
    req: HttpRequest,
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let client_ip = req
        .connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string();

    match handler.check_rate_limit(&client_ip).await {
        Ok(rate_info) if rate_info.remaining_exports == 0 => {
            return too_many_requests!("Rate limit exceeded");
        }
        Err(e) => {
            return error_json!("Rate limit check failed: {}", e);
        }
        _ => {}
    }

    // Every share has a creator, so someone can always delete it (VC-19); an anonymous caller
    // cannot create one.
    if caller.0.is_none() {
        return unauthorized!("Sharing a graph requires a signed-in caller");
    }

    let graph = match handler.get_current_graph(&app_state, &caller).await {
        Ok(graph) => graph,
        Err(e) => {
            return error_json!("Failed to get graph: {}", e);
        }
    };

    match handler
        .serialization_service
        .export_graph(&graph, &request)
        .await
    {
        Ok(export_response) => ok_json!(export_response),
        Err(e) => error_json!("Export failed: {}", e),
    }
}

pub async fn share_graph(
    app_state: web::Data<AppState>,
    handler: web::Data<GraphExportHandler>,
    request: web::Json<ShareRequest>,
    req: HttpRequest,
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let client_ip = req
        .connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string();

    match handler.check_rate_limit(&client_ip).await {
        Ok(rate_info) if rate_info.remaining_exports == 0 => {
            return too_many_requests!("Rate limit exceeded");
        }
        Err(e) => {
            return error_json!("Rate limit check failed: {}", e);
        }
        _ => {}
    }

    let graph = match handler.get_current_graph(&app_state, &caller).await {
        Ok(graph) => graph,
        Err(e) => {
            return error_json!("Failed to get graph: {}", e);
        }
    };

    match handler
        .serialization_service
        .create_shared_graph(
            &graph,
            &request,
            caller.0.as_ref().map(|u| u.pubkey.clone()),
        )
        .await
    {
        Ok((shared_graph, share_response)) => match handler.shares.insert(&shared_graph).await {
            Ok(()) => ok_json!(share_response),
            Err(e) => {
                if let Err(rm) = std::fs::remove_file(&shared_graph.file_path) {
                    log::warn!("Failed to remove unrecorded share file: {}", rm);
                }
                error_json!("Failed to record shared graph: {}", e)
            }
        },
        Err(e) => error_json!("Failed to create shared graph: {}", e),
    }
}

pub async fn get_shared_graph(
    handler: web::Data<GraphExportHandler>,
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
) -> ActixResult<HttpResponse> {
    let share_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(id) => id,
        Err(_) => {
            return bad_request!("Invalid share ID format");
        }
    };

    let shared_graph = match handler.shares.get(share_id).await {
        Ok(Some(graph)) => graph,
        Ok(None) => return not_found!("Shared graph not found"),
        Err(e) => return error_json!("Failed to read share store: {}", e),
    };

    if shared_graph.is_expired() {
        return Ok(HttpResponse::Gone().json(serde_json::json!({
            "error": "Shared graph has expired"
        })));
    }

    if shared_graph.access_limit_reached() {
        return forbidden!("Access limit reached for this shared graph");
    }

    if let Some(password) = query.get("password") {
        if !shared_graph.validate_password(password) {
            return unauthorized!("Invalid password");
        }
    } else if shared_graph.password_hash.is_some() {
        return unauthorized!("Password required");
    }

    match handler.shares.claim_access(share_id).await {
        Ok(AccessClaim::Granted) => {}
        Ok(AccessClaim::LimitReached) => {
            return forbidden!("Access limit reached for this shared graph")
        }
        Ok(AccessClaim::NotFound) => return not_found!("Shared graph not found"),
        Err(e) => return error_json!("Failed to read share store: {}", e),
    }

    match std::fs::read(&shared_graph.file_path) {
        Ok(file_data) => {
            let content_type = match shared_graph.original_format {
                ExportFormat::Json => "application/json",
                ExportFormat::Gexf | ExportFormat::Graphml => "application/xml",
                ExportFormat::Csv => "text/csv",
                ExportFormat::Dot => "text/plain",
            };

            let mut response = HttpResponse::Ok()
                .content_type(content_type)
                .body(file_data);

            if shared_graph.compressed {
                response.headers_mut().insert(
                    actix_web::http::header::CONTENT_ENCODING,
                    HeaderValue::from_static("gzip"),
                );
            }

            Ok(response)
        }
        Err(e) => error_json!("Failed to read shared graph file: {}", e),
    }
}

pub async fn publish_graph(
    _app_state: web::Data<AppState>,
    _request: web::Json<PublishRequest>,
    _req: HttpRequest,
) -> ActixResult<HttpResponse> {
    Ok(HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "Graph publishing not yet implemented"
    })))
}

pub async fn delete_shared_graph(
    handler: web::Data<GraphExportHandler>,
    path: web::Path<String>,
    caller: crate::settings::OptionalAuth,
) -> ActixResult<HttpResponse> {
    let share_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(id) => id,
        Err(_) => {
            return bad_request!("Invalid share ID format");
        }
    };

    // Only the recorded creator may delete, plus a power user for a legacy creator-less share;
    // anyone else and an anonymous caller get "not found", so the refusal does not confirm the
    // share exists (review L1). The rules are the store's `delete_owned` and `delete_unowned`.
    let is_power_user = caller.0.as_ref().is_some_and(|u| u.is_power_user);
    let caller = caller.0.as_ref().map(|u| u.pubkey.as_str());
    let removed_graph = match handler.shares.delete_owned(share_id, caller).await {
        Ok(Some(removed)) => Some(removed),
        // A legacy share with no recorded creator (made before creators were required) may be
        // deleted by a power user, so it is not undeletable forever. Any other refusal still
        // reads as "not found".
        Ok(None) if is_power_user => match handler.shares.delete_unowned(share_id).await {
            Ok(removed) => removed,
            Err(e) => return error_json!("Failed to update share store: {}", e),
        },
        Ok(None) => None,
        Err(e) => return error_json!("Failed to update share store: {}", e),
    };

    match removed_graph {
        Some(graph) => {
            if let Err(e) = std::fs::remove_file(&graph.file_path) {
                log::warn!("Failed to delete shared graph file: {}", e);
            }

            ok_json!(serde_json::json!({
                "message": "Shared graph deleted successfully",
                "deleted_id": share_id
            }))
        }
        None => not_found!("Shared graph not found"),
    }
}

pub async fn get_export_stats() -> ActixResult<HttpResponse> {
    Ok(HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "Export statistics not yet implemented"
    })))
}

/// Mount `/graph-export`. `handler` is built once, before `HttpServer::new`, and cloned into
/// every worker: a handler built here would be per worker (the share-loss bug).
pub fn configure_routes(cfg: &mut web::ServiceConfig, handler: web::Data<GraphExportHandler>) {
    // Note: Using /graph-export to avoid conflict with /graph scope from api_handler/graph/mod.rs
    // Routes become /api/graph-export/*
    cfg.service(
        web::scope("/graph-export")
            .app_data(handler)
            .wrap(RequireAuth::authenticated())
            .route("", web::post().to(export_graph))
            .route("/share", web::post().to(share_graph))
            .route("/shared/{id}", web::get().to(get_shared_graph))
            .route("/shared/{id}", web::delete().to(delete_shared_graph))
            .route("/publish", web::post().to(publish_graph))
            .route("/stats", web::get().to(get_export_stats)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::SqliteShareRepository;
    use actix_web::body::MessageBody;
    use visionclaw_domain::models::graph_export::{NewSharedGraph, SharedGraph};

    async fn store(dir: &std::path::Path) -> Arc<SqliteShareRepository> {
        Arc::new(
            SqliteShareRepository::open(&dir.join("shares.sqlite3"))
                .await
                .unwrap(),
        )
    }

    #[actix_rt::test]
    async fn test_rate_limiting() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let handler =
            GraphExportHandler::new(temp_dir.path().to_path_buf(), store(temp_dir.path()).await);

        let client_ip = "127.0.0.1";

        let rate_info = handler.check_rate_limit(client_ip).await?;
        assert!(rate_info.remaining_exports > 0);

        let rate_info2 = handler.check_rate_limit(client_ip).await?;
        assert!(rate_info2.remaining_exports < rate_info.remaining_exports);

        Ok(())
    }

    fn share_of(file: &std::path::Path) -> SharedGraph {
        SharedGraph::new(NewSharedGraph {
            title: "t".into(),
            description: None,
            creator_id: Some("a".into()),
            file_path: file.to_string_lossy().into_owned(),
            file_size: 2,
            compressed: false,
            original_format: ExportFormat::Json,
            node_count: 0,
            edge_count: 0,
        })
    }

    async fn fetch(handler: &web::Data<GraphExportHandler>, id: Uuid) -> HttpResponse {
        get_shared_graph(
            handler.clone(),
            web::Path::from(id.to_string()),
            web::Query(HashMap::new()),
        )
        .await
        .unwrap()
    }

    #[actix_rt::test]
    async fn a_share_made_on_one_worker_is_found_on_another_and_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.json");
        std::fs::write(&file, b"{}").unwrap();
        let share = share_of(&file);
        let id = share.id;

        // `main` builds one handler and every worker clones the same `web::Data`.
        let built_once = web::Data::new(GraphExportHandler::new(
            dir.path().to_path_buf(),
            store(dir.path()).await,
        ));
        let worker_a = built_once.clone();
        let worker_b = built_once.clone();
        worker_a.shares.insert(&share).await.unwrap();
        let resp = fetch(&worker_b, id).await;
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        assert_eq!(resp.into_body().try_into_bytes().unwrap().as_ref(), b"{}");
        drop((built_once, worker_a, worker_b));

        // A restart opens the same store file.
        let restarted = web::Data::new(GraphExportHandler::new(
            dir.path().to_path_buf(),
            store(dir.path()).await,
        ));
        assert_eq!(
            fetch(&restarted, id).await.status(),
            actix_web::http::StatusCode::OK
        );
        assert_eq!(
            restarted
                .shares
                .get(id)
                .await
                .unwrap()
                .unwrap()
                .access_count,
            2,
            "both reads were counted in the one store"
        );
    }

    #[actix_rt::test]
    async fn a_share_past_its_access_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.json");
        std::fs::write(&file, b"{}").unwrap();
        let mut share = share_of(&file);
        share.max_access_count = Some(1);
        let handler = web::Data::new(GraphExportHandler::new(
            dir.path().to_path_buf(),
            store(dir.path()).await,
        ));
        handler.shares.insert(&share).await.unwrap();
        assert_eq!(fetch(&handler, share.id).await.status(), 200);
        assert_eq!(fetch(&handler, share.id).await.status(), 403);
        assert_eq!(fetch(&handler, Uuid::new_v4()).await.status(), 404);
    }
}
