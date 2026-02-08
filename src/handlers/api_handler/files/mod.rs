use crate::actors::messages::{
    AddNodesFromMetadata, GetNodeData as GetGpuNodeData, GetSettings, UpdateMetadata,
};
use crate::{ok_json, error_json};
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
            error!("Failed to retrieve settings from SettingsActor");
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
) -> HttpResponse {
    let file_path = format!("{}/{}", MARKDOWN_DIR, file_name);
    match std::fs::read_to_string(&file_path) {
        Ok(content) => HttpResponse::Ok().body(content),
        Err(e) => {
            error!("Failed to read file {}: {}", file_name, e);
            HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": format!("File not found or unreadable: {}", file_name)
            }))
        }
    }
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
