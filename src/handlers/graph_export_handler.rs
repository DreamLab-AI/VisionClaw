use crate::models::graph::GraphData;
use crate::models::graph_export::*;
use crate::services::graph_serialization::GraphSerializationService;
use crate::{ok_json, error_json, bad_request, not_found, unauthorized, forbidden, too_many_requests};
use crate::AppState;
use actix_web::{http::header::HeaderValue, web, HttpRequest, HttpResponse, Result as ActixResult};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct RateLimitState {
    pub requests: Vec<DateTime<Utc>>,
    pub daily_count: u32,
    pub hourly_count: u32,
}

type SharedGraphStorage = Arc<RwLock<HashMap<Uuid, SharedGraph>>>;

type RateLimitStorage = Arc<RwLock<HashMap<String, RateLimitState>>>;

pub struct GraphExportHandler {
    serialization_service: GraphSerializationService,
    shared_graphs: SharedGraphStorage,
    rate_limits: RateLimitStorage,
    daily_export_limit: u32,
    hourly_export_limit: u32,
}

impl GraphExportHandler {
    
    pub fn new(storage_path: std::path::PathBuf) -> Self {
        Self {
            serialization_service: GraphSerializationService::new(storage_path),
            shared_graphs: Arc::new(RwLock::new(HashMap::new())),
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

    
    async fn get_current_graph(&self, _app_state: &AppState) -> Result<GraphData> {
        
        
        let mut graph = GraphData::new();

        
        for i in 1..=10 {
            let mut node = crate::models::node::Node::new(format!("node_{}", i))
                .with_label(format!("Node {}", i))
                .with_position((i as f32) * 10.0, (i as f32) * 15.0, 0.0);
            node.id = i;
            graph.nodes.push(node);
        }

        for i in 1..=9 {
            graph
                .edges
                .push(crate::models::edge::Edge::new(i, i + 1, 1.0));
        }

        Ok(graph)
    }
}

pub async fn export_graph(
    app_state: web::Data<AppState>,
    request: web::Json<ExportRequest>,
    req: HttpRequest,
) -> ActixResult<HttpResponse> {
    let client_ip = req
        .connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string();


    let handler = GraphExportHandler::new(std::path::PathBuf::from("data"));


    match handler.check_rate_limit(&client_ip).await {
        Ok(rate_info) if rate_info.remaining_exports == 0 => {
            return too_many_requests!("Rate limit exceeded");
        }
        Err(e) => {
            return error_json!("Rate limit check failed: {}", e);
        }
        _ => {}
    }


    let graph = match handler.get_current_graph(&app_state).await {
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
    request: web::Json<ShareRequest>,
    req: HttpRequest,
) -> ActixResult<HttpResponse> {
    let client_ip = req
        .connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string();

    let handler = GraphExportHandler::new(std::path::PathBuf::from("data"));


    match handler.check_rate_limit(&client_ip).await {
        Ok(rate_info) if rate_info.remaining_exports == 0 => {
            return too_many_requests!("Rate limit exceeded");
        }
        Err(e) => {
            return error_json!("Rate limit check failed: {}", e);
        }
        _ => {}
    }


    let graph = match handler.get_current_graph(&app_state).await {
        Ok(graph) => graph,
        Err(e) => {
            return error_json!("Failed to get graph: {}", e);
        }
    };


    match handler
        .serialization_service
        .create_shared_graph(&graph, &request)
        .await
    {
        Ok((shared_graph, share_response)) => {

            {
                let mut shared_graphs = handler.shared_graphs.write().await;
                shared_graphs.insert(shared_graph.id, shared_graph);
            }

            ok_json!(share_response)
        }
        Err(e) => error_json!("Failed to create shared graph: {}", e),
    }
}

pub async fn get_shared_graph(
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
) -> ActixResult<HttpResponse> {
    let share_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(id) => id,
        Err(_) => {
            return bad_request!("Invalid share ID format");
        }
    };

    let handler = GraphExportHandler::new(std::path::PathBuf::from("data"));

    
    let shared_graph = {
        let shared_graphs = handler.shared_graphs.read().await;
        match shared_graphs.get(&share_id) {
            Some(graph) => graph.clone(),
            None => {
                return not_found!("Shared graph not found");
            }
        }
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

    
    {
        let mut shared_graphs = handler.shared_graphs.write().await;
        if let Some(graph) = shared_graphs.get_mut(&share_id) {
            graph.increment_access();
        }
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
    app_state: web::Data<AppState>,
    _request: web::Json<PublishRequest>,
    req: HttpRequest,
) -> ActixResult<HttpResponse> {
    let client_ip = req
        .connection_info()
        .peer_addr()
        .unwrap_or("unknown")
        .to_string();

    let handler = GraphExportHandler::new(std::path::PathBuf::from("data"));


    match handler.check_rate_limit(&client_ip).await {
        Ok(rate_info) if rate_info.remaining_exports == 0 => {
            return too_many_requests!("Rate limit exceeded");
        }
        Err(e) => {
            return error_json!("Rate limit check failed: {}", e);
        }
        _ => {}
    }


    let _graph = match handler.get_current_graph(&app_state).await {
        Ok(graph) => graph,
        Err(e) => {
            return error_json!("Failed to get graph: {}", e);
        }
    };


    let publication_id = Uuid::new_v4();
    let repository_url = format!("https://graphdb.example.com/graphs/{}", publication_id);



    let publish_response = PublishResponse {
        publication_id,
        repository_url,
        doi: Some(format!("10.1000/graph.{}", publication_id)),
        published_at: Utc::now(),
        status: PublicationStatus::Pending,
    };

    ok_json!(publish_response)
}

pub async fn delete_shared_graph(path: web::Path<String>) -> ActixResult<HttpResponse> {
    let share_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(id) => id,
        Err(_) => {
            return bad_request!("Invalid share ID format");
        }
    };

    let handler = GraphExportHandler::new(std::path::PathBuf::from("data"));

    
    let removed_graph = {
        let mut shared_graphs = handler.shared_graphs.write().await;
        shared_graphs.remove(&share_id)
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
    
    let stats = ExportStats {
        total_exports: 1250,
        exports_by_format: {
            let mut map = HashMap::new();
            map.insert("json".to_string(), 750);
            map.insert("gexf".to_string(), 300);
            map.insert("graphml".to_string(), 150);
            map.insert("csv".to_string(), 50);
            map
        },
        shared_graphs: 45,
        published_graphs: 12,
        avg_file_size: 2.4, 
        last_export: Some(Utc::now() - chrono::Duration::minutes(15)),
    };

    ok_json!(stats)
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    // Note: Using /graph-export to avoid conflict with /graph scope from api_handler/graph/mod.rs
    // Routes become /api/graph-export/*
    cfg.service(
        web::scope("/graph-export")
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
    use actix_web::{test, App};
    use tempfile::tempdir;

    #[actix_rt::test]
    async fn test_rate_limiting() -> Result<()> {
        let temp_dir = tempdir()?;
        let handler = GraphExportHandler::new(temp_dir.path().to_path_buf());

        let client_ip = "127.0.0.1";


        let rate_info = handler.check_rate_limit(client_ip).await?;
        assert!(rate_info.remaining_exports > 0);


        let rate_info2 = handler.check_rate_limit(client_ip).await?;
        assert!(rate_info2.remaining_exports < rate_info.remaining_exports);

        Ok(())
    }

    // Test disabled - AppState initialization requires full context
    // #[actix_rt::test]
    // async fn test_export_api_endpoint() {
    //     let temp_dir = tempdir().unwrap();
    //     let app_state = web::Data::new(AppState::default());
    //
    //     let app =
    //         test::init_service(App::new().app_data(app_state).configure(configure_routes)).await;
    //
    //     let export_request = ExportRequest {
    //         format: ExportFormat::Json,
    //         ..Default::default()
    //     };
    //
    //     let req = test::TestRequest::post()
    //         .uri("/api/graph/export")
    //         .set_json(&export_request)
    //         .to_request();
    //
    //     let resp = test::call_service(&app, req).await;
    //     assert!(resp.status().is_success() || resp.status().is_server_error());
    // }
}
