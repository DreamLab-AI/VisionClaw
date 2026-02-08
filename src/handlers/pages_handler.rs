use crate::actors::messages::{GetMetadata, GetSettings};
use crate::models::metadata:::Metadata;
use crate::services::github::content_enhanced:::ExtendedFileMetadata;
use crate::ok_json;
use crate:::AppState;
use actix_web::{web, HttpResponse, Result};
use futures::future:::join_all;
use serde:::Serialize;

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

pub async fn get_pages(app_state: web::Data<AppState>) -> Result<HttpResponse> {
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
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;
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
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;

    if debug_enabled {
        log::debug!("Found {} metadata entries to process", metadata.len());
    }

    let futures: Vec<_> = metadata
        .iter()
        .map(|(id, meta)| {
            let content_api = app_state.content_api.clone();
            let file_name = meta.file_name.clone();
            let id = id.clone();
            let meta = meta.clone();
            let debug_enabled = debug_enabled;

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

