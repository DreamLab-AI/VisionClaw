use crate::types::vec3::BinaryNodeData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

// Static counter for generating unique numeric IDs
static NEXT_NODE_ID: AtomicU32 = AtomicU32::new(1);

/// Default initial placement radius minimum.
/// Matches the main crate's `dev_config::physics().initial_radius_min`.
const DEFAULT_INITIAL_RADIUS_MIN: f32 = 100.0;

/// Default initial placement radius range.
/// Matches the main crate's `dev_config::physics().initial_radius_range`.
const DEFAULT_INITIAL_RADIUS_RANGE: f32 = 300.0;

/// The three graph populations along the dual-graph X-axis.
///
/// A node's population is its ORIGIN (which graph it belongs to), NOT its
/// elevated/enriched class. There is exactly ONE authoritative origin field:
/// `metadata["type"]`. See [`Node::population`] for the single source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Population {
    /// Working/wiki graph: authored `page` and wikilink-stub `linked_page` nodes.
    Knowledge,
    /// Formal OWL graph: `owl_class`, `ontology_node`, `owl_individual`, `owl_property`.
    Ontology,
    /// Agent/bot swarm nodes.
    Agent,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: u32,
    pub metadata_id: String,
    pub label: String,
    pub data: BinaryNodeData,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vx: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vz: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub owl_class_iri: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    #[serde(skip)]
    pub file_size: u64,

    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<HashMap<String, String>>,
}

impl Node {
    pub fn new(metadata_id: String) -> Self {
        Self::new_with_id(metadata_id, None)
    }

    pub fn new_with_id(metadata_id: String, provided_id: Option<u32>) -> Self {
        let id = match provided_id {
            Some(id) if id != 0 => id,
            _ => NEXT_NODE_ID.fetch_add(1, Ordering::SeqCst),
        };

        // Generate random initial position on a sphere shell
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let theta = rng.gen::<f32>() * 2.0 * std::f32::consts::PI; // azimuthal [0, 2pi)
        let phi = (rng.gen::<f32>() * 2.0 - 1.0).acos(); // polar: acos(uniform(-1,1)) for uniform sphere surface

        // cbrt gives uniform distribution within the volume (not just on the surface)
        let radius =
            DEFAULT_INITIAL_RADIUS_MIN + rng.gen::<f32>().cbrt() * DEFAULT_INITIAL_RADIUS_RANGE;

        let pos_x = radius * phi.sin() * theta.cos();
        let pos_y = radius * phi.sin() * theta.sin();
        let pos_z = radius * phi.cos();

        Self {
            id,
            metadata_id,
            label: String::new(),
            data: BinaryNodeData {
                node_id: id,
                x: pos_x,
                y: pos_y,
                z: pos_z,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
            },
            x: Some(pos_x),
            y: Some(pos_y),
            z: Some(pos_z),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            mass: Some(1.0),
            owl_class_iri: None,
            metadata: HashMap::new(),
            file_size: 0,
            node_type: None,
            size: None,
            color: None,
            weight: None,
            group: None,
            user_data: None,
        }
    }

    pub fn set_file_size(&mut self, size: u64) {
        self.file_size = size;
        if size > 0 {
            self.metadata
                .insert("fileSize".to_string(), size.to_string());
        }
    }

    pub fn with_position(mut self, x: f32, y: f32, z: f32) -> Self {
        self.data.x = x;
        self.data.y = y;
        self.data.z = z;
        self.x = Some(x);
        self.y = Some(y);
        self.z = Some(z);
        self
    }

    pub fn with_velocity(mut self, vx: f32, vy: f32, vz: f32) -> Self {
        self.data.vx = vx;
        self.data.vy = vy;
        self.data.vz = vz;
        self.vx = Some(vx);
        self.vy = Some(vy);
        self.vz = Some(vz);
        self
    }

    pub fn with_mass(mut self, mass: f32) -> Self {
        self.mass = Some(mass);
        self
    }

    pub fn with_owl_class_iri(mut self, iri: String) -> Self {
        self.owl_class_iri = Some(iri);
        self
    }

    pub fn with_label(mut self, label: String) -> Self {
        self.label = label;
        self
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn with_type(mut self, node_type: String) -> Self {
        self.node_type = Some(node_type);
        self
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_color(mut self, color: String) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn with_group(mut self, group: String) -> Self {
        self.group = Some(group);
        self
    }

    /// Create a node with a deterministic position derived from a stored ID.
    /// Uses golden-ratio spiral placement instead of random sphere distribution.
    pub fn new_with_stored_id(metadata_id: String, stored_node_id: Option<u32>) -> Self {
        let id = match stored_node_id {
            Some(stored_id) => stored_id,
            None => NEXT_NODE_ID.fetch_add(1, Ordering::SeqCst),
        };

        let id_hash = id as f32;
        let angle = id_hash * 0.618_034;
        let radius = (id_hash * 0.1).min(100.0);

        let pos_x = radius * angle.cos() * 2.0;
        let pos_y = radius * angle.sin() * 2.0;
        let pos_z = (id_hash * 0.01 - 50.0).clamp(-100.0, 100.0);

        Self {
            id,
            metadata_id: metadata_id.clone(),
            label: metadata_id,
            data: BinaryNodeData {
                node_id: id,
                x: pos_x,
                y: pos_y,
                z: pos_z,
                vx: 0.0,
                vy: 0.0,
                vz: 0.0,
            },
            x: Some(pos_x),
            y: Some(pos_y),
            z: Some(pos_z),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            mass: Some(1.0),
            owl_class_iri: None,
            metadata: HashMap::new(),
            file_size: 0,
            node_type: None,
            size: None,
            color: None,
            weight: None,
            group: None,
            user_data: None,
        }
    }

    /// Resolve the AUTHORITATIVE origin-type string for population classification.
    ///
    /// `metadata["type"]` is the single source of truth for a node's origin
    /// (which graph it belongs to). `node_type` is non-classifying scaffold for
    /// the future, not-yet-built elevation process (forum → decision brokers →
    /// agent panels) that would migrate a page from the knowledge graph into the
    /// ontology graph by rewriting `metadata["type"]`. Until elevation exists and
    /// rewrites `metadata["type"]`, `node_type` MUST NOT change a node's origin.
    ///
    /// `node_type` is therefore consulted ONLY as a legacy fallback when
    /// `metadata["type"]` is absent (e.g. nodes built before metadata was
    /// populated, or test fixtures that set only `node_type`). When both are
    /// present, `metadata["type"]` wins unconditionally.
    pub fn population_type(&self) -> Option<&str> {
        self.metadata
            .get("type")
            .map(|s| s.as_str())
            .or(self.node_type.as_deref())
    }

    /// Classify this node into its graph [`Population`] — the SINGLE source of
    /// truth shared by every reader (GPU disc projection, server-side filter,
    /// client visual mode and filter, colour). Reads [`Node::population_type`]
    /// and applies the canonical match arms, falling back to `owl_class_iri` as
    /// the sole secondary ontology signal when the type string is unknown.
    pub fn population(&self) -> Population {
        match self.population_type() {
            Some("agent") | Some("bot") => Population::Agent,
            Some("owl_class")
            | Some("ontology_node")
            | Some("owl_individual")
            | Some("owl_property") => Population::Ontology,
            Some("page") | Some("linked_page") => Population::Knowledge,
            _ => {
                if self.owl_class_iri.is_some() {
                    Population::Ontology
                } else {
                    Population::Knowledge
                }
            }
        }
    }

    pub fn calculate_mass(file_size: u64) -> u8 {
        let base_mass = ((file_size + 1) as f32).log10() / 4.0;
        let mass = base_mass.clamp(0.1, 10.0);
        (mass * 255.0 / 10.0) as u8
    }

    // Position/velocity accessors via BinaryNodeData
    pub fn x(&self) -> f32 {
        self.data.x
    }
    pub fn y(&self) -> f32 {
        self.data.y
    }
    pub fn z(&self) -> f32 {
        self.data.z
    }
    pub fn vx(&self) -> f32 {
        self.data.vx
    }
    pub fn vy(&self) -> f32 {
        self.data.vy
    }
    pub fn vz(&self) -> f32 {
        self.data.vz
    }

    pub fn set_x(&mut self, val: f32) {
        self.data.x = val;
        self.x = Some(val);
    }
    pub fn set_y(&mut self, val: f32) {
        self.data.y = val;
        self.y = Some(val);
    }
    pub fn set_z(&mut self, val: f32) {
        self.data.z = val;
        self.z = Some(val);
    }
    pub fn set_vx(&mut self, val: f32) {
        self.data.vx = val;
        self.vx = Some(val);
    }
    pub fn set_vy(&mut self, val: f32) {
        self.data.vy = val;
        self.vy = Some(val);
    }
    pub fn set_vz(&mut self, val: f32) {
        self.data.vz = val;
        self.vz = Some(val);
    }

    pub fn set_mass(&mut self, val: f32) {
        self.mass = Some(val);
    }

    pub fn get_mass(&self) -> f32 {
        self.mass.unwrap_or(1.0)
    }

    pub fn id_as_string(&self) -> String {
        self.id.to_string()
    }

    pub fn from_string_id(
        id_str: &str,
        metadata_id: String,
    ) -> Result<Self, std::num::ParseIntError> {
        let id: u32 = id_str.parse()?;
        Ok(Self::new_with_stored_id(metadata_id, Some(id)))
    }
}

impl Default for Node {
    fn default() -> Self {
        Self::new("default".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_id_generation() {
        // Note: the global NEXT_NODE_ID counter is shared across all tests
        // running in parallel, so we can only assert relative ordering, not
        // exact counter values.
        let node1 = Node::new("test-file-1.md".to_string());
        let node2 = Node::new("test-file-2.md".to_string());

        assert_ne!(node1.id, node2.id);
        assert_eq!(node1.metadata_id, "test-file-1.md");
        assert_eq!(node2.metadata_id, "test-file-2.md");
        // node2 should have a higher ID than node1
        assert!(node2.id > node1.id);
    }

    #[test]
    fn test_node_creation() {
        let node = Node::new("test".to_string())
            .with_label("Test Node".to_string())
            .with_position(1.0, 2.0, 3.0)
            .with_velocity(0.1, 0.2, 0.3)
            .with_type("test_type".to_string())
            .with_size(1.5)
            .with_color("#FF0000".to_string())
            .with_weight(2.0)
            .with_group("group1".to_string());

        assert!(node.id > 0, "ID should be positive, got: {}", node.id);
        assert_eq!(node.metadata_id, "test");
        assert_eq!(node.label, "Test Node");
        assert_eq!(node.data.x, 1.0);
        assert_eq!(node.data.y, 2.0);
        assert_eq!(node.data.z, 3.0);
        assert_eq!(node.data.vx, 0.1);
        assert_eq!(node.data.vy, 0.2);
        assert_eq!(node.data.vz, 0.3);
        assert_eq!(node.node_type, Some("test_type".to_string()));
        assert_eq!(node.size, Some(1.5));
        assert_eq!(node.color, Some("#FF0000".to_string()));
        assert_eq!(node.weight, Some(2.0));
        assert_eq!(node.group, Some("group1".to_string()));
    }

    #[test]
    fn test_position_velocity_getters_setters() {
        let mut node = Node::new("test".to_string());

        node.set_x(1.0);
        node.set_y(2.0);
        node.set_z(3.0);
        node.set_vx(0.1);
        node.set_vy(0.2);
        node.set_vz(0.3);

        assert_eq!(node.x(), 1.0);
        assert_eq!(node.y(), 2.0);
        assert_eq!(node.z(), 3.0);
        assert_eq!(node.vx(), 0.1);
        assert_eq!(node.vy(), 0.2);
        assert_eq!(node.vz(), 0.3);
    }

    #[test]
    fn test_calculate_mass_boundary_cases() {
        // Zero bytes: (0+1) as f32 = 1.0, log10(1.0)/4 = 0; clamps to 0.1
        let mass_zero = Node::calculate_mass(0);
        assert!(
            mass_zero < 50,
            "zero-byte mass should be small: {}",
            mass_zero
        );

        // Large but safe: u64::MAX would overflow (file_size + 1), use a large safe value
        let mass_large = Node::calculate_mass(1_000_000_000_000u64); // 1 TB
        assert!(mass_large > 0);
        // 1 TB → log10(1e12)/4 = 3.0 → mass 3.0 → 76.5 → 76: bounded well below
        // the u8 saturation ceiling. (`<= 255` would be a u8 tautology.)
        assert!(mass_large < 255);

        // 1 MiB → log10(1048577)/4 ≈ 1.5 → mass ≈ 0.375 → clamps min 0.1 → * 25.5 ≈ 9
        let mass_1mib = Node::calculate_mass(1024 * 1024);
        assert!(mass_1mib > 0 && mass_1mib < 255);

        // 1 byte: log10(2)/4 ≈ 0.075 → clamped to 0.1 → same small result
        let mass_1byte = Node::calculate_mass(1);
        assert!(mass_1byte < 50);
    }

    #[test]
    fn test_set_file_size_inserts_metadata_for_nonzero() {
        let mut node = Node::new("test".to_string());
        node.set_file_size(2048);
        assert_eq!(node.file_size, 2048);
        assert_eq!(
            node.metadata.get("fileSize").map(String::as_str),
            Some("2048")
        );
    }

    #[test]
    fn test_set_file_size_zero_does_not_insert_metadata() {
        let mut node = Node::new("test".to_string());
        node.set_file_size(0);
        assert_eq!(node.file_size, 0);
        assert!(!node.metadata.contains_key("fileSize"));
    }

    #[test]
    fn test_get_mass_default_is_one() {
        let mut node = Node::new("test".to_string());
        assert!((node.get_mass() - 1.0).abs() < f32::EPSILON);
        node.set_mass(2.5);
        assert!((node.get_mass() - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_new_with_stored_id_uses_provided_id() {
        let node = Node::new_with_stored_id("doc.md".to_string(), Some(99));
        assert_eq!(node.id, 99);
        assert_eq!(node.label, "doc.md"); // label mirrors metadata_id in new_with_stored_id
    }

    #[test]
    fn test_new_with_stored_id_position_in_valid_range() {
        let node = Node::new_with_stored_id("doc.md".to_string(), Some(50));
        // position is deterministic from golden-ratio spiral — just confirm finite values
        assert!(node.x().is_finite());
        assert!(node.y().is_finite());
        assert!(node.z().is_finite());
    }

    #[test]
    fn test_id_as_string() {
        let node = Node::new_with_id("m".to_string(), Some(42));
        assert_eq!(node.id_as_string(), "42");
    }

    #[test]
    fn test_from_string_id_valid() {
        let node = Node::from_string_id("7", "meta.md".to_string()).unwrap();
        assert_eq!(node.id, 7);
    }

    #[test]
    fn test_from_string_id_invalid_returns_err() {
        assert!(Node::from_string_id("not_a_number", "meta.md".to_string()).is_err());
    }

    #[test]
    fn test_new_with_id_zero_gets_counter_id() {
        let node = Node::new_with_id("meta".to_string(), Some(0));
        assert!(node.id > 0, "id 0 should be replaced by counter");
    }

    #[test]
    fn test_with_owl_class_iri() {
        let node = Node::new("meta".to_string())
            .with_owl_class_iri("http://example.org/Class".to_string());
        assert_eq!(
            node.owl_class_iri.as_deref(),
            Some("http://example.org/Class")
        );
    }

    #[test]
    fn test_with_metadata_inserts_key_value() {
        let node =
            Node::new("meta".to_string()).with_metadata("foo".to_string(), "bar".to_string());
        assert_eq!(node.metadata.get("foo").map(String::as_str), Some("bar"));
    }

    #[test]
    fn test_node_serde_omits_none_optional_fields() {
        let node = Node::new_with_stored_id("meta".to_string(), Some(1));
        let json = serde_json::to_string(&node).unwrap();
        // owl_class_iri is None — must not appear
        assert!(
            !json.contains("owlClassIri"),
            "none fields should be omitted: {}",
            json
        );
    }

    #[test]
    fn test_node_serde_roundtrip_preserves_position() {
        let node = Node::new("meta".to_string()).with_position(10.0, 20.0, 30.0);
        let json = serde_json::to_string(&node).unwrap();
        let back: Node = serde_json::from_str(&json).unwrap();
        assert!((back.x() - 10.0).abs() < 0.001);
        assert!((back.y() - 20.0).abs() < 0.001);
        assert!((back.z() - 30.0).abs() < 0.001);
    }
}
