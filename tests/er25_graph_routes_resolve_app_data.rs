//! ER-25: the schema, pathfinding, semantic and physics route groups used to
//! extract `Addr<GraphStateActor>` or `Arc<RwLock<GraphData>>`, neither of
//! which `main.rs` registers, so Actix rejected every request on them with
//! "Requested application data is not configured correctly" before the
//! handler ran. They now read the graph through the registered
//! `Addr<GraphServiceSupervisor>`. This test registers only what `main.rs`
//! registers for the pathfinding group and checks the handler is reached.

use actix::Actor;
use actix_web::{test, web, App};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use visionclaw_domain::models::edge::Edge;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::node::Node;
use visionclaw_server::actors::graph_service_supervisor::GraphServiceSupervisor;
use visionclaw_server::adapters::actix_semantic_adapter::ActixSemanticAdapter;
use visionclaw_server::application::semantic_service::SemanticService;
use visionclaw_server::handlers::graph_access::current_graph;
use visionclaw_server::handlers::{configure_pathfinding_routes, configure_semantic_routes};
use visionclaw_server::ports::knowledge_graph_repository::{
    GraphStatistics, KnowledgeGraphRepository, Result as KgResult,
};
use visionclaw_server::services::semantic_pathfinding_service::SemanticPathfindingService;

/// An empty repository: the supervisor needs one to construct, and these
/// tests only need it to answer with nothing.
struct EmptyRepo;

#[async_trait]
impl KnowledgeGraphRepository for EmptyRepo {
    async fn load_graph(&self) -> KgResult<Arc<GraphData>> {
        Ok(Arc::new(GraphData::default()))
    }
    async fn save_graph(&self, _: &GraphData) -> KgResult<()> {
        Ok(())
    }
    async fn add_node(&self, _: &Node) -> KgResult<u32> {
        Ok(0)
    }
    async fn batch_add_nodes(&self, _: Vec<Node>) -> KgResult<Vec<u32>> {
        Ok(vec![])
    }
    async fn batch_add_nodes_if_absent(&self, _: Vec<Node>) -> KgResult<Vec<u32>> {
        Ok(vec![])
    }
    async fn update_node(&self, _: &Node) -> KgResult<()> {
        Ok(())
    }
    async fn batch_update_nodes(&self, _: Vec<Node>) -> KgResult<()> {
        Ok(())
    }
    async fn remove_node(&self, _: u32) -> KgResult<()> {
        Ok(())
    }
    async fn batch_remove_nodes(&self, _: Vec<u32>) -> KgResult<()> {
        Ok(())
    }
    async fn get_node(&self, _: u32) -> KgResult<Option<Node>> {
        Ok(None)
    }
    async fn get_nodes(&self, _: Vec<u32>) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_nodes_by_metadata_id(&self, _: &str) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_nodes_by_owl_class_iri(&self, _: &str) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn search_nodes_by_label(&self, _: &str) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn add_edge(&self, _: &Edge) -> KgResult<String> {
        Ok(String::new())
    }
    async fn batch_add_edges(&self, _: Vec<Edge>) -> KgResult<Vec<String>> {
        Ok(vec![])
    }
    async fn update_edge(&self, _: &Edge) -> KgResult<()> {
        Ok(())
    }
    async fn remove_edge(&self, _: &str) -> KgResult<()> {
        Ok(())
    }
    async fn batch_remove_edges(&self, _: Vec<String>) -> KgResult<()> {
        Ok(())
    }
    async fn get_node_edges(&self, _: u32) -> KgResult<Vec<Edge>> {
        Ok(vec![])
    }
    async fn get_edges_between(&self, _: u32, _: u32) -> KgResult<Vec<Edge>> {
        Ok(vec![])
    }
    async fn batch_update_positions(&self, _: Vec<(u32, f32, f32, f32)>) -> KgResult<()> {
        Ok(())
    }
    async fn get_all_positions(&self) -> KgResult<HashMap<u32, (f32, f32, f32)>> {
        Ok(HashMap::new())
    }
    async fn query_nodes(&self, _: &str) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_neighbors(&self, _: u32) -> KgResult<Vec<Node>> {
        Ok(vec![])
    }
    async fn get_statistics(&self) -> KgResult<GraphStatistics> {
        Ok(GraphStatistics {
            node_count: 0,
            edge_count: 0,
            average_degree: 0.0,
            connected_components: 0,
            last_updated: chrono::Utc::now(),
        })
    }
    async fn clear_graph(&self) -> KgResult<()> {
        Ok(())
    }
    async fn health_check(&self) -> KgResult<bool> {
        Ok(true)
    }
}

const MISSING_APP_DATA: &str = "Requested application data is not configured correctly";

fn supervisor() -> actix::Addr<GraphServiceSupervisor> {
    GraphServiceSupervisor::new(Arc::new(EmptyRepo)).start()
}

#[actix_rt::test]
async fn current_graph_answers_through_the_supervisor() {
    let addr = supervisor();
    let graph = current_graph(&addr)
        .await
        .expect("supervisor answers GetGraphData");
    assert!(
        graph.nodes.is_empty(),
        "no graph state actor yet, so an empty graph"
    );
}

#[actix_rt::test]
async fn pathfinding_routes_reach_the_handler_with_mains_app_data() {
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(supervisor()))
            .app_data(web::Data::new(Arc::new(
                SemanticPathfindingService::default(),
            )))
            .configure(configure_pathfinding_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/pathfinding/semantic-path")
        .set_json(serde_json::json!({ "start_id": 1, "end_id": 2 }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let body = test::read_body(resp).await;
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains(MISSING_APP_DATA),
        "extractor still names unregistered app data: {text}"
    );
}

#[actix_rt::test]
async fn semantic_routes_reach_the_handler_with_mains_app_data() {
    let adapter = Arc::new(tokio::sync::RwLock::new(ActixSemanticAdapter::new()));
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(supervisor()))
            .app_data(web::Data::new(Arc::new(SemanticService::new(adapter))))
            .configure(configure_semantic_routes),
    )
    .await;
    for req in [
        test::TestRequest::get()
            .uri("/semantic/statistics")
            .to_request(),
        test::TestRequest::post()
            .uri("/semantic/communities")
            .set_json(serde_json::json!({ "algorithm": "louvain" }))
            .to_request(),
    ] {
        let resp = test::call_service(&app, req).await;
        let text = String::from_utf8_lossy(&test::read_body(resp).await).into_owned();
        assert!(
            !text.contains(MISSING_APP_DATA),
            "extractor still names unregistered app data: {text}"
        );
    }
}

#[actix_rt::test]
async fn pathfinding_routes_are_rate_limited() {
    use visionclaw_server::handlers::semantic_pathfinding_handler::PATHFINDING_REQUESTS_PER_MINUTE;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(supervisor()))
            .app_data(web::Data::new(Arc::new(
                SemanticPathfindingService::default(),
            )))
            .configure(configure_pathfinding_routes),
    )
    .await;
    let mut limited = false;
    for _ in 0..=PATHFINDING_REQUESTS_PER_MINUTE {
        let req = test::TestRequest::post()
            .uri("/pathfinding/chunk-traversal")
            .peer_addr("203.0.113.7:1".parse().unwrap())
            .set_json(serde_json::json!({ "start_id": 1, "max_nodes": 1_000_000 }))
            .to_request();
        let status = match test::try_call_service(&app, req).await {
            Ok(resp) => resp.status(),
            Err(e) => e.as_response_error().status_code(),
        };
        if status == actix_web::http::StatusCode::TOO_MANY_REQUESTS {
            limited = true;
        }
    }
    assert!(limited, "one client got more than the per-minute budget");

    use actix_web::HttpMessage;
    // Keyed on the verified pubkey: a different user keeps their own budget,
    // and changing the forwarding headers does not reset one.
    let req = test::TestRequest::post()
        .uri("/pathfinding/chunk-traversal")
        .peer_addr("203.0.113.7:1".parse().unwrap())
        .insert_header(("X-Forwarded-For", "198.51.100.9"))
        .set_json(serde_json::json!({ "start_id": 1 }))
        .to_request();
    req.extensions_mut()
        .insert(visionclaw_server::middleware::AuthenticatedUser {
            pubkey: "ab".repeat(32),
        });
    let status = match test::try_call_service(&app, req).await {
        Ok(resp) => resp.status(),
        Err(e) => e.as_response_error().status_code(),
    };
    assert_ne!(
        status,
        actix_web::http::StatusCode::TOO_MANY_REQUESTS,
        "a signed-in user shares an anonymous budget"
    );
}
