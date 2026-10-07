use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use crate::events::types::{
    DomainEvent, EventError, EventHandler, EventMetadata, EventMiddleware, EventResult, StoredEvent,
};
use crate::utils::time;

#[derive(Debug, Clone)]
pub struct DeadLetterEntry {
    pub event: StoredEvent,
    pub handler_id: String,
    pub error: EventError,
    pub failed_at: DateTime<Utc>,
    pub attempt_count: u32,
}

pub struct DeadLetterQueue {
    entries: Arc<RwLock<Vec<DeadLetterEntry>>>,
    max_entries: usize,
}

impl DeadLetterQueue {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            max_entries,
        }
    }

    pub async fn push(&self, entry: DeadLetterEntry) {
        let mut entries = self.entries.write().await;
        if entries.len() >= self.max_entries {
            entries.remove(0);
        }
        entries.push(entry);
    }

    pub async fn get_entries(&self) -> Vec<DeadLetterEntry> {
        self.entries.read().await.clone()
    }

    pub async fn count(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl Default for DeadLetterQueue {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// Execute a single handler with retry logic and middleware hooks.
/// Extracted as a free function so it can be spawned concurrently without
/// borrowing the entire `EventBus`.
async fn execute_handler_concurrent(
    handler: Arc<dyn EventHandler>,
    event: StoredEvent,
    middleware: Arc<RwLock<Vec<Arc<dyn EventMiddleware>>>>,
) -> Result<(), EventError> {
    let handler_id = handler.handler_id().to_string();
    let max_retries = handler.max_retries();

    // Pre-handle middleware
    {
        let mw_list = middleware.read().await;
        for mw in mw_list.iter() {
            mw.before_handle(&event, &handler_id).await?;
        }
    }

    // ADR-031 item 6: Panic isolation.
    // Wrap the retry loop in `tokio::task::spawn` so that a panic inside a
    // handler is caught as a `JoinError` rather than unwinding through the
    // EventBus and killing the entire task. Each handler failure is logged and
    // reported as `EventError::Handler` without affecting other handlers.
    let spawn_handler = Arc::clone(&handler);
    let spawn_event = event.clone();
    let spawn_middleware = Arc::clone(&middleware);
    let spawn_handler_id = handler_id.clone();

    let join_handle = tokio::task::spawn(async move {
        let mut last_error = None;
        for attempt in 0..=max_retries {
            match spawn_handler.handle(&spawn_event).await {
                Ok(_) => {
                    let mw_list = spawn_middleware.read().await;
                    for mw in mw_list.iter() {
                        mw.after_handle(&spawn_event, &spawn_handler_id, &Ok(()))
                            .await?;
                    }
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        let delay = std::time::Duration::from_millis(100 * 2_u64.pow(attempt));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        let error = last_error.unwrap_or_else(|| {
            EventError::Handler("Handler failed with unknown error after retries".to_string())
        });
        let result = Err(error.clone());

        let mw_list = spawn_middleware.read().await;
        for mw in mw_list.iter() {
            let _ = mw
                .after_handle(&spawn_event, &spawn_handler_id, &result)
                .await;
        }

        Err(error)
    });

    match join_handle.await {
        Ok(result) => result,
        Err(join_error) => Err(EventError::Handler(format!(
            "Handler '{}' panicked: {}",
            handler_id, join_error
        ))),
    }
}

/// Event type -> the handlers subscribed to it.
type SubscriberMap = HashMap<String, Vec<Arc<dyn EventHandler>>>;

pub struct EventBus {
    subscribers: Arc<RwLock<SubscriberMap>>,

    middleware: Arc<RwLock<Vec<Arc<dyn EventMiddleware>>>>,

    sequence: Arc<RwLock<i64>>,

    enabled: Arc<RwLock<bool>>,

    dead_letter_queue: DeadLetterQueue,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            middleware: Arc::new(RwLock::new(Vec::new())),
            sequence: Arc::new(RwLock::new(0)),
            enabled: Arc::new(RwLock::new(true)),
            dead_letter_queue: DeadLetterQueue::default(),
        }
    }

    pub async fn publish<E: DomainEvent>(&self, event: E) -> EventResult<()> {
        if !*self.enabled.read().await {
            return Ok(());
        }

        let metadata = EventMetadata::new(
            event.aggregate_id().to_string(),
            event.aggregate_type().to_string(),
            event.event_type().to_string(),
        );

        let data = event
            .to_json_string()
            .map_err(|e| EventError::Serialization(e.to_string()))?;

        let mut seq = self.sequence.write().await;
        *seq += 1;
        let sequence = *seq;
        drop(seq);

        let mut stored_event = StoredEvent {
            metadata,
            data,
            sequence,
        };

        let middleware = self.middleware.read().await;
        for mw in middleware.iter() {
            mw.before_publish(&mut stored_event).await?;
        }
        drop(middleware);

        let subscribers = self.subscribers.read().await;
        let mut handlers = subscribers
            .get(event.event_type())
            .cloned()
            .unwrap_or_default();
        // Also dispatch to wildcard ("*") handlers (audit, notification, etc.)
        if event.event_type() != "*" {
            if let Some(wildcard_handlers) = subscribers.get("*") {
                handlers.extend(wildcard_handlers.iter().cloned());
            }
        }
        drop(subscribers);

        let handler_count = handlers.len();
        let middleware_ref = Arc::clone(&self.middleware);
        let handler_futures: Vec<_> = handlers
            .iter()
            .map(|handler| {
                let handler = Arc::clone(handler);
                let event = stored_event.clone();
                let mw = Arc::clone(&middleware_ref);
                let handler_id = handler.handler_id().to_string();
                async move {
                    let result = execute_handler_concurrent(handler, event, mw).await;
                    (handler_id, result)
                }
            })
            .collect();
        let results = futures::future::join_all(handler_futures).await;

        let mut errors = Vec::new();
        for (handler_id, result) in results {
            if let Err(e) = result {
                warn!(
                    handler_id = handler_id.as_str(),
                    event_type = stored_event.metadata.event_type.as_str(),
                    error = %e,
                    "Event handler failed"
                );
                errors.push((handler_id, e));
            }
        }

        for (ref handler_id, ref error) in &errors {
            let entry = DeadLetterEntry {
                event: stored_event.clone(),
                handler_id: handler_id.clone(),
                error: error.clone(),
                failed_at: time::now(),
                attempt_count: handlers
                    .iter()
                    .find(|h| h.handler_id() == handler_id.as_str())
                    .map(|h| h.max_retries() + 1)
                    .unwrap_or(1),
            };
            self.dead_letter_queue.push(entry).await;
        }

        let middleware = self.middleware.read().await;
        for mw in middleware.iter() {
            mw.after_publish(&stored_event).await?;
        }
        drop(middleware);

        if !errors.is_empty() {
            let failure_summary: String = errors
                .iter()
                .map(|(id, e)| format!("{}={}", id, e))
                .collect::<Vec<_>>()
                .join(", ");

            if errors.len() == handler_count {
                return Err(EventError::Handler(format!(
                    "All {} handlers failed for event '{}': {}",
                    errors.len(),
                    stored_event.metadata.event_type,
                    failure_summary,
                )));
            }
            // Partial failure: some handlers succeeded, log but return Ok
            warn!(
                event_type = stored_event.metadata.event_type.as_str(),
                failed = errors.len(),
                total = handler_count,
                failures = failure_summary.as_str(),
                "Partial event handler failure"
            );
        }

        Ok(())
    }

    pub async fn subscribe(&self, handler: Arc<dyn EventHandler>) {
        let event_type = handler.event_type().to_string();
        let mut subscribers = self.subscribers.write().await;

        subscribers
            .entry(event_type)
            .or_insert_with(Vec::new)
            .push(handler);
    }

    pub async fn unsubscribe(&self, handler_id: &str, event_type: &str) {
        let mut subscribers = self.subscribers.write().await;

        if let Some(handlers) = subscribers.get_mut(event_type) {
            handlers.retain(|h| h.handler_id() != handler_id);
        }
    }

    pub async fn add_middleware(&self, middleware: Arc<dyn EventMiddleware>) {
        let mut mw_list = self.middleware.write().await;
        mw_list.push(middleware);
    }

    /// Return a snapshot of the registered middleware list for observability.
    pub async fn middlewares(&self) -> Vec<Arc<dyn EventMiddleware>> {
        self.middleware.read().await.clone()
    }

    pub async fn subscriber_count(&self, event_type: &str) -> usize {
        let subscribers = self.subscribers.read().await;
        subscribers.get(event_type).map(|h| h.len()).unwrap_or(0)
    }

    pub async fn clear_subscribers(&self) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.clear();
    }

    pub async fn set_enabled(&self, enabled: bool) {
        let mut flag = self.enabled.write().await;
        *flag = enabled;
    }

    pub async fn is_enabled(&self) -> bool {
        *self.enabled.read().await
    }

    pub async fn current_sequence(&self) -> i64 {
        *self.sequence.read().await
    }

    pub async fn get_dead_letters(&self) -> Vec<DeadLetterEntry> {
        self.dead_letter_queue.get_entries().await
    }

    pub async fn dead_letter_count(&self) -> usize {
        self.dead_letter_queue.count().await
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::domain_events::NodeAddedEvent;
    use crate::utils::time;
    use async_trait::async_trait;

    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestHandler {
        id: String,
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventHandler for TestHandler {
        fn event_type(&self) -> &'static str {
            "NodeAdded"
        }

        fn handler_id(&self) -> &str {
            &self.id
        }

        async fn handle(&self, _event: &StoredEvent) -> Result<(), EventError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_event_bus_publish() {
        let bus = EventBus::new();
        let call_count = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            id: "test-handler".to_string(),
            call_count: call_count.clone(),
        });

        bus.subscribe(handler).await;

        let event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        bus.publish(event).await.unwrap();

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new();
        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        bus.subscribe(Arc::new(TestHandler {
            id: "handler-1".to_string(),
            call_count: count1.clone(),
        }))
        .await;

        bus.subscribe(Arc::new(TestHandler {
            id: "handler-2".to_string(),
            call_count: count2.clone(),
        }))
        .await;

        let event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        bus.publish(event).await.unwrap();

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let bus = EventBus::new();
        let call_count = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            id: "test-handler".to_string(),
            call_count: call_count.clone(),
        });

        bus.subscribe(handler).await;
        assert_eq!(bus.subscriber_count("NodeAdded").await, 1);

        bus.unsubscribe("test-handler", "NodeAdded").await;
        assert_eq!(bus.subscriber_count("NodeAdded").await, 0);
    }

    #[tokio::test]
    async fn test_disabled_bus() {
        let bus = EventBus::new();
        bus.set_enabled(false).await;

        let call_count = Arc::new(AtomicUsize::new(0));
        bus.subscribe(Arc::new(TestHandler {
            id: "test-handler".to_string(),
            call_count: call_count.clone(),
        }))
        .await;

        let event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        bus.publish(event).await.unwrap();
        assert_eq!(call_count.load(Ordering::SeqCst), 0);
    }
}
