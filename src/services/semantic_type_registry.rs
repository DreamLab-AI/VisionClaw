//! Semantic Type Registry
//!
//! Dynamic registry for ontology relationship types that decouples ontology from code.
//! Eliminates hard-coded edge_type_to_int mappings and enables runtime type registration.
//!
//! ## Schema-Code Decoupling
//!
//! This registry enables adding new relationship types (e.g., ngm:requires, ngm:enables)
//! without requiring CUDA recompilation. The workflow is:
//!
//! 1. Register new relationship type with `registry.register("ngm:new-type", config)`
//! 2. Build GPU buffer with `registry.build_dynamic_gpu_buffer()`
//! 3. Upload to GPU with `set_dynamic_relationship_buffer(buffer.as_ptr(), count, true)`
//! 4. GPU kernel uses lookup table instead of switch statement
//!
//! Hot-reload is supported: call `update_dynamic_relationship_config` to update
//! individual types without full buffer re-upload.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;

/// Force configuration for a relationship type
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RelationshipForceConfig {
    /// Spring strength (0.0 - 1.0, can be negative for repulsion)
    pub strength: f32,
    /// Rest length for spring calculations
    pub rest_length: f32,
    /// Whether the force is directional (source → target only)
    pub is_directional: bool,
    /// Force type identifier for GPU kernel dispatch:
    /// - 0: Standard spring force
    /// - 1: Orbit clustering (has-part)
    /// - 2: Cross-domain long-range spring
    /// - 3: Repulsion force
    pub force_type: u32,
}

/// GPU-compatible dynamic force configuration
/// Matches the DynamicForceConfig struct in semantic_forces.cu
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct DynamicForceConfigGPU {
    /// Spring strength (can be negative for repulsion)
    pub strength: f32,
    /// Rest length for spring calculations
    pub rest_length: f32,
    /// 1 = directional (source → target), 0 = bidirectional
    pub is_directional: i32,
    /// Force behavior type (0=spring, 1=orbit, 2=cross-domain, 3=repulsion)
    pub force_type: u32,
}

impl Default for DynamicForceConfigGPU {
    fn default() -> Self {
        Self {
            strength: 0.5,
            rest_length: 100.0,
            is_directional: 0,
            force_type: 0,
        }
    }
}

impl From<&RelationshipForceConfig> for DynamicForceConfigGPU {
    fn from(config: &RelationshipForceConfig) -> Self {
        Self {
            strength: config.strength,
            rest_length: config.rest_length,
            is_directional: if config.is_directional { 1 } else { 0 },
            force_type: config.force_type,
        }
    }
}

impl Default for RelationshipForceConfig {
    fn default() -> Self {
        Self {
            strength: 0.5,
            rest_length: 100.0,
            is_directional: false,
            force_type: 0,
        }
    }
}

/// Thread-safe registry for semantic relationship types
pub struct SemanticTypeRegistry {
    uri_to_id: RwLock<HashMap<String, u32>>,
    id_to_config: RwLock<Vec<RelationshipForceConfig>>,
    id_to_uri: RwLock<Vec<String>>,
    next_id: AtomicU32,
}

// Lock helper methods that recover from poisoned locks
impl SemanticTypeRegistry {
    fn read_uri_map(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, u32>> {
        self.uri_to_id
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_uri_map(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, u32>> {
        self.uri_to_id
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_configs(&self) -> std::sync::RwLockReadGuard<'_, Vec<RelationshipForceConfig>> {
        self.id_to_config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_configs(&self) -> std::sync::RwLockWriteGuard<'_, Vec<RelationshipForceConfig>> {
        self.id_to_config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_uris(&self) -> std::sync::RwLockReadGuard<'_, Vec<String>> {
        self.id_to_uri
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_uris(&self) -> std::sync::RwLockWriteGuard<'_, Vec<String>> {
        self.id_to_uri
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SemanticTypeRegistry {
    /// Create a new registry with default relationship types
    pub fn new() -> Self {
        let registry = Self {
            uri_to_id: RwLock::new(HashMap::new()),
            id_to_config: RwLock::new(Vec::new()),
            id_to_uri: RwLock::new(Vec::new()),
            next_id: AtomicU32::new(0),
        };

        // Register default relationship types
        // Generic/unknown type
        registry.register_internal(
            "generic",
            RelationshipForceConfig {
                strength: 0.3,
                rest_length: 100.0,
                is_directional: false,
                force_type: 0,
            },
        );

        // Basic relationship types
        registry.register_internal(
            "dependency",
            RelationshipForceConfig {
                strength: 0.6,
                rest_length: 80.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "hierarchy",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 60.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "association",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 120.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "sequence",
            RelationshipForceConfig {
                strength: 0.5,
                rest_length: 90.0,
                is_directional: true,
                force_type: 0,
            },
        );

        // OWL relationship types
        registry.register_internal(
            "subClassOf",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 60.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "rdfs:subClassOf",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 60.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "instanceOf",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 70.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "rdf:type",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 70.0,
                is_directional: true,
                force_type: 0,
            },
        );

        // NGM ontology relationship types
        registry.register_internal(
            "ngm:requires",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 80.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "requires",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 80.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "ngm:enables",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 120.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "enables",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 120.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "ngm:has-part",
            RelationshipForceConfig {
                strength: 0.9,
                rest_length: 40.0,
                is_directional: true,
                force_type: 1, // Orbit clustering
            },
        );

        registry.register_internal(
            "has-part",
            RelationshipForceConfig {
                strength: 0.9,
                rest_length: 40.0,
                is_directional: true,
                force_type: 1,
            },
        );

        registry.register_internal(
            "ngm:bridges-to",
            RelationshipForceConfig {
                strength: 0.3,
                rest_length: 200.0,
                is_directional: false,
                force_type: 2, // Cross-domain long-range
            },
        );

        registry.register_internal(
            "bridges-to",
            RelationshipForceConfig {
                strength: 0.3,
                rest_length: 200.0,
                is_directional: false,
                force_type: 2,
            },
        );

        // Additional common relationship types
        registry.register_internal(
            "owl:equivalentClass",
            RelationshipForceConfig {
                strength: 0.9,
                rest_length: 30.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "owl:disjointWith",
            RelationshipForceConfig {
                strength: -0.3, // Repulsive
                rest_length: 150.0,
                is_directional: false,
                force_type: 3, // Repulsion
            },
        );

        registry.register_internal(
            "skos:broader",
            RelationshipForceConfig {
                strength: 0.6,
                rest_length: 70.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "skos:narrower",
            RelationshipForceConfig {
                strength: 0.6,
                rest_length: 70.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "skos:related",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 100.0,
                is_directional: false,
                force_type: 0,
            },
        );

        // ── Canonical edge-type strings ─────────────────────────────
        // These are the labels that predicate_to_edge_type() emits and
        // that Edge.edge_type carries at runtime. Force configs align
        // with SemanticEdgeType::spring_multiplier() values, normalised
        // to the registry's 0.0–1.0 strength scale.
        //
        // Any IRI not in this list auto-registers via get_or_register_id()
        // with default config — the system is domain-agnostic.

        registry.register_internal(
            "explicit_link",
            RelationshipForceConfig {
                strength: 0.5,
                rest_length: 100.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "hierarchical",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 60.0,
                is_directional: true,
                force_type: 0,
            },
        );

        // Domain root → member. Spring config is the one these edges had
        // under `hierarchical`, so the physics is unchanged; only the label,
        // and with it the rank/fold/colour semantics, differs.
        registry.register_internal(
            "domain_member",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 60.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "structural",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 50.0,
                is_directional: true,
                force_type: 1, // Orbit clustering (has-part)
            },
        );

        registry.register_internal(
            "dependency",
            RelationshipForceConfig {
                strength: 0.7,
                rest_length: 70.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "associative",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 120.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "bridge",
            RelationshipForceConfig {
                strength: 0.3,
                rest_length: 200.0,
                is_directional: false,
                force_type: 2, // Cross-domain long-range
            },
        );

        registry.register_internal(
            "namespace",
            RelationshipForceConfig {
                strength: 0.3,
                rest_length: 180.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "inferred",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 130.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "implements",
            RelationshipForceConfig {
                strength: 0.8,
                rest_length: 55.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "enhancement",
            RelationshipForceConfig {
                strength: 0.55,
                rest_length: 90.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "security",
            RelationshipForceConfig {
                strength: 0.6,
                rest_length: 80.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "goal",
            RelationshipForceConfig {
                strength: 0.5,
                rest_length: 100.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "tracking",
            RelationshipForceConfig {
                strength: 0.35,
                rest_length: 150.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "similarity",
            RelationshipForceConfig {
                strength: 0.4,
                rest_length: 120.0,
                is_directional: false,
                force_type: 0,
            },
        );

        registry.register_internal(
            "provenance",
            RelationshipForceConfig {
                strength: 0.25,
                rest_length: 200.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "utilisation",
            RelationshipForceConfig {
                strength: 0.45,
                rest_length: 95.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "standardisation",
            RelationshipForceConfig {
                strength: 0.55,
                rest_length: 85.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "contrastsWith",
            RelationshipForceConfig {
                strength: 0.35,
                rest_length: 150.0,
                is_directional: false,
                force_type: 2,
            },
        );

        registry.register_internal(
            "standardizedBy",
            RelationshipForceConfig {
                strength: 0.55,
                rest_length: 85.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry.register_internal(
            "enabledBy",
            RelationshipForceConfig {
                strength: 0.6,
                rest_length: 75.0,
                is_directional: true,
                force_type: 0,
            },
        );

        registry
    }

    /// Internal registration (bypasses lock acquisition for initialization)
    fn register_internal(&self, uri: &str, config: RelationshipForceConfig) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let mut uri_map = self.write_uri_map();
        let mut configs = self.write_configs();
        let mut uris = self.write_uris();

        uri_map.insert(uri.to_string(), id);
        configs.push(config);
        uris.push(uri.to_string());

        id
    }

    /// Register a new relationship type with force configuration
    /// Returns the assigned ID for the type
    pub fn register(&self, uri: &str, config: RelationshipForceConfig) -> u32 {
        // Check if already registered
        {
            let uri_map = self.read_uri_map();
            if let Some(&existing_id) = uri_map.get(uri) {
                // Update existing config
                let mut configs = self.write_configs();
                if (existing_id as usize) < configs.len() {
                    configs[existing_id as usize] = config;
                }
                return existing_id;
            }
        }

        self.register_internal(uri, config)
    }

    /// Get the ID for a relationship type URI
    pub fn get_id(&self, uri: &str) -> Option<u32> {
        let uri_map = self.read_uri_map();
        uri_map.get(uri).copied()
    }

    /// Get the ID for a relationship type, registering with defaults if not found
    pub fn get_or_register_id(&self, uri: &str) -> u32 {
        if let Some(id) = self.get_id(uri) {
            return id;
        }

        // Register with default config
        self.register(uri, RelationshipForceConfig::default())
    }

    /// Get the force configuration for a relationship type ID
    pub fn get_config(&self, id: u32) -> Option<RelationshipForceConfig> {
        let configs = self.read_configs();
        configs.get(id as usize).copied()
    }

    /// Get the URI for a relationship type ID
    pub fn get_uri(&self, id: u32) -> Option<String> {
        let uris = self.read_uris();
        uris.get(id as usize).cloned()
    }

    /// Update the configuration for an existing relationship type
    pub fn update_config(&self, uri: &str, config: RelationshipForceConfig) -> bool {
        let uri_map = self.read_uri_map();
        if let Some(&id) = uri_map.get(uri) {
            drop(uri_map); // Release read lock before acquiring write lock
            let mut configs = self.write_configs();
            if (id as usize) < configs.len() {
                configs[id as usize] = config;
                return true;
            }
        }
        false
    }

    /// Build a GPU-compatible buffer of all force configurations
    /// Buffer is indexed by relationship type ID
    pub fn build_gpu_buffer(&self) -> Vec<RelationshipForceConfig> {
        let configs = self.read_configs();
        configs.clone()
    }

    /// Build a GPU buffer with the proper C-compatible struct layout
    /// for the dynamic relationship system in semantic_forces.cu
    pub fn build_dynamic_gpu_buffer(&self) -> Vec<DynamicForceConfigGPU> {
        let configs = self.read_configs();
        configs
            .iter()
            .map(|c| DynamicForceConfigGPU::from(c))
            .collect()
    }

    /// Get the buffer version (incremented on each registration/update)
    /// Useful for hot-reload detection
    pub fn version(&self) -> u32 {
        self.next_id.load(Ordering::SeqCst)
    }

    /// Get the number of registered relationship types
    pub fn len(&self) -> usize {
        let configs = self.read_configs();
        configs.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get all registered URIs
    pub fn registered_uris(&self) -> Vec<String> {
        let uris = self.read_uris();
        uris.clone()
    }

    /// Convert edge type string to integer ID (legacy compatibility)
    /// Returns the ID if the type is registered, or 0 (generic) if not found
    pub fn edge_type_to_int(&self, edge_type: &Option<String>) -> i32 {
        edge_type
            .as_deref()
            .and_then(|uri| self.get_id(uri))
            .map(|id| id as i32)
            .unwrap_or(0)
    }
}

impl Default for SemanticTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Global singleton registry instance
lazy_static::lazy_static! {
    pub static ref SEMANTIC_TYPE_REGISTRY: SemanticTypeRegistry = SemanticTypeRegistry::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = SemanticTypeRegistry::new();
        // Should have default types registered
        assert!(registry.len() > 0);
    }

    #[test]
    fn test_default_types_registered() {
        let registry = SemanticTypeRegistry::new();

        // Legacy prefixed types
        assert!(registry.get_id("generic").is_some());
        assert!(registry.get_id("ngm:requires").is_some());
        assert!(registry.get_id("ngm:enables").is_some());
        assert!(registry.get_id("ngm:has-part").is_some());
        assert!(registry.get_id("ngm:bridges-to").is_some());
        assert!(registry.get_id("rdfs:subClassOf").is_some());

        // Canonical edge-type labels (from predicate_to_edge_type)
        for label in &[
            "explicit_link",
            "hierarchical",
            "structural",
            "dependency",
            "associative",
            "bridge",
            "namespace",
            "inferred",
            "implements",
            "enhancement",
            "security",
            "goal",
            "tracking",
            "similarity",
            "provenance",
        ] {
            assert!(
                registry.get_id(label).is_some(),
                "missing canonical type: {}",
                label
            );
        }
    }

    #[test]
    fn test_register_new_type() {
        let registry = SemanticTypeRegistry::new();
        let initial_len = registry.len();

        let id = registry.register(
            "custom:test-type",
            RelationshipForceConfig {
                strength: 0.5,
                rest_length: 100.0,
                is_directional: true,
                force_type: 0,
            },
        );

        assert_eq!(registry.len(), initial_len + 1);
        assert_eq!(registry.get_id("custom:test-type"), Some(id));
    }

    #[test]
    fn test_get_config() {
        let registry = SemanticTypeRegistry::new();

        let id = registry.get_id("ngm:requires").unwrap();
        let config = registry.get_config(id).unwrap();

        assert_eq!(config.strength, 0.7);
        assert!(config.is_directional);
    }

    #[test]
    fn test_update_config() {
        let registry = SemanticTypeRegistry::new();

        let updated = registry.update_config(
            "ngm:requires",
            RelationshipForceConfig {
                strength: 0.9,
                rest_length: 50.0,
                is_directional: true,
                force_type: 0,
            },
        );

        assert!(updated);

        let id = registry.get_id("ngm:requires").unwrap();
        let config = registry.get_config(id).unwrap();
        assert_eq!(config.strength, 0.9);
        assert_eq!(config.rest_length, 50.0);
    }

    #[test]
    fn test_gpu_buffer() {
        let registry = SemanticTypeRegistry::new();
        let buffer = registry.build_gpu_buffer();

        assert_eq!(buffer.len(), registry.len());
    }

    #[test]
    fn test_edge_type_to_int() {
        let registry = SemanticTypeRegistry::new();

        // Registered type
        let id = registry.edge_type_to_int(&Some("ngm:requires".to_string()));
        assert!(id > 0);

        // Unregistered type returns 0 (generic)
        let unknown_id = registry.edge_type_to_int(&Some("unknown:type".to_string()));
        assert_eq!(unknown_id, 0);

        // None returns 0
        let none_id = registry.edge_type_to_int(&None);
        assert_eq!(none_id, 0);
    }

    #[test]
    fn test_get_or_register_id() {
        let registry = SemanticTypeRegistry::new();

        // Existing type
        let id1 = registry.get_or_register_id("ngm:requires");
        let id2 = registry.get_or_register_id("ngm:requires");
        assert_eq!(id1, id2);

        // New type gets registered
        let new_id = registry.get_or_register_id("new:auto-registered");
        assert!(registry.get_id("new:auto-registered").is_some());
        assert_eq!(registry.get_id("new:auto-registered"), Some(new_id));
    }
}
