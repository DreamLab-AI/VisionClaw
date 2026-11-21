use crate::utils::time;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub trait DomainEvent: Send + Sync + Debug {
    fn event_type(&self) -> &'static str;

    fn aggregate_id(&self) -> &str;

    fn timestamp(&self) -> DateTime<Utc>;

    fn aggregate_type(&self) -> &'static str;

    fn version(&self) -> u32 {
        1
    }

    fn to_json_string(&self) -> Result<String, serde_json::Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub event_id: String,

    pub aggregate_id: String,

    pub aggregate_type: String,

    pub event_type: String,

    pub timestamp: DateTime<Utc>,

    pub causation_id: Option<String>,

    pub correlation_id: Option<String>,

    pub user_id: Option<String>,

    pub version: u32,
}

impl EventMetadata {
    pub fn new(aggregate_id: String, aggregate_type: String, event_type: String) -> Self {
        Self {
            event_id: uuid::Uuid::new_v4().to_string(),
            aggregate_id,
            aggregate_type,
            event_type,
            timestamp: time::now(),
            causation_id: None,
            correlation_id: None,
            user_id: None,
            version: 1,
        }
    }

    pub fn with_causation(mut self, causation_id: String) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    pub fn with_correlation(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    pub fn with_user(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub metadata: EventMetadata,

    pub data: String,

    pub sequence: i64,
}

#[async_trait]
pub trait EventHandler: Send + Sync {
    fn event_type(&self) -> &'static str;

    fn handler_id(&self) -> &str;

    async fn handle(&self, event: &StoredEvent) -> Result<(), EventError>;

    fn is_async(&self) -> bool {
        true
    }

    fn max_retries(&self) -> u32 {
        3
    }
}

#[async_trait]
pub trait EventMiddleware: Send + Sync {
    async fn before_publish(&self, event: &mut StoredEvent) -> Result<(), EventError>;

    async fn after_publish(&self, event: &StoredEvent) -> Result<(), EventError>;

    async fn before_handle(&self, event: &StoredEvent, handler_id: &str) -> Result<(), EventError>;

    async fn after_handle(
        &self,
        event: &StoredEvent,
        handler_id: &str,
        result: &Result<(), EventError>,
    ) -> Result<(), EventError>;

    /// Return self as `&dyn Any` so callers can downcast to concrete middleware
    /// types (e.g. MetricsMiddleware) for observability.
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum EventError {
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Handler error: {0}")]
    Handler(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Bus error: {0}")]
    Bus(String),

    #[error("Middleware error: {0}")]
    Middleware(String),

    #[error("Event not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Concurrency error: {0}")]
    Concurrency(String),
}

pub type EventResult<T> = Result<T, EventError>;

pub struct EventEnvelope {
    pub metadata: EventMetadata,
    pub event: Box<dyn std::any::Any + Send + Sync>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_metadata_creation() {
        let metadata = EventMetadata::new(
            "node-123".to_string(),
            "Node".to_string(),
            "NodeAdded".to_string(),
        );

        assert_eq!(metadata.aggregate_id, "node-123");
        assert_eq!(metadata.aggregate_type, "Node");
        assert_eq!(metadata.event_type, "NodeAdded");
        assert!(metadata.causation_id.is_none());
    }

    #[test]
    fn test_event_metadata_builder() {
        let metadata = EventMetadata::new(
            "node-123".to_string(),
            "Node".to_string(),
            "NodeAdded".to_string(),
        )
        .with_causation("cmd-456".to_string())
        .with_correlation("corr-789".to_string())
        .with_user("user-1".to_string());

        assert_eq!(metadata.causation_id, Some("cmd-456".to_string()));
        assert_eq!(metadata.correlation_id, Some("corr-789".to_string()));
        assert_eq!(metadata.user_id, Some("user-1".to_string()));
    }
}
