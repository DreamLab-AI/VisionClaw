use crate::utils::time;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Gexf,
    Graphml,
    Csv,
    Dot,
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportFormat::Json => write!(f, "json"),
            ExportFormat::Gexf => write!(f, "gexf"),
            ExportFormat::Graphml => write!(f, "graphml"),
            ExportFormat::Csv => write!(f, "csv"),
            ExportFormat::Dot => write!(f, "dot"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    pub format: ExportFormat,
    pub graph_id: Option<String>,
    pub include_metadata: bool,
    pub compress: bool,
    pub custom_attributes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResponse {
    pub export_id: String,
    pub format: ExportFormat,
    pub file_size: u64,
    pub compressed: bool,
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedGraph {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub creator_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub access_count: u32,
    pub max_access_count: Option<u32>,
    pub is_public: bool,
    pub password_hash: Option<String>,
    pub file_path: String,
    pub file_size: u64,
    pub compressed: bool,
    pub original_format: ExportFormat,
    pub node_count: u32,
    pub edge_count: u32,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRequest {
    pub title: String,
    pub description: Option<String>,
    pub expires_in_hours: Option<u32>,
    pub max_access_count: Option<u32>,
    pub is_public: bool,
    pub password: Option<String>,
    pub graph_id: Option<String>,
    pub export_format: ExportFormat,
    pub include_metadata: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResponse {
    pub share_id: Uuid,
    pub share_url: String,
    pub qr_code_url: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishRequest {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub license: Option<String>,
    pub category: String,
    pub is_featured: bool,
    pub graph_id: Option<String>,
    pub export_format: ExportFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResponse {
    pub publication_id: Uuid,
    pub repository_url: String,
    pub doi: Option<String>,
    pub published_at: DateTime<Utc>,
    pub status: PublicationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PublicationStatus {
    Pending,
    Approved,
    Rejected,
    Published,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportStats {
    pub total_exports: u64,
    pub exports_by_format: HashMap<String, u64>,
    pub shared_graphs: u64,
    pub published_graphs: u64,
    pub avg_file_size: f64,
    pub last_export: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub remaining_exports: u32,
    pub reset_time: DateTime<Utc>,
    pub daily_limit: u32,
    pub hourly_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f64,
    pub algorithm: String,
}

/// Caller-supplied fields for [`SharedGraph::new`]; every other field takes
/// its creation-time default (fresh id, public, no expiry or access limit).
#[derive(Debug, Clone)]
pub struct NewSharedGraph {
    pub title: String,
    pub description: Option<String>,
    pub creator_id: Option<String>,
    pub file_path: String,
    pub file_size: u64,
    pub compressed: bool,
    pub original_format: ExportFormat,
    pub node_count: u32,
    pub edge_count: u32,
}

impl SharedGraph {
    pub fn new(new: NewSharedGraph) -> Self {
        let NewSharedGraph {
            title,
            description,
            creator_id,
            file_path,
            file_size,
            compressed,
            original_format,
            node_count,
            edge_count,
        } = new;
        let now = time::now();
        Self {
            id: Uuid::new_v4(),
            title,
            description,
            creator_id,
            created_at: now,
            updated_at: now,
            expires_at: None,
            access_count: 0,
            max_access_count: None,
            is_public: true,
            password_hash: None,
            file_path,
            file_size,
            compressed,
            original_format,
            node_count,
            edge_count,
            metadata: HashMap::new(),
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            time::now() > expires_at
        } else {
            false
        }
    }

    pub fn access_limit_reached(&self) -> bool {
        if let Some(max_count) = self.max_access_count {
            self.access_count >= max_count
        } else {
            false
        }
    }

    pub fn increment_access(&mut self) {
        self.access_count += 1;
        self.updated_at = time::now();
    }

    pub fn set_expiration(&mut self, hours: u32) {
        self.expires_at = Some(time::now() + chrono::Duration::hours(hours as i64));
    }

    pub fn validate_password(&self, password: &str) -> bool {
        if let Some(hash) = &self.password_hash {
            bcrypt::verify(password, hash).unwrap_or(false)
        } else {
            true
        }
    }
}

impl Default for ExportRequest {
    fn default() -> Self {
        Self {
            format: ExportFormat::Json,
            graph_id: None,
            include_metadata: true,
            compress: false,
            custom_attributes: None,
        }
    }
}

impl Default for ShareRequest {
    fn default() -> Self {
        Self {
            title: "Shared Graph".to_string(),
            description: None,
            expires_in_hours: Some(24 * 7),
            max_access_count: None,
            is_public: true,
            password: None,
            graph_id: None,
            export_format: ExportFormat::Json,
            include_metadata: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shared_graph() -> SharedGraph {
        SharedGraph::new(NewSharedGraph {
            title: "Test Graph".to_string(),
            description: Some("Desc".to_string()),
            creator_id: Some("creator-1".to_string()),
            file_path: "/tmp/graph.json".to_string(),
            file_size: 1024,
            compressed: false,
            original_format: ExportFormat::Json,
            node_count: 10,
            edge_count: 5,
        })
    }

    // --- ExportFormat ---

    #[test]
    fn export_format_display_all_variants() {
        assert_eq!(ExportFormat::Json.to_string(), "json");
        assert_eq!(ExportFormat::Gexf.to_string(), "gexf");
        assert_eq!(ExportFormat::Graphml.to_string(), "graphml");
        assert_eq!(ExportFormat::Csv.to_string(), "csv");
        assert_eq!(ExportFormat::Dot.to_string(), "dot");
    }

    #[test]
    fn export_format_serde_roundtrip() {
        for fmt in [
            ExportFormat::Json,
            ExportFormat::Gexf,
            ExportFormat::Graphml,
            ExportFormat::Csv,
            ExportFormat::Dot,
        ] {
            let json = serde_json::to_string(&fmt).unwrap();
            let back: ExportFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(back, fmt);
        }
    }

    // --- ExportRequest ---

    #[test]
    fn export_request_default_values() {
        let req = ExportRequest::default();
        assert_eq!(req.format, ExportFormat::Json);
        assert!(req.include_metadata);
        assert!(!req.compress);
        assert!(req.graph_id.is_none());
        assert!(req.custom_attributes.is_none());
    }

    // --- ShareRequest ---

    #[test]
    fn share_request_default_values() {
        let req = ShareRequest::default();
        assert_eq!(req.title, "Shared Graph");
        assert_eq!(req.expires_in_hours, Some(168)); // 7 days
        assert!(req.is_public);
        assert!(req.password.is_none());
    }

    // --- SharedGraph ---

    #[test]
    fn shared_graph_new_initialises_correctly() {
        let sg = make_shared_graph();
        assert_eq!(sg.title, "Test Graph");
        assert_eq!(sg.node_count, 10);
        assert_eq!(sg.edge_count, 5);
        assert_eq!(sg.file_size, 1024);
        assert_eq!(sg.access_count, 0);
        assert!(sg.is_public);
        assert!(sg.password_hash.is_none());
        assert!(sg.expires_at.is_none());
        assert!(sg.max_access_count.is_none());
    }

    #[test]
    fn shared_graph_is_expired_false_when_no_expiry() {
        let sg = make_shared_graph();
        assert!(!sg.is_expired());
    }

    #[test]
    fn shared_graph_is_expired_true_when_past_expiry() {
        let mut sg = make_shared_graph();
        // Set expiry in the past
        sg.expires_at = Some(chrono::DateTime::<chrono::Utc>::from_timestamp(1, 0).unwrap());
        assert!(sg.is_expired());
    }

    #[test]
    fn shared_graph_access_limit_not_reached_when_no_max() {
        let sg = make_shared_graph();
        assert!(!sg.access_limit_reached());
    }

    #[test]
    fn shared_graph_access_limit_reached_when_at_max() {
        let mut sg = make_shared_graph();
        sg.max_access_count = Some(3);
        sg.access_count = 3;
        assert!(sg.access_limit_reached());
        sg.access_count = 2;
        assert!(!sg.access_limit_reached());
    }

    #[test]
    fn shared_graph_increment_access_increases_count() {
        let mut sg = make_shared_graph();
        sg.increment_access();
        sg.increment_access();
        assert_eq!(sg.access_count, 2);
    }

    #[test]
    fn shared_graph_set_expiration_sets_future_expiry() {
        let mut sg = make_shared_graph();
        sg.set_expiration(1);
        assert!(sg.expires_at.is_some());
        assert!(!sg.is_expired()); // just set 1 hour in the future
    }

    #[test]
    fn shared_graph_validate_password_no_hash_returns_true() {
        let sg = make_shared_graph();
        assert!(sg.validate_password("anything"));
        assert!(sg.validate_password(""));
    }

    #[test]
    fn publication_status_serde_roundtrip() {
        for status in [
            PublicationStatus::Pending,
            PublicationStatus::Approved,
            PublicationStatus::Rejected,
            PublicationStatus::Published,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: PublicationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }
}
