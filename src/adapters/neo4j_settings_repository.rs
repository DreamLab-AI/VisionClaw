// src/adapters/neo4j_settings_repository.rs
//! Neo4j Settings Repository Adapter
//!
//! Implements the SettingsRepository port using Neo4j graph database with
//! category-based schema modeling, caching, and comprehensive error handling.
//!
//! ## Schema Design
//!
//! The settings are organized using a hierarchical node structure:
//! - `:SettingsRoot` - Root node (singleton, id: "default")
//! - Category nodes: `:PhysicsSettings`, `:RenderingSettings`, `:SystemSettings`, etc.
//! - Settings stored as properties on category nodes
//! - Relationships: `(:SettingsRoot)-[:HAS_PHYSICS_SETTINGS]->(:PhysicsSettings)`

use async_trait::async_trait;
use neo4rs::{Graph, query, ConfigBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, instrument};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::PhysicsSettings;
use crate::ports::settings_repository::{
    AppFullSettings, Result as RepoResult, SettingValue, SettingsRepository,
    SettingsRepositoryError,
};
use crate::utils::json::{from_json, to_json};
use crate::utils::neo4j_helpers::json_to_bolt;

/// User node - Nostr pubkey-based user identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub pubkey: String,
    pub is_power_user: bool,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub display_name: Option<String>,
}

/// UserSettings node - user's personal settings (full AppFullSettings)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsNode {
    pub pubkey: String,
    pub settings_json: String,
    pub updated_at: DateTime<Utc>,
}

/// UserFilter node - user's graph filter preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserFilter {
    pub pubkey: String,
    pub enabled: bool,
    pub quality_threshold: f64,
    pub authority_threshold: f64,
    pub filter_by_quality: bool,
    pub filter_by_authority: bool,
    pub filter_mode: String,
    pub max_nodes: Option<i32>,
    pub updated_at: DateTime<Utc>,
}

impl Default for UserFilter {
    fn default() -> Self {
        Self {
            pubkey: String::new(),
            enabled: true,
            quality_threshold: 0.7,
            authority_threshold: 0.5,
            filter_by_quality: true,
            filter_by_authority: false,
            filter_mode: "or".to_string(),
            max_nodes: Some(10000),
            updated_at: Utc::now(),
        }
    }
}

/// Neo4j configuration for settings repository
#[derive(Debug, Clone)]
pub struct Neo4jSettingsConfig {
    pub uri: String,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
    pub fetch_size: usize,
    pub max_connections: usize,
}

impl Default for Neo4jSettingsConfig {
    fn default() -> Self {
        Self {
            uri: std::env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
            user: std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string()),
            password: std::env::var("NEO4J_PASSWORD").unwrap_or_else(|_| "password".to_string()),
            database: std::env::var("NEO4J_DATABASE").ok(),
            fetch_size: 500,
            max_connections: 10,
        }
    }
}

/// Cache entry with TTL support
struct CachedSetting {
    value: SettingValue,
    timestamp: std::time::Instant,
}

/// Settings cache with TTL
struct SettingsCache {
    settings: HashMap<String, CachedSetting>,
    last_updated: std::time::Instant,
    ttl_seconds: u64,
}

impl SettingsCache {
    fn new(ttl_seconds: u64) -> Self {
        Self {
            settings: HashMap::new(),
            last_updated: std::time::Instant::now(),
            ttl_seconds,
        }
    }

    fn get(&self, key: &str) -> Option<SettingValue> {
        if let Some(cached) = self.settings.get(key) {
            if cached.timestamp.elapsed().as_secs() < self.ttl_seconds {
                return Some(cached.value.clone());
            }
        }
        None
    }

    fn insert(&mut self, key: String, value: SettingValue) {
        self.settings.insert(
            key,
            CachedSetting {
                value,
                timestamp: std::time::Instant::now(),
            },
        );
    }

    fn remove(&mut self, key: &str) {
        self.settings.remove(key);
    }

    fn clear(&mut self) {
        self.settings.clear();
        self.last_updated = std::time::Instant::now();
    }
}

/// Neo4j Settings Repository implementation
pub struct Neo4jSettingsRepository {
    graph: Arc<Graph>,
    cache: Arc<RwLock<SettingsCache>>,
    _config: Neo4jSettingsConfig,
}

impl Neo4jSettingsRepository {
    /// Create a new Neo4j settings repository with configuration
    pub async fn new(config: Neo4jSettingsConfig) -> RepoResult<Self> {
        info!("Initializing Neo4jSettingsRepository with URI: {}", config.uri);

        // Build Neo4j configuration
        let mut builder = ConfigBuilder::default()
            .uri(&config.uri)
            .user(&config.user)
            .password(&config.password)
            .fetch_size(config.fetch_size)
            .max_connections(config.max_connections);

        if let Some(ref db) = config.database {
            builder = builder.db(neo4rs::Database::from(db.as_str()));
        }

        let neo4j_config = builder.build()
            .map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to build Neo4j config: {}", e)
            ))?;

        // Connect to Neo4j
        let graph = Graph::connect(neo4j_config)
            .map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to connect to Neo4j: {}", e)
            ))?;

        let repository = Self {
            graph: Arc::new(graph),
            cache: Arc::new(RwLock::new(SettingsCache::new(300))), // 5 min TTL
            _config: config,
        };

        // Initialize schema
        repository.initialize_schema().await?;

        info!("✅ Neo4jSettingsRepository initialized successfully");
        Ok(repository)
    }

    /// Initialize the Neo4j schema for settings storage
    async fn initialize_schema(&self) -> RepoResult<()> {
        info!("Initializing Neo4j settings schema");

        // Create constraints for unique settings root
        let constraints = vec![
            "CREATE CONSTRAINT settings_root_id IF NOT EXISTS FOR (s:SettingsRoot) REQUIRE s.id IS UNIQUE",
            "CREATE CONSTRAINT user_pubkey_unique IF NOT EXISTS FOR (u:User) REQUIRE u.pubkey IS UNIQUE",
        ];

        for constraint_query in constraints {
            self.graph.run(query(constraint_query))
                .await
                .map_err(|e| SettingsRepositoryError::DatabaseError(
                    format!("Failed to create constraint: {}", e)
                ))?;
        }

        // Create indices for performance
        let indices = vec![
            "CREATE INDEX settings_key_idx IF NOT EXISTS FOR (s:Setting) ON (s.key)",
            "CREATE INDEX physics_profile_idx IF NOT EXISTS FOR (p:PhysicsProfile) ON (p.name)",
            "CREATE INDEX user_settings_pubkey_idx IF NOT EXISTS FOR (us:UserSettings) ON (us.pubkey)",
            "CREATE INDEX user_filter_pubkey_idx IF NOT EXISTS FOR (uf:UserFilter) ON (uf.pubkey)",
        ];

        for index_query in indices {
            self.graph.run(query(index_query))
                .await
                .map_err(|e| SettingsRepositoryError::DatabaseError(
                    format!("Failed to create index: {}", e)
                ))?;
        }

        // Create root settings node if it doesn't exist
        let init_query = query(
            "MERGE (s:SettingsRoot {id: 'default'})
             ON CREATE SET s.created_at = datetime(), s.version = '1.0.0'
             RETURN s"
        );

        self.graph.run(init_query)
            .await
            .map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to initialize settings root: {}", e)
            ))?;

        info!("✅ Neo4j settings schema initialized with user support");
        Ok(())
    }

    /// Get setting from cache
    async fn get_from_cache(&self, key: &str) -> Option<SettingValue> {
        let cache = self.cache.read().await;
        if let Some(value) = cache.get(key) {
            debug!("Cache hit for setting: {}", key);
            return Some(value);
        }
        None
    }

    /// Update cache
    async fn update_cache(&self, key: String, value: SettingValue) {
        let mut cache = self.cache.write().await;
        cache.insert(key, value);
    }

    /// Invalidate cache entry
    async fn invalidate_cache(&self, key: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(key);
    }

    /// Clear entire cache
    async fn clear_cache_internal(&self) -> RepoResult<()> {
        let mut cache = self.cache.write().await;
        cache.clear();
        Ok(())
    }

    /// Convert SettingValue to Cypher parameter value
    fn setting_value_to_param(&self, value: &SettingValue) -> serde_json::Value {
        match value {
            SettingValue::String(s) => serde_json::json!({"type": "string", "value": s}),
            SettingValue::Integer(i) => serde_json::json!({"type": "integer", "value": i}),
            SettingValue::Float(f) => serde_json::json!({"type": "float", "value": f}),
            SettingValue::Boolean(b) => serde_json::json!({"type": "boolean", "value": b}),
            SettingValue::Json(j) => {
                // JSON serialization should not fail for valid serde_json::Value,
                // but we handle the edge case by storing as empty string with a warning
                let json_str = to_json(j).unwrap_or_else(|e| {
                    warn!("Failed to serialize JSON setting value: {}", e);
                    String::new()
                });
                serde_json::json!({"type": "json", "value": json_str})
            }
        }
    }

    /// Parse setting value from Neo4j result
    fn parse_setting_value(&self, value_type: &str, value: &serde_json::Value) -> Option<SettingValue> {
        match value_type {
            "string" => value.as_str().map(|s| SettingValue::String(s.to_string())),
            "integer" => value.as_i64().map(SettingValue::Integer),
            "float" => value.as_f64().map(SettingValue::Float),
            "boolean" => value.as_bool().map(SettingValue::Boolean),
            "json" => {
                if let Some(json_str) = value.as_str() {
                    from_json(json_str).ok().map(SettingValue::Json)
                } else {
                    Some(SettingValue::Json(value.clone()))
                }
            }
            _ => None,
        }
    }

    /// Get or create a user node
    #[instrument(skip(self), level = "debug")]
    pub async fn get_or_create_user(&self, pubkey: &str) -> RepoResult<User> {
        let query_str =
            "MERGE (u:User {pubkey: $pubkey})
             ON CREATE SET
                u.is_power_user = false,
                u.created_at = datetime(),
                u.last_seen = datetime()
             ON MATCH SET
                u.last_seen = datetime()
             RETURN u.pubkey AS pubkey, u.is_power_user AS is_power_user,
                    u.created_at AS created_at, u.last_seen AS last_seen,
                    u.display_name AS display_name";

        let mut result = self.graph.execute(
            query(query_str).param("pubkey", pubkey)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to get or create user: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch user row: {}", e))
        )? {
            let pubkey: String = row.get("pubkey").map_err(|e|
                SettingsRepositoryError::DatabaseError(format!("Failed to get pubkey: {}", e))
            )?;

            let is_power_user: bool = row.get("is_power_user").unwrap_or(false);
            let display_name: Option<String> = row.get("display_name").ok();

            Ok(User {
                pubkey,
                is_power_user,
                created_at: Utc::now(),
                last_seen: Utc::now(),
                display_name,
            })
        } else {
            Err(SettingsRepositoryError::DatabaseError(
                "Failed to create or retrieve user".to_string()
            ))
        }
    }

    /// Update user's last seen timestamp
    #[instrument(skip(self), level = "debug")]
    pub async fn update_user_last_seen(&self, pubkey: &str) -> RepoResult<()> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})
             SET u.last_seen = datetime()
             RETURN u";

        self.graph.run(
            query(query_str).param("pubkey", pubkey)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to update user last seen: {}", e)
        ))?;

        Ok(())
    }

    /// Get user settings (full AppFullSettings)
    #[instrument(skip(self), level = "debug")]
    pub async fn get_user_settings(&self, pubkey: &str) -> RepoResult<Option<AppFullSettings>> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})-[:HAS_SETTINGS]->(us:UserSettings)
             RETURN us.settings_json AS settings_json";

        let mut result = self.graph.execute(
            query(query_str).param("pubkey", pubkey)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to query user settings: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch settings row: {}", e))
        )? {
            let settings_json: String = row.get("settings_json").map_err(|e|
                SettingsRepositoryError::DatabaseError(format!("Failed to get settings_json: {}", e))
            )?;

            let settings: AppFullSettings = from_json(&settings_json)
                .map_err(|e| SettingsRepositoryError::SerializationError(e.to_string()))?;

            return Ok(Some(settings));
        }

        Ok(None)
    }

    /// Save user settings (full AppFullSettings)
    #[instrument(skip(self, settings), level = "debug")]
    pub async fn save_user_settings(&self, pubkey: &str, settings: &AppFullSettings) -> RepoResult<()> {
        let settings_json = to_json(settings)
            .map_err(|e| SettingsRepositoryError::SerializationError(e.to_string()))?;

        let query_str =
            "MATCH (u:User {pubkey: $pubkey})
             MERGE (u)-[:HAS_SETTINGS]->(us:UserSettings {pubkey: $pubkey})
             ON CREATE SET
                us.settings_json = $settings_json,
                us.updated_at = datetime()
             ON MATCH SET
                us.settings_json = $settings_json,
                us.updated_at = datetime()
             RETURN us";

        self.graph.run(
            query(query_str)
                .param("pubkey", pubkey)
                .param("settings_json", settings_json)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to save user settings: {}", e)
        ))?;

        info!("Saved user settings for pubkey: {}", pubkey);
        Ok(())
    }

    /// Get user's filter settings
    #[instrument(skip(self), level = "debug")]
    pub async fn get_user_filter(&self, pubkey: &str) -> RepoResult<Option<UserFilter>> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})-[:HAS_FILTER]->(uf:UserFilter)
             RETURN uf.enabled AS enabled, uf.quality_threshold AS quality_threshold,
                    uf.authority_threshold AS authority_threshold, uf.filter_by_quality AS filter_by_quality,
                    uf.filter_by_authority AS filter_by_authority, uf.filter_mode AS filter_mode,
                    uf.max_nodes AS max_nodes";

        let mut result = self.graph.execute(
            query(query_str).param("pubkey", pubkey)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to query user filter: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch filter row: {}", e))
        )? {
            let filter = UserFilter {
                pubkey: pubkey.to_string(),
                enabled: row.get("enabled").unwrap_or(true),
                quality_threshold: row.get("quality_threshold").unwrap_or(0.7),
                authority_threshold: row.get("authority_threshold").unwrap_or(0.5),
                filter_by_quality: row.get("filter_by_quality").unwrap_or(true),
                filter_by_authority: row.get("filter_by_authority").unwrap_or(false),
                filter_mode: row.get("filter_mode").unwrap_or_else(|_| "or".to_string()),
                max_nodes: row.get("max_nodes").ok(),
                updated_at: Utc::now(),
            };

            return Ok(Some(filter));
        }

        Ok(None)
    }

    /// Save user's filter settings
    #[instrument(skip(self, filter), level = "debug")]
    pub async fn save_user_filter(&self, pubkey: &str, filter: &UserFilter) -> RepoResult<()> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})
             MERGE (u)-[:HAS_FILTER]->(uf:UserFilter {pubkey: $pubkey})
             ON CREATE SET
                uf.enabled = $enabled,
                uf.quality_threshold = $quality_threshold,
                uf.authority_threshold = $authority_threshold,
                uf.filter_by_quality = $filter_by_quality,
                uf.filter_by_authority = $filter_by_authority,
                uf.filter_mode = $filter_mode,
                uf.max_nodes = $max_nodes,
                uf.updated_at = datetime()
             ON MATCH SET
                uf.enabled = $enabled,
                uf.quality_threshold = $quality_threshold,
                uf.authority_threshold = $authority_threshold,
                uf.filter_by_quality = $filter_by_quality,
                uf.filter_by_authority = $filter_by_authority,
                uf.filter_mode = $filter_mode,
                uf.max_nodes = $max_nodes,
                uf.updated_at = datetime()
             RETURN uf";

        self.graph.run(
            query(query_str)
                .param("pubkey", pubkey)
                .param("enabled", filter.enabled)
                .param("quality_threshold", filter.quality_threshold)
                .param("authority_threshold", filter.authority_threshold)
                .param("filter_by_quality", filter.filter_by_quality)
                .param("filter_by_authority", filter.filter_by_authority)
                .param("filter_mode", filter.filter_mode.as_str())
                .param("max_nodes", filter.max_nodes.unwrap_or(10000))
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to save user filter: {}", e)
        ))?;

        info!("Saved user filter for pubkey: {}", pubkey);
        Ok(())
    }

    /// Check if user is a power user
    #[instrument(skip(self), level = "debug")]
    pub async fn is_power_user(&self, pubkey: &str) -> RepoResult<bool> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})
             RETURN u.is_power_user AS is_power_user";

        let mut result = self.graph.execute(
            query(query_str).param("pubkey", pubkey)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to query power user status: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch power user row: {}", e))
        )? {
            return Ok(row.get("is_power_user").unwrap_or(false));
        }

        Ok(false)
    }

    /// Set user as power user (or revoke)
    #[instrument(skip(self), level = "debug")]
    pub async fn set_power_user(&self, pubkey: &str, is_power: bool) -> RepoResult<()> {
        let query_str =
            "MATCH (u:User {pubkey: $pubkey})
             SET u.is_power_user = $is_power
             RETURN u";

        self.graph.run(
            query(query_str)
                .param("pubkey", pubkey)
                .param("is_power", is_power)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to set power user status: {}", e)
        ))?;

        info!("Set power user status for pubkey {}: {}", pubkey, is_power);
        Ok(())
    }
}

#[async_trait]
impl SettingsRepository for Neo4jSettingsRepository {
    #[instrument(skip(self), level = "debug")]
    async fn get_setting(&self, key: &str) -> RepoResult<Option<SettingValue>> {
        // Check cache first
        if let Some(cached_value) = self.get_from_cache(key).await {
            return Ok(Some(cached_value));
        }

        // Query Neo4j
        let query_str =
            "MATCH (s:Setting {key: $key})
             RETURN s.value_type AS value_type, s.value AS value";

        let mut result = self.graph.execute(
            query(query_str).param("key", key)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to query setting: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
        )? {
            let value_type: String = row.get("value_type").map_err(|e|
                SettingsRepositoryError::DatabaseError(format!("Failed to get value_type: {}", e))
            )?;

            let value: serde_json::Value = row.get("value").map_err(|e|
                SettingsRepositoryError::DatabaseError(format!("Failed to get value: {}", e))
            )?;

            if let Some(setting_value) = self.parse_setting_value(&value_type, &value) {
                // Update cache
                self.update_cache(key.to_string(), setting_value.clone()).await;
                return Ok(Some(setting_value));
            }
        }

        Ok(None)
    }

    #[instrument(skip(self, value), level = "debug")]
    async fn set_setting(
        &self,
        key: &str,
        value: SettingValue,
        description: Option<&str>,
    ) -> RepoResult<()> {
        let value_param = self.setting_value_to_param(&value);
        let value_type = value_param["type"].as_str().unwrap_or("unknown");
        let value_data = &value_param["value"];

        let query_str =
            "MERGE (s:Setting {key: $key})
             ON CREATE SET
                s.created_at = datetime(),
                s.value_type = $value_type,
                s.value = $value,
                s.description = $description
             ON MATCH SET
                s.updated_at = datetime(),
                s.value_type = $value_type,
                s.value = $value,
                s.description = COALESCE($description, s.description)
             RETURN s";

        self.graph.run(
            query(query_str)
                .param("key", key)
                .param("value_type", value_type)
                .param("value", json_to_bolt(value_data.clone()))
                .param("description", description.unwrap_or(""))
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to set setting: {}", e)
        ))?;

        // Invalidate cache
        self.invalidate_cache(key).await;

        Ok(())
    }

    async fn get_settings_batch(
        &self,
        keys: &[String],
    ) -> RepoResult<HashMap<String, SettingValue>> {
        let mut results = HashMap::new();

        // Try to get from cache first
        for key in keys {
            if let Some(value) = self.get_from_cache(key).await {
                results.insert(key.clone(), value);
            }
        }

        // Get remaining keys from database
        let remaining_keys: Vec<String> = keys.iter()
            .filter(|k| !results.contains_key(*k))
            .cloned()
            .collect();

        if !remaining_keys.is_empty() {
            let query_str =
                "MATCH (s:Setting)
                 WHERE s.key IN $keys
                 RETURN s.key AS key, s.value_type AS value_type, s.value AS value";

            let mut result = self.graph.execute(
                query(query_str).param("keys", remaining_keys)
            ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to query batch settings: {}", e)
            ))?;

            while let Some(row) = result.next().await.map_err(|e|
                SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
            )? {
                let key: String = row.get("key").unwrap_or_default();
                let value_type: String = row.get("value_type").unwrap_or_default();
                let value: serde_json::Value = row.get("value").unwrap_or_default();

                if let Some(setting_value) = self.parse_setting_value(&value_type, &value) {
                    self.update_cache(key.clone(), setting_value.clone()).await;
                    results.insert(key, setting_value);
                }
            }
        }

        Ok(results)
    }

    async fn set_settings_batch(&self, updates: HashMap<String, SettingValue>) -> RepoResult<()> {
        // Use transaction for batch updates
        let mut txn = self.graph.start_txn().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to start transaction: {}", e))
        )?;

        for (key, value) in &updates {
            let value_param = self.setting_value_to_param(value);
            let value_type = value_param["type"].as_str().unwrap_or("unknown");
            let value_data = &value_param["value"];

            let query_str =
                "MERGE (s:Setting {key: $key})
                 ON CREATE SET
                    s.created_at = datetime(),
                    s.value_type = $value_type,
                    s.value = $value
                 ON MATCH SET
                    s.updated_at = datetime(),
                    s.value_type = $value_type,
                    s.value = $value";

            txn.run_queries(vec![
                query(query_str)
                    .param("key", key.as_str())
                    .param("value_type", value_type)
                    .param("value", json_to_bolt(value_data.clone()))
            ]).await.map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to execute batch update: {}", e)
            ))?;
        }

        txn.commit().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to commit transaction: {}", e))
        )?;

        // Clear cache after batch update
        self.clear_cache_internal().await?;

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn load_all_settings(&self) -> RepoResult<Option<AppFullSettings>> {
        info!("Loading all settings: trying Neo4j first, then YAML fallback");

        // Step 1: Try loading from Neo4j SettingsRoot node
        let query_str =
            "MATCH (s:SettingsRoot {id: 'default'})
             WHERE s.full_settings IS NOT NULL
             RETURN s.full_settings AS full_settings";

        let neo4j_result = self.graph.execute(query(query_str)).await;

        if let Ok(mut result) = neo4j_result {
            if let Ok(Some(row)) = result.next().await {
                if let Ok(settings_json) = row.get::<String>("full_settings") {
                    if !settings_json.is_empty() {
                        match from_json::<AppFullSettings>(&settings_json) {
                            Ok(settings) => {
                                info!("Loaded settings from Neo4j SettingsRoot node");
                                return Ok(Some(settings));
                            }
                            Err(e) => {
                                warn!("Failed to deserialize settings from Neo4j: {}, falling back to YAML", e);
                            }
                        }
                    }
                }
            }
        }

        // Step 2: Fall back to YAML file
        let yaml_path = std::env::var("SETTINGS_FILE_PATH")
            .unwrap_or_else(|_| "/app/settings.yaml".to_string());

        let yaml_paths = [yaml_path.as_str(), "data/settings.yaml"];

        for path in &yaml_paths {
            match tokio::fs::read_to_string(path).await {
                Ok(yaml_content) => {
                    match serde_yaml::from_str::<AppFullSettings>(&yaml_content) {
                        Ok(settings) => {
                            info!("Loaded settings from YAML file: {}", path);

                            // Step 3: Persist to Neo4j for future use
                            if let Err(e) = self.save_all_settings(&settings).await {
                                warn!("Failed to persist YAML settings to Neo4j: {}", e);
                            } else {
                                info!("Persisted YAML settings to Neo4j for future loads");
                            }

                            return Ok(Some(settings));
                        }
                        Err(e) => {
                            warn!("Failed to parse YAML settings from {}: {}", path, e);
                        }
                    }
                }
                Err(e) => {
                    debug!("YAML settings file not found at {}: {}", path, e);
                }
            }
        }

        // Step 4: Return defaults as last resort
        warn!("No settings found in Neo4j or YAML files, returning defaults");
        Ok(Some(AppFullSettings::default()))
    }

    #[instrument(skip(self, settings), level = "debug")]
    async fn save_all_settings(&self, settings: &AppFullSettings) -> RepoResult<()> {
        info!("Saving all settings to Neo4j");

        // Serialize settings to JSON
        let settings_json = serde_json::to_value(settings)
            .map_err(|e| SettingsRepositoryError::SerializationError(e.to_string()))?;

        // Store as JSON on root node for now
        let query_str =
            "MERGE (s:SettingsRoot {id: 'default'})
             SET s.full_settings = $settings,
                 s.updated_at = datetime(),
                 s.version = $version
             RETURN s";

        let settings_str = to_json(&settings_json)
            .map_err(|e| SettingsRepositoryError::SerializationError(
                format!("Failed to serialize settings JSON: {}", e)
            ))?;

        self.graph.run(
            query(query_str)
                .param("settings", settings_str)
                .param("version", settings.version.as_str())
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to save all settings: {}", e)
        ))?;

        // Clear cache
        self.clear_cache_internal().await?;

        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    async fn get_physics_settings(&self, profile_name: &str) -> RepoResult<PhysicsSettings> {
        let query_str =
            "MATCH (p:PhysicsProfile {name: $profile_name})
             RETURN p.settings AS settings";

        let mut result = self.graph.execute(
            query(query_str).param("profile_name", profile_name)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to query physics settings: {}", e)
        ))?;

        if let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
        )? {
            let settings_json: String = row.get("settings").unwrap_or_default();
            let settings: PhysicsSettings = from_json(&settings_json)
                .map_err(|e| SettingsRepositoryError::SerializationError(e.to_string()))?;
            return Ok(settings);
        }

        // Return default if not found
        Ok(PhysicsSettings::default())
    }

    #[instrument(skip(self, settings), level = "debug")]
    async fn save_physics_settings(
        &self,
        profile_name: &str,
        settings: &PhysicsSettings,
    ) -> RepoResult<()> {
        let settings_json = to_json(settings)
            .map_err(|e| SettingsRepositoryError::SerializationError(e.to_string()))?;

        let query_str =
            "MERGE (p:PhysicsProfile {name: $profile_name})
             ON CREATE SET
                p.created_at = datetime(),
                p.settings = $settings
             ON MATCH SET
                p.updated_at = datetime(),
                p.settings = $settings
             RETURN p";

        self.graph.run(
            query(query_str)
                .param("profile_name", profile_name)
                .param("settings", settings_json)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to save physics settings: {}", e)
        ))?;

        Ok(())
    }

    async fn delete_setting(&self, key: &str) -> RepoResult<()> {
        let query_str = "MATCH (s:Setting {key: $key}) DELETE s";

        self.graph.run(
            query(query_str).param("key", key)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to delete setting: {}", e)
        ))?;

        self.invalidate_cache(key).await;
        Ok(())
    }

    async fn has_setting(&self, key: &str) -> RepoResult<bool> {
        Ok(self.get_setting(key).await?.is_some())
    }

    async fn list_settings(&self, prefix: Option<&str>) -> RepoResult<Vec<String>> {
        let query_str = if let Some(_p) = prefix {
            "MATCH (s:Setting) WHERE s.key STARTS WITH $prefix RETURN s.key AS key ORDER BY s.key"
        } else {
            "MATCH (s:Setting) RETURN s.key AS key ORDER BY s.key"
        };

        let mut query_obj = query(query_str);
        if let Some(p) = prefix {
            query_obj = query_obj.param("prefix", p);
        }

        let mut result = self.graph.execute(query_obj)
            .await.map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to list settings: {}", e)
            ))?;

        let mut keys = Vec::new();
        while let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
        )? {
            if let Ok(key) = row.get::<String>("key") {
                keys.push(key);
            }
        }

        Ok(keys)
    }

    async fn list_physics_profiles(&self) -> RepoResult<Vec<String>> {
        let query_str = "MATCH (p:PhysicsProfile) RETURN p.name AS name ORDER BY p.name";

        let mut result = self.graph.execute(query(query_str))
            .await.map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to list physics profiles: {}", e)
            ))?;

        let mut profiles = Vec::new();
        while let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
        )? {
            if let Ok(name) = row.get::<String>("name") {
                profiles.push(name);
            }
        }

        Ok(profiles)
    }

    async fn delete_physics_profile(&self, profile_name: &str) -> RepoResult<()> {
        let query_str = "MATCH (p:PhysicsProfile {name: $name}) DELETE p";

        self.graph.run(
            query(query_str).param("name", profile_name)
        ).await.map_err(|e| SettingsRepositoryError::DatabaseError(
            format!("Failed to delete physics profile: {}", e)
        ))?;

        Ok(())
    }

    async fn export_settings(&self) -> RepoResult<serde_json::Value> {
        let query_str =
            "MATCH (s:Setting)
             RETURN s.key AS key, s.value_type AS value_type, s.value AS value, s.description AS description";

        let mut result = self.graph.execute(query(query_str))
            .await.map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Failed to export settings: {}", e)
            ))?;

        let mut settings = serde_json::Map::new();
        while let Some(row) = result.next().await.map_err(|e|
            SettingsRepositoryError::DatabaseError(format!("Failed to fetch row: {}", e))
        )? {
            let key: String = row.get("key").unwrap_or_default();
            let value_type: String = row.get("value_type").unwrap_or_default();
            let value: serde_json::Value = row.get("value").unwrap_or_default();
            let description: String = row.get("description").unwrap_or_default();

            settings.insert(key, serde_json::json!({
                "type": value_type,
                "value": value,
                "description": description
            }));
        }

        Ok(serde_json::Value::Object(settings))
    }

    async fn import_settings(&self, settings_json: &serde_json::Value) -> RepoResult<()> {
        if let Some(settings_map) = settings_json.as_object() {
            let mut updates = HashMap::new();

            for (key, value_obj) in settings_map {
                if let Some(obj) = value_obj.as_object() {
                    let value_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("string");
                    let value = obj.get("value").cloned().unwrap_or(serde_json::Value::Null);

                    if let Some(setting_value) = self.parse_setting_value(value_type, &value) {
                        updates.insert(key.clone(), setting_value);
                    }
                }
            }

            self.set_settings_batch(updates).await?;
        }

        Ok(())
    }

    async fn clear_cache(&self) -> RepoResult<()> {
        self.clear_cache_internal().await
    }

    async fn health_check(&self) -> RepoResult<bool> {
        let query_str = "RETURN 1 AS health";

        self.graph.run(query(query_str))
            .await
            .map_err(|e| SettingsRepositoryError::DatabaseError(
                format!("Health check failed: {}", e)
            ))?;

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Neo4j instance
    async fn test_neo4j_settings_repository() {
        let config = Neo4jSettingsConfig::default();
        let repo = Neo4jSettingsRepository::new(config).await.unwrap();

        // Test set and get
        repo.set_setting("test.key", SettingValue::String("test_value".to_string()), Some("Test setting"))
            .await.unwrap();

        let value = repo.get_setting("test.key").await.unwrap();
        assert_eq!(value, Some(SettingValue::String("test_value".to_string())));

        // Test delete
        repo.delete_setting("test.key").await.unwrap();
        let value = repo.get_setting("test.key").await.unwrap();
        assert_eq!(value, None);

        // Test health check
        assert!(repo.health_check().await.unwrap());
    }

    #[tokio::test]
    #[ignore] // Requires Neo4j instance
    async fn test_user_management() {
        let config = Neo4jSettingsConfig::default();
        let repo = Neo4jSettingsRepository::new(config).await.unwrap();
        let test_pubkey = "test_pubkey_12345";

        // Test get or create user
        let user = repo.get_or_create_user(test_pubkey).await.unwrap();
        assert_eq!(user.pubkey, test_pubkey);
        assert!(!user.is_power_user);

        // Test set power user
        repo.set_power_user(test_pubkey, true).await.unwrap();
        assert!(repo.is_power_user(test_pubkey).await.unwrap());

        // Test update last seen
        repo.update_user_last_seen(test_pubkey).await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Neo4j instance
    async fn test_user_settings() {
        let config = Neo4jSettingsConfig::default();
        let repo = Neo4jSettingsRepository::new(config).await.unwrap();
        let test_pubkey = "test_pubkey_settings";

        // Create user first
        repo.get_or_create_user(test_pubkey).await.unwrap();

        // Test save and get user settings
        let settings = AppFullSettings::default();
        repo.save_user_settings(test_pubkey, &settings).await.unwrap();

        let loaded_settings = repo.get_user_settings(test_pubkey).await.unwrap();
        assert!(loaded_settings.is_some());
    }

    #[tokio::test]
    #[ignore] // Requires Neo4j instance
    async fn test_user_filter() {
        let config = Neo4jSettingsConfig::default();
        let repo = Neo4jSettingsRepository::new(config).await.unwrap();
        let test_pubkey = "test_pubkey_filter";

        // Create user first
        repo.get_or_create_user(test_pubkey).await.unwrap();

        // Test save and get user filter
        let mut filter = UserFilter::default();
        filter.pubkey = test_pubkey.to_string();
        filter.quality_threshold = 0.8;
        filter.max_nodes = Some(5000);

        repo.save_user_filter(test_pubkey, &filter).await.unwrap();

        let loaded_filter = repo.get_user_filter(test_pubkey).await.unwrap();
        assert!(loaded_filter.is_some());
        let loaded = loaded_filter.unwrap();
        assert_eq!(loaded.quality_threshold, 0.8);
        assert_eq!(loaded.max_nodes, Some(5000));
    }
}
