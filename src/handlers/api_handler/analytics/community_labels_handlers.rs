//! `GET /api/analytics/communities/labels` (ADR-2206).
//!
//! Per community of the detection run the client holds: its stable identity, the
//! core members a hull is drawn around, a deterministic c-TF-IDF label, the
//! domain mix and exemplar nodes. Everything is computed over the caller's view
//! (`graph_access::caller_view`), so a node the caller cannot see contributes
//! nothing: no term, no count, no exemplar, no IDF weight.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use actix_web::{web, HttpResponse, Result};
use log::error;
use serde::Serialize;
use visionclaw_domain::models::edge::EdgeProvenance;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::utils::visibility_filter::visible_subgraph;

use crate::actors::messages::GetGraphData;
use crate::services::community_labels::{
    build_labels, registry, CommunityLabel, EdgeInput, LabelParams, MemberInput, StableRegistry,
};
use crate::utils::binary_protocol::NODE_ID_MASK;
use crate::{error_json, ok_json, AppState};

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DomainShareDto {
    pub domain: String,
    pub share: f32,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExemplarDto {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLabelDto {
    pub community_id: u32,
    pub stable_id: u64,
    pub size: usize,
    pub core_size: usize,
    pub core_ids: Vec<u32>,
    pub label: String,
    pub terms: Vec<String>,
    pub domain_mix: Vec<DomainShareDto>,
    pub cross_domain: bool,
    pub exemplars: Vec<ExemplarDto>,
    pub has_hull: bool,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommunityLabelsResponse {
    /// Detection runs observed since start. A client holding community ids from
    /// an older run must not apply labels of a newer one (it compares `size`).
    pub generation: u64,
    pub communities: Vec<CommunityLabelDto>,
}

impl CommunityLabelDto {
    fn from_label(l: CommunityLabel, stable_id: u64) -> Self {
        Self {
            community_id: l.community_id,
            stable_id,
            size: l.size,
            core_size: l.core_size,
            core_ids: l.core_ids,
            label: l.label,
            terms: l.terms,
            domain_mix: l.domain_mix.into_iter().map(|d| DomainShareDto { domain: d.domain, share: d.share }).collect(),
            cross_domain: l.cross_domain,
            exemplars: l.exemplars.into_iter().map(|e| ExemplarDto { id: e.id, label: e.label }).collect(),
            has_hull: l.has_hull,
        }
    }
}

/// An ontology class (the only kind of node whose text and domain describe a
/// community): `type == ontology_node` with a domain. Pages, agents and domain
/// roots are not.
fn domain_of(n: &visionclaw_domain::models::node::Node) -> Option<String> {
    n.metadata
        .get("domain")
        .or_else(|| n.metadata.get("source_domain"))
        .filter(|d| !d.is_empty())
        .cloned()
}

/// Labels over `graph` as seen by `pubkey` (`None` = anonymous).
///
/// `community_of` maps a graph node id to the wire community id (0 = none).
pub fn labels_for_view(
    graph: &Arc<GraphData>,
    pubkey: Option<&str>,
    filter_on: bool,
    community_of: &dyn Fn(u32) -> u32,
    stable: &StableRegistry,
    params: &LabelParams,
) -> CommunityLabelsResponse {
    let view = visible_subgraph(graph, pubkey, filter_on);
    let members: Vec<MemberInput> = view
        .nodes
        .iter()
        .map(|n| {
            let ty = n.metadata.get("type").map(String::as_str);
            let domain = domain_of(n);
            MemberInput {
                id: n.id,
                community: community_of(n.id),
                label: n.label.clone(),
                definition: n.metadata.get("definition").cloned().unwrap_or_default(),
                is_content: ty == Some("ontology_node") && domain.is_some(),
                is_domain_root: ty == Some("domain_root"),
                domain,
                pos: Some([n.data.x, n.data.y, n.data.z]),
            }
        })
        .collect();
    let edges: Vec<EdgeInput> = view
        .edges
        .iter()
        .map(|e| EdgeInput { a: e.source, b: e.target, asserted: e.provenance == Some(EdgeProvenance::Asserted) })
        .collect();
    let communities = build_labels(&members, &edges, params)
        .into_iter()
        .map(|l| {
            let sid = stable.stable_id(l.community_id).unwrap_or(l.community_id as u64);
            CommunityLabelDto::from_label(l, sid)
        })
        .collect();
    CommunityLabelsResponse { generation: stable.generation(), communities }
}

const CACHE_TTL: Duration = Duration::from_secs(20);
const CACHE_MAX: usize = 16;
type CacheKey = (u64, Option<String>);

fn cache() -> &'static Mutex<HashMap<CacheKey, (Instant, Arc<CommunityLabelsResponse>)>> {
    static C: OnceLock<Mutex<HashMap<CacheKey, (Instant, Arc<CommunityLabelsResponse>)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn get_community_labels(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    let graph = match app_state.graph_service_addr.send(GetGraphData).await {
        Ok(Ok(g)) => g,
        Ok(Err(e)) => {
            error!("community labels: graph unavailable: {}", e);
            return error_json!(format!("Graph unavailable: {}", e));
        }
        Err(e) => {
            error!("community labels: graph actor mailbox error: {}", e);
            return error_json!(format!("Graph actor unavailable: {}", e));
        }
    };
    let pubkey = crate::handlers::graph_access::caller_pubkey(&caller).map(str::to_owned);
    let generation = registry().lock().map(|r| r.generation()).unwrap_or(0);
    let key: CacheKey = (generation, pubkey.clone());
    if let Ok(c) = cache().lock() {
        if let Some((at, hit)) = c.get(&key) {
            if at.elapsed() < CACHE_TTL {
                return ok_json!(hit.as_ref());
            }
        }
    }
    let analytics = Arc::clone(&app_state.node_analytics);
    let computed = web::block(move || {
        let ids: HashMap<u32, u32> = analytics
            .read()
            .map(|m| m.iter().map(|(&id, a)| (id, a.community_id)).collect())
            .unwrap_or_default();
        let reg = registry().lock().map_err(|_| "registry poisoned".to_string())?;
        Ok::<_, String>(labels_for_view(
            &graph,
            pubkey.as_deref(),
            visionclaw_domain::utils::visibility_filter::filter_enabled(),
            &|id| ids.get(&(id & NODE_ID_MASK)).copied().unwrap_or(0),
            &reg,
            &LabelParams::default(),
        ))
    })
    .await;
    match computed {
        Ok(Ok(resp)) => {
            let resp = Arc::new(resp);
            if let Ok(mut c) = cache().lock() {
                if c.len() >= CACHE_MAX {
                    c.clear();
                }
                c.insert(key, (Instant::now(), Arc::clone(&resp)));
            }
            ok_json!(resp.as_ref())
        }
        Ok(Err(e)) => error_json!(format!("Community labels unavailable: {}", e)),
        Err(e) => error_json!(format!("Community labels task failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use visionclaw_domain::models::edge::Edge;
    use visionclaw_domain::models::node::Node;

    fn class_node(id: u32, label: &str, def: &str, public: bool, x: f32) -> Node {
        let mut n = Node::new_with_id(format!("n{id}"), None);
        n.id = id;
        n.label = label.into();
        (n.data.x, n.data.y, n.data.z) = (x, 0.0, 0.0);
        n.metadata.insert("type".into(), "ontology_node".into());
        n.metadata.insert("domain".into(), "ai".into());
        n.metadata.insert("definition".into(), def.into());
        if public {
            n.metadata.insert("public".into(), "true".into());
        } else {
            n.metadata.insert("owner_pubkey".into(), "owner-key".into());
        }
        n
    }

    fn fixture() -> Arc<GraphData> {
        let mut nodes: Vec<Node> = (1..=6)
            .map(|i| class_node(i, "Agent", "agent agents coordinate", true, i as f32))
            .collect();
        // Private nodes whose unique term would top the ranking if it leaked.
        for i in 7..=12 {
            nodes.push(class_node(i, "Zebrafish", "zebrafish zebrafish zebrafish", false, i as f32));
        }
        Arc::new(GraphData {
            nodes,
            edges: vec![Edge::new(1, 2, 1.0).with_provenance(EdgeProvenance::Asserted)],
            ..GraphData::default()
        })
    }

    fn params() -> LabelParams {
        LabelParams { k: 3, hull_min_core: 2, min_corpus_count: 2, min_community_count: 2, ..Default::default() }
    }

    fn registry_for(g: &GraphData) -> StableRegistry {
        let mut r = StableRegistry::new();
        let a: Vec<(u32, u32)> = g.nodes.iter().map(|n| (n.id, 1)).collect();
        r.observe(&a);
        r
    }

    #[test]
    fn a_private_node_contributes_nothing_to_an_anonymous_callers_labels() {
        let g = fixture();
        let reg = registry_for(&g);
        let anon = labels_for_view(&g, None, true, &|_| 1, &reg, &params());
        assert_eq!(anon.communities.len(), 1);
        let c = &anon.communities[0];
        assert_eq!(c.size, 6, "hidden members are not counted");
        assert!(!c.label.to_lowercase().contains("zebrafish"), "{}", c.label);
        assert!(c.terms.iter().all(|t| !t.contains("zebrafish")));
        assert!(c.core_ids.iter().all(|&id| id <= 6), "hidden ids never named: {:?}", c.core_ids);
        assert!(c.exemplars.iter().all(|e| e.id <= 6));
    }

    #[test]
    fn the_owner_sees_their_private_nodes_and_the_filter_off_sees_all() {
        let g = fixture();
        let reg = registry_for(&g);
        let owner = labels_for_view(&g, Some("owner-key"), true, &|_| 1, &reg, &params());
        assert_eq!(owner.communities[0].size, 12);
        assert!(owner.communities[0].terms.iter().any(|t| t.contains("zebrafish")));
        let off = labels_for_view(&g, None, false, &|_| 1, &reg, &params());
        assert_eq!(off.communities[0].size, 12);
        let stranger = labels_for_view(&g, Some("someone-else"), true, &|_| 1, &reg, &params());
        assert_eq!(stranger.communities[0].size, 6);
    }

    #[test]
    fn response_carries_generation_stable_id_and_camel_case_wire_names() {
        let g = fixture();
        let reg = registry_for(&g);
        let r = labels_for_view(&g, None, true, &|_| 1, &reg, &params());
        assert_eq!(r.generation, 1);
        assert_eq!(r.communities[0].stable_id, 1);
        let v = serde_json::to_value(&r).unwrap();
        let c = &v["communities"][0];
        for k in ["communityId", "stableId", "size", "coreSize", "coreIds", "label", "terms", "domainMix", "crossDomain", "exemplars", "hasHull"] {
            assert!(c.get(k).is_some(), "missing {k}");
        }
        assert!(v.get("generation").is_some());
    }

    #[test]
    fn pages_and_domain_roots_do_not_describe_a_community() {
        let mut g = (*fixture()).clone();
        for n in g.nodes.iter_mut().filter(|n| n.id <= 6) {
            n.metadata.insert("type".into(), "page".into());
        }
        let g = Arc::new(g);
        let reg = registry_for(&g);
        let r = labels_for_view(&g, None, true, &|_| 1, &reg, &params());
        assert!(r.communities[0].terms.is_empty());
        assert!(r.communities[0].domain_mix.is_empty());
    }
}
