pub mod bus;
pub mod domain_events;
pub mod handlers;
pub mod middleware;
pub mod types;

pub use types::{
    DomainEvent, EventError, EventHandler, EventMetadata, EventMiddleware, EventResult, StoredEvent,
};

pub use domain_events::*;

pub use bus::{DeadLetterEntry, DeadLetterQueue, EventBus};

// Re-export EventBus in event_bus module for backward compatibility
pub mod event_bus {
    pub use super::bus::EventBus;
}

pub use middleware::{
    EnrichmentMiddleware, LoggingMiddleware, MetricsMiddleware, RetryMiddleware,
    ValidationMiddleware,
};

pub use handlers::{
    AuditEventHandler, GraphEventHandler, NotificationEventHandler, OntologyEventHandler,
};

// ADR-2066: `inference_triggers` (`register_inference_triggers`,
// `AutoInferenceConfig`, `InferenceTriggerHandler`, its own `OntologyEvent`) was
// dead code — `register_inference_triggers` was never called from anywhere.
// Removed alongside the rest of the unwired Phase 7 inference stack.
