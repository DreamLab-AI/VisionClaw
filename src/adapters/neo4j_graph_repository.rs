//! Neo4j Graph Repository - Direct queries with intelligent caching
//!
//! Professional architecture:
//! - Neo4j as single source of truth
//! - Read-through LRU cache for performance
//! - Lazy loading with pagination
//! - Batch operations for efficiency

use async_trait::async_trait;
use lru::LruCache;
use neo4rs::{Graph, query, BoltInteger, BoltFloat};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, instrument};

use crate::actors::graph_actor::{AutoBalanceNotification, PhysicsState};
use crate::models::constraints::ConstraintSet;
use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::metadata::Metadata;
use crate::models::node::Node;
use crate::ports::graph_repository::{
    GraphRepository, GraphRepositoryError, PathfindingParams, PathfindingResult, Result,
};
use crate::ports::settings_repository::{SettingsRepository, SettingValue};
use crate::settings::models::NodeFilterSettings;
use glam::Vec3;

const CACHE_SIZE: usize = 10_000;
const BATCH_SIZE: usize = 1000;

/// Neo4j-backed graph repository with intelligent caching
pub struct Neo4jGraphRepository {
    graph: Arc<Graph>,

    /// LRU cache for nodes (id -> Node)
    node_cache: Arc<RwLock<LruCache<u32, Node>>>,

    /// LRU cache for edges (id -> Edge)
    edge_cache: Arc<RwLock<LruCache<String, Edge>>>,

    /// Cached graph snapshot (refreshed periodically or on demand)
    graph_snapshot: Arc<RwLock<Option<Arc<GraphData>>>>,

    /// Track if full graph is loaded
    is_loaded: Arc<RwLock<bool>>,

    /// Settings repository for reading node filter settings
    settings_repository: Option<Arc<dyn SettingsRepository>>,

    /// Cached node filter settings
    node_filter_settings: Arc<RwLock<NodeFilterSettings>>,
}

impl Neo4jGraphRepository {
    pub fn new(graph: Arc<Graph>) -> Self {
        Self {
            graph,
            node_cache: Arc::new(RwLock::new(
                LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())
            )),
            edge_cache: Arc::new(RwLock::new(
                LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())
            )),
            graph_snapshot: Arc::new(RwLock::new(None)),
            is_loaded: Arc::new(RwLock::new(false)),
            settings_repository: None,
            node_filter_settings: Arc::new(RwLock::new(NodeFilterSettings::default())),
        }
    }

    /// Create repository with settings support for node filtering
    pub fn with_settings(graph: Arc<Graph>, settings_repository: Arc<dyn SettingsRepository>) -> Self {
        Self {
            graph,
            node_cache: Arc::new(RwLock::new(
                LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())
            )),
            edge_cache: Arc::new(RwLock::new(
                LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap())
            )),
            graph_snapshot: Arc::new(RwLock::new(None)),
            is_loaded: Arc::new(RwLock::new(false)),
            settings_repository: Some(settings_repository),
            node_filter_settings: Arc::new(RwLock::new(NodeFilterSettings::default())),
        }
    }

    /// Update node filter settings (called from settings actor when settings change)
    pub async fn set_node_filter_settings(&self, settings: NodeFilterSettings) {
        info!("Updating node filter settings: enabled={}, quality_threshold={}",
              settings.enabled, settings.quality_threshold);
        *self.node_filter_settings.write().await = settings;
        // Invalidate graph cache to force reload with new filters
        self.invalidate_cache().await;
    }

    /// Get current node filter settings
    pub async fn get_node_filter_settings(&self) -> NodeFilterSettings {
        self.node_filter_settings.read().await.clone()
    }

    /// Load node filter settings from repository
    async fn load_node_filter_settings(&self) -> NodeFilterSettings {
        if let Some(ref repo) = self.settings_repository {
            match repo.get_setting("node_filter").await {
                Ok(Some(SettingValue::Json(json))) => {
                    match serde_json::from_value::<NodeFilterSettings>(json) {
                        Ok(settings) => {
                            info!("Loaded node filter settings: enabled={}, threshold={}",
                                  settings.enabled, settings.quality_threshold);
                            return settings;
                        }
                        Err(e) => {
                            warn!("Failed to parse node filter settings: {}", e);
                        }
                    }
                }
                Ok(_) => {
                    debug!("No node filter settings found, using defaults");
                }
                Err(e) => {
                    warn!("Failed to load node filter settings: {}", e);
                }
            }
        }
        NodeFilterSettings::default()
    }

    /// Load full graph from Neo4j (called on startup or refresh)
    #[instrument(skip(self))]
    pub async fn load_graph(&self) -> Result<()> {
        info!("Loading full graph from Neo4j...");

        // Load and cache node filter settings first
        let filter_settings = self.load_node_filter_settings().await;
        *self.node_filter_settings.write().await = filter_settings.clone();

        info!("Node filter: enabled={}, quality_threshold={}, filter_by_quality={}",
              filter_settings.enabled, filter_settings.quality_threshold, filter_settings.filter_by_quality);

        // Load nodes with filter applied
        let nodes = self.load_all_nodes_filtered(&filter_settings).await?;
        let edges = self.load_all_edges().await?;
        let metadata = self.load_all_metadata().await?;

        info!("Loaded {} nodes (filtered), {} edges, {} metadata entries",
              nodes.len(), edges.len(), metadata.len());

        // Update cache
        {
            let mut node_cache = self.node_cache.write().await;
            for node in &nodes {
                node_cache.put(node.id, node.clone());
            }
        }

        {
            let mut edge_cache = self.edge_cache.write().await;
            for edge in &edges {
                edge_cache.put(edge.id.clone(), edge.clone());
            }
        }

        // Create snapshot
        let graph_data = Arc::new(GraphData {
            nodes,
            edges,
            metadata,
            id_to_metadata: HashMap::new(),
        });

        *self.graph_snapshot.write().await = Some(graph_data);
        *self.is_loaded.write().await = true;

        Ok(())
    }

    /// Load all nodes from Neo4j with optional quality/authority filtering
    async fn load_all_nodes_filtered(&self, filter: &NodeFilterSettings) -> Result<Vec<Node>> {
        // Build WHERE clause based on filter settings
        let where_clause = if filter.enabled {
            let mut conditions = Vec::new();

            if filter.filter_by_quality {
                conditions.push(format!(
                    "(n.quality_score IS NULL OR n.quality_score >= {})",
                    filter.quality_threshold
                ));
            }

            if filter.filter_by_authority {
                conditions.push(format!(
                    "(n.authority_score IS NULL OR n.authority_score >= {})",
                    filter.authority_threshold
                ));
            }

            if conditions.is_empty() {
                String::new()
            } else {
                let join_op = if filter.filter_mode == "and" { " AND " } else { " OR " };
                format!("WHERE {}", conditions.join(join_op))
            }
        } else {
            String::new()
        };

        // Use COALESCE to prefer sim_* (GPU physics state) over x/y/z (initial positions)
        // This preserves the calculated layout during content sync
        let query_str = format!("
            MATCH (n:GraphNode)
            {}
            RETURN n.id as id,
                   n.metadata_id as metadata_id,
                   n.label as label,
                   COALESCE(n.sim_x, n.x) as x,
                   COALESCE(n.sim_y, n.y) as y,
                   COALESCE(n.sim_z, n.z) as z,
                   n.vx as vx,
                   n.vy as vy,
                   n.vz as vz,
                   n.mass as mass,
                   n.size as size,
                   n.color as color,
                   n.weight as weight,
                   n.node_type as node_type,
                   n.cluster as cluster,
                   n.cluster_id as cluster_id,
                   n.anomaly_score as anomaly_score,
                   n.community_id as community_id,
                   n.hierarchy_level as hierarchy_level,
                   n.quality_score as quality_score,
                   n.authority_score as authority_score,
                   n.metadata as metadata_json
            ORDER BY id
        ", where_clause);

        info!("Executing node query with filter: {}", if filter.enabled { &where_clause } else { "disabled" });

        let mut result = self.graph
            .execute(query(&query_str))
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to query nodes: {}", e)))?;

        let mut nodes = Vec::new();

        while let Some(row) = result.next().await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to fetch row: {}", e)))?
        {
            let id: BoltInteger = row.get("id")
                .map_err(|e| GraphRepositoryError::DeserializationError(format!("Missing id: {}", e)))?;

            let metadata_id: String = row.get("metadata_id").unwrap_or_default();
            let label: String = row.get("label").unwrap_or_default();

            // Position
            let x: BoltFloat = row.get("x").unwrap_or(BoltFloat { value: 0.0 });
            let y: BoltFloat = row.get("y").unwrap_or(BoltFloat { value: 0.0 });
            let z: BoltFloat = row.get("z").unwrap_or(BoltFloat { value: 0.0 });

            // Velocity
            let vx: BoltFloat = row.get("vx").unwrap_or(BoltFloat { value: 0.0 });
            let vy: BoltFloat = row.get("vy").unwrap_or(BoltFloat { value: 0.0 });
            let vz: BoltFloat = row.get("vz").unwrap_or(BoltFloat { value: 0.0 });

            // Properties
            let mass: BoltFloat = row.get("mass").unwrap_or(BoltFloat { value: 1.0 });
            let size: BoltFloat = row.get("size").unwrap_or(BoltFloat { value: 1.0 });
            let color: String = row.get("color").unwrap_or_else(|_| "#888888".to_string());
            let weight: BoltFloat = row.get("weight").unwrap_or(BoltFloat { value: 1.0 });
            let node_type: String = row.get("node_type").unwrap_or_else(|_| "default".to_string());
            let cluster: Option<i64> = row.get("cluster").ok();

            // Analytics fields (P0-4)
            let cluster_id: Option<BoltInteger> = row.get("cluster_id").ok();
            let anomaly_score: Option<BoltFloat> = row.get("anomaly_score").ok();
            let community_id: Option<BoltInteger> = row.get("community_id").ok();
            let hierarchy_level: Option<BoltInteger> = row.get("hierarchy_level").ok();

            // Quality/authority scores for filtering
            let quality_score: Option<BoltFloat> = row.get("quality_score").ok();
            let authority_score: Option<BoltFloat> = row.get("authority_score").ok();

            // Metadata JSON
            let metadata_json: String = row.get("metadata_json").unwrap_or_else(|_| "{}".to_string());
            let mut metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
                .unwrap_or_default();

            // Store analytics in metadata for now (Node struct doesn't have dedicated fields yet)
            if let Some(cid) = cluster_id {
                metadata.insert("cluster_id".to_string(), cid.value.to_string());
            }
            if let Some(score) = anomaly_score {
                metadata.insert("anomaly_score".to_string(), score.value.to_string());
            }
            if let Some(cid) = community_id {
                metadata.insert("community_id".to_string(), cid.value.to_string());
            }
            if let Some(level) = hierarchy_level {
                metadata.insert("hierarchy_level".to_string(), level.value.to_string());
            }
            // Store quality/authority scores in metadata
            if let Some(qs) = quality_score {
                metadata.insert("quality_score".to_string(), qs.value.to_string());
            }
            if let Some(as_score) = authority_score {
                metadata.insert("authority_score".to_string(), as_score.value.to_string());
            }

            let node = Node {
                id: id.value as u32,
                metadata_id,
                label,
                data: crate::utils::socket_flow_messages::BinaryNodeData {
                    node_id: id.value as u32,
                    x: x.value as f32,
                    y: y.value as f32,
                    z: z.value as f32,
                    vx: vx.value as f32,
                    vy: vy.value as f32,
                    vz: vz.value as f32,
                },
                x: Some(x.value as f32),
                y: Some(y.value as f32),
                z: Some(z.value as f32),
                vx: Some(vx.value as f32),
                vy: Some(vy.value as f32),
                vz: Some(vz.value as f32),
                mass: Some(mass.value as f32),
                size: Some(size.value as f32),
                color: Some(color),
                weight: Some(weight.value as f32),
                node_type: Some(node_type),
                group: cluster.map(|c| c.to_string()),
                metadata,
                owl_class_iri: None,
                file_size: 0,
                user_data: None,
            };

            nodes.push(node);
        }

        Ok(nodes)
    }

    /// Load all edges from Neo4j
    /// Matches any relationship type between GraphNode nodes, not just :EDGE,
    /// to handle cases where relationships were created with different types.
    async fn load_all_edges(&self) -> Result<Vec<Edge>> {
        let query_str = "
            MATCH (source:GraphNode)-[r]->(target:GraphNode)
            RETURN source.id as source_id,
                   target.id as target_id,
                   COALESCE(r.weight, 1.0) as weight,
                   COALESCE(r.edge_type, r.relation_type, type(r)) as edge_type
        ";

        let mut result = self.graph
            .execute(query(query_str))
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to query edges: {}", e)))?;

        let mut edges = Vec::new();

        while let Some(row) = result.next().await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to fetch row: {}", e)))?
        {
            let source_id: BoltInteger = row.get("source_id")
                .map_err(|e| GraphRepositoryError::DeserializationError(format!("Missing source_id: {}", e)))?;
            let target_id: BoltInteger = row.get("target_id")
                .map_err(|e| GraphRepositoryError::DeserializationError(format!("Missing target_id: {}", e)))?;

            let weight: BoltFloat = row.get("weight").unwrap_or(BoltFloat { value: 1.0 });
            let edge_type: String = row.get("edge_type").unwrap_or_else(|_| "default".to_string());

            let edge = Edge {
                id: format!("{}-{}", source_id.value, target_id.value),
                source: source_id.value as u32,
                target: target_id.value as u32,
                weight: weight.value as f32,
                edge_type: Some(edge_type),
                owl_property_iri: None,
                metadata: None,
            };

            edges.push(edge);
        }

        Ok(edges)
    }

    /// Load all metadata from Neo4j
    async fn load_all_metadata(&self) -> Result<HashMap<String, Metadata>> {
        // For now, extract from nodes
        // Could be separate MATCH query if metadata is stored separately
        Ok(HashMap::new())
    }

    /// Invalidate cache (call after mutations)
    pub async fn invalidate_cache(&self) {
        *self.is_loaded.write().await = false;
        *self.graph_snapshot.write().await = None;
        self.node_cache.write().await.clear();
        self.edge_cache.write().await.clear();
    }
}

#[async_trait]
impl GraphRepository for Neo4jGraphRepository {
    async fn add_nodes(&self, nodes: Vec<Node>) -> Result<Vec<u32>> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }

        // PERF: Use UNWIND for batch insert - 50-100x faster than sequential inserts
        // CRITICAL: Preserve physics state (sim_x/y/z, vx/vy/vz) during content sync
        // ON CREATE: Initialize all positions (content AND physics)
        // ON MATCH: Only update content properties, NEVER touch sim_* or velocity
        let query_str = "
            UNWIND range(0, size($ids)-1) AS i
            MERGE (n:GraphNode {id: $ids[i]})
            ON CREATE SET
                n.created_at = datetime(),
                n.metadata_id = $metadata_ids[i],
                n.label = $labels[i],
                n.x = $xs[i],
                n.y = $ys[i],
                n.z = $zs[i],
                n.sim_x = $xs[i],
                n.sim_y = $ys[i],
                n.sim_z = $zs[i],
                n.vx = $vxs[i],
                n.vy = $vys[i],
                n.vz = $vzs[i],
                n.mass = $masses[i],
                n.size = $sizes[i],
                n.color = $colors[i],
                n.weight = $weights[i],
                n.node_type = $node_types[i],
                n.quality_score = $quality_scores[i],
                n.authority_score = $authority_scores[i],
                n.metadata = $metadatas[i]
            ON MATCH SET
                n.updated_at = datetime(),
                n.metadata_id = $metadata_ids[i],
                n.label = $labels[i],
                n.mass = $masses[i],
                n.size = COALESCE($sizes[i], n.size),
                n.color = COALESCE($colors[i], n.color),
                n.weight = COALESCE($weights[i], n.weight),
                n.node_type = COALESCE($node_types[i], n.node_type),
                n.quality_score = $quality_scores[i],
                n.authority_score = $authority_scores[i],
                n.metadata = $metadatas[i]
            RETURN n.id AS id
        ";

        // Prepare parallel arrays for UNWIND (neo4rs native type support)
        let mut ids: Vec<i64> = Vec::with_capacity(nodes.len());
        let mut metadata_ids: Vec<String> = Vec::with_capacity(nodes.len());
        let mut labels: Vec<String> = Vec::with_capacity(nodes.len());
        let mut xs: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut ys: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut zs: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut vxs: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut vys: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut vzs: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut masses: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut sizes: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut colors: Vec<String> = Vec::with_capacity(nodes.len());
        let mut weights: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut node_types: Vec<String> = Vec::with_capacity(nodes.len());
        let mut quality_scores: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut authority_scores: Vec<f64> = Vec::with_capacity(nodes.len());
        let mut metadatas: Vec<String> = Vec::with_capacity(nodes.len());
        let mut added_ids = Vec::with_capacity(nodes.len());

        for node in &nodes {
            let metadata_json = serde_json::to_string(&node.metadata)
                .map_err(|e| GraphRepositoryError::SerializationError(format!("Failed to serialize metadata: {}", e)))?;

            let quality_score: f64 = node.metadata
                .get("quality_score")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0);

            let authority_score: f64 = node.metadata
                .get("authority_score")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0);

            ids.push(node.id as i64);
            metadata_ids.push(node.metadata_id.clone());
            labels.push(node.label.clone());
            xs.push(node.data.position().x as f64);
            ys.push(node.data.position().y as f64);
            zs.push(node.data.position().z as f64);
            vxs.push(node.data.velocity().x as f64);
            vys.push(node.data.velocity().y as f64);
            vzs.push(node.data.velocity().z as f64);
            masses.push(node.data.mass() as f64);
            sizes.push(node.size.unwrap_or(1.0) as f64);
            colors.push(node.color.clone().unwrap_or_else(|| "#888888".to_string()));
            weights.push(node.weight.unwrap_or(1.0) as f64);
            node_types.push(node.node_type.clone().unwrap_or_else(|| "default".to_string()));
            quality_scores.push(quality_score);
            authority_scores.push(authority_score);
            metadatas.push(metadata_json);
            added_ids.push(node.id);
        }

        // Execute single batch query with parallel arrays
        self.graph
            .run(query(query_str)
                .param("ids", ids)
                .param("metadata_ids", metadata_ids)
                .param("labels", labels)
                .param("xs", xs)
                .param("ys", ys)
                .param("zs", zs)
                .param("vxs", vxs)
                .param("vys", vys)
                .param("vzs", vzs)
                .param("masses", masses)
                .param("sizes", sizes)
                .param("colors", colors)
                .param("weights", weights)
                .param("node_types", node_types)
                .param("quality_scores", quality_scores)
                .param("authority_scores", authority_scores)
                .param("metadatas", metadatas))
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to batch add nodes: {}", e)))?;

        // Update cache for all nodes
        {
            let mut cache = self.node_cache.write().await;
            for node in nodes {
                cache.put(node.id, node);
            }
        }

        // Invalidate full graph snapshot
        self.invalidate_cache().await;

        Ok(added_ids)
    }

    async fn add_edges(&self, edges: Vec<Edge>) -> Result<Vec<String>> {
        if edges.is_empty() {
            return Ok(Vec::new());
        }

        // PERF: Use UNWIND with parallel arrays - neo4rs native type support
        let query_str = "
            UNWIND range(0, size($edge_ids)-1) AS i
            MATCH (source:GraphNode {id: $source_ids[i]})
            MATCH (target:GraphNode {id: $target_ids[i]})
            MERGE (source)-[r:EDGE]->(target)
            ON CREATE SET r.created_at = datetime()
            ON MATCH SET r.updated_at = datetime()
            SET r.weight = $weights[i],
                r.edge_type = $edge_types[i],
                r.edge_id = $edge_ids[i]
            RETURN $edge_ids[i] AS id
        ";

        // Prepare parallel arrays for UNWIND (neo4rs native type support)
        let mut edge_ids: Vec<String> = Vec::with_capacity(edges.len());
        let mut source_ids: Vec<i64> = Vec::with_capacity(edges.len());
        let mut target_ids: Vec<i64> = Vec::with_capacity(edges.len());
        let mut weights: Vec<f64> = Vec::with_capacity(edges.len());
        let mut edge_types: Vec<String> = Vec::with_capacity(edges.len());
        let mut added_ids = Vec::with_capacity(edges.len());

        for edge in &edges {
            edge_ids.push(edge.id.clone());
            source_ids.push(edge.source as i64);
            target_ids.push(edge.target as i64);
            weights.push(edge.weight as f64);
            edge_types.push(edge.edge_type.clone().unwrap_or_else(|| "default".to_string()));
            added_ids.push(edge.id.clone());
        }

        // Execute single batch query with parallel arrays
        self.graph
            .run(query(query_str)
                .param("edge_ids", edge_ids)
                .param("source_ids", source_ids)
                .param("target_ids", target_ids)
                .param("weights", weights)
                .param("edge_types", edge_types))
            .await
            .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to batch add edges: {}", e)))?;

        // Update cache for all edges
        {
            let mut cache = self.edge_cache.write().await;
            for edge in edges {
                cache.put(edge.id.clone(), edge);
            }
        }

        // Invalidate full graph snapshot
        self.invalidate_cache().await;

        Ok(added_ids)
    }

    async fn get_graph(&self) -> Result<Arc<GraphData>> {
        // Check if loaded
        if !*self.is_loaded.read().await {
            self.load_graph().await?;
        }

        // Return cached snapshot
        self.graph_snapshot.read().await
            .clone()
            .ok_or_else(|| GraphRepositoryError::AccessError("Graph not loaded".to_string()))
    }

    async fn get_node_map(&self) -> Result<Arc<HashMap<u32, Node>>> {
        let graph = self.get_graph().await?;
        let map: HashMap<u32, Node> = graph.nodes.iter()
            .map(|n| (n.id, n.clone()))
            .collect();
        Ok(Arc::new(map))
    }

    async fn get_physics_state(&self) -> Result<PhysicsState> {
        // Physics state would be managed separately by PhysicsActor
        Ok(PhysicsState::default())
    }

    async fn update_positions(
        &self,
        updates: Vec<(u32, crate::ports::graph_repository::BinaryNodeData)>,
    ) -> Result<()> {
        // Update sim_* properties (GPU physics state) and velocities
        // x/y/z remain as initial/content positions - NEVER overwritten by physics
        // BinaryNodeData format: (x, y, z, vx, vy, vz)
        for (node_id, data) in updates {
            let query_str = "
                MATCH (n:GraphNode {id: $id})
                SET n.sim_x = $x,
                    n.sim_y = $y,
                    n.sim_z = $z,
                    n.vx = $vx,
                    n.vy = $vy,
                    n.vz = $vz
            ";

            self.graph
                .run(query(query_str)
                    .param("id", node_id as i64)
                    .param("x", data.0 as f64)
                    .param("y", data.1 as f64)
                    .param("z", data.2 as f64)
                    .param("vx", data.3 as f64)
                    .param("vy", data.4 as f64)
                    .param("vz", data.5 as f64))
                .await
                .map_err(|e| GraphRepositoryError::AccessError(format!("Failed to update position/velocity: {}", e)))?;
        }

        Ok(())
    }

    async fn clear_dirty_nodes(&self) -> Result<()> {
        // Not applicable for Neo4j-backed repo
        Ok(())
    }

    // Implement remaining trait methods with Neo4j queries...
    async fn get_auto_balance_notifications(&self) -> Result<Vec<AutoBalanceNotification>> {
        Ok(Vec::new())
    }

    async fn get_constraints(&self) -> Result<ConstraintSet> {
        Ok(ConstraintSet::default())
    }

    async fn compute_shortest_paths(&self, _params: PathfindingParams) -> Result<PathfindingResult> {
        Err(GraphRepositoryError::NotImplemented)
    }

    async fn get_dirty_nodes(&self) -> Result<HashSet<u32>> {
        Ok(HashSet::new())
    }

    async fn get_node_positions(&self) -> Result<Vec<(u32, Vec3)>> {
        // The graph already uses sim_* positions via COALESCE in load query
        // So this returns GPU physics state when available
        let graph = self.get_graph().await?;
        let positions = graph.nodes.iter()
            .map(|n| (n.id, Vec3::new(
                n.x.unwrap_or(0.0),
                n.y.unwrap_or(0.0),
                n.z.unwrap_or(0.0)
            )))
            .collect();
        Ok(positions)
    }

    async fn get_bots_graph(&self) -> Result<Arc<GraphData>> {
        // For now, return the same graph
        // In the future, this could filter for bot nodes
        self.get_graph().await
    }

    async fn get_equilibrium_status(&self) -> Result<bool> {
        // This would check physics equilibrium state
        // For now, always return false (not in equilibrium)
        Ok(false)
    }
}
