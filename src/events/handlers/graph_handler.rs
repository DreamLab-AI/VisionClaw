use async_trait:::async_trait;
use std::collections:::HashMap;
use std::sync:::Arc;
use tokio::sync:::RwLock;

use crate::events::domain_events::*;
use crate::events::types::{EventError, EventHandler, EventResult, StoredEvent};
use crate::utils::json::from_json;

pub struct GraphEventHandler {
    handler_id: String,
    cache: Arc<RwLock<GraphCache>>,
}

#[derive(Debug, Default)]
struct GraphCache {
    nodes: HashMap<String, NodeInfo>,
    edges: HashMap<String, EdgeInfo>,
    graph_stats: GraphStats,
}

#[derive(Debug, Clone)]
struct NodeInfo {
    label: String,
    node_type: String,
}

#[derive(Debug, Clone)]
struct EdgeInfo {
    source_id: String,
    target_id: String,
    edge_type: String,
}

#[derive(Debug, Default, Clone)]
struct GraphStats {
    node_count: usize,
    edge_count: usize,
}

impl GraphEventHandler {
    pub fn new(handler_id: impl Into<String>) -> Self {
        Self {
            handler_id: handler_id.into(),
            cache: Arc::new(RwLock::new(GraphCache::default())),
        }
    }

    pub async fn get_node_count(&self) -> usize {
        self.cache.read().await.graph_stats.node_count
    }

    pub async fn get_edge_count(&self) -> usize {
        self.cache.read().await.graph_stats.edge_count
    }

    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.nodes.clear();
        cache.edges.clear();
        cache.graph_stats = GraphStats::default();
    }

    async fn handle_node_added(&self, event: &StoredEvent) -> EventResult<()> {
        let data: NodeAddedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse NodeAddedEvent: {}", e)))?;

        let mut cache = self.cache.write().await;
        cache.nodes.insert(
            data.node_id.clone(),
            NodeInfo {
                label: data.label,
                node_type: data.node_type,
            },
        );
        cache.graph_stats.node_count += 1;

        Ok(())
    }

    async fn handle_node_removed(&self, event: &StoredEvent) -> EventResult<()> {
        let data: NodeRemovedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse NodeRemovedEvent: {}", e)))?;

        let mut cache = self.cache.write().await;
        if cache.nodes.remove(&data.node_id).is_some() {
            cache.graph_stats.node_count = cache.graph_stats.node_count.saturating_sub(1);
        }

        Ok(())
    }

    async fn handle_edge_added(&self, event: &StoredEvent) -> EventResult<()> {
        let data: EdgeAddedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse EdgeAddedEvent: {}", e)))?;

        let mut cache = self.cache.write().await;
        cache.edges.insert(
            data.edge_id.clone(),
            EdgeInfo {
                source_id: data.source_id,
                target_id: data.target_id,
                edge_type: data.edge_type,
            },
        );
        cache.graph_stats.edge_count += 1;

        Ok(())
    }

    async fn handle_edge_removed(&self, event: &StoredEvent) -> EventResult<()> {
        let data: EdgeRemovedEvent = from_json(&event.data)
            .map_err(|e| EventError::Handler(format!("Failed to parse EdgeRemovedEvent: {}", e)))?;

        let mut cache = self.cache.write().await;
        if cache.edges.remove(&data.edge_id).is_some() {
            cache.graph_stats.edge_count = cache.graph_stats.edge_count.saturating_sub(1);
        }

        Ok(())
    }

    async fn handle_graph_cleared(&self, _event: &StoredEvent) -> EventResult<()> {
        self.clear_cache().await;
        Ok(())
    }
}

#[async_trait]
impl EventHandler for GraphEventHandler {
    fn event_type(&self) -> &'static str {
        
        "Graph"
    }

    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    async fn handle(&self, event: &StoredEvent) -> EventResult<()> {
        match event.metadata.event_type.as_str() {
            "NodeAdded" => self.handle_node_added(event).await,
            "NodeRemoved" => self.handle_node_removed(event).await,
            "EdgeAdded" => self.handle_edge_added(event).await,
            "EdgeRemoved" => self.handle_edge_removed(event).await,
            "GraphCleared" => self.handle_graph_cleared(event).await,
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
    async fn test_node_added_event() {
        let handler = GraphEventHandler::new("graph-handler");

        let event_data = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test Node".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        let stored_event = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: to_json(&event_data).unwrap(),
            sequence: 1,
        };

        handler.handle(&stored_event).await.unwrap();
        assert_eq!(handler.get_node_count().await, 1);
    }

    #[tokio::test]
    async fn test_edge_added_event() {
        let handler = GraphEventHandler::new("graph-handler");

        let event_data = EdgeAddedEvent {
            edge_id: "edge-1".to_string(),
            source_id: "node-1".to_string(),
            target_id: "node-2".to_string(),
            edge_type: "knows".to_string(),
            weight: 1.0,
            timestamp: time::now(),
        };

        let stored_event = StoredEvent {
            metadata: EventMetadata::new(
                "edge-1".to_string(),
                "Edge".to_string(),
                "EdgeAdded".to_string(),
            ),
            data: to_json(&event_data).unwrap(),
            sequence: 1,
        };

        handler.handle(&stored_event).await.unwrap();
        assert_eq!(handler.get_edge_count().await, 1);
    }

    #[tokio::test]
    async fn test_graph_cleared_event() {
        let handler = GraphEventHandler::new("graph-handler");

        
        let add_event = NodeAddedEvent {
            node_id: "node-1".to_string(),
            label: "Test".to_string(),
            node_type: "Person".to_string(),
            properties: HashMap::new(),
            timestamp: time::now(),
        };

        let stored_add = StoredEvent {
            metadata: EventMetadata::new(
                "node-1".to_string(),
                "Node".to_string(),
                "NodeAdded".to_string(),
            ),
            data: to_json(&add_event).unwrap(),
            sequence: 1,
        };

        handler.handle(&stored_add).await.unwrap();
        assert_eq!(handler.get_node_count().await, 1);

        
        let clear_event = GraphClearedEvent {
            graph_id: "graph-1".to_string(),
            timestamp: time::now(),
        };

        let stored_clear = StoredEvent {
            metadata: EventMetadata::new(
                "graph-1".to_string(),
                "Graph".to_string(),
                "GraphCleared".to_string(),
            ),
            data: to_json(&clear_event).unwrap(),
            sequence: 2,
        };

        handler.handle(&stored_clear).await.unwrap();
        assert_eq!(handler.get_node_count().await, 0);
    }
}

