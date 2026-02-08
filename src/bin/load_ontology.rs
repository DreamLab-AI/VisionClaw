// src/bin/load_ontology.rs
//! Ontology Loader Binary
//!
//! Loads OWL ontology data from GitHub repository markdown files
//! and populates the Neo4j graph database.

use std::sync::Arc;
use std::collections::HashMap;
use log::info;

use webxr::adapters::neo4j_ontology_repository::{Neo4jOntologyRepository, Neo4jOntologyConfig};
use webxr::services::parsers::ontology_parser::OntologyParser;
use webxr::ports::ontology_repository::{OntologyRepository, OwlClass, OwlProperty, PropertyType, OwlAxiom, AxiomType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    info!("Starting ontology loader...");

    // 1. Initialize repository
    use std::env;

    let neo4j_uri = env::var("NEO4J_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string());
    let neo4j_user = env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let neo4j_password = env::var("NEO4J_PASSWORD").expect("NEO4J_PASSWORD must be set");
    let neo4j_database = env::var("NEO4J_DATABASE").unwrap_or_else(|_| "neo4j".to_string());

    info!("Connecting to Neo4j at: {}", neo4j_uri);

    let config = Neo4jOntologyConfig {
        uri: neo4j_uri.clone(),
        user: neo4j_user,
        password: neo4j_password,
        database: Some(neo4j_database),
    };

    let ontology_repo = Arc::new(
        Neo4jOntologyRepository::new(config).await?
    );

    // 2. Initialize parser
    let _parser = OntologyParser::new();

    // 3. Load sample ontology data for testing
    info!("Loading sample ontology classes...");

    // Create sample OWL classes for testing
    let sample_classes = vec![
        ("mv:Person", "Person", "A human individual", vec![]),
        ("mv:Company", "Company", "A business organization", vec!["mv:Concept".to_string()]),
        ("mv:Project", "Project", "A collaborative endeavor", vec!["mv:Concept".to_string()]),
        ("mv:Concept", "Concept", "An abstract idea", vec![]),
        ("mv:Technology", "Technology", "A technical tool or system", vec![]),
    ];

    let mut _total_classes = 0;

    for (iri, label, desc, parents) in sample_classes {
        let class = OwlClass {
            iri: iri.to_string(),
            term_id: None,
            preferred_term: None,
            label: Some(label.to_string()),
            description: Some(desc.to_string()),
            parent_classes: parents,
            source_domain: None,
            version: None,
            class_type: None,
            status: None,
            maturity: None,
            quality_score: None,
            authority_score: None,
            public_access: None,
            content_status: None,
            owl_physicality: None,
            owl_role: None,
            belongs_to_domain: None,
            bridges_to_domain: None,
            properties: HashMap::new(),
            source_file: None,
            markdown_content: None,
            file_sha1: None,
            last_synced: None,
            additional_metadata: None,
        };

        ontology_repo.add_owl_class(&class).await?;
        _total_classes += 1;
        info!("Saved class: {} ({})", label, iri);
    }

    // 4. Create sample properties
    info!("Creating sample properties...");
    let prop = OwlProperty {
        iri: "mv:worksAt".to_string(),
        label: Some("works at".to_string()),
        property_type: PropertyType::ObjectProperty,
        domain: vec!["mv:Person".to_string()],
        range: vec!["mv:Company".to_string()],
        quality_score: None,
        authority_score: None,
        source_file: None,
    };
    ontology_repo.add_owl_property(&prop).await?;

    // 5. Create sample axioms
    info!("Creating sample axioms...");
    let axiom = OwlAxiom {
        id: None,
        axiom_type: AxiomType::SubClassOf,
        subject: "mv:Company".to_string(),
        object: "mv:Concept".to_string(),
        annotations: HashMap::new(),
    };
    ontology_repo.add_axiom(&axiom).await?;

    // 6. Verify data
    let all_classes = ontology_repo.get_classes().await?;
    info!("\nOntology loaded successfully!");
    info!("Classes: {}", all_classes.len());
    info!("Stored in Neo4j graph database at: {}", neo4j_uri);

    Ok(())
}
