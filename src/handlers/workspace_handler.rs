//! Workspace HTTP handlers for REST API endpoints
//!
//! This module provides HTTP handlers for workspace management operations:
//! - GET /api/workspace/list - List all workspaces with pagination
//! - POST /api/workspace/create - Create new workspace with validation
//! - PUT /api/workspace/{id} - Update workspace metadata
//! - DELETE /api/workspace/{id} - Soft delete workspace
//! - POST /api/workspace/{id}/favorite - Toggle favorite status
//! - POST /api/workspace/{id}/archive - Archive/unarchive workspace

use actix::Addr;
use actix_web::{web, HttpResponse, Result as ActixResult};
use log::{debug, error, info, warn};
use serde_json::json;
use validator::Validate;
use crate::{ok_json, created_json};

use crate::utils::actor_timeout::{send_with_default_timeout, ActorTimeoutError};

use crate::actors::messages::{
    ArchiveWorkspace, CreateWorkspace, DeleteWorkspace, GetWorkspace, GetWorkspaceCount,
    GetWorkspaces, ToggleFavoriteWorkspace, UpdateWorkspace,
};
use crate::actors::workspace_actor::WorkspaceActor;
use crate::models::workspace::{
    CreateWorkspaceRequest, SortDirection, UpdateWorkspaceRequest, WorkspaceFilter,
    WorkspaceListResponse, WorkspaceQuery, WorkspaceResponse, WorkspaceSortBy, WorkspaceStatus,
    WorkspaceType,
};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/workspace")
            .route("/list", web::get().to(list_workspaces))
            .route("/create", web::post().to(create_workspace))
            .route("/count", web::get().to(get_workspace_count))
            .route("/{id}", web::get().to(get_workspace))
            .route("/{id}", web::put().to(update_workspace))
            .route("/{id}", web::delete().to(delete_workspace))
            .route("/{id}/favorite", web::post().to(toggle_favorite_workspace))
            .route("/{id}/archive", web::post().to(archive_workspace)),
    );
}

async fn list_workspaces(
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    query: web::Query<WorkspaceQueryParams>,
) -> ActixResult<HttpResponse> {
    debug!("Received workspace list request: {:?}", query);

    
    let workspace_query = WorkspaceQuery {
        page: query.page,
        page_size: query.page_size,
        sort_by: query.sort_by.clone(),
        sort_direction: query.sort_direction.clone(),
        filter: build_filter_from_query(&query),
    };

    
    if let Err(validation_errors) = workspace_query.validate() {
        warn!("Workspace query validation failed: {:?}", validation_errors);
        return Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "message": format!("Validation error: {}", validation_errors),
            "workspaces": [],
            "total_count": 0,
            "page": 0,
            "page_size": 0
        })));
    }

    
    match send_with_default_timeout(
        &workspace_actor,
        GetWorkspaces {
            query: workspace_query,
        },
        "Workspace",
    )
    .await
    {
        Ok(Ok(response)) => {
            info!(
                "Successfully retrieved {} workspaces",
                response.workspaces.len()
            );
            ok_json!(response)
        }
        Ok(Err(e)) => {
            error!("Workspace actor error: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(WorkspaceListResponse::error(format!(
                    "Failed to retrieve workspaces: {}",
                    e
                ))),
            )
        }
        Err(ActorTimeoutError::Timeout {
            duration,
            actor_type,
        }) => {
            error!("{} actor timeout after {:?}", actor_type, duration);
            Ok(
                HttpResponse::GatewayTimeout().json(WorkspaceListResponse::error(
                    "Request timeout - workspace service took too long to respond",
                )),
            )
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(WorkspaceListResponse::error(
                    "Service temporarily unavailable",
                )),
            )
        }
    }
}

async fn get_workspace(
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    path: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let workspace_id = path.into_inner();
    debug!("Received get workspace request for ID: {}", workspace_id);

    if workspace_id.trim().is_empty() {
        return Ok(HttpResponse::BadRequest()
            .json(WorkspaceResponse::error("Workspace ID cannot be empty")));
    }

    match send_with_default_timeout(
        &workspace_actor,
        GetWorkspace {
            workspace_id: workspace_id.clone(),
        },
        "Workspace",
    )
    .await
    {
        Ok(Ok(workspace)) => {
            info!("Successfully retrieved workspace: {}", workspace.name);
            ok_json!(WorkspaceResponse::success(
                workspace,
                "Workspace retrieved successfully",
            ))
        }
        Ok(Err(e)) => {
            warn!("Workspace not found or error: {}", e);
            Ok(HttpResponse::NotFound().json(WorkspaceResponse::error(e)))
        }
        Err(ActorTimeoutError::Timeout {
            duration,
            actor_type,
        }) => {
            error!("{} actor timeout after {:?}", actor_type, duration);
            Ok(
                HttpResponse::GatewayTimeout().json(WorkspaceResponse::error(
                    "Request timeout - workspace service took too long to respond",
                )),
            )
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(WorkspaceResponse::error("Service temporarily unavailable")))
        }
    }
}

async fn create_workspace(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    payload: web::Json<CreateWorkspaceRequest>,
) -> ActixResult<HttpResponse> {
    debug!("Received create workspace request: {:?}", payload.name);

    
    if let Err(validation_errors) = payload.validate() {
        warn!(
            "Workspace creation validation failed: {:?}",
            validation_errors
        );
        return Ok(
            HttpResponse::BadRequest().json(WorkspaceResponse::error(format!(
                "Validation error: {}",
                validation_errors
            ))),
        );
    }

    let request = payload.into_inner();

    match workspace_actor.send(CreateWorkspace { request }).await {
        Ok(Ok(workspace)) => {
            info!(
                "Successfully created workspace: {} (ID: {})",
                workspace.name, workspace.id
            );
            created_json!(WorkspaceResponse::success(
                workspace,
                "Workspace created successfully",
            ))
        }
        Ok(Err(e)) => {
            error!("Failed to create workspace: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(WorkspaceResponse::error(format!(
                    "Failed to create workspace: {}",
                    e
                ))),
            )
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(WorkspaceResponse::error("Service temporarily unavailable")))
        }
    }
}

async fn update_workspace(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    path: web::Path<String>,
    payload: web::Json<UpdateWorkspaceRequest>,
) -> ActixResult<HttpResponse> {
    let workspace_id = path.into_inner();
    debug!("Received update workspace request for ID: {}", workspace_id);

    if workspace_id.trim().is_empty() {
        return Ok(HttpResponse::BadRequest()
            .json(WorkspaceResponse::error("Workspace ID cannot be empty")));
    }

    
    if let Err(validation_errors) = payload.validate() {
        warn!(
            "Workspace update validation failed: {:?}",
            validation_errors
        );
        return Ok(
            HttpResponse::BadRequest().json(WorkspaceResponse::error(format!(
                "Validation error: {}",
                validation_errors
            ))),
        );
    }

    let request = payload.into_inner();

    match workspace_actor
        .send(UpdateWorkspace {
            workspace_id: workspace_id.clone(),
            request,
        })
        .await
    {
        Ok(Ok(workspace)) => {
            info!(
                "Successfully updated workspace: {} (ID: {})",
                workspace.name, workspace.id
            );
            ok_json!(WorkspaceResponse::success(
                workspace,
                "Workspace updated successfully",
            ))
        }
        Ok(Err(e)) => {
            if e.contains("not found") {
                Ok(HttpResponse::NotFound().json(WorkspaceResponse::error(e)))
            } else {
                error!("Failed to update workspace: {}", e);
                Ok(
                    HttpResponse::InternalServerError().json(WorkspaceResponse::error(format!(
                        "Failed to update workspace: {}",
                        e
                    ))),
                )
            }
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(WorkspaceResponse::error("Service temporarily unavailable")))
        }
    }
}

async fn delete_workspace(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    path: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let workspace_id = path.into_inner();
    debug!("Received delete workspace request for ID: {}", workspace_id);

    if workspace_id.trim().is_empty() {
        return Ok(HttpResponse::BadRequest()
            .json(WorkspaceResponse::error("Workspace ID cannot be empty")));
    }

    match workspace_actor
        .send(DeleteWorkspace {
            workspace_id: workspace_id.clone(),
        })
        .await
    {
        Ok(Ok(())) => {
            info!(
                "Successfully deleted (archived) workspace with ID: {}",
                workspace_id
            );
            ok_json!(WorkspaceResponse::success_no_data(
                "Workspace deleted successfully",
            ))
        }
        Ok(Err(e)) => {
            if e.contains("not found") {
                Ok(HttpResponse::NotFound().json(WorkspaceResponse::error(e)))
            } else {
                error!("Failed to delete workspace: {}", e);
                Ok(
                    HttpResponse::InternalServerError().json(WorkspaceResponse::error(format!(
                        "Failed to delete workspace: {}",
                        e
                    ))),
                )
            }
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(WorkspaceResponse::error("Service temporarily unavailable")))
        }
    }
}

async fn toggle_favorite_workspace(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    path: web::Path<String>,
) -> ActixResult<HttpResponse> {
    let workspace_id = path.into_inner();
    debug!(
        "Received toggle favorite request for workspace ID: {}",
        workspace_id
    );

    if workspace_id.trim().is_empty() {
        return Ok(HttpResponse::BadRequest().json(json!({
            "success": false,
            "message": "Workspace ID cannot be empty",
            "is_favorite": false
        })));
    }

    match workspace_actor
        .send(ToggleFavoriteWorkspace {
            workspace_id: workspace_id.clone(),
        })
        .await
    {
        Ok(Ok(is_favorite)) => {
            let message = if is_favorite {
                "Workspace added to favorites"
            } else {
                "Workspace removed from favorites"
            };
            info!(
                "Successfully toggled favorite for workspace {}: {}",
                workspace_id, is_favorite
            );
            ok_json!(json!({
                "success": true,
                "message": message,
                "is_favorite": is_favorite
            }))
        }
        Ok(Err(e)) => {
            if e.contains("not found") {
                Ok(HttpResponse::NotFound().json(json!({
                    "success": false,
                    "message": e,
                    "is_favorite": false
                })))
            } else {
                error!("Failed to toggle favorite for workspace: {}", e);
                Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "message": format!("Failed to toggle favorite: {}", e),
                    "is_favorite": false
                })))
            }
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "message": "Service temporarily unavailable",
                "is_favorite": false
            })))
        }
    }
}

async fn archive_workspace(
    _auth: crate::settings::auth_extractor::AuthenticatedUser,
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    path: web::Path<String>,
    payload: web::Json<ArchiveRequest>,
) -> ActixResult<HttpResponse> {
    let workspace_id = path.into_inner();
    let archive = payload.archive;
    debug!(
        "Received archive request for workspace ID: {}, archive: {}",
        workspace_id, archive
    );

    if workspace_id.trim().is_empty() {
        return Ok(HttpResponse::BadRequest()
            .json(WorkspaceResponse::error("Workspace ID cannot be empty")));
    }

    match workspace_actor
        .send(ArchiveWorkspace {
            workspace_id: workspace_id.clone(),
            archive,
        })
        .await
    {
        Ok(Ok(())) => {
            let message = if archive {
                "Workspace archived successfully"
            } else {
                "Workspace unarchived successfully"
            };
            info!(
                "Successfully {} workspace with ID: {}",
                if archive { "archived" } else { "unarchived" },
                workspace_id
            );
            ok_json!(WorkspaceResponse::success_no_data(message))
        }
        Ok(Err(e)) => {
            if e.contains("not found") {
                Ok(HttpResponse::NotFound().json(WorkspaceResponse::error(e)))
            } else {
                error!("Failed to archive workspace: {}", e);
                Ok(
                    HttpResponse::InternalServerError().json(WorkspaceResponse::error(format!(
                        "Failed to archive workspace: {}",
                        e
                    ))),
                )
            }
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(WorkspaceResponse::error("Service temporarily unavailable")))
        }
    }
}

async fn get_workspace_count(
    workspace_actor: web::Data<Addr<WorkspaceActor>>,
    query: web::Query<WorkspaceCountQuery>,
) -> ActixResult<HttpResponse> {
    debug!("Received workspace count request");

    let filter = build_filter_from_count_query(&query);

    match workspace_actor.send(GetWorkspaceCount { filter }).await {
        Ok(Ok(count)) => {
            info!("Successfully retrieved workspace count: {}", count);
            ok_json!(json!({
                "success": true,
                "message": "Workspace count retrieved successfully",
                "count": count
            }))
        }
        Ok(Err(e)) => {
            error!("Failed to get workspace count: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "message": format!("Failed to get workspace count: {}", e),
                "count": 0
            })))
        }
        Err(e) => {
            error!("Failed to communicate with workspace actor: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "message": "Service temporarily unavailable",
                "count": 0
            })))
        }
    }
}

// ============================================================================
// Helper Types and Functions
// ============================================================================

#[derive(serde::Deserialize, Debug)]
struct WorkspaceQueryParams {
    page: Option<usize>,
    page_size: Option<usize>,
    sort_by: Option<WorkspaceSortBy>,
    sort_direction: Option<SortDirection>,

    
    status: Option<WorkspaceStatus>,
    workspace_type: Option<WorkspaceType>,
    is_favorite: Option<bool>,
    owner_id: Option<String>,
    search: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct ArchiveRequest {
    archive: bool,
}

#[derive(serde::Deserialize, Debug)]
struct WorkspaceCountQuery {
    status: Option<WorkspaceStatus>,
    workspace_type: Option<WorkspaceType>,
    is_favorite: Option<bool>,
    owner_id: Option<String>,
    search: Option<String>,
}

fn build_filter_from_query(query: &WorkspaceQueryParams) -> Option<WorkspaceFilter> {
    if query.status.is_none()
        && query.workspace_type.is_none()
        && query.is_favorite.is_none()
        && query.owner_id.is_none()
        && query.search.is_none()
    {
        return None;
    }

    Some(WorkspaceFilter {
        status: query.status.clone(),
        workspace_type: query.workspace_type.clone(),
        is_favorite: query.is_favorite,
        owner_id: query.owner_id.clone(),
        search: query.search.clone(),
    })
}

fn build_filter_from_count_query(query: &WorkspaceCountQuery) -> Option<WorkspaceFilter> {
    if query.status.is_none()
        && query.workspace_type.is_none()
        && query.is_favorite.is_none()
        && query.owner_id.is_none()
        && query.search.is_none()
    {
        return None;
    }

    Some(WorkspaceFilter {
        status: query.status.clone(),
        workspace_type: query.workspace_type.clone(),
        is_favorite: query.is_favorite,
        owner_id: query.owner_id.clone(),
        search: query.search.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace::{WorkspaceStatus, WorkspaceType};

    #[test]
    fn test_build_filter_from_query_empty() {
        let query = WorkspaceQueryParams {
            page: Some(0),
            page_size: Some(20),
            sort_by: None,
            sort_direction: None,
            status: None,
            workspace_type: None,
            is_favorite: None,
            owner_id: None,
            search: None,
        };

        let filter = build_filter_from_query(&query);
        assert!(filter.is_none());
    }

    #[test]
    fn test_build_filter_from_query_with_values() {
        let query = WorkspaceQueryParams {
            page: Some(0),
            page_size: Some(20),
            sort_by: None,
            sort_direction: None,
            status: Some(WorkspaceStatus::Active),
            workspace_type: Some(WorkspaceType::Team),
            is_favorite: Some(true),
            owner_id: Some("user123".to_string()),
            search: Some("test".to_string()),
        };

        let filter = build_filter_from_query(&query);
        assert!(filter.is_some());

        let filter = filter.unwrap();
        assert_eq!(filter.status, Some(WorkspaceStatus::Active));
        assert_eq!(filter.workspace_type, Some(WorkspaceType::Team));
        assert_eq!(filter.is_favorite, Some(true));
        assert_eq!(filter.owner_id, Some("user123".to_string()));
        assert_eq!(filter.search, Some("test".to_string()));
    }
}
