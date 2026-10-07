// src/ontology/services/owl_validator.rs

//! Core service for OWL/RDF validation, reasoning, and graph mapping.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

// Re-export types from services module
pub use crate::services::owl_validator::{
    GraphEdge, GraphNode, PropertyGraph, RdfTriple, Severity, ValidationConfig, ValidationError,
    ValidationReport, Violation,
};

#[derive(Debug, Clone, Deserialize)]
pub struct MappingConfig {
    pub metadata: MappingMetadata,
    pub global: GlobalConfig,
    pub defaults: DefaultsConfig,
    pub namespaces: HashMap<String, String>,
    pub class_mappings: HashMap<String, ClassMapping>,
    pub object_property_mappings: HashMap<String, ObjectPropertyMapping>,
    pub data_property_mappings: HashMap<String, DataPropertyMapping>,
    pub iri_templates: IriTemplates,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MappingMetadata {
    pub title: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub created: String,
    pub last_modified: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlobalConfig {
    pub base_iri: String,
    pub default_vocabulary: String,
    pub version_iri: String,
    pub default_language: String,
    pub strict_mode: bool,
    pub auto_generate_inverses: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DefaultsConfig {
    pub default_node_class: String,
    pub default_edge_property: String,
    pub default_datatype: String,
    pub fallback_namespace: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClassMapping {
    pub owl_class: String,
    pub rdfs_label: String,
    pub rdfs_comment: String,
    #[serde(default)]
    pub rdfs_subclass_of: Vec<String>,
    #[serde(default)]
    pub equivalent_classes: Vec<String>,
    #[serde(default)]
    pub disjoint_with: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObjectPropertyMapping {
    pub owl_property: String,
    pub rdfs_label: String,
    pub rdfs_comment: String,
    pub rdfs_domain: PropertyDomain,
    pub rdfs_range: PropertyRange,
    #[serde(default)]
    pub owl_inverse_of: Option<String>,
    pub property_type: String,
    #[serde(default)]
    pub characteristics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataPropertyMapping {
    pub owl_property: String,
    pub rdfs_label: String,
    pub rdfs_comment: String,
    pub rdfs_domain: PropertyDomain,
    pub rdfs_range: String,
    pub property_type: String,
    #[serde(default)]
    pub characteristics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PropertyDomain {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PropertyRange {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct IriTemplates {
    pub nodes: HashMap<String, String>,
    pub edges: HashMap<String, String>,
    pub metadata: MetadataTemplates,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetadataTemplates {
    pub property: String,
    pub class: String,
}

pub struct OwlValidatorService {
    mapping_config: Arc<MappingConfig>,
}

impl OwlValidatorService {
    pub fn new() -> Result<Self> {
        let mapping_toml = std::fs::read_to_string("ontology/mapping.toml")
            .context("Failed to read ontology/mapping.toml")?;

        let mapping_config: MappingConfig =
            toml::from_str(&mapping_toml).context("Failed to parse mapping.toml")?;

        Ok(Self {
            mapping_config: Arc::new(mapping_config),
        })
    }

    pub fn with_config(mapping_config: MappingConfig) -> Self {
        Self {
            mapping_config: Arc::new(mapping_config),
        }
    }

    pub fn map_graph_to_rdf(&self, graph: &PropertyGraph) -> Result<Vec<RdfTriple>> {
        let mut triples = Vec::new();

        for node in &graph.nodes {
            triples.extend(self.map_node_to_triples(node)?);
        }

        for edge in &graph.edges {
            triples.extend(self.map_edge_to_triples(edge)?);
        }

        Ok(triples)
    }

    fn map_node_to_triples(&self, node: &GraphNode) -> Result<Vec<RdfTriple>> {
        let mut triples = Vec::new();

        let node_iri = self.generate_node_iri(node)?;

        for label in &node.labels {
            if let Some(class_mapping) = self.mapping_config.class_mappings.get(label) {
                let owl_class_iri = self.expand_prefixed_iri(&class_mapping.owl_class)?;
                triples.push(RdfTriple {
                    subject: node_iri.clone(),
                    predicate: self.expand_prefixed_iri("rdf:type")?,
                    object: owl_class_iri,
                    is_literal: false,
                    datatype: None,
                    language: None,
                });
            } else {
                triples.push(RdfTriple {
                    subject: node_iri.clone(),
                    predicate: self.expand_prefixed_iri("rdf:type")?,
                    object: self
                        .expand_prefixed_iri(&self.mapping_config.defaults.default_node_class)?,
                    is_literal: false,
                    datatype: None,
                    language: None,
                });
            }
        }

        for (prop_name, prop_value) in &node.properties {
            if let Some(data_prop_mapping) =
                self.mapping_config.data_property_mappings.get(prop_name)
            {
                let prop_iri = self.expand_prefixed_iri(&data_prop_mapping.owl_property)?;

                let values = if let Some(arr) = prop_value.as_array() {
                    arr.iter().collect()
                } else {
                    vec![prop_value]
                };

                for value in values {
                    let (object_str, datatype) =
                        self.serialize_literal_value(value, &data_prop_mapping.rdfs_range)?;

                    triples.push(RdfTriple {
                        subject: node_iri.clone(),
                        predicate: prop_iri.clone(),
                        object: object_str,
                        is_literal: true,
                        datatype: Some(datatype),
                        language: None,
                    });
                }
            }
        }

        Ok(triples)
    }

    fn map_edge_to_triples(&self, edge: &GraphEdge) -> Result<Vec<RdfTriple>> {
        let mut triples = Vec::new();

        let source_iri = self.generate_node_iri_from_id(&edge.source)?;
        let target_iri = self.generate_node_iri_from_id(&edge.target)?;

        if let Some(obj_prop_mapping) = self
            .mapping_config
            .object_property_mappings
            .get(&edge.relationship_type)
        {
            let prop_iri = self.expand_prefixed_iri(&obj_prop_mapping.owl_property)?;

            triples.push(RdfTriple {
                subject: source_iri.clone(),
                predicate: prop_iri,
                object: target_iri.clone(),
                is_literal: false,
                datatype: None,
                language: None,
            });

            if self.mapping_config.global.auto_generate_inverses {
                if let Some(inverse_prop) = &obj_prop_mapping.owl_inverse_of {
                    let inverse_iri = self.expand_prefixed_iri(inverse_prop)?;
                    triples.push(RdfTriple {
                        subject: target_iri,
                        predicate: inverse_iri,
                        object: source_iri,
                        is_literal: false,
                        datatype: None,
                        language: None,
                    });
                }
            }
        } else {
            let default_prop =
                self.expand_prefixed_iri(&self.mapping_config.defaults.default_edge_property)?;
            triples.push(RdfTriple {
                subject: source_iri,
                predicate: default_prop,
                object: target_iri,
                is_literal: false,
                datatype: None,
                language: None,
            });
        }

        for (prop_name, prop_value) in &edge.properties {
            if let Some(data_prop_mapping) =
                self.mapping_config.data_property_mappings.get(prop_name)
            {
                let edge_iri = self.generate_edge_iri(edge)?;
                let prop_iri = self.expand_prefixed_iri(&data_prop_mapping.owl_property)?;

                let (object_str, datatype) =
                    self.serialize_literal_value(prop_value, &data_prop_mapping.rdfs_range)?;

                triples.push(RdfTriple {
                    subject: edge_iri,
                    predicate: prop_iri,
                    object: object_str,
                    is_literal: true,
                    datatype: Some(datatype),
                    language: None,
                });
            }
        }

        Ok(triples)
    }

    fn generate_node_iri(&self, node: &GraphNode) -> Result<String> {
        if let Some(label) = node.labels.first() {
            let label_lower = label.to_lowercase();
            if let Some(template) = self.mapping_config.iri_templates.nodes.get(&label_lower) {
                return self.apply_template(template, &node.id, node);
            }
        }

        Ok(format!(
            "{}{}",
            self.mapping_config.global.base_iri, node.id
        ))
    }

    fn generate_node_iri_from_id(&self, node_id: &str) -> Result<String> {
        Ok(format!(
            "{}{}",
            self.mapping_config.global.base_iri, node_id
        ))
    }

    fn generate_edge_iri(&self, edge: &GraphEdge) -> Result<String> {
        let rel_type_lower = edge.relationship_type.to_lowercase();
        if let Some(template) = self.mapping_config.iri_templates.edges.get(&rel_type_lower) {
            let template_str = template
                .replace("{base_iri}", &self.mapping_config.global.base_iri)
                .replace("{source_id}", &edge.source)
                .replace("{target_id}", &edge.target);
            return Ok(template_str);
        }

        Ok(format!(
            "{}edge/{}",
            self.mapping_config.global.base_iri, edge.id
        ))
    }

    fn apply_template(&self, template: &str, node_id: &str, _node: &GraphNode) -> Result<String> {
        let mut result = template.to_string();

        result = result.replace("{base_iri}", &self.mapping_config.global.base_iri);
        result = result.replace("{id}", node_id);

        if result.contains("{hash}") || result.contains("{path_hash}") {
            let hash = self.calculate_hash(node_id);
            result = result.replace("{hash}", &hash);
            result = result.replace("{path_hash}", &hash);
        }

        Ok(result)
    }

    fn calculate_hash(&self, input: &str) -> String {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(input.as_bytes());
        let hash = hasher.finalize();
        hash.to_hex()[..16].to_string()
    }

    fn expand_prefixed_iri(&self, prefixed: &str) -> Result<String> {
        if prefixed.contains("://") {
            return Ok(prefixed.to_string());
        }

        if let Some(colon_pos) = prefixed.find(':') {
            let (prefix, local) = prefixed.split_at(colon_pos);
            let local = &local[1..];

            if let Some(namespace) = self.mapping_config.namespaces.get(prefix) {
                return Ok(format!("{}{}", namespace, local));
            } else {
                bail!("Unknown namespace prefix: {}", prefix);
            }
        }

        Ok(format!(
            "{}{}",
            self.mapping_config.global.default_vocabulary, prefixed
        ))
    }

    fn serialize_literal_value(
        &self,
        value: &serde_json::Value,
        expected_range: &str,
    ) -> Result<(String, String)> {
        let full_range_iri = self.expand_prefixed_iri(expected_range)?;

        match value {
            serde_json::Value::String(s) => {
                if s.starts_with("http://") || s.starts_with("https://") {
                    return Ok((s.clone(), full_range_iri));
                }

                if s.contains('T')
                    && (s.contains('Z') || s.contains('+') || s.contains('-'))
                    && expected_range == "xsd:dateTime"
                {
                    return Ok((s.clone(), self.expand_prefixed_iri("xsd:dateTime")?));
                }

                Ok((s.clone(), full_range_iri))
            }
            serde_json::Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    let datatype = if expected_range == "xsd:nonNegativeInteger" {
                        self.expand_prefixed_iri("xsd:nonNegativeInteger")?
                    } else {
                        self.expand_prefixed_iri("xsd:integer")?
                    };
                    Ok((n.to_string(), datatype))
                } else {
                    Ok((n.to_string(), self.expand_prefixed_iri("xsd:double")?))
                }
            }
            serde_json::Value::Bool(b) => {
                Ok((b.to_string(), self.expand_prefixed_iri("xsd:boolean")?))
            }
            _ => Ok((value.to_string(), full_range_iri)),
        }
    }

    pub fn run_consistency_checks(&self) -> Result<()> {
        Ok(())
    }

    pub fn perform_inference(&self) -> Result<()> {
        Ok(())
    }
}

impl Default for OwlValidatorService {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            log::warn!(
                "Failed to load mapping config: {}. Using minimal fallback.",
                e
            );
            // Create minimal config with essential namespaces
            let mut namespaces = std::collections::HashMap::new();
            namespaces.insert(
                "rdf".to_string(),
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
            );
            namespaces.insert(
                "rdfs".to_string(),
                "http://www.w3.org/2000/01/rdf-schema#".to_string(),
            );
            namespaces.insert(
                "owl".to_string(),
                "http://www.w3.org/2002/07/owl#".to_string(),
            );
            namespaces.insert(
                "xsd".to_string(),
                "http://www.w3.org/2001/XMLSchema#".to_string(),
            );

            let minimal_config = MappingConfig {
                metadata: MappingMetadata {
                    title: "Minimal Config".to_string(),
                    version: "1.0".to_string(),
                    description: "Fallback minimal configuration".to_string(),
                    author: "System".to_string(),
                    created: "2024-01-01".to_string(),
                    last_modified: "2024-01-01".to_string(),
                },
                global: GlobalConfig {
                    base_iri: "http://example.org/".to_string(),
                    default_vocabulary: "http://example.org/vocab#".to_string(),
                    version_iri: "http://example.org/1.0.0".to_string(),
                    default_language: "en".to_string(),
                    strict_mode: false,
                    auto_generate_inverses: false,
                },
                defaults: DefaultsConfig {
                    default_node_class: "owl:Thing".to_string(),
                    default_edge_property: "owl:relatedTo".to_string(),
                    default_datatype: "xsd:string".to_string(),
                    fallback_namespace: "http://example.org/".to_string(),
                },
                namespaces,
                class_mappings: std::collections::HashMap::new(),
                object_property_mappings: std::collections::HashMap::new(),
                data_property_mappings: std::collections::HashMap::new(),
                iri_templates: IriTemplates {
                    nodes: std::collections::HashMap::new(),
                    edges: std::collections::HashMap::new(),
                    metadata: MetadataTemplates {
                        property: "{base_iri}property/{id}".to_string(),
                        class: "{base_iri}class/{id}".to_string(),
                    },
                },
            };
            Self::with_config(minimal_config)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_config() -> MappingConfig {
        let mut namespaces = HashMap::new();
        namespaces.insert(
            "rdf".to_string(),
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
        );
        namespaces.insert(
            "rdfs".to_string(),
            "http://www.w3.org/2000/01/rdf-schema#".to_string(),
        );
        namespaces.insert(
            "owl".to_string(),
            "http://www.w3.org/2002/07/owl#".to_string(),
        );
        namespaces.insert(
            "xsd".to_string(),
            "http://www.w3.org/2001/XMLSchema#".to_string(),
        );
        namespaces.insert("foaf".to_string(), "http://xmlns.com/foaf/0.1/".to_string());
        namespaces.insert("mv".to_string(), "http://example.org/mv#".to_string());

        let mut class_mappings = HashMap::new();
        class_mappings.insert(
            "Person".to_string(),
            ClassMapping {
                owl_class: "foaf:Person".to_string(),
                rdfs_label: "Person".to_string(),
                rdfs_comment: "A person".to_string(),
                rdfs_subclass_of: Vec::new(),
                equivalent_classes: Vec::new(),
                disjoint_with: Vec::new(),
            },
        );
        class_mappings.insert(
            "Company".to_string(),
            ClassMapping {
                owl_class: "foaf:Organization".to_string(),
                rdfs_label: "Company".to_string(),
                rdfs_comment: "A company".to_string(),
                rdfs_subclass_of: Vec::new(),
                equivalent_classes: Vec::new(),
                disjoint_with: Vec::new(),
            },
        );

        let mut data_property_mappings = HashMap::new();
        data_property_mappings.insert(
            "name".to_string(),
            DataPropertyMapping {
                owl_property: "foaf:name".to_string(),
                rdfs_label: "name".to_string(),
                rdfs_comment: "Name of entity".to_string(),
                rdfs_domain: PropertyDomain::Single("owl:Thing".to_string()),
                rdfs_range: "xsd:string".to_string(),
                property_type: "data".to_string(),
                characteristics: Vec::new(),
            },
        );
        data_property_mappings.insert(
            "age".to_string(),
            DataPropertyMapping {
                owl_property: "foaf:age".to_string(),
                rdfs_label: "age".to_string(),
                rdfs_comment: "Age in years".to_string(),
                rdfs_domain: PropertyDomain::Single("foaf:Person".to_string()),
                rdfs_range: "xsd:integer".to_string(),
                property_type: "data".to_string(),
                characteristics: Vec::new(),
            },
        );
        data_property_mappings.insert(
            "email".to_string(),
            DataPropertyMapping {
                owl_property: "foaf:mbox".to_string(),
                rdfs_label: "email".to_string(),
                rdfs_comment: "Email address".to_string(),
                rdfs_domain: PropertyDomain::Single("foaf:Agent".to_string()),
                rdfs_range: "xsd:string".to_string(),
                property_type: "data".to_string(),
                characteristics: Vec::new(),
            },
        );

        let mut object_property_mappings = HashMap::new();
        object_property_mappings.insert(
            "employedBy".to_string(),
            ObjectPropertyMapping {
                owl_property: "mv:employedBy".to_string(),
                rdfs_label: "employed by".to_string(),
                rdfs_comment: "Employment relationship".to_string(),
                rdfs_domain: PropertyDomain::Single("foaf:Person".to_string()),
                rdfs_range: PropertyRange::Single("foaf:Organization".to_string()),
                owl_inverse_of: Some("mv:employs".to_string()),
                property_type: "object".to_string(),
                characteristics: Vec::new(),
            },
        );

        MappingConfig {
            metadata: MappingMetadata {
                title: "Test Mapping".to_string(),
                version: "1.0.0".to_string(),
                description: "Test mapping config".to_string(),
                author: "Test".to_string(),
                created: "2024-01-01".to_string(),
                last_modified: "2024-01-01".to_string(),
            },
            global: GlobalConfig {
                base_iri: "http://example.org/".to_string(),
                default_vocabulary: "http://example.org/vocab#".to_string(),
                version_iri: "http://example.org/1.0.0".to_string(),
                default_language: "en".to_string(),
                strict_mode: false,
                auto_generate_inverses: true,
            },
            defaults: DefaultsConfig {
                default_node_class: "owl:Thing".to_string(),
                default_edge_property: "mv:relatedTo".to_string(),
                default_datatype: "xsd:string".to_string(),
                fallback_namespace: "http://example.org/".to_string(),
            },
            namespaces,
            class_mappings,
            object_property_mappings,
            data_property_mappings,
            iri_templates: IriTemplates {
                nodes: HashMap::new(),
                edges: HashMap::new(),
                metadata: MetadataTemplates {
                    property: "{base_iri}property/{id}".to_string(),
                    class: "{base_iri}class/{id}".to_string(),
                },
            },
        }
    }

    fn create_test_service() -> OwlValidatorService {
        OwlValidatorService::with_config(create_test_config())
    }

    #[test]
    fn test_service_creation() {
        let service = create_test_service();
        assert!(!service.mapping_config.namespaces.is_empty());
    }

    #[test]
    fn test_expand_prefixed_iri() {
        let service = create_test_service();

        let expanded = service.expand_prefixed_iri("foaf:Person").unwrap();
        assert_eq!(expanded, "http://xmlns.com/foaf/0.1/Person");

        let expanded = service.expand_prefixed_iri("rdf:type").unwrap();
        assert_eq!(expanded, "http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
    }

    #[test]
    fn test_map_simple_node() {
        let service = create_test_service();

        let node = GraphNode {
            id: "person1".to_string(),
            labels: vec!["Person".to_string()],
            properties: {
                let mut props = HashMap::new();
                props.insert("name".to_string(), json!("John Doe"));
                props.insert("age".to_string(), json!(30));
                props
            },
        };

        let triples = service.map_node_to_triples(&node).unwrap();

        assert!(triples
            .iter()
            .any(|t| t.predicate.contains("rdf-syntax-ns#type") && t.object.contains("Person")));

        assert!(triples
            .iter()
            .any(|t| t.predicate.contains("foaf") && t.object == "John Doe"));
    }

    #[test]
    fn test_map_graph_to_rdf() {
        let service = create_test_service();

        let graph = PropertyGraph {
            nodes: vec![
                GraphNode {
                    id: "person1".to_string(),
                    labels: vec!["Person".to_string()],
                    properties: {
                        let mut props = HashMap::new();
                        props.insert("name".to_string(), json!("Alice"));
                        props.insert("email".to_string(), json!("alice@example.com"));
                        props
                    },
                },
                GraphNode {
                    id: "company1".to_string(),
                    labels: vec!["Company".to_string()],
                    properties: {
                        let mut props = HashMap::new();
                        props.insert("name".to_string(), json!("ACME Corp"));
                        props
                    },
                },
            ],
            edges: vec![GraphEdge {
                id: "edge1".to_string(),
                source: "person1".to_string(),
                target: "company1".to_string(),
                relationship_type: "employedBy".to_string(),
                properties: HashMap::new(),
            }],
            metadata: HashMap::new(),
        };

        let triples = service.map_graph_to_rdf(&graph).unwrap();

        assert!(!triples.is_empty());

        let type_triples: Vec<_> = triples
            .iter()
            .filter(|t| t.predicate.contains("rdf-syntax-ns#type"))
            .collect();
        assert!(!type_triples.is_empty());

        assert!(triples.iter().any(|t| t.predicate.contains("employedBy")));
    }
}
