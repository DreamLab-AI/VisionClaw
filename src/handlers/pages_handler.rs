use crate::actors::messages::{GetMetadata, GetSettings};
use crate::ok_json;
use crate::services::github::content_enhanced::ExtendedFileMetadata;
use crate::AppState;
use actix_web::{web, HttpResponse, Result};
use futures::future::join_all;
use serde::Serialize;
use visionclaw_domain::models::graph::GraphData;
use visionclaw_domain::models::metadata::{Metadata, MetadataStore};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    id: String,
    title: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<String>,
    modified: i64,
}

/// The metadata rows `/api/pages` may list for a caller. `view` is the caller's view when
/// something is hidden from them (`None` when nothing is). A row is then listed only when it
/// names a node in that view (row key or file name against the node's `metadata_id` or label,
/// `.md` trimmed, case-insensitive), so a hidden page reads exactly as one that does not exist
/// and a row with no node fails closed (ADR-060). The match is on the nodes, not on
/// `view.metadata`, because the live graph carries no metadata rows of its own.
fn visible_page_rows(store: &MetadataStore, view: Option<&GraphData>) -> Vec<(String, Metadata)> {
    let norm = |s: &str| s.trim().trim_end_matches(".md").to_lowercase();
    let names: Option<std::collections::HashSet<String>> = view.map(|v| {
        v.nodes
            .iter()
            .flat_map(|n| [norm(&n.metadata_id), norm(&n.label)])
            .filter(|s| !s.is_empty())
            .collect()
    });
    let mut rows: Vec<(String, Metadata)> = store
        .iter()
        .filter(|(k, m)| {
            names
                .as_ref()
                .is_none_or(|n| n.contains(&norm(k)) || n.contains(&norm(&m.file_name)))
        })
        .map(|(k, m)| (k.clone(), m.clone()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

pub async fn get_pages(
    app_state: web::Data<AppState>,
    caller: crate::settings::OptionalAuth,
) -> Result<HttpResponse> {
    let _settings = app_state
        .settings_addr
        .send(GetSettings)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!(
                "Settings actor mailbox error: {}",
                e
            ))
        })?
        .map_err(actix_web::error::ErrorInternalServerError)?;
    let debug_enabled = crate::utils::logging::is_debug_enabled();

    if debug_enabled {
        log::debug!("Starting pages retrieval");
    }

    let metadata = app_state
        .metadata_addr
        .send(GetMetadata)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!(
                "Metadata actor mailbox error: {}",
                e
            ))
        })?
        .map_err(actix_web::error::ErrorInternalServerError)?;

    // ADR-060: only the pages the caller may see. An unreadable graph fails closed.
    let gate = crate::handlers::graph_access::caller_gate(
        &app_state.graph_service_addr,
        crate::handlers::graph_access::caller_pubkey(&caller),
    )
    .await
    .map_err(actix_web::error::ErrorServiceUnavailable)?;
    let rows = visible_page_rows(&metadata, gate.as_ref().map(|g| g.view().as_ref()));

    if debug_enabled {
        log::debug!("Found {} metadata entries to process", rows.len());
    }

    let futures: Vec<_> = rows
        .iter()
        .map(|(id, meta)| {
            let content_api = app_state.content_api.clone();
            let file_name = meta.file_name.clone();
            let id = id.clone();
            let meta = meta.clone();

            async move {
                if debug_enabled {
                    log::debug!("Processing file: {} (ID: {})", file_name, id);
                }

                let extended_github_meta = content_api.get_file_metadata_extended(&file_name).await;

                match extended_github_meta {
                    Ok(file_meta) => {
                        if debug_enabled {
                            log::debug!(
                                "Found extended GitHub metadata for {}: {:?}",
                                file_name,
                                file_meta
                            );
                        }
                        Ok((id, meta, Some(file_meta)))
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to fetch extended GitHub metadata for {}: {}",
                            file_name,
                            e
                        );
                        Ok((id, meta, None))
                    }
                }
            }
        })
        .collect();

    if debug_enabled {
        log::debug!("Created {} futures for parallel processing", futures.len());
    }

    let results = join_all(futures).await;

    let pages: Vec<PageInfo> = results
        .into_iter()
        .filter_map(
            |result: Result<(String, Metadata, Option<ExtendedFileMetadata>), actix_web::Error>| {
                match result {
                    Ok((id, meta, github_meta)) => {
                        if debug_enabled {
                            log::debug!("Building page info for {} (ID: {})", meta.file_name, id);
                        }

                        let modified = github_meta
                            .map(|gm| gm.last_content_modified.timestamp())
                            .unwrap_or_else(|| {
                                if debug_enabled {
                                    log::debug!(
                                        "No modification time found for {}, using 0",
                                        meta.file_name
                                    );
                                }
                                0
                            });

                        Some(PageInfo {
                            id,
                            title: meta.file_name.clone(),
                            path: format!("/app/data/markdown/{}", meta.file_name),
                            parent: None,
                            modified,
                        })
                    }
                    Err(e) => {
                        log::error!("Failed to process page: {}", e);
                        None
                    }
                }
            },
        )
        .collect();

    if debug_enabled {
        log::debug!("Returning {} processed pages", pages.len());
    }

    ok_json!(pages)
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("").route(web::get().to(get_pages)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::graph_access::gate_for_graph_with;
    use std::sync::Arc;
    use visionclaw_domain::models::node::Node;

    fn row(name: &str) -> Metadata {
        Metadata {
            file_name: format!("{name}.md"),
            ..Metadata::default()
        }
    }

    /// `Public` and `Secret` pages; `Secret` has no `public` flag, so it is hidden from an
    /// anonymous caller. `with_secret = false` is the same corpus without the page at all.
    fn corpus(with_secret: bool) -> (MetadataStore, Arc<GraphData>) {
        let mut store = MetadataStore::new();
        let mut nodes = Vec::new();
        let names: &[&str] = if with_secret {
            &["Public", "Secret"]
        } else {
            &["Public"]
        };
        for (i, name) in names.iter().enumerate() {
            store.insert(format!("{name}.md"), row(name));
            let mut n = Node::new_with_id(format!("{name}.md"), None);
            n.id = i as u32;
            n.label = name.to_string();
            if *name == "Public" {
                n.metadata.insert("public".into(), "true".into());
            }
            nodes.push(n);
        }
        // The live graph carries no metadata rows: the match is on the nodes alone.
        (
            store,
            Arc::new(GraphData {
                nodes,
                ..GraphData::default()
            }),
        )
    }

    fn listed(with_secret: bool) -> String {
        let (store, graph) = corpus(with_secret);
        let gate = gate_for_graph_with(&graph, None, true);
        let rows = visible_page_rows(&store, gate.as_ref().map(|g| g.view().as_ref()));
        serde_json::to_string(
            &rows
                .iter()
                .map(|(k, m)| (k, &m.file_name))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn a_hidden_page_is_listed_exactly_as_a_page_that_does_not_exist() {
        assert!(
            !listed(true).contains("Secret"),
            "the hidden page's name must not be listed"
        );
        assert_eq!(listed(true), listed(false));
    }

    #[test]
    fn a_visible_page_is_listed_for_a_gated_caller() {
        assert!(
            listed(true).contains("Public"),
            "the visible page must still be listed"
        );
    }

    #[test]
    fn with_nothing_hidden_every_row_is_listed() {
        let (store, _) = corpus(true);
        assert_eq!(visible_page_rows(&store, None).len(), 2);
    }
}
