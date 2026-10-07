use async_trait::async_trait;
use log;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::events::types::{EventError, EventMiddleware, EventResult, StoredEvent};
use crate::utils::json::from_json;

pub struct LoggingMiddleware {
    verbose: bool,
}

impl LoggingMiddleware {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

#[async_trait]
impl EventMiddleware for LoggingMiddleware {
    async fn before_publish(&self, event: &mut StoredEvent) -> EventResult<()> {
        if self.verbose {
            log::info!(
                "[EventBus] Publishing: {} (seq: {}, agg: {})",
                event.metadata.event_type,
                event.sequence,
                event.metadata.aggregate_id
            );
        }
        Ok(())
    }

    async fn after_publish(&self, event: &StoredEvent) -> EventResult<()> {
        if self.verbose {
            log::info!(
                "[EventBus] Published: {} successfully",
                event.metadata.event_type
            );
        }
        Ok(())
    }

    async fn before_handle(&self, event: &StoredEvent, handler_id: &str) -> EventResult<()> {
        if self.verbose {
            log::info!(
                "[EventBus] Handler {} processing {}",
                handler_id,
                event.metadata.event_type
            );
        }
        Ok(())
    }

    async fn after_handle(
        &self,
        _event: &StoredEvent,
        handler_id: &str,
        result: &Result<(), EventError>,
    ) -> EventResult<()> {
        if self.verbose {
            match result {
                Ok(_) => log::info!("[EventBus] Handler {} succeeded", handler_id),
                Err(e) => log::error!("[EventBus] Handler {} failed: {}", handler_id, e),
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct MetricsMiddleware {
    published_count: Arc<RwLock<HashMap<String, usize>>>,
    handler_count: Arc<RwLock<HashMap<String, usize>>>,
    error_count: Arc<RwLock<HashMap<String, usize>>>,
}

impl MetricsMiddleware {
    pub fn new() -> Self {
        Self {
            published_count: Arc::new(RwLock::new(HashMap::new())),
            handler_count: Arc::new(RwLock::new(HashMap::new())),
            error_count: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_published_count(&self, event_type: &str) -> usize {
        self.published_count
            .read()
            .await
            .get(event_type)
            .copied()
            .unwrap_or(0)
    }

    pub async fn get_handler_count(&self, handler_id: &str) -> usize {
        self.handler_count
            .read()
            .await
            .get(handler_id)
            .copied()
            .unwrap_or(0)
    }

    pub async fn get_error_count(&self, handler_id: &str) -> usize {
        self.error_count
            .read()
            .await
            .get(handler_id)
            .copied()
            .unwrap_or(0)
    }

    pub async fn clear_metrics(&self) {
        self.published_count.write().await.clear();
        self.handler_count.write().await.clear();
        self.error_count.write().await.clear();
    }

    /// Snapshot all published counts keyed by event type.
    pub async fn get_all_published_counts(&self) -> HashMap<String, usize> {
        self.published_count.read().await.clone()
    }

    /// Snapshot all handler invocation counts keyed by handler id.
    pub async fn get_all_handler_counts(&self) -> HashMap<String, usize> {
        self.handler_count.read().await.clone()
    }

    /// Snapshot all error counts keyed by handler id.
    pub async fn get_all_error_counts(&self) -> HashMap<String, usize> {
        self.error_count.read().await.clone()
    }
}

impl Default for MetricsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventMiddleware for MetricsMiddleware {
    async fn before_publish(&self, _event: &mut StoredEvent) -> EventResult<()> {
        Ok(())
    }

    async fn after_publish(&self, event: &StoredEvent) -> EventResult<()> {
        let mut counts = self.published_count.write().await;
        *counts.entry(event.metadata.event_type.clone()).or_insert(0) += 1;
        Ok(())
    }

    async fn before_handle(&self, _event: &StoredEvent, _handler_id: &str) -> EventResult<()> {
        Ok(())
    }

    async fn after_handle(
        &self,
        _event: &StoredEvent,
        handler_id: &str,
        result: &Result<(), EventError>,
    ) -> EventResult<()> {
        let mut handler_counts = self.handler_count.write().await;
        *handler_counts.entry(handler_id.to_string()).or_insert(0) += 1;

        if result.is_err() {
            let mut error_counts = self.error_count.write().await;
            *error_counts.entry(handler_id.to_string()).or_insert(0) += 1;
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct ValidationMiddleware;

impl ValidationMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ValidationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventMiddleware for ValidationMiddleware {
    async fn before_publish(&self, event: &mut StoredEvent) -> EventResult<()> {
        if event.metadata.aggregate_id.is_empty() {
            return Err(EventError::Validation(
                "Aggregate ID cannot be empty".to_string(),
            ));
        }

        if event.metadata.event_type.is_empty() {
            return Err(EventError::Validation(
                "Event type cannot be empty".to_string(),
            ));
        }

        if event.data.is_empty() {
            return Err(EventError::Validation(
                "Event data cannot be empty".to_string(),
            ));
        }

        from_json::<serde_json::Value>(&event.data)
            .map_err(|e| EventError::Validation(format!("Invalid JSON: {}", e)))?;

        Ok(())
    }

    async fn after_publish(&self, _event: &StoredEvent) -> EventResult<()> {
        Ok(())
    }

    async fn before_handle(&self, _event: &StoredEvent, _handler_id: &str) -> EventResult<()> {
        Ok(())
    }

    async fn after_handle(
        &self,
        _event: &StoredEvent,
        _handler_id: &str,
        _result: &Result<(), EventError>,
    ) -> EventResult<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct RetryMiddleware {
    max_retries: u32,
}

impl RetryMiddleware {
    pub fn new(max_retries: u32) -> Self {
        Self { max_retries }
    }
}

impl Default for RetryMiddleware {
    fn default() -> Self {
        Self::new(3)
    }
}

#[async_trait]
impl EventMiddleware for RetryMiddleware {
    async fn before_publish(&self, _event: &mut StoredEvent) -> EventResult<()> {
        Ok(())
    }

    async fn after_publish(&self, _event: &StoredEvent) -> EventResult<()> {
        Ok(())
    }

    async fn before_handle(&self, _event: &StoredEvent, _handler_id: &str) -> EventResult<()> {
        Ok(())
    }

    async fn after_handle(
        &self,
        event: &StoredEvent,
        handler_id: &str,
        result: &Result<(), EventError>,
    ) -> EventResult<()> {
        match result {
            Ok(_) => {
                log::debug!(
                    "[RetryMiddleware] Handler '{}' succeeded for event '{}' (seq: {})",
                    handler_id,
                    event.metadata.event_type,
                    event.sequence
                );
            }
            Err(e) => {
                log::warn!(
                    "[RetryMiddleware] Handler '{}' exhausted {} retries for event '{}' (seq: {}): {}",
                    handler_id,
                    self.max_retries,
                    event.metadata.event_type,
                    event.sequence,
                    e
                );
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct EnrichmentMiddleware {
    user_id: Option<String>,
    correlation_id: Option<String>,
}

impl EnrichmentMiddleware {
    pub fn new() -> Self {
        Self {
            user_id: None,
            correlation_id: None,
        }
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }
}

impl Default for EnrichmentMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventMiddleware for EnrichmentMiddleware {
    async fn before_publish(&self, event: &mut StoredEvent) -> EventResult<()> {
        if let Some(ref user_id) = self.user_id {
            event.metadata.user_id = Some(user_id.clone());
        }

        if let Some(ref correlation_id) = self.correlation_id {
            event.metadata.correlation_id = Some(correlation_id.clone());
        }

        Ok(())
    }

    async fn after_publish(&self, _event: &StoredEvent) -> EventResult<()> {
        Ok(())
    }

    async fn before_handle(&self, _event: &StoredEvent, _handler_id: &str) -> EventResult<()> {
        Ok(())
    }

    async fn after_handle(
        &self,
        _event: &StoredEvent,
        _handler_id: &str,
        _result: &Result<(), EventError>,
    ) -> EventResult<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::types::EventMetadata;

    #[tokio::test]
    async fn test_logging_middleware() {
        let middleware = LoggingMiddleware::new(false);
        let mut event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: "{}".to_string(),
            sequence: 1,
        };

        assert!(middleware.before_publish(&mut event).await.is_ok());
        assert!(middleware.after_publish(&event).await.is_ok());
    }

    #[tokio::test]
    async fn test_metrics_middleware() {
        let middleware = MetricsMiddleware::new();
        let event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: "{}".to_string(),
            sequence: 1,
        };

        middleware.after_publish(&event).await.unwrap();
        assert_eq!(middleware.get_published_count("NodeAdded").await, 1);
    }

    #[tokio::test]
    async fn test_validation_middleware() {
        let middleware = ValidationMiddleware::new();

        let mut valid_event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: r#"{"test":"value"}"#.to_string(),
            sequence: 1,
        };

        assert!(middleware.before_publish(&mut valid_event).await.is_ok());

        let mut invalid_event = StoredEvent {
            metadata: EventMetadata::new(
                "".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: "{}".to_string(),
            sequence: 1,
        };

        assert!(middleware.before_publish(&mut invalid_event).await.is_err());
    }

    #[tokio::test]
    async fn test_enrichment_middleware() {
        let middleware = EnrichmentMiddleware::new()
            .with_user_id("user-123".to_string())
            .with_correlation_id("corr-456".to_string());

        let mut event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: "{}".to_string(),
            sequence: 1,
        };

        middleware.before_publish(&mut event).await.unwrap();

        assert_eq!(event.metadata.user_id, Some("user-123".to_string()));
        assert_eq!(event.metadata.correlation_id, Some("corr-456".to_string()));
    }
}
