use crate::actors::messages::{
    AddNodesFromMetadata, GetNodeData as GetGpuNodeData, GetSettings, UpdateMetadata,
};
use crate::{error_json, ok_json};
use actix_web::{web, Error as ActixError, HttpResponse, Responder, Result};
use log::{debug, error, info};
use serde_json::json;
use std::sync::Arc;

use crate::services::file_service::{FileService, MARKDOWN_DIR};
use crate::AppState;

pub async fn fetch_and_process_files(state: web::Data<AppState>) -> Result<impl Responder> {
    info!("Initiating optimized file fetch and processing");

    let mut metadata_store = match FileService::load_or_create_metadata() {
        Ok(store) => store,
        Err(e) => {
            error!("Failed to load or create metadata: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": format!("Failed to initialize metadata: {}", e)
            })));
        }
    };

    let settings = match state.settings_addr.send(GetSettings).await {
        Ok(Ok(s)) => Arc::new(tokio::sync::RwLock::new(s)),
        _ => {
            error!("Failed to retrieve settings from OptimizedSettingsActor");
            return Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to retrieve application settings"
            })));
        }
    };

    let file_service = FileService::new(settings.clone());

    match file_service
        .fetch_and_process_files(
            state.content_api.clone(),
            settings.clone(),
            &mut metadata_store,
        )
        .await
    {
        Ok(processed_files) => {
            let file_names: Vec<String> = processed_files
                .iter()
                .map(|pf| pf.file_name.clone())
                .collect();

            info!(
                "Successfully processed {} public markdown files",
                processed_files.len()
            );

            {
                if let Err(e) = state
                    .metadata_addr
                    .send(UpdateMetadata {
                        metadata: metadata_store.clone(),
                    })
                    .await
                {
                    error!(
                        "Failed to send UpdateMetadata message to MetadataActor: {}",
                        e
                    );
                }
            }

            if let Err(e) = FileService::save_metadata(&metadata_store) {
                error!("Failed to save metadata: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Failed to save metadata: {}", e)
                })));
            }

            match state
                .graph_service_addr
                .send(AddNodesFromMetadata {
                    metadata: metadata_store.clone(),
                })
                .await
            {
                Ok(Ok(())) => {
                    info!("Graph data structure updated successfully via GraphServiceActor");

                    #[cfg(feature = "gpu")]
                    if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                        match gpu_addr.send(GetGpuNodeData).await {
                            Ok(Ok(_nodes)) => {
                                debug!("GPU node data fetched successfully after graph update");
                            }
                            Ok(Err(e)) => {
                                error!("Failed to get node data from GPU actor: {}", e);
                            }
                            Err(e) => {
                                error!("Mailbox error getting node data from GPU actor: {}", e);
                            }
                        }
                    }

                    ok_json!(json!({
                        "status": "success",
                        "processed_files": file_names
                    }))
                }
                Ok(Err(e)) => {
                    error!(
                        "GraphServiceActor failed to build graph from metadata: {}",
                        e
                    );
                    error_json!(json!({
                        "status": "error",
                        "message": format!("Failed to build graph: {}", e)
                    }))
                }
                Err(e) => {
                    error!("Failed to build graph data: {}", e);
                    error_json!(json!({
                        "status": "error",
                        "message": format!("Failed to build graph data: {}", e)
                    }))
                }
            }
        }
        Err(e) => {
            error!("Error processing files: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": format!("Error processing files: {}", e)
            })))
        }
    }
}

pub async fn get_file_content(
    _state: web::Data<AppState>,
    file_name: web::Path<String>,
    caller: crate::settings::OptionalAuth,
) -> HttpResponse {
    serve_file_content(
        std::path::Path::new(MARKDOWN_DIR),
        &file_name,
        caller.0.as_ref().map(|u| u.pubkey.as_str()),
        visionclaw_domain::utils::visibility_filter::filter_enabled(),
    )
}

/// The one "no such file" reply. A page the caller may not see gets exactly this too, so a
/// hidden name and an unknown one are indistinguishable (ADR-060).
fn file_not_found(file_name: &str) -> HttpResponse {
    HttpResponse::NotFound().json(json!({
        "status": "error",
        "message": format!("File not found or unreadable: {}", file_name)
    }))
}

/// Serve `file_name` from `base_dir`, refusing traversal. With the visibility filter on, only a
/// markdown page whose frontmatter says `public: true` is served, judged by the same
/// `vault_core` reading the sync uses to mark a node public (`page_parser.rs`), then put
/// through the node policy (`node_is_visible`). Anything else, including a page that does not
/// parse, answers as missing (fail closed).
pub(crate) fn serve_file_content(
    base_dir: &std::path::Path,
    file_name: &str,
    caller: Option<&str>,
    filter_on: bool,
) -> HttpResponse {
    // SECURITY: Reject path traversal attempts
    if file_name.contains("..") || file_name.starts_with('/') || file_name.contains('\0') {
        error!(
            "Path traversal attempt blocked for file_name: {}",
            file_name
        );
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid file name"
        }));
    }

    let base = match base_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to canonicalize MARKDOWN_DIR: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Server configuration error"
            }));
        }
    };

    let canonical_path = match base_dir.join(file_name).canonicalize() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to read file {}: {}", file_name, e);
            return file_not_found(file_name);
        }
    };

    // SECURITY: Verify the resolved path is within MARKDOWN_DIR
    if !canonical_path.starts_with(&base) {
        error!(
            "Path traversal attempt blocked: resolved path escapes MARKDOWN_DIR for file_name: {}",
            file_name
        );
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid file name"
        }));
    }

    match std::fs::read_to_string(&canonical_path) {
        Ok(content) => {
            if filter_on && !page_is_visible(file_name, &content, caller) {
                return file_not_found(file_name);
            }
            HttpResponse::Ok().body(content)
        }
        Err(e) => {
            error!("Failed to read file {}: {}", file_name, e);
            file_not_found(file_name)
        }
    }
}

/// Whether the caller may read this page: project it to the node a sync would build (public iff
/// `vault_core` reads `public: true`; no producer sets an owner) and ask the node policy.
fn page_is_visible(file_name: &str, content: &str, caller: Option<&str>) -> bool {
    if !file_name.ends_with(".md") {
        return false;
    }
    let Ok(page) = vault_core::page::parse_page("", file_name, content) else {
        return false;
    };
    let mut node = visionclaw_domain::models::node::Node::default();
    if page.is_public() {
        node.metadata
            .insert("public".to_string(), "true".to_string());
    }
    visionclaw_domain::utils::visibility_filter::node_is_visible(&node, caller)
}

pub async fn refresh_graph(state: web::Data<AppState>) -> Result<impl Responder> {
    info!("Manually triggering graph refresh - returning current state");

    match state
        .graph_service_addr
        .send(crate::actors::messages::GetGraphData)
        .await
    {
        Ok(Ok(graph_data)) => {
            debug!(
                "Retrieved current graph state with {} nodes and {} edges",
                graph_data.nodes.len(),
                graph_data.edges.len()
            );

            ok_json!(json!({
                "status": "success",
                "message": "Graph data retrieved successfully",
                "nodes_count": graph_data.nodes.len(),
                "edges_count": graph_data.edges.len()
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to get current graph data: {}", e);
            error_json!(json!({
                "status": "error",
                "message": format!("Failed to retrieve current graph data: {}", e)
            }))
        }
        Err(e) => {
            error!("Mailbox error getting graph data: {}", e);
            error_json!(json!({
                "status": "error",
                "message": "Graph service unavailable"
            }))
        }
    }
}

pub async fn update_graph(state: web::Data<AppState>) -> Result<HttpResponse, ActixError> {
    let metadata_store = match FileService::load_or_create_metadata() {
        Ok(store) => store,
        Err(e) => {
            error!("Failed to load metadata: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": format!("Failed to load metadata: {}", e)
            })));
        }
    };

    match state
        .graph_service_addr
        .send(AddNodesFromMetadata {
            metadata: metadata_store.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!(
                "Graph data structure updated successfully via GraphServiceActor in update_graph"
            );

            #[cfg(feature = "gpu")]
            if let Some(gpu_addr) = state.get_gpu_compute_addr().await {
                match gpu_addr.send(GetGpuNodeData).await {
                    Ok(Ok(_nodes)) => {
                        debug!(
                            "GPU node data fetched successfully after graph update in update_graph"
                        );
                    }
                    Ok(Err(e)) => {
                        error!("Failed to get node data from GPU actor after update in update_graph: {}", e);
                    }
                    Err(e) => {
                        error!("Mailbox error getting node data from GPU actor after update in update_graph: {}", e);
                    }
                }
            }

            Ok(HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Graph updated successfully"
            })))
        }
        Err(e) => {
            error!("Failed to build graph: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": format!("Failed to build graph: {}", e)
            })))
        }
        Ok(Err(e)) => {
            error!(
                "GraphServiceActor failed to build graph from metadata: {}",
                e
            );
            Ok(HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": format!("Failed to build graph: {}", e)
            })))
        }
    }
}

// Configure routes using snake_case
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/files")
            .route("/process", web::post().to(fetch_and_process_files))
            .route("/get_content/{filename}", web::get().to(get_file_content))
            .route("/refresh_graph", web::post().to(refresh_graph))
            .route("/update_graph", web::post().to(update_graph)),
    );
}

#[cfg(test)]
mod content_visibility_tests {
    use super::serve_file_content;
    use actix_web::body::MessageBody;

    fn body(r: actix_web::HttpResponse) -> (u16, String) {
        let status = r.status().as_u16();
        let bytes = r.into_body().try_into_bytes().unwrap_or_default();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Open.md"),
            "---\npublic: true\n---\n\nOpen body.\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Secret.md"),
            "---\nowl-class: Secret\n---\n\nSecret body.\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("metadata.json"), "{}").unwrap();
        dir
    }

    #[test]
    fn a_hidden_page_reads_exactly_as_an_unknown_one() {
        let dir = vault();
        let hidden = body(serve_file_content(dir.path(), "Secret.md", None, true));
        let unknown = body(serve_file_content(
            dir.path(),
            "Secret.md.missing",
            None,
            true,
        ));
        assert_eq!(
            hidden.0, 404,
            "a hidden page must not be served: {}",
            hidden.1
        );
        assert_eq!(
            hidden.1.replace("Secret.md", "X"),
            unknown.1.replace("Secret.md.missing", "X"),
            "hidden and unknown must share one reply"
        );
        let json = body(serve_file_content(dir.path(), "metadata.json", None, true));
        assert_eq!(
            json.0, 404,
            "a non-page file is not served under the filter"
        );
    }

    #[test]
    fn a_public_page_is_served_and_the_flag_off_serves_everything() {
        let dir = vault();
        let open = body(serve_file_content(dir.path(), "Open.md", None, true));
        assert_eq!(
            open,
            (200, "---\npublic: true\n---\n\nOpen body.\n".to_string())
        );
        let off = body(serve_file_content(dir.path(), "Secret.md", None, false));
        assert_eq!(
            off.0, 200,
            "PUBKEY_VISIBILITY_FILTER=0 keeps the old behaviour"
        );
    }
}
