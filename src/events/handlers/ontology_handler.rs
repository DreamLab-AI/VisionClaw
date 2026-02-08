use async_trait:::async_trait;
use std::sync:::Arc;
use tokio::sync:::RwLock;

use crate::events::domain_events::*;
use crate::events::types::{EventError, EventHandler, EventResult, StoredEvent};
use crate::utils::json::from_json;

pub struct OntologyEventHandler {
    handler_id: String,
    state: Arc<RwLock<OntologyState>>,
}

#[derive(Debug, Default)]
struct OntologyState {
    class_count: usize,
    property_count: usize,
    inference_pending: bool,
    last_inference_duration_ms: Option<u64>,
}

impl OntologyEventHandler {
    pub fn new(handler_id: impl Into<String>) -> Self {
        Self {
            handler_id: handler_id.into(),
            state: Arc::new(RwLock::new(OntologyState::default())),
        }
    }

    pub async fn get_class_count(&self) -> usize {
        self.state.read().await.class_count
    }

    pub async fn get_property_count(&self) -> usize {
        self.state.read().await.property_count
    }

    pub async fn is_inference_pending(&self) -> bool {
        self.state.read().await.inference_pending
    }

    pub async fn get_last_inference_duration(&self) -> Option<u64> {
        self.state.read().await.last_inference_duration_ms
    }

    async fn handle_class_added(&self, event: &StoredEvent) -> EventResult<()> {
        let _data: ClassAddedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse ClassAddedEvent: {}", e)))?;

        let mut state = self.state.write().await;
        state.class_count += 1;
        state.inference_pending = true;

        println!("[OntologyHandler] Class added, inference pending");
        Ok(())
    }

    async fn handle_property_added(&self, event: &StoredEvent) -> EventResult<()> {
        let _data: PropertyAddedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse PropertyAddedEvent: {}", e)))?;

        let mut state = self.state.write().await;
        state.property_count += 1;
        state.inference_pending = true;

        println!("[OntologyHandler] Property added, inference pending");
        Ok(())
    }

    async fn handle_axiom_added(&self, event: &StoredEvent) -> EventResult<()> {
        let _data: AxiomAddedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse AxiomAddedEvent: {}", e)))?;

        let mut state = self.state.write().await;
        state.inference_pending = true;

        println!("[OntologyHandler] Axiom added, inference pending");
        Ok(())
    }

    async fn handle_ontology_imported(&self, event: &StoredEvent) -> EventResult<()> {
        let data: OntologyImportedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse OntologyImportedEvent: {}", e)))?;

        let mut state = self.state.write().await;
        state.class_count += data.class_count;
        state.property_count += data.property_count;
        state.inference_pending = true;

        println!(
            "[OntologyHandler] Ontology imported: {} classes, {} properties",
            data.class_count, data.property_count
        );
        Ok(())
    }

    async fn handle_inference_completed(&self, event: &StoredEvent) -> EventResult<()> {
        let data: InferenceCompletedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse InferenceCompletedEvent: {}", e)))?;

        let mut state = self.state.write().await;
        state.inference_pending = false;
        state.last_inference_duration_ms = Some(data.duration_ms);

        println!(
            "[OntologyHandler] Inference completed: {} axioms in {}ms",
            data.inferred_axioms, data.duration_ms
        );
        Ok(())
    }
}

#[async_trait]
impl EventHandler for OntologyEventHandler {
    fn event_type(&self) -> &'static str {
        "Ontology"
    }

    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    async fn handle(&self, event: &StoredEvent) -> EventResult<()> {
        match event.metadata.event_type.as_str() {
            "ClassAdded" => self.handle_class_added(event).await,
            "PropertyAdded" => self.handle_property_added(event).await,
            "AxiomAdded" => self.handle_axiom_added(event).await,
            "OntologyImported" => self.handle_ontology_imported(event).await,
            "InferenceCompleted" => self.handle_inference_completed(event).await,
            _ => Ok(()),
        }
    }

    fn max_retries(&self) -> u32 {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::types:::EventMetadata;
    use chrono:::Utc;
use crate::utils:::time;

    #[tokio::test]
    async fn test_class_added_triggers_inference() {
        let handler = OntologyEventHandler::new("ontology-handler");

        let event_data = ClassAddedEvent {
            class_id: "class-1".to_string(),
            class_iri: "http://example.org/Person".to_string(),
            label: Some("Person".to_string()),
            parent_classes: vec![],
            timestamp: time::now(),
        };

        let stored_event = StoredEvent {
            metadata: EventMetadata::new(
                "class-1".to_string(),
                "OntologyClass".to_string(),
                "ClassAdded".to_string(),
            ),
            data: to_json(&event_data).unwrap(),
            sequence: 1,
        };

        handler.handle(&stored_event).await.unwrap();
        assert_eq!(handler.get_class_count().await, 1);
        assert!(handler.is_inference_pending().await);
    }

    #[tokio::test]
    async fn test_inference_completed_clears_pending() {
        let handler = OntologyEventHandler::new("ontology-handler");

        let event_data = InferenceCompletedEvent {
            ontology_id: "onto-1".to_string(),
            reasoner_type: "HermiT".to_string(),
            inferred_axioms: 100,
            duration_ms: 250,
            timestamp: time::now(),
        };

        let stored_event = StoredEvent {
            metadata: EventMetadata::new(
                "onto-1".to_string(),
                "Ontology".to_string(),
                "InferenceCompleted".to_string(),
            ),
            data: to_json(&event_data).unwrap(),
            sequence: 1,
        };

        handler.handle(&stored_event).await.unwrap();
        assert!(!handler.is_inference_pending().await);
        assert_eq!(handler.get_last_inference_duration().await, Some(250));
    }
}

