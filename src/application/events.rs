// src/application/events.rs
//! Domain Events
//!
//! All events that can occur in the VisionFlow system.
//! Events are published asynchronously and handled by subscribers.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::ports::settings_repository::SettingValue;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    
    SettingUpdated {
        key: String,
        value: SettingValue,
        timestamp: i64,
    },
    SettingDeleted {
        key: String,
        timestamp: i64,
    },
    PhysicsSettingsUpdated {
        profile_name: String,
        timestamp: i64,
    },

    
    NodeAdded {
        node_id: String,
        node_type: String,
        timestamp: i64,
    },
    NodeUpdated {
        node_id: String,
        changes: Vec<String>,
        timestamp: i64,
    },
    NodeRemoved {
        node_id: String,
        timestamp: i64,
    },
    EdgeAdded {
        edge_id: String,
        source_id: String,
        target_id: String,
        timestamp: i64,
    },
    EdgeRemoved {
        edge_id: String,
        timestamp: i64,
    },
    GraphUpdated {
        node_count: usize,
        edge_count: usize,
        timestamp: i64,
    },
    PositionsBatchUpdated {
        node_count: usize,
        timestamp: i64,
    },

    
    OntologyClassAdded {
        class_uri: String,
        timestamp: i64,
    },
    OntologyPropertyAdded {
        property_uri: String,
        timestamp: i64,
    },
    OntologyAxiomAdded {
        axiom_id: String,
        timestamp: i64,
    },
    OntologyUpdated {
        class_count: usize,
        property_count: usize,
        axiom_count: usize,
        timestamp: i64,
    },
    InferenceCompleted {
        inferred_count: usize,
        duration_ms: u64,
        timestamp: i64,
    },

    
    SimulationStarted {
        graph_name: String,
        timestamp: i64,
    },
    SimulationStopped {
        graph_name: String,
        timestamp: i64,
    },
    PhysicsParamsUpdated {
        graph_name: String,
        timestamp: i64,
    },
    ConstraintsApplied {
        constraint_count: usize,
        timestamp: i64,
    },

    
    CacheInvalidated {
        cache_key: String,
        timestamp: i64,
    },
    ErrorOccurred {
        error_type: String,
        message: String,
        timestamp: i64,
    },
}

impl DomainEvent {
    
    pub fn timestamp(&self) -> i64 {
        match self {
            DomainEvent::SettingUpdated { timestamp, .. }
            | DomainEvent::SettingDeleted { timestamp, .. }
            | DomainEvent::PhysicsSettingsUpdated { timestamp, .. }
            | DomainEvent::NodeAdded { timestamp, .. }
            | DomainEvent::NodeUpdated { timestamp, .. }
            | DomainEvent::NodeRemoved { timestamp, .. }
            | DomainEvent::EdgeAdded { timestamp, .. }
            | DomainEvent::EdgeRemoved { timestamp, .. }
            | DomainEvent::GraphUpdated { timestamp, .. }
            | DomainEvent::PositionsBatchUpdated { timestamp, .. }
            | DomainEvent::OntologyClassAdded { timestamp, .. }
            | DomainEvent::OntologyPropertyAdded { timestamp, .. }
            | DomainEvent::OntologyAxiomAdded { timestamp, .. }
            | DomainEvent::OntologyUpdated { timestamp, .. }
            | DomainEvent::InferenceCompleted { timestamp, .. }
            | DomainEvent::SimulationStarted { timestamp, .. }
            | DomainEvent::SimulationStopped { timestamp, .. }
            | DomainEvent::PhysicsParamsUpdated { timestamp, .. }
            | DomainEvent::ConstraintsApplied { timestamp, .. }
            | DomainEvent::CacheInvalidated { timestamp, .. }
            | DomainEvent::ErrorOccurred { timestamp, .. } => *timestamp,
        }
    }

    
    pub fn event_name(&self) -> &'static str {
        match self {
            DomainEvent::SettingUpdated { .. } => "setting_updated",
            DomainEvent::SettingDeleted { .. } => "setting_deleted",
            DomainEvent::PhysicsSettingsUpdated { .. } => "physics_settings_updated",
            DomainEvent::NodeAdded { .. } => "node_added",
            DomainEvent::NodeUpdated { .. } => "node_updated",
            DomainEvent::NodeRemoved { .. } => "node_removed",
            DomainEvent::EdgeAdded { .. } => "edge_added",
            DomainEvent::EdgeRemoved { .. } => "edge_removed",
            DomainEvent::GraphUpdated { .. } => "graph_updated",
            DomainEvent::PositionsBatchUpdated { .. } => "positions_batch_updated",
            DomainEvent::OntologyClassAdded { .. } => "ontology_class_added",
            DomainEvent::OntologyPropertyAdded { .. } => "ontology_property_added",
            DomainEvent::OntologyAxiomAdded { .. } => "ontology_axiom_added",
            DomainEvent::OntologyUpdated { .. } => "ontology_updated",
            DomainEvent::InferenceCompleted { .. } => "inference_completed",
            DomainEvent::SimulationStarted { .. } => "simulation_started",
            DomainEvent::SimulationStopped { .. } => "simulation_stopped",
            DomainEvent::PhysicsParamsUpdated { .. } => "physics_params_updated",
            DomainEvent::ConstraintsApplied { .. } => "constraints_applied",
            DomainEvent::CacheInvalidated { .. } => "cache_invalidated",
            DomainEvent::ErrorOccurred { .. } => "error_occurred",
        }
    }

    
    pub fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }
}

impl fmt::Display for DomainEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (ts: {})", self.event_name(), self.timestamp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
use crate::utils::json::{from_json, to_json};

    #[test]
    fn test_event_timestamp() {
        let event = DomainEvent::NodeAdded {
            node_id: "test".to_string(),
            node_type: "file".to_string(),
            timestamp: 123456789,
        };
        assert_eq!(event.timestamp(), 123456789);
    }

    #[test]
    fn test_event_name() {
        let event = DomainEvent::GraphUpdated {
            node_count: 10,
            edge_count: 5,
            timestamp: DomainEvent::now(),
        };
        assert_eq!(event.event_name(), "graph_updated");
    }

    #[test]
    fn test_event_display() {
        let event = DomainEvent::SimulationStarted {
            graph_name: "test".to_string(),
            timestamp: 123456789,
        };
        let display = format!("{}", event);
        assert!(display.contains("simulation_started"));
        assert!(display.contains("123456789"));
    }

    #[test]
    fn test_now_timestamp() {
        let ts1 = DomainEvent::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ts2 = DomainEvent::now();
        assert!(ts2 > ts1);
    }

    #[test]
    fn test_event_serialization() {
        let event = DomainEvent::SettingUpdated {
            key: "test_key".to_string(),
            value: SettingValue::String("test_value".to_string()),
            timestamp: 123456789,
        };

        let json = to_json(&event).unwrap();
        assert!(json.contains("setting_updated"));
        assert!(json.contains("test_key"));

        let deserialized: DomainEvent = from_json(&json).unwrap();
        match deserialized {
            DomainEvent::SettingUpdated { key, .. } => {
                assert_eq!(key, "test_key");
            }
            _ => panic!("Wrong event type"),
        }
    }
}
