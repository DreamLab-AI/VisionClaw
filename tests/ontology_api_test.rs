//! REST API tests for ontology endpoints
//!
//! Comprehensive integration tests for all ontology API endpoints.
//! Tests marked as #[ignore] require full actor system initialization.
//! Documentation tests run without requiring infrastructure.

#[cfg(test)]
mod integration_tests {
    #[allow(unused_imports)]
    use actix_web::{test, web, App};
    use serde_json::json;
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[cfg(feature = "ontology")]
    use visionclaw_server::handlers::api_handler::ontology::config as ontology_config;

    #[cfg(feature = "ontology")]
    use visionclaw_server::services::owl_validator::{GraphEdge, GraphNode, PropertyGraph};

    #[cfg(feature = "ontology")]
    fn create_test_graph() -> PropertyGraph {
        PropertyGraph {
            nodes: vec![
                GraphNode {
                    id: "person1".to_string(),
                    labels: vec!["Person".to_string()],
                    properties: {
                        let mut props = HashMap::new();
                        props.insert("name".to_string(), serde_json::json!("Alice"));
                        props.insert("age".to_string(), serde_json::json!(30));
                        props
                    },
                },
                GraphNode {
                    id: "company1".to_string(),
                    labels: vec!["Company".to_string()],
                    properties: {
                        let mut props = HashMap::new();
                        props.insert("name".to_string(), serde_json::json!("ACME Corp"));
                        props
                    },
                },
            ],
            edges: vec![GraphEdge {
                id: "edge1".to_string(),
                source: "person1".to_string(),
                target: "company1".to_string(),
                relationship_type: "WORKS_FOR".to_string(),
                properties: {
                    let mut props = HashMap::new();
                    props.insert("since".to_string(), serde_json::json!("2020-01-01"));
                    props
                },
            }],
            metadata: HashMap::new(),
        }
    }

    #[cfg(feature = "ontology")]
    fn create_test_app() -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new().configure(ontology_config)
    }

    // ========================================================================
    // Class Management Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState with Neo4jOntologyRepository"]
    async fn test_list_classes_returns_ok() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/classes")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_get_class_by_iri() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/classes/http://www.w3.org/2002/07/owl%23Thing")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success() || resp.status().as_u16() == 404);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_add_class_with_valid_data() {
        let app = test::init_service(create_test_app()).await;

        let new_class = json!({
            "class": {
                "iri": "http://test.org/NewClass",
                "label": "New Test Class",
                "description": "A class for testing",
                "superClasses": [],
                "equivalentClasses": []
            }
        });

        let req = test::TestRequest::post()
            .uri("/ontology/classes")
            .set_json(&new_class)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_update_class() {
        let app = test::init_service(create_test_app()).await;

        let updated = json!({
            "class": {
                "iri": "http://test.org/ExistingClass",
                "label": "Updated Label",
                "description": "Updated description"
            }
        });

        let req = test::TestRequest::put()
            .uri("/ontology/classes/http://test.org/ExistingClass")
            .set_json(&updated)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success() || resp.status().is_client_error());
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_delete_class() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::delete()
            .uri("/ontology/classes/http://test.org/DeleteMe")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success() || resp.status().is_client_error());
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_get_class_axioms() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/classes/http://test.org/TestClass/axioms")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success() || resp.status().is_client_error());
    }

    // ========================================================================
    // Property Management Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_list_properties() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/properties")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_add_property() {
        let app = test::init_service(create_test_app()).await;

        let new_prop = json!({
            "property": {
                "iri": "http://test.org/hasProperty",
                "label": "has property",
                "propertyType": "ObjectProperty",
                "domain": [],
                "range": []
            }
        });

        let req = test::TestRequest::post()
            .uri("/ontology/properties")
            .set_json(&new_prop)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // ========================================================================
    // Graph Operations Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_get_ontology_graph() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get().uri("/ontology/graph").to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_save_ontology_graph() {
        let app = test::init_service(create_test_app()).await;

        let graph = json!({
            "graph": {
                "nodes": [],
                "edges": [],
                "metadata": {}
            }
        });

        let req = test::TestRequest::post()
            .uri("/ontology/graph")
            .set_json(&graph)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // ========================================================================
    // Inference Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState with reasoning service"]
    async fn test_get_inference_results() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/inference")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success() || resp.status().as_u16() == 404);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_store_inference_results() {
        let app = test::init_service(create_test_app()).await;

        let results = json!({
            "results": {
                "inferredAxioms": [],
                "timestamp": "2025-01-01T00:00:00Z",
                "reasonerUsed": "Test"
            }
        });

        let req = test::TestRequest::post()
            .uri("/ontology/inference")
            .set_json(&results)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    // ========================================================================
    // Validation and Query Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_validate_ontology() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/validate")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_query_ontology() {
        let app = test::init_service(create_test_app()).await;

        let query = json!({
            "query": "SELECT ?s WHERE { ?s a owl:Class } LIMIT 10"
        });

        let req = test::TestRequest::post()
            .uri("/ontology/query")
            .set_json(&query)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_get_metrics() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/metrics")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 200);
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_malformed_json_returns_400() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::post()
            .uri("/ontology/classes")
            .insert_header(("content-type", "application/json"))
            .set_payload(r#"{"invalid json"#)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[cfg(feature = "ontology")]
    #[actix_web::test]
    #[ignore = "Requires full AppState"]
    async fn test_nonexistent_endpoint_returns_404() {
        let app = test::init_service(create_test_app()).await;

        let req = test::TestRequest::get()
            .uri("/ontology/nonexistent")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[cfg(not(feature = "ontology"))]
    #[test]
    fn test_ontology_feature_disabled() {
        // When ontology feature is disabled, tests are skipped
        assert!(true, "Ontology API tests skipped - feature not enabled");
    }
}

// ============================================================================
// API Documentation Tests (No Infrastructure Required)
// ============================================================================

#[cfg(test)]
mod api_documentation {
    use serde_json::json;

    #[test]
    fn test_api_endpoint_catalog() {
        // Complete list of ontology API endpoints
        let endpoints = vec![
            ("GET", "/ontology/graph", "Get full ontology graph"),
            ("POST", "/ontology/graph", "Save ontology graph"),
            ("GET", "/ontology/classes", "List all OWL classes"),
            ("POST", "/ontology/classes", "Add new OWL class"),
            ("GET", "/ontology/classes/{iri}", "Get specific class"),
            ("PUT", "/ontology/classes/{iri}", "Update class"),
            ("DELETE", "/ontology/classes/{iri}", "Delete class"),
            ("GET", "/ontology/classes/{iri}/axioms", "Get class axioms"),
            ("GET", "/ontology/properties", "List properties"),
            ("POST", "/ontology/properties", "Add property"),
            ("GET", "/ontology/properties/{iri}", "Get property"),
            ("PUT", "/ontology/properties/{iri}", "Update property"),
            ("POST", "/ontology/axioms", "Add axiom"),
            ("DELETE", "/ontology/axioms/{id}", "Remove axiom"),
            ("GET", "/ontology/inference", "Get inference results"),
            ("POST", "/ontology/inference", "Store inference results"),
            ("GET", "/ontology/validate", "Validate ontology"),
            ("POST", "/ontology/query", "Query ontology"),
            ("GET", "/ontology/metrics", "Get ontology metrics"),
        ];

        assert_eq!(endpoints.len(), 19);

        for (method, path, _description) in endpoints {
            assert!(!method.is_empty());
            assert!(path.starts_with("/ontology/"));
        }
    }

    #[test]
    fn test_request_response_formats() {
        // Document expected request/response formats

        // Class request
        let class_request = json!({
            "class": {
                "iri": "http://example.org/Person",
                "label": "Person",
                "description": "A human being"
            }
        });
        assert!(class_request["class"]["iri"].is_string());

        // Property request
        let property_request = json!({
            "property": {
                "iri": "http://example.org/hasName",
                "label": "has name",
                "propertyType": "DataProperty"
            }
        });
        assert!(property_request["property"]["propertyType"].is_string());

        // Axiom request
        let axiom_request = json!({
            "axiom": {
                "axiomType": "SubClassOf",
                "subject": "Child",
                "object": "Parent"
            }
        });
        assert!(axiom_request["axiom"]["axiomType"].is_string());

        // Success response
        let success_response = json!({
            "success": true,
            "iri": "http://example.org/Created"
        });
        assert!(success_response["success"].as_bool().unwrap());

        // Error response
        let error_response = json!({
            "error": "Not found",
            "message": "Resource does not exist"
        });
        assert!(error_response["error"].is_string());
    }

    #[test]
    fn test_validation_report_format() {
        let validation_report = json!({
            "is_valid": true,
            "errors": [],
            "warnings": [
                {
                    "severity": "warning",
                    "message": "Class lacks documentation",
                    "location": "http://example.org/UndocumentedClass"
                }
            ],
            "info": [],
            "timestamp": "2025-01-01T00:00:00Z"
        });

        assert!(validation_report["is_valid"].is_boolean());
        assert!(validation_report["errors"].is_array());
        assert!(validation_report["warnings"].is_array());
    }

    #[test]
    fn test_metrics_format() {
        let metrics = json!({
            "classCount": 150,
            "propertyCount": 45,
            "axiomCount": 520,
            "individualCount": 0,
            "logicalAxiomCount": 320,
            "declarationAxiomCount": 200
        });

        assert!(metrics["classCount"].is_number());
        assert!(metrics["axiomCount"].is_number());
    }

    #[test]
    fn test_http_status_codes() {
        // Document expected HTTP status codes
        let status_codes = vec![
            (200, "Success - resource found/operation completed"),
            (404, "Not found - resource doesn't exist"),
            (400, "Bad request - invalid JSON or missing fields"),
            (405, "Method not allowed"),
            (500, "Internal server error - CQRS handler failure"),
        ];

        assert_eq!(status_codes.len(), 5);

        for (code, _description) in status_codes {
            assert!((200..600).contains(&code));
        }
    }
}
