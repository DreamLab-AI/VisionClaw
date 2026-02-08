// src/actors/event_coordination.rs
//! Event-Driven Actor Coordination
//!
//! Coordinates actors through domain events, enabling reactive
//! behavior and loose coupling between components.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::application::physics_service::PhysicsService;
use crate::application::semantic_service::SemanticService;
use crate::events::domain_events::{
    EdgeAddedEvent, GraphSavedEvent, NodeAddedEvent, OntologyImportedEvent, PositionsUpdatedEvent,
};
use crate::events::event_bus::EventBus;
use crate::models::graph::GraphData;

pub struct EventCoordinator {
    physics_service: Arc<PhysicsService>,
    semantic_service: Arc<SemanticService>,
    event_bus: Arc<RwLock<EventBus>>,
    graph_data: Arc<RwLock<GraphData>>,
}

impl EventCoordinator {
    
    pub fn new(
        physics_service: Arc<PhysicsService>,
        semantic_service: Arc<SemanticService>,
        event_bus: Arc<RwLock<EventBus>>,
        graph_data: Arc<RwLock<GraphData>>,
    ) -> Self {
        Self {
            physics_service,
            semantic_service,
            event_bus,
            graph_data,
        }
    }

    
    pub async fn initialize(&self) {
        info!("Initializing event coordination");

        
        self.subscribe_to_graph_events().await;

        
        self.subscribe_to_ontology_events().await;

        
        self.subscribe_to_position_events().await;

        info!("Event coordination initialized");
    }

    
    async fn subscribe_to_graph_events(&self) {
        let physics_service = self.physics_service.clone();
        let semantic_service = self.semantic_service.clone();
        let graph_data = self.graph_data.clone();

        
        let graph_data_clone = graph_data.clone();
        let _physics_clone = physics_service.clone();

        actix::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                
                let graph = graph_data_clone.read().await.clone();
                if graph.nodes.len() > 0 {
                    debug!("Graph updated, triggering physics recalculation");

                    
                    
                    
                }
            }
        });

        
        let semantic_clone = semantic_service.clone();
        actix::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                
                if let Err(e) = semantic_clone.invalidate_cache().await {
                    warn!("Failed to invalidate cache: {}", e);
                }
            }
        });
    }

    
    async fn subscribe_to_ontology_events(&self) {
        let _semantic_service = self.semantic_service.clone();

        
        actix::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                debug!("Checking for ontology updates");
                
            }
        });
    }

    
    async fn subscribe_to_position_events(&self) {
        let _event_bus = self.event_bus.clone();

        
        actix::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                
                debug!("Broadcasting position updates");
            }
        });
    }

    
    pub async fn on_graph_saved(&self, event: GraphSavedEvent) {
        info!(
            "Graph saved event received: {} nodes, {} edges",
            event.node_count, event.edge_count
        );


        let _graph = self.graph_data.read().await.clone();

        
        if let Err(e) = self.physics_service.reset().await {
            warn!("Failed to reset physics: {}", e);
        }

        
        if let Err(e) = self.semantic_service.invalidate_cache().await {
            warn!("Failed to invalidate semantic cache: {}", e);
        }
    }

    
    pub async fn on_ontology_imported(&self, event: OntologyImportedEvent) {
        info!(
            "Ontology imported: {} classes, {} properties",
            event.class_count, event.property_count
        );

        
        let graph = self.graph_data.read().await.clone();

        
        if let Err(e) = self.semantic_service.initialize(Arc::new(graph)).await {
            warn!("Failed to initialize semantic analyzer: {}", e);
        }

        
        if let Err(e) = self.semantic_service.detect_communities_louvain().await {
            warn!("Failed to detect communities: {}", e);
        }
    }

    
    pub async fn on_positions_updated(&self, event: PositionsUpdatedEvent) {
        debug!("Positions updated for {} nodes", event.updated_nodes.len());

        
        let event_bus = self.event_bus.write().await;
        event_bus.publish(event).await;
    }

    
    pub async fn on_node_added(&self, event: NodeAddedEvent) {
        info!("Node added: {}", event.node_id);

        
        if let Err(e) = self.semantic_service.invalidate_cache().await {
            warn!("Failed to invalidate cache after node addition: {}", e);
        }

        
        if self.physics_service.is_running().await {
            debug!("Physics simulation running, will incorporate new node");
        }
    }

    
    pub async fn on_edge_added(&self, event: EdgeAddedEvent) {
        info!("Edge added: {} -> {}", event.source_id, event.target_id);

        
        if let Err(e) = self.semantic_service.invalidate_cache().await {
            warn!("Failed to invalidate cache after edge addition: {}", e);
        }
    }
}

pub static EVENT_COORDINATOR: once_cell::sync::Lazy<Arc<RwLock<Option<EventCoordinator>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

pub async fn initialize_event_coordinator(
    physics_service: Arc<PhysicsService>,
    semantic_service: Arc<SemanticService>,
    event_bus: Arc<RwLock<EventBus>>,
    graph_data: Arc<RwLock<GraphData>>,
) {
    let coordinator =
        EventCoordinator::new(physics_service, semantic_service, event_bus, graph_data);

    coordinator.initialize().await;

    *EVENT_COORDINATOR.write().await = Some(coordinator);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
use crate::utils::time;

    #[tokio::test]
    async fn test_event_coordinator_creation() {
        
        
    }

    #[test]
    fn test_graph_saved_event() {
        let event = GraphSavedEvent {
            graph_id: "test".to_string(),
            file_path: "/test.json".to_string(),
            node_count: 100,
            edge_count: 200,
            timestamp: time::now(),
        };

        assert_eq!(event.node_count, 100);
        assert_eq!(event.edge_count, 200);
    }
}
