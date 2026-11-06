//! Where a full-access analytics request goes.
//!
//! Full-access callers (filter off, or nothing hidden from them) are answered by the GPU
//! analytics actors. Every one of those requests goes through the `GPUManagerActor`, which
//! routes it to the `AnalyticsSupervisor` and from there to `ClusteringActor` or
//! `PageRankActor`. Gated callers never reach this module; they are answered on the CPU
//! over their own view (`visible_analytics`).
//!
//! Before this module the community route sent to `ForceComputeActor`, whose handler
//! always answered "should be handled by ClusteringActor", and the PageRank routes read an
//! `AppState.analytics.pagerank` address that nothing ever set. Both always failed. The
//! routes are recipients so that a test can stand in for the GPU.

use actix::{Addr, Recipient};

use crate::actors::gpu::gpu_manager_actor::GPUManagerActor;
use crate::actors::messages::{
    ClearPageRankCache, ComputePageRank, GetPageRankResult, RunCommunityDetection,
};
use crate::AppState;

/// The four analytics messages a full-access caller can send, each with its destination.
#[derive(Clone)]
pub(crate) struct GpuAnalyticsRoutes {
    pub community: Recipient<RunCommunityDetection>,
    pub pagerank: Recipient<ComputePageRank>,
    pub pagerank_cached: Recipient<GetPageRankResult>,
    pub pagerank_clear: Recipient<ClearPageRankCache>,
}

impl GpuAnalyticsRoutes {
    /// Every route through the GPU manager.
    pub fn via_manager(manager: &Addr<GPUManagerActor>) -> Self {
        Self {
            community: manager.clone().recipient(),
            pagerank: manager.clone().recipient(),
            pagerank_cached: manager.clone().recipient(),
            pagerank_clear: manager.clone().recipient(),
        }
    }

    /// The routes of a running server, or `None` when the GPU manager never started.
    pub fn of(app_state: &AppState) -> Option<Self> {
        app_state.gpu_manager_addr.as_ref().map(Self::via_manager)
    }
}

#[cfg(test)]
pub(crate) mod stub {
    //! A stand-in for the GPU analytics actors that answers every route with a fixed result.
    use actix::prelude::*;

    use crate::actors::gpu::pagerank_actor::PageRankResult;
    use crate::actors::messages::{
        ClearPageRankCache, CommunityDetectionResult, ComputePageRank, GetPageRankResult,
        RunCommunityDetection,
    };

    #[derive(Default)]
    pub struct StubGpu {
        pub community: Option<CommunityDetectionResult>,
        pub pagerank: Option<PageRankResult>,
        pub cleared: bool,
    }

    impl Actor for StubGpu {
        type Context = Context<Self>;
    }

    impl Handler<RunCommunityDetection> for StubGpu {
        type Result = Result<CommunityDetectionResult, String>;
        fn handle(&mut self, _m: RunCommunityDetection, _c: &mut Context<Self>) -> Self::Result {
            self.community
                .clone()
                .ok_or_else(|| "stub: no result".to_string())
        }
    }

    impl Handler<ComputePageRank> for StubGpu {
        type Result = Result<PageRankResult, String>;
        fn handle(&mut self, _m: ComputePageRank, _c: &mut Context<Self>) -> Self::Result {
            self.pagerank
                .clone()
                .ok_or_else(|| "stub: no result".to_string())
        }
    }

    impl Handler<GetPageRankResult> for StubGpu {
        type Result = MessageResult<GetPageRankResult>;
        fn handle(&mut self, _m: GetPageRankResult, _c: &mut Context<Self>) -> Self::Result {
            MessageResult(if self.cleared {
                None
            } else {
                self.pagerank.clone()
            })
        }
    }

    impl Handler<ClearPageRankCache> for StubGpu {
        type Result = ();
        fn handle(&mut self, _m: ClearPageRankCache, _c: &mut Context<Self>) {
            self.cleared = true;
        }
    }

    pub fn routes(addr: &Addr<StubGpu>) -> super::GpuAnalyticsRoutes {
        super::GpuAnalyticsRoutes {
            community: addr.clone().recipient(),
            pagerank: addr.clone().recipient(),
            pagerank_cached: addr.clone().recipient(),
            pagerank_clear: addr.clone().recipient(),
        }
    }
}

/// A two-community GPU result whose buffer-slot order differs from its graph node ids.
#[cfg(all(test, feature = "gpu"))]
pub(crate) fn gpu_result_fixture() -> crate::actors::messages::CommunityDetectionResult {
    tests::two_community_result()
}

#[cfg(all(test, feature = "gpu"))]
mod tests {
    use super::stub::{routes, StubGpu};
    use crate::actors::gpu::clustering_actor::{Community, CommunityDetectionStats};
    use crate::actors::gpu::pagerank_actor::{PageRankParams, PageRankResult, PageRankStats};
    use crate::actors::messages::{CommunityDetectionAlgorithm, CommunityDetectionResult};
    use crate::handlers::api_handler::analytics::community::{
        run_community_detection_via, CommunityDetectionRequest,
    };
    use crate::handlers::api_handler::analytics::pagerank_handlers::{
        whole_graph_cached_pagerank, whole_graph_clear_pagerank, whole_graph_pagerank,
    };
    use actix::Actor;

    /// Graph ids 10, 11, 20, 21 sit in GPU slots 0..4: the slot order is not the id order.
    pub(crate) fn two_community_result() -> CommunityDetectionResult {
        let c = |id: &str, nodes: Vec<u32>| Community {
            id: id.into(),
            nodes,
            internal_edges: 1,
            external_edges: 1,
            density: 1.0,
        };
        CommunityDetectionResult {
            node_labels: vec![0, 0, 1, 1],
            num_communities: 2,
            modularity: 0.25,
            iterations: 3,
            community_sizes: vec![2, 2],
            converged: true,
            communities: vec![c("0", vec![10, 11]), c("1", vec![20, 21])],
            stats: CommunityDetectionStats {
                total_communities: 2,
                modularity: 0.25,
                average_community_size: 2.0,
                largest_community: 2,
                smallest_community: 2,
                computation_time_ms: 1,
            },
            algorithm: CommunityDetectionAlgorithm::Louvain,
        }
    }

    fn pagerank_result() -> PageRankResult {
        PageRankResult {
            pagerank_values: vec![0.5, 0.5],
            iterations: 7,
            converged: true,
            convergence_value: 1e-7,
            top_nodes: vec![],
            stats: PageRankStats {
                total_nodes: 2,
                max_pagerank: 0.5,
                min_pagerank: 0.5,
                mean_pagerank: 0.5,
                median_pagerank: 0.5,
                std_deviation: 0.0,
                computation_time_ms: 1,
                converged: true,
                iterations: 7,
            },
        }
    }

    fn request() -> CommunityDetectionRequest {
        serde_json::from_value(serde_json::json!({ "algorithm": "louvain" })).unwrap()
    }

    #[actix_rt::test]
    async fn a_full_access_community_request_is_answered_by_the_gpu_route_in_graph_node_ids() {
        let gpu = StubGpu {
            community: Some(two_community_result()),
            ..Default::default()
        }
        .start();
        let r = run_community_detection_via(&routes(&gpu), &request())
            .await
            .expect("routed");
        assert!(r.success, "{:?}", r.error);
        let mut members: Vec<Vec<u32>> = r
            .communities
            .iter()
            .map(|c| {
                let mut n = c.nodes.clone();
                n.sort();
                n
            })
            .collect();
        members.sort();
        assert_eq!(members, vec![vec![10, 11], vec![20, 21]]);
        assert_eq!(r.total_communities, 2);
    }

    #[actix_rt::test]
    async fn full_access_pagerank_compute_cache_and_clear_all_reach_the_gpu_route() {
        let gpu = StubGpu {
            pagerank: Some(pagerank_result()),
            ..Default::default()
        }
        .start();
        let routes = routes(&gpu);
        let computed = whole_graph_pagerank(Some(&routes), PageRankParams::default())
            .await
            .expect("computed");
        assert!(computed.success && computed.result.is_some());
        let cached = whole_graph_cached_pagerank(Some(&routes))
            .await
            .expect("cached");
        assert!(cached.cached);
        whole_graph_clear_pagerank(Some(&routes))
            .await
            .expect("cleared");
        let after = whole_graph_cached_pagerank(Some(&routes))
            .await
            .expect("read");
        assert!(!after.cached && after.result.is_none());
    }

    #[actix_rt::test]
    async fn without_a_gpu_manager_the_whole_graph_routes_fail_rather_than_answer_empty() {
        assert!(whole_graph_pagerank(None, PageRankParams::default())
            .await
            .is_err());
        assert!(whole_graph_cached_pagerank(None).await.is_err());
        assert!(whole_graph_clear_pagerank(None).await.is_err());
    }
}
