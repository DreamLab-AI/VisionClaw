//! Workspace model definitions and related structures

use crate::utils::time;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
pub enum WorkspaceType {
    #[serde(rename = "personal")]
    #[default]
    Personal,
    #[serde(rename = "team")]
    Team,
    #[serde(rename = "public")]
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
pub enum WorkspaceStatus {
    #[serde(rename = "active")]
    #[default]
    Active,
    #[serde(rename = "archived")]
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Validate)]
pub struct Workspace {
    pub id: String,

    #[validate(length(
        min = 1,
        max = 100,
        message = "Name must be between 1 and 100 characters"
    ))]
    pub name: String,

    #[validate(length(max = 500, message = "Description cannot exceed 500 characters"))]
    pub description: Option<String>,

    pub workspace_type: WorkspaceType,

    pub status: WorkspaceStatus,

    pub member_count: u32,

    pub is_favorite: bool,

    pub owner_id: Option<String>,

    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[specta(skip)]
    pub metadata: HashMap<String, serde_json::Value>,

    #[specta(type = String)]
    pub created_at: DateTime<Utc>,

    #[specta(type = String)]
    pub updated_at: DateTime<Utc>,
}

impl Default for Workspace {
    fn default() -> Self {
        let now = time::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New Workspace".to_string(),
            description: None,
            workspace_type: WorkspaceType::default(),
            status: WorkspaceStatus::default(),
            member_count: 1,
            is_favorite: false,
            owner_id: None,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

impl Workspace {
    pub fn new(name: String, description: Option<String>, workspace_type: WorkspaceType) -> Self {
        let now = time::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            description,
            workspace_type,
            status: WorkspaceStatus::Active,
            member_count: 1,
            is_favorite: false,
            owner_id: None,
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn update(
        &mut self,
        name: Option<String>,
        description: Option<String>,
        workspace_type: Option<WorkspaceType>,
    ) {
        if let Some(new_name) = name {
            self.name = new_name;
        }
        if let Some(new_description) = description {
            self.description = Some(new_description);
        }
        if let Some(new_type) = workspace_type {
            self.workspace_type = new_type;
        }
        self.updated_at = time::now();
    }

    pub fn toggle_favorite(&mut self) -> bool {
        self.is_favorite = !self.is_favorite;
        self.updated_at = time::now();
        self.is_favorite
    }

    pub fn archive(&mut self) {
        self.status = WorkspaceStatus::Archived;
        self.updated_at = time::now();
    }

    pub fn unarchive(&mut self) {
        self.status = WorkspaceStatus::Active;
        self.updated_at = time::now();
    }

    pub fn is_archived(&self) -> bool {
        self.status == WorkspaceStatus::Archived
    }

    pub fn set_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
        self.updated_at = time::now();
    }

    pub fn remove_metadata(&mut self, key: &str) -> Option<serde_json::Value> {
        let result = self.metadata.remove(key);
        if result.is_some() {
            self.updated_at = time::now();
        }
        result
    }

    pub fn set_member_count(&mut self, count: u32) {
        self.member_count = count;
        self.updated_at = time::now();
    }

    pub fn set_owner(&mut self, owner_id: String) {
        self.owner_id = Some(owner_id);
        self.updated_at = time::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Validate)]
pub struct CreateWorkspaceRequest {
    #[validate(length(
        min = 1,
        max = 100,
        message = "Name must be between 1 and 100 characters"
    ))]
    pub name: String,

    #[validate(length(max = 500, message = "Description cannot exceed 500 characters"))]
    pub description: Option<String>,

    pub workspace_type: Option<WorkspaceType>,
    pub owner_id: Option<String>,

    #[specta(skip)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Validate)]
pub struct UpdateWorkspaceRequest {
    #[validate(length(
        min = 1,
        max = 100,
        message = "Name must be between 1 and 100 characters"
    ))]
    pub name: Option<String>,

    #[validate(length(max = 500, message = "Description cannot exceed 500 characters"))]
    pub description: Option<String>,

    pub workspace_type: Option<WorkspaceType>,

    #[specta(skip)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkspaceResponse {
    pub success: bool,
    pub message: String,
    pub workspace: Option<Workspace>,
}

impl WorkspaceResponse {
    pub fn success(workspace: Workspace, message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            workspace: Some(workspace),
        }
    }

    pub fn success_no_data(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            workspace: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            workspace: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkspaceListResponse {
    pub success: bool,
    pub message: String,
    pub workspaces: Vec<Workspace>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
}

impl WorkspaceListResponse {
    pub fn success(
        workspaces: Vec<Workspace>,
        total_count: usize,
        page: usize,
        page_size: usize,
    ) -> Self {
        Self {
            success: true,
            message: "Workspaces retrieved successfully".to_string(),
            workspaces,
            total_count,
            page,
            page_size,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            workspaces: Vec::new(),
            total_count: 0,
            page: 0,
            page_size: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkspaceFilter {
    pub status: Option<WorkspaceStatus>,

    pub workspace_type: Option<WorkspaceType>,

    pub is_favorite: Option<bool>,

    pub owner_id: Option<String>,

    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
pub enum WorkspaceSortBy {
    #[serde(rename = "name")]
    Name,
    #[serde(rename = "created_at")]
    CreatedAt,
    #[serde(rename = "updated_at")]
    #[default]
    UpdatedAt,
    #[serde(rename = "member_count")]
    MemberCount,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
pub enum SortDirection {
    #[serde(rename = "asc")]
    Ascending,
    #[serde(rename = "desc")]
    #[default]
    Descending,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Validate)]
pub struct WorkspaceQuery {
    #[validate(range(min = 1, max = 1000, message = "Page size must be between 1 and 1000"))]
    pub page_size: Option<usize>,

    #[validate(range(min = 0, message = "Page must be non-negative"))]
    pub page: Option<usize>,

    pub sort_by: Option<WorkspaceSortBy>,
    pub sort_direction: Option<SortDirection>,
    pub filter: Option<WorkspaceFilter>,
}

impl Default for WorkspaceQuery {
    fn default() -> Self {
        Self {
            page_size: Some(20),
            page: Some(0),
            sort_by: Some(WorkspaceSortBy::default()),
            sort_direction: Some(SortDirection::default()),
            filter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_new_sets_expected_fields() {
        let ws = Workspace::new(
            "My Workspace".to_string(),
            Some("A description".to_string()),
            WorkspaceType::Team,
        );
        assert_eq!(ws.name, "My Workspace");
        assert_eq!(ws.description.as_deref(), Some("A description"));
        assert_eq!(ws.workspace_type, WorkspaceType::Team);
        assert_eq!(ws.status, WorkspaceStatus::Active);
        assert_eq!(ws.member_count, 1);
        assert!(!ws.is_favorite);
        assert!(ws.owner_id.is_none());
        assert!(ws.metadata.is_empty());
        // UUID format: 32 hex + 4 hyphens = 36 chars
        assert_eq!(ws.id.len(), 36);
    }

    #[test]
    fn workspace_default_is_personal() {
        let ws = Workspace::default();
        assert_eq!(ws.name, "New Workspace");
        assert_eq!(ws.workspace_type, WorkspaceType::Personal);
        assert_eq!(ws.status, WorkspaceStatus::Active);
    }

    #[test]
    fn workspace_toggle_favorite_flips_state() {
        let mut ws = Workspace::default();
        assert!(!ws.is_favorite);
        assert!(ws.toggle_favorite());
        assert!(ws.is_favorite);
        assert!(!ws.toggle_favorite());
        assert!(!ws.is_favorite);
    }

    #[test]
    fn workspace_archive_and_unarchive() {
        let mut ws = Workspace::default();
        assert!(!ws.is_archived());
        ws.archive();
        assert!(ws.is_archived());
        assert_eq!(ws.status, WorkspaceStatus::Archived);
        ws.unarchive();
        assert!(!ws.is_archived());
        assert_eq!(ws.status, WorkspaceStatus::Active);
    }

    #[test]
    fn workspace_update_partial_only_changes_provided_fields() {
        let mut ws = Workspace::new("Old".to_string(), None, WorkspaceType::Personal);
        let original_type = ws.workspace_type.clone();
        ws.update(Some("New".to_string()), None, None);
        assert_eq!(ws.name, "New");
        assert_eq!(ws.workspace_type, original_type);
    }

    #[test]
    fn workspace_set_and_remove_metadata() {
        let mut ws = Workspace::default();
        ws.set_metadata("color".to_string(), serde_json::json!("blue"));
        assert!(ws.metadata.contains_key("color"));
        let removed = ws.remove_metadata("color");
        assert!(removed.is_some());
        assert!(!ws.metadata.contains_key("color"));
        // Removing non-existent key returns None
        assert!(ws.remove_metadata("nonexistent").is_none());
    }

    #[test]
    fn workspace_set_member_count_and_owner() {
        let mut ws = Workspace::default();
        ws.set_member_count(5);
        assert_eq!(ws.member_count, 5);
        ws.set_owner("owner-123".to_string());
        assert_eq!(ws.owner_id.as_deref(), Some("owner-123"));
    }

    #[test]
    fn workspace_serde_roundtrip() {
        let mut ws = Workspace::new("Test".to_string(), None, WorkspaceType::Public);
        // Insert a metadata entry so the field is serialized (skip_serializing_if = is_empty)
        ws.set_metadata("key".to_string(), serde_json::json!("value"));
        let json = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, ws.name);
        assert_eq!(back.id, ws.id);
        assert_eq!(back.workspace_type, ws.workspace_type);
    }

    #[test]
    fn workspace_response_constructors() {
        let ws = Workspace::default();
        let ok = WorkspaceResponse::success(ws, "created");
        assert!(ok.success);
        assert_eq!(ok.message, "created");
        assert!(ok.workspace.is_some());

        let ok_no_data = WorkspaceResponse::success_no_data("done");
        assert!(ok_no_data.success);
        assert!(ok_no_data.workspace.is_none());

        let err = WorkspaceResponse::error("something broke");
        assert!(!err.success);
        assert!(err.workspace.is_none());
    }

    #[test]
    fn workspace_list_response_constructors() {
        let list = WorkspaceListResponse::success(vec![], 0, 0, 20);
        assert!(list.success);
        assert_eq!(list.page_size, 20);

        let err = WorkspaceListResponse::error("failed");
        assert!(!err.success);
        assert!(err.workspaces.is_empty());
        assert_eq!(err.total_count, 0);
    }

    #[test]
    fn workspace_sort_by_default_is_updated_at() {
        assert!(matches!(
            WorkspaceSortBy::default(),
            WorkspaceSortBy::UpdatedAt
        ));
    }

    #[test]
    fn sort_direction_default_is_descending() {
        assert!(matches!(
            SortDirection::default(),
            SortDirection::Descending
        ));
    }

    #[test]
    fn workspace_query_default_has_page_size_20() {
        let q = WorkspaceQuery::default();
        assert_eq!(q.page_size, Some(20));
        assert_eq!(q.page, Some(0));
    }

    #[test]
    fn workspace_type_and_status_defaults() {
        assert_eq!(WorkspaceType::default(), WorkspaceType::Personal);
        assert_eq!(WorkspaceStatus::default(), WorkspaceStatus::Active);
    }
}
