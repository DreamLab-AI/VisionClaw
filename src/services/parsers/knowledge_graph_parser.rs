// src/services/parsers/knowledge_graph_parser.rs
//! Knowledge Graph Parser
//!
//! Parses markdown files marked with `public:: true` to extract:
//! - Nodes (pages, concepts)
//! - Edges (links, relationships)
//! - Metadata (properties, tags)

use crate::models::edge::Edge;
use crate::models::graph::GraphData;
use crate::models::metadata::{MaturityLevel, MetadataStore, PhysicalityCode, RoleCode};
use crate::models::node::Node;
use crate::utils::socket_flow_messages::BinaryNodeData;
use log::{debug, info};
use std::collections::HashMap;

/// Knowledge graph parser with position preservation support
pub struct KnowledgeGraphParser {
    /// Existing positions from database (node_id -> (x, y, z))
    existing_positions: Option<HashMap<u32, (f32, f32, f32)>>,
}

impl KnowledgeGraphParser {
    pub fn new() -> Self {
        Self {
            existing_positions: None,
        }
    }

    /// Create parser with existing positions from database
    /// These positions will be used instead of generating random ones
    pub fn with_positions(existing_positions: HashMap<u32, (f32, f32, f32)>) -> Self {
        Self {
            existing_positions: Some(existing_positions),
        }
    }

    /// Set existing positions for position preservation
    pub fn set_positions(&mut self, positions: HashMap<u32, (f32, f32, f32)>) {
        self.existing_positions = Some(positions);
    }

    /// Get position for a node ID, using existing position or generating random
    fn get_position(&self, node_id: u32) -> (f32, f32, f32) {
        if let Some(ref positions) = self.existing_positions {
            if let Some(&(x, y, z)) = positions.get(&node_id) {
                return (x, y, z);
            }
        }
        // Generate random position only if no existing position found
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (
            rng.gen_range(-100.0..100.0),
            rng.gen_range(-100.0..100.0),
            rng.gen_range(-100.0..100.0),
        )
    }

    
    pub fn parse(&self, content: &str, filename: &str) -> Result<GraphData, String> {
        info!("Parsing knowledge graph file: {}", filename);

        
        let page_name = filename.strip_suffix(".md").unwrap_or(filename).to_string();

        
        let mut nodes = vec![self.create_page_node(&page_name, content)];
        let mut id_to_metadata = HashMap::new();
        id_to_metadata.insert(nodes[0].id.to_string(), page_name.clone());

        
        
        
        // Wikilink edges-only: create Edge objects for [[WikiLinks]] without
        // inflating the node count. Only edges are emitted; target nodes are NOT
        // created here. Edges whose target doesn't exist as a page node will
        // still be stored — the Neo4j MERGE will create stubs or the edge will
        // dangle harmlessly until the target page is synced.
        let wikilink_edges = self.extract_wikilink_edges(content, &nodes[0].id);

        let metadata = self.extract_metadata_store(content, &page_name);

        debug!(
            "Parsed {}: {} nodes, {} wikilink edges",
            filename,
            nodes.len(),
            wikilink_edges.len(),
        );

        Ok(GraphData {
            nodes,
            edges: wikilink_edges,
            metadata,
            id_to_metadata,
        })
    }

    /// Create a page node, preserving existing position if available
    fn create_page_node(&self, page_name: &str, content: &str) -> Node {
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), "page".to_string());
        metadata.insert("source_file".to_string(), format!("{}.md", page_name));
        metadata.insert("public".to_string(), "true".to_string());

        // Extract Logseq-style `key:: value` properties from the page and fold
        // them into node metadata so they reach Neo4j. Previously these were
        // parsed into a local HashMap in extract_metadata_store() but discarded
        // before returning, leaving source_domain/term-id/owl:class NULL on
        // every GraphNode in the graph.
        //
        // Lowercase `source-domain` / `domain` values so the downstream domain
        // filter (`graph_types.rs::classify_node_population` and
        // `neo4j_adapter.rs::domain_to_color`) matches regardless of authoring
        // case.
        let prop_re = regex::Regex::new(r"(?m)^\s*-?\s*([a-zA-Z][a-zA-Z0-9_:\-]*)::\s*(.+)$")
            .expect("invalid property regex");
        let mut owl_class_iri: Option<String> = None;
        for cap in prop_re.captures_iter(content) {
            let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) else { continue };
            let key = k.as_str();
            let mut value = v.as_str().trim().to_string();
            if matches!(key, "source-domain" | "domain" | "source_domain") {
                value = value.to_lowercase();
            }
            if key == "owl:class" {
                owl_class_iri = Some(value.clone());
            }
            if matches!(key, "type" | "source_file" | "public") {
                // Don't overwrite fields we set explicitly above.
                continue;
            }
            metadata.entry(key.to_string()).or_insert(value);
        }

        let tags = self.extract_tags(content);
        if !tags.is_empty() {
            metadata.insert("tags".to_string(), tags.join(", "));
        }

        // Derive integer OWL codes for the CUDA semantic-forces kernel.
        // We read from the metadata HashMap we just populated so the property
        // regex only runs once.  The `maturity` key is checked first, then
        // `status` as a fallback (some older pages use `status::` instead).
        let physicality = PhysicalityCode::from_logseq(
            metadata.get("owl:physicality").map(|s| s.as_str()).unwrap_or(""),
        );
        let role = RoleCode::from_logseq(
            metadata.get("owl:role").map(|s| s.as_str()).unwrap_or(""),
        );
        let maturity = MaturityLevel::from_logseq(
            metadata
                .get("maturity")
                .or_else(|| metadata.get("status"))
                .map(|s| s.as_str())
                .unwrap_or(""),
        );

        // Persist the integer codes into the node metadata HashMap so they
        // propagate to Neo4j node properties and are available to the kernel.
        metadata.insert("physicality_code".into(), physicality.as_i32().to_string());
        metadata.insert("role_code".into(), role.as_i32().to_string());
        metadata.insert("maturity_level".into(), maturity.as_i32().to_string());

        let id = self.page_name_to_id(page_name);

        // Use existing position or generate random (position preservation)
        let (x, y, z) = self.get_position(id);
        let data = BinaryNodeData {
            node_id: id,
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
        };

        // Display label: prefer preferred-term over the raw filename slug.
        // Files named "AI-0424-confidential-computing.md" carry
        // `preferred-term:: Confidential Computing` — use the human name
        // for display, keep the slug as metadata_id for stable identity.
        let display_label = metadata
            .get("preferred-term")
            .cloned()
            .unwrap_or_else(|| page_name.to_string());

        Node {
            id,
            metadata_id: page_name.to_string(),
            label: display_label,
            data,
            metadata,
            file_size: 0,
            node_type: Some("page".to_string()),
            color: Some("#4A90E2".to_string()),
            size: Some(1.0),
            weight: Some(1.0),
            group: None,
            user_data: None,
            mass: Some(1.0),
            x: Some(data.x),
            y: Some(data.y),
            z: Some(data.z),
            vx: Some(0.0),
            vy: Some(0.0),
            vz: Some(0.0),
            // If the page carries `owl:class:: bc:SomeClass` (or similar) that
            // declaration is now promoted to the node's first-class owl_class_iri
            // field so ontology enrichment and GPU semantic-force IRI lookup can
            // actually find the node.
            owl_class_iri,
        }
    }

    /// Extract wikilink edges only — no new nodes created.
    /// Returns Edge objects for each [[WikiLink]] found in content.
    /// Deduplicates by target to avoid multiple edges to the same page.
    fn extract_wikilink_edges(&self, content: &str, source_id: &u32) -> Vec<Edge> {
        let mut edges = Vec::new();
        let mut seen_targets = std::collections::HashSet::new();

        let link_pattern = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")
            .expect("Invalid regex pattern");

        for cap in link_pattern.captures_iter(content) {
            if let Some(link_match) = cap.get(1) {
                let target_page = link_match.as_str().trim().to_string();
                let target_id = self.page_name_to_id(&target_page);

                // Skip self-loops and duplicates
                if target_id == *source_id || !seen_targets.insert(target_id) {
                    continue;
                }

                edges.push(Edge {
                    id: format!("{}_{}", source_id, target_id),
                    source: *source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("explicit_link".to_string()),
                    metadata: None,
                    owl_property_iri: None,
                });
            }
        }

        edges
    }

    /// Extract links from content, preserving existing positions (legacy — creates nodes)
    #[allow(dead_code)]
    fn extract_links(&self, content: &str, source_id: &u32) -> (Vec<Node>, Vec<Edge>) {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        let link_pattern = regex::Regex::new(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]").expect("Invalid regex pattern");

        for cap in link_pattern.captures_iter(content) {
            if let Some(link_match) = cap.get(1) {
                let target_page = link_match.as_str().trim().to_string();
                let target_id = self.page_name_to_id(&target_page);

                let mut metadata = HashMap::new();
                metadata.insert("type".to_string(), "linked_page".to_string());

                // Use existing position or generate random (position preservation)
                let (x, y, z) = self.get_position(target_id);
                let data = BinaryNodeData {
                    node_id: target_id,
                    x,
                    y,
                    z,
                    vx: 0.0,
                    vy: 0.0,
                    vz: 0.0,
                };

                nodes.push(Node {
                    id: target_id,
                    metadata_id: target_page.clone(),
                    label: target_page.clone(),
                    data,
                    metadata,
                    file_size: 0,
                    node_type: Some("linked_page".to_string()),
                    color: Some("#7C3AED".to_string()),
                    size: Some(0.8),
                    weight: Some(0.8),
                    group: None,
                    user_data: None,
                    mass: Some(1.0),
                    x: Some(data.x),
                    y: Some(data.y),
                    z: Some(data.z),
                    vx: Some(0.0),
                    vy: Some(0.0),
                    vz: Some(0.0),
                    owl_class_iri: None,
                });

                edges.push(Edge {
                    id: format!("{}_{}", source_id, target_id),
                    source: *source_id,
                    target: target_id,
                    weight: 1.0,
                    edge_type: Some("link".to_string()),
                    metadata: Some(HashMap::new()),
                    owl_property_iri: None,
                });
            }
        }

        (nodes, edges)
    }

    /// Extract a MetadataStore of parsed Logseq properties keyed by page name.
    ///
    /// Historically this function parsed every `key:: value` pair in the content
    /// into a local HashMap and then returned an empty MetadataStore — the
    /// properties HashMap was never used. All ontology metadata (term-id,
    /// source-domain, owl:class, preferred-term, etc.) was silently discarded
    /// here before Neo4j ever saw it.
    ///
    /// Now we populate a Metadata record with the fields the downstream schema
    /// expects, so FileMetadata rows actually carry source attribution.
    fn extract_metadata_store(&self, content: &str, page_name: &str) -> MetadataStore {
        let mut store = MetadataStore::new();

        let prop_re = regex::Regex::new(r"(?m)^\s*-?\s*([a-zA-Z][a-zA-Z0-9_:\-]*)::\s*(.+)$")
            .expect("invalid property regex");

        let mut m = crate::models::metadata::Metadata::default();
        m.file_name = format!("{}.md", page_name);
        m.node_id = self.page_name_to_id(page_name).to_string();

        for cap in prop_re.captures_iter(content) {
            let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) else { continue };
            let key = k.as_str();
            let value = v.as_str().trim().to_string();
            match key {
                "term-id" => m.term_id = Some(value),
                "preferred-term" => m.preferred_term = Some(value),
                "source-domain" | "domain" | "source_domain" =>
                    m.source_domain = Some(value.to_lowercase()),
                "status" | "ontology-status" => m.ontology_status = Some(value),
                "owl:class" => m.owl_class = Some(value),
                "owl:physicality" => m.owl_physicality = Some(value),
                "owl:role" => m.owl_role = Some(value),
                "quality-score" => m.quality_score = value.parse().ok(),
                "authority-score" => m.authority_score = value.parse().ok(),
                "maturity" => m.maturity = Some(value),
                "definition" => m.definition = Some(value),
                "belongsToDomain" | "belongs-to-domain" =>
                    m.belongs_to_domain.push(value),
                "is-subclass-of" => m.is_subclass_of.push(value),
                _ => { /* preserved via node.metadata HashMap in create_page_node */ }
            }
        }

        store.insert(page_name.to_string(), m);
        store
    }

    
    fn extract_tags(&self, content: &str) -> Vec<String> {
        let mut tags = Vec::new();

        
        let tag_pattern =
            regex::Regex::new(r"#([a-zA-Z0-9_-]+)|tag::\s*#?([a-zA-Z0-9_-]+)").expect("Invalid regex pattern");

        for cap in tag_pattern.captures_iter(content) {
            if let Some(tag) = cap.get(1).or_else(|| cap.get(2)) {
                tags.push(tag.as_str().to_string());
            }
        }

        tags.dedup();
        tags
    }

    
    pub fn page_name_to_id(&self, page_name: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        page_name.hash(&mut hasher);
        let hash_val = hasher.finish();
        
        // Use full u32 range to minimize collision probability (birthday paradox)
        // Reserve 0 as sentinel; map to [1, u32::MAX]
        let id = (hash_val & 0xFFFF_FFFE) as u32 + 1;
        id
    }
}

impl Default for KnowledgeGraphParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_preservation() {
        let mut positions = HashMap::new();
        positions.insert(12345u32, (10.0f32, 20.0f32, 30.0f32));

        let parser = KnowledgeGraphParser::with_positions(positions);
        let pos = parser.get_position(12345);

        assert_eq!(pos, (10.0, 20.0, 30.0));
    }

    #[test]
    fn test_fallback_to_random() {
        let parser = KnowledgeGraphParser::new();
        let pos = parser.get_position(99999);

        // Should be within random range
        assert!(pos.0 >= -100.0 && pos.0 <= 100.0);
        assert!(pos.1 >= -100.0 && pos.1 <= 100.0);
        assert!(pos.2 >= -100.0 && pos.2 <= 100.0);
    }
}
