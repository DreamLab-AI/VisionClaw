//! Solid Pod Handler — embedded solid-pod-rs backend
//!
//! Serves Solid Protocol resources directly via `solid_pod_rs::storage::fs::FsBackend`
//! instead of HTTP-proxying to an external JavaScript Solid Server (JSS).
//!
//! Routes:
//!   /solid/health           — storage readiness check
//!   /solid/.notifications   — WebSocket upgrade (solid-0.1 protocol)
//!   /solid/pods             — pod creation
//!   /solid/pods/check       — check pod existence
//!   /solid/pods/init        — pod auto-provisioning (Bearer auth)
//!   /solid/pods/init-nip98  — pod auto-provisioning (NIP-98 auth)
//!   /solid/{tail:.*}        — LDP CRUD (GET, PUT, POST, DELETE, PATCH, HEAD)
//!   /.well-known/did.json   — DID document (did:web)
//!   /did/{tail:.*}          — DID resolution (did:nostr)
//!
//! Features:
//! - NIP-98 authentication (BIP-340 Schnorr) via solid_pod_rs::auth::nip98
//! - WAC ACL enforcement via solid_pod_rs::wac::evaluate_access
//! - Pod auto-provisioning via solid_pod_rs::provision::provision_pod
//! - WebSocket notifications (solid-0.1 protocol)
//! - DID resolution via solid_pod_rs::interop::did_nostr
//! - Content negotiation (JSON-LD, Turtle)

#[cfg(feature = "solid-pod-embed")]
use actix_web::http::Method;
use actix_web::{web, HttpRequest, HttpResponse};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[cfg(feature = "solid-pod-embed")]
use std::sync::Arc;

// ADR-2067: `NostrUser`/`NostrService` are only reached by the
// `solid-pod-embed` handlers below (`handle_solid_proxy`, `create_pod`,
// `check_pod_exists`, `init_pod`, `get_user_from_request`) — the feature-off
// `configure_routes` registers nothing, so these are otherwise unused there.
#[cfg(feature = "solid-pod-embed")]
use crate::models::protected_settings::NostrUser;
#[cfg(feature = "solid-pod-embed")]
use crate::services::nostr_service::NostrService;
use crate::utils::nip98::validate_nip98_token;
use nostr_sdk::Keys;
#[cfg(feature = "solid-pod-embed")]
use nostr_sdk::{PublicKey, ToBech32};

#[cfg(feature = "solid-pod-embed")]
use crate::services::ontology_generation::PublishedStorage as FsBackend;
#[cfg(feature = "solid-pod-embed")]
use bytes::Bytes;
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::error::PodError;
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::ldp::{
    self, apply_n3_patch, apply_patch_to_absent, apply_sparql_patch, cache_control_for,
    evaluate_preconditions, is_container, link_headers, negotiate_format, not_found_headers,
    patch_dialect_from_mime, render_container_jsonld, render_container_turtle, resolve_slug,
    vary_header, ConditionalOutcome, PatchDialect, PreferHeader, ACCEPT_PATCH, ACCEPT_POST,
};
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::provision::{provision_pod, ProvisionPlan};
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::wac::{evaluate_access, method_to_mode, AccessMode};
#[cfg(feature = "solid-pod-embed")]
use solid_pod_rs::Storage;

/// Response from pod creation
#[derive(Debug, Serialize, Deserialize)]
pub struct PodCreationResponse {
    pub pod_url: String,
    pub webid: String,
    pub created: bool,
    pub structure: PodStructure,
}

/// Pod directory structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodStructure {
    /// User's WebID profile card
    pub profile: String,
    /// Ontology contributions directory
    pub ontology_contributions: String,
    /// Ontology proposals directory
    pub ontology_proposals: String,
    /// Ontology annotations directory
    pub ontology_annotations: String,
    /// User preferences
    pub preferences: String,
    /// Notifications inbox
    pub inbox: String,
}

/// Error response structure
#[derive(Debug, Serialize)]
pub struct SolidProxyError {
    pub error: String,
    pub details: Option<String>,
}

/// Result of extracting user identity from request
#[derive(Debug, Clone)]
pub struct UserIdentity {
    /// User's Nostr public key (hex)
    pub pubkey: String,
    /// Original NIP-98 token to forward
    pub nip98_token: String,
    /// Full Authorization header value
    pub auth_header: String,
}

/// Shared state for the embedded Solid pod backend
pub struct SolidPodState {
    /// Filesystem-backed Solid storage
    #[cfg(feature = "solid-pod-embed")]
    pub storage: Arc<FsBackend>,
    /// Pod data root directory
    pub data_root: PathBuf,
    /// Server-side signing key for anonymous requests
    pub server_keys: Option<Keys>,
    /// Whether anonymous requests are allowed
    pub allow_anonymous: bool,
}

impl SolidPodState {
    /// Create a new SolidPodState. Must be called from an async context
    /// because FsBackend::new is async.
    #[cfg(feature = "solid-pod-embed")]
    pub async fn new_async() -> Self {
        let data_root = PathBuf::from(
            std::env::var("SOLID_DATA_ROOT").unwrap_or_else(|_| "/data/solid".to_string()),
        );

        let server_keys = std::env::var("SOLID_PROXY_SECRET_KEY")
            .ok()
            .and_then(|hex| nostr_sdk::SecretKey::from_hex(&hex).ok().map(Keys::new));

        let allow_anonymous = std::env::var("SOLID_ALLOW_ANONYMOUS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let storage = match FsBackend::new(&data_root).await {
            Ok(fs) => Arc::new(fs),
            Err(e) => {
                error!(
                    "Failed to create FsBackend at {}: {}. Falling back to temp dir.",
                    data_root.display(),
                    e
                );
                let fallback = std::env::temp_dir().join("solid-pod-fallback");
                Arc::new(
                    FsBackend::new(&fallback)
                        .await
                        .expect("fallback FsBackend creation must succeed"),
                )
            }
        };

        if server_keys.is_some() {
            info!("Solid pod backend initialized with server-side signing key (for anonymous fallback)");
        } else {
            info!("Solid pod backend initialized without server-side signing");
        }

        if allow_anonymous {
            info!("Anonymous Solid requests enabled (will use server identity)");
        } else {
            info!("Anonymous Solid requests disabled (user auth required)");
        }

        info!("Solid storage root: {}", data_root.display());

        Self {
            storage,
            data_root,
            server_keys,
            allow_anonymous,
        }
    }

    // ADR-2067: the synchronous `SolidPodState::new()` stub constructor for
    // feature-off builds used to live here. Its only caller was the
    // feature-off `configure_routes`, which no longer constructs a
    // `SolidPodState` at all (it registers no routes and no app_data).
    // Removed as dead code.

    /// Extract and verify user identity from NIP-98 Authorization header.
    /// Validates the NIP-98 signature, timestamp, URL, and method.
    pub fn extract_user_identity(&self, req: &HttpRequest) -> Option<UserIdentity> {
        let auth_header = req.headers().get("Authorization")?;
        let auth_str = auth_header.to_str().ok()?;

        if !auth_str.starts_with("Nostr ") {
            debug!("Authorization header is not NIP-98 format");
            return None;
        }

        let token = &auth_str[6..]; // Skip "Nostr "

        // Reconstruct the request URL for NIP-98 validation.
        // Behind a TLS-terminating proxy, prefer X-Forwarded-* headers.
        let conn_info = req.connection_info();
        let scheme = req
            .headers()
            .get("X-Forwarded-Proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.scheme());
        let host = req
            .headers()
            .get("X-Forwarded-Host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.host());
        let path = req
            .headers()
            .get("X-Forwarded-URI")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| {
                req.uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            });
        let expected_url = format!("{}://{}{}", scheme, host, path);
        let expected_method = req.method().as_str();

        match validate_nip98_token(token, &expected_url, expected_method, None) {
            Ok(validation) => {
                debug!(
                    "Verified NIP-98 user identity: pubkey={}...",
                    &validation.pubkey[..16.min(validation.pubkey.len())]
                );
                Some(UserIdentity {
                    pubkey: validation.pubkey,
                    nip98_token: token.to_string(),
                    auth_header: auth_str.to_string(),
                })
            }
            Err(e) => {
                warn!("NIP-98 token validation failed: {}", e);
                None
            }
        }
    }

    /// Authenticate a request and return the agent WebID (did:nostr:<pubkey>)
    /// or None for anonymous. Returns Err for auth-required-but-missing.
    fn authenticate_request(&self, req: &HttpRequest) -> Result<Option<String>, HttpResponse> {
        if let Some(identity) = self.extract_user_identity(req) {
            Ok(Some(format!("did:nostr:{}", identity.pubkey)))
        } else if self.allow_anonymous {
            Ok(None)
        } else {
            Err(HttpResponse::Unauthorized().json(SolidProxyError {
                error: "Authentication required".to_string(),
                details: Some("NIP-98 Authorization header required for Solid access".to_string()),
            }))
        }
    }

    /// Resolve the storage path for a given request path within a pod.
    /// Converts /solid/<npub>/rest/of/path to /<npub>/rest/of/path for storage.
    fn storage_path(target_path: &str) -> String {
        let normalized = if target_path.starts_with('/') {
            target_path.to_string()
        } else {
            format!("/{}", target_path)
        };
        normalized
    }

    /// Build pod base URL from request info
    fn pod_base_url(req: &HttpRequest) -> String {
        let conn_info = req.connection_info();
        let scheme = req
            .headers()
            .get("X-Forwarded-Proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.scheme());
        let host = req
            .headers()
            .get("X-Forwarded-Host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.host());
        format!("{}://{}/solid", scheme, host)
    }

    /// Build a PodStructure for a given npub
    fn pod_structure(pod_base_url: &str, npub: &str) -> PodStructure {
        let pod = format!("{}/{}", pod_base_url, npub);
        PodStructure {
            profile: format!("{}/profile/card#me", pod),
            ontology_contributions: format!("{}/ontology/contributions/", pod),
            ontology_proposals: format!("{}/ontology/proposals/", pod),
            ontology_annotations: format!("{}/ontology/annotations/", pod),
            preferences: format!("{}/preferences/", pod),
            inbox: format!("{}/inbox/", pod),
        }
    }
}

// ============================================================================
// LDP CRUD Handler
// ============================================================================

/// Main handler for all /solid/* LDP routes.
///
/// Authentication flow:
/// 1. If user has NIP-98 Authorization header -> extract identity, enforce WAC
/// 2. If no user auth AND anonymous allowed -> proceed with public ACL check
/// 3. If no user auth AND anonymous NOT allowed -> Return 401
#[cfg(feature = "solid-pod-embed")]
pub async fn handle_solid_proxy(
    req: HttpRequest,
    body: web::Bytes,
    path: web::Path<String>,
    state: web::Data<SolidPodState>,
    _nostr_service: web::Data<NostrService>,
) -> HttpResponse {
    let target_path = path.into_inner();
    let method = req.method().clone();
    let storage_path = SolidPodState::storage_path(&target_path);

    debug!("Solid LDP: {} /solid/{}", method, target_path);

    // Authenticate
    let agent = match state.authenticate_request(&req) {
        Ok(agent) => agent,
        Err(resp) => return resp,
    };

    if let Some(ref a) = agent {
        debug!("Authenticated agent: {}...", &a[..20.min(a.len())]);
    }

    // Determine the required WAC access mode
    let access_mode = method_to_mode(method.as_str());

    // Try to load the ACL for this resource.
    // WAC lookup: check for .acl sidecar, walk up to parent containers.
    let authorization_path = FsBackend::authorization_path(&storage_path);
    let acl_doc = load_acl_for_path(&state.storage, &authorization_path).await;

    // Evaluate WAC access
    let allowed = evaluate_access(
        acl_doc.as_ref(),
        agent.as_deref(),
        &authorization_path,
        access_mode,
        None,
    );

    if !allowed {
        if agent.is_none() {
            return HttpResponse::Unauthorized().json(SolidProxyError {
                error: "Authentication required".to_string(),
                details: Some("Resource requires authentication".to_string()),
            });
        }
        return HttpResponse::Forbidden().json(SolidProxyError {
            error: "Access denied".to_string(),
            details: Some(format!(
                "WAC denies {:?} access to {}",
                access_mode, storage_path
            )),
        });
    }

    // Dispatch by HTTP method
    match method.as_str() {
        "GET" => handle_get(&state.storage, &req, &storage_path).await,
        "HEAD" => handle_head(&state.storage, &storage_path).await,
        "PUT" => handle_put(&state.storage, &req, &storage_path, body).await,
        "POST" => handle_post(&state.storage, &req, &storage_path, body).await,
        "DELETE" => handle_delete(&state.storage, &storage_path).await,
        "PATCH" => handle_patch(&state.storage, &req, &storage_path, body).await,
        _ => HttpResponse::MethodNotAllowed().json(SolidProxyError {
            error: "Method not allowed".to_string(),
            details: Some(format!("Unsupported method: {}", method)),
        }),
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `handle_solid_proxy` used to live here. Now that `configure_routes` registers
// no `/solid` routes at all when the feature is off (see the bottom of this
// file), nothing can ever dispatch to it, so it has been removed.

// ============================================================================
// LDP Method Implementations
// ============================================================================

#[cfg(feature = "solid-pod-embed")]
async fn handle_get(storage: &Arc<FsBackend>, req: &HttpRequest, path: &str) -> HttpResponse {
    // Check if this is a container listing
    if is_container(path) {
        return handle_get_container(storage, req, path).await;
    }

    match storage.get(path).await {
        Ok((body, meta)) => {
            // Evaluate preconditions (If-Match, If-None-Match)
            let if_match = req.headers().get("if-match").and_then(|v| v.to_str().ok());
            let if_none_match = req
                .headers()
                .get("if-none-match")
                .and_then(|v| v.to_str().ok());

            match evaluate_preconditions("GET", Some(&meta.etag), if_match, if_none_match) {
                ConditionalOutcome::Proceed => {}
                ConditionalOutcome::NotModified => {
                    return HttpResponse::NotModified()
                        .insert_header(("ETag", format!("\"{}\"", meta.etag)))
                        .finish();
                }
                ConditionalOutcome::PreconditionFailed => {
                    return HttpResponse::PreconditionFailed().finish();
                }
            }

            let mut resp = HttpResponse::Ok();
            resp.insert_header(("Content-Type", meta.content_type.as_str()));
            resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
            resp.insert_header((
                "Last-Modified",
                meta.modified
                    .format("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string(),
            ));

            // LDP Link headers
            for link in link_headers(path) {
                resp.insert_header(("Link", link));
            }

            // WAC-Allow (simplified: advertise full modes for now)
            resp.insert_header((
                "WAC-Allow",
                "user=\"read write append control\",public=\"read\"",
            ));
            resp.insert_header(("Accept-Patch", ACCEPT_PATCH));
            resp.insert_header(("Accept-Post", ACCEPT_POST));

            if let Some(cc) = cache_control_for(&meta.content_type) {
                resp.insert_header(("Cache-Control", cc));
            }
            resp.insert_header(("Vary", vary_header(true)));

            resp.body(body)
        }
        Err(PodError::NotFound(_)) => {
            let mut resp = HttpResponse::NotFound();
            for (k, v) in not_found_headers(path, true) {
                resp.insert_header((k, v));
            }
            resp.json(SolidProxyError {
                error: "Not found".to_string(),
                details: Some(format!("Resource not found: {}", path)),
            })
        }
        Err(e) => {
            error!("Storage GET error for {}: {}", path, e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Storage error".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_get_container(
    storage: &Arc<FsBackend>,
    req: &HttpRequest,
    path: &str,
) -> HttpResponse {
    // List container children
    match storage.list(path).await {
        Ok(members) => {
            let accept = req.headers().get("accept").and_then(|v| v.to_str().ok());
            let format = negotiate_format(accept);
            let prefer = req
                .headers()
                .get("prefer")
                .and_then(|v| v.to_str().ok())
                .map(PreferHeader::parse)
                .unwrap_or_default();

            let (content_type, body_str) = match format {
                ldp::RdfFormat::Turtle => {
                    let turtle = render_container_turtle(path, &members, prefer);
                    ("text/turtle", turtle)
                }
                _ => {
                    let json = render_container_jsonld(path, &members, prefer);
                    (
                        "application/ld+json",
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    )
                }
            };

            let mut resp = HttpResponse::Ok();
            resp.insert_header(("Content-Type", content_type));
            for link in link_headers(path) {
                resp.insert_header(("Link", link));
            }
            resp.insert_header((
                "WAC-Allow",
                "user=\"read write append control\",public=\"read\"",
            ));
            resp.insert_header(("Accept-Post", ACCEPT_POST));
            resp.insert_header(("Vary", vary_header(true)));

            resp.body(body_str)
        }
        Err(PodError::NotFound(_)) => HttpResponse::NotFound().json(SolidProxyError {
            error: "Container not found".to_string(),
            details: Some(format!("Container not found: {}", path)),
        }),
        Err(e) => {
            error!("Storage LIST error for {}: {}", path, e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Storage error".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_head(storage: &Arc<FsBackend>, path: &str) -> HttpResponse {
    match storage.head(path).await {
        Ok(meta) => {
            let mut resp = HttpResponse::Ok();
            resp.insert_header(("Content-Type", meta.content_type.as_str()));
            resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
            resp.insert_header((
                "Last-Modified",
                meta.modified
                    .format("%a, %d %b %Y %H:%M:%S GMT")
                    .to_string(),
            ));
            resp.insert_header(("Content-Length", meta.size.to_string()));
            for link in link_headers(path) {
                resp.insert_header(("Link", link));
            }
            resp.insert_header(("Accept-Patch", ACCEPT_PATCH));
            resp.insert_header(("Vary", vary_header(true)));
            resp.finish()
        }
        Err(PodError::NotFound(_)) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!("Storage HEAD error for {}: {}", path, e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_put(
    storage: &Arc<FsBackend>,
    req: &HttpRequest,
    path: &str,
    body: web::Bytes,
) -> HttpResponse {
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    // Check if this is a container creation (Link: <ldp:BasicContainer>)
    let is_container_create = req
        .headers()
        .get("link")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("ldp#Container") || v.contains("ldp#BasicContainer"))
        .unwrap_or(false);

    if is_container_create {
        match storage.create_container(path).await {
            Ok(meta) => {
                let mut resp = HttpResponse::Created();
                resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                for link in link_headers(path) {
                    resp.insert_header(("Link", link));
                }
                resp.finish()
            }
            Err(PodError::AlreadyExists(_)) => HttpResponse::Conflict().json(SolidProxyError {
                error: "Container already exists".to_string(),
                details: None,
            }),
            Err(e) => {
                error!("Storage create_container error for {}: {}", path, e);
                HttpResponse::InternalServerError().json(SolidProxyError {
                    error: "Storage error".to_string(),
                    details: Some(e.to_string()),
                })
            }
        }
    } else {
        // Check preconditions
        let existing_etag = storage.head(path).await.ok().map(|m| m.etag);
        let if_match = req.headers().get("if-match").and_then(|v| v.to_str().ok());
        let if_none_match = req
            .headers()
            .get("if-none-match")
            .and_then(|v| v.to_str().ok());

        match evaluate_preconditions("PUT", existing_etag.as_deref(), if_match, if_none_match) {
            ConditionalOutcome::Proceed => {}
            ConditionalOutcome::NotModified => {
                return HttpResponse::NotModified().finish();
            }
            ConditionalOutcome::PreconditionFailed => {
                return HttpResponse::PreconditionFailed().finish();
            }
        }

        match storage
            .put(path, Bytes::from(body.to_vec()), content_type)
            .await
        {
            Ok(meta) => {
                let mut resp = HttpResponse::Created();
                resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                resp.insert_header(("Content-Type", meta.content_type.as_str()));
                for link in link_headers(path) {
                    resp.insert_header(("Link", link));
                }
                resp.finish()
            }
            Err(e) => {
                error!("Storage PUT error for {}: {}", path, e);
                HttpResponse::InternalServerError().json(SolidProxyError {
                    error: "Storage error".to_string(),
                    details: Some(e.to_string()),
                })
            }
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_post(
    storage: &Arc<FsBackend>,
    req: &HttpRequest,
    container_path: &str,
    body: web::Bytes,
) -> HttpResponse {
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let slug = req.headers().get("slug").and_then(|v| v.to_str().ok());

    // Resolve the child path from the Slug header
    let child_path = match resolve_slug(container_path, slug) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(SolidProxyError {
                error: "Invalid Slug".to_string(),
                details: Some(e.to_string()),
            });
        }
    };

    // Check if this is a container creation
    let is_container_create = req
        .headers()
        .get("link")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("ldp#Container") || v.contains("ldp#BasicContainer"))
        .unwrap_or(false);

    if is_container_create {
        match storage.create_container(&child_path).await {
            Ok(meta) => {
                let mut resp = HttpResponse::Created();
                resp.insert_header(("Location", child_path.as_str()));
                resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                for link in link_headers(&child_path) {
                    resp.insert_header(("Link", link));
                }
                resp.finish()
            }
            Err(PodError::AlreadyExists(_)) => HttpResponse::Conflict().json(SolidProxyError {
                error: "Resource already exists".to_string(),
                details: None,
            }),
            Err(e) => {
                error!("Storage POST container error for {}: {}", child_path, e);
                HttpResponse::InternalServerError().json(SolidProxyError {
                    error: "Storage error".to_string(),
                    details: Some(e.to_string()),
                })
            }
        }
    } else {
        match storage
            .put(&child_path, Bytes::from(body.to_vec()), content_type)
            .await
        {
            Ok(meta) => {
                let mut resp = HttpResponse::Created();
                resp.insert_header(("Location", child_path.as_str()));
                resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                for link in link_headers(&child_path) {
                    resp.insert_header(("Link", link));
                }
                resp.finish()
            }
            Err(e) => {
                error!("Storage POST error for {}: {}", child_path, e);
                HttpResponse::InternalServerError().json(SolidProxyError {
                    error: "Storage error".to_string(),
                    details: Some(e.to_string()),
                })
            }
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_delete(storage: &Arc<FsBackend>, path: &str) -> HttpResponse {
    match storage.delete(path).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(PodError::NotFound(_)) => HttpResponse::NotFound().json(SolidProxyError {
            error: "Not found".to_string(),
            details: Some(format!("Resource not found: {}", path)),
        }),
        Err(e) => {
            error!("Storage DELETE error for {}: {}", path, e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Storage error".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

#[cfg(feature = "solid-pod-embed")]
async fn handle_patch(
    storage: &Arc<FsBackend>,
    req: &HttpRequest,
    path: &str,
    body: web::Bytes,
) -> HttpResponse {
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let dialect = match patch_dialect_from_mime(content_type) {
        Some(d) => d,
        None => {
            return HttpResponse::UnsupportedMediaType().json(SolidProxyError {
                error: "Unsupported patch format".to_string(),
                details: Some(format!(
                    "Content-Type '{}' is not a supported patch format. Use: {}",
                    content_type, ACCEPT_PATCH
                )),
            });
        }
    };

    let patch_str = match std::str::from_utf8(&body) {
        Ok(s) => s.to_string(),
        Err(e) => {
            return HttpResponse::BadRequest().json(SolidProxyError {
                error: "Invalid patch body".to_string(),
                details: Some(format!("Body is not valid UTF-8: {}", e)),
            });
        }
    };

    // Fetch current resource (may not exist for insert-only patches)
    let current = storage.get(path).await;

    match current {
        Ok((current_body, _meta)) => {
            // Parse current body as N-Triples graph for RDF patches
            let current_str = String::from_utf8_lossy(&current_body).to_string();
            let graph = match ldp::Graph::parse_ntriples(&current_str) {
                Ok(g) => g,
                Err(_) => ldp::Graph::new(), // Non-RDF resource, start empty
            };

            let outcome = match dialect {
                PatchDialect::N3 => apply_n3_patch(graph, &patch_str),
                PatchDialect::SparqlUpdate => apply_sparql_patch(graph, &patch_str),
                PatchDialect::JsonPatch => {
                    // JSON Patch on RDF resources not supported via this path
                    return HttpResponse::UnsupportedMediaType().json(SolidProxyError {
                        error: "JSON Patch not supported for RDF resources".to_string(),
                        details: None,
                    });
                }
            };

            match outcome {
                Ok(patch_out) => {
                    let new_body = patch_out.graph.to_ntriples();
                    match storage
                        .put(
                            path,
                            Bytes::from(new_body.into_bytes()),
                            "application/n-triples",
                        )
                        .await
                    {
                        Ok(meta) => {
                            let mut resp = HttpResponse::Ok();
                            resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                            resp.finish()
                        }
                        Err(e) => {
                            error!("Storage PUT after PATCH error for {}: {}", path, e);
                            HttpResponse::InternalServerError().json(SolidProxyError {
                                error: "Storage error".to_string(),
                                details: Some(e.to_string()),
                            })
                        }
                    }
                }
                Err(e) => HttpResponse::UnprocessableEntity().json(SolidProxyError {
                    error: "Patch failed".to_string(),
                    details: Some(e.to_string()),
                }),
            }
        }
        Err(PodError::NotFound(_)) => {
            // Resource does not exist — apply patch to absent resource
            match apply_patch_to_absent(dialect, &patch_str) {
                Ok(create_outcome) => {
                    let new_graph = match create_outcome {
                        ldp::PatchCreateOutcome::Created { graph, .. } => graph,
                        ldp::PatchCreateOutcome::Applied { graph, .. } => graph,
                    };
                    let new_body = new_graph.to_ntriples();
                    let ct = "application/n-triples";
                    match storage
                        .put(path, Bytes::from(new_body.into_bytes()), ct)
                        .await
                    {
                        Ok(meta) => {
                            let mut resp = HttpResponse::Created();
                            resp.insert_header(("ETag", format!("\"{}\"", meta.etag)));
                            resp.finish()
                        }
                        Err(e) => {
                            error!("Storage PUT after PATCH-create error for {}: {}", path, e);
                            HttpResponse::InternalServerError().json(SolidProxyError {
                                error: "Storage error".to_string(),
                                details: Some(e.to_string()),
                            })
                        }
                    }
                }
                Err(e) => HttpResponse::UnprocessableEntity().json(SolidProxyError {
                    error: "Patch-to-absent failed".to_string(),
                    details: Some(e.to_string()),
                }),
            }
        }
        Err(e) => {
            error!("Storage GET for PATCH error on {}: {}", path, e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Storage error".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

// ============================================================================
// WAC ACL Resolution
// ============================================================================

/// Walk up the path hierarchy to find the nearest .acl sidecar.
/// Returns the parsed ACL document, or None if no ACL is found.
#[cfg(feature = "solid-pod-embed")]
async fn load_acl_for_path(
    storage: &Arc<FsBackend>,
    path: &str,
) -> Option<solid_pod_rs::wac::AclDocument> {
    // Resource-specific ACL: for containers use /<container>/.acl,
    // for non-containers use /<resource>.acl (WAC spec §4.1).
    let trimmed = path.trim_end_matches('/');
    let resource_acl = if is_container(path) {
        format!("{}/.acl", trimmed)
    } else {
        format!("{}.acl", trimmed)
    };
    if let Ok((body, _)) = storage.get(&resource_acl).await {
        return parse_acl_body(&body);
    }

    // Walk up parent containers — each parent's ACL is at /<parent>/.acl
    let mut current = trimmed.to_string();
    loop {
        match current.rfind('/') {
            Some(0) => {
                // Root container
                if let Ok((body, _)) = storage.get("/.acl").await {
                    return parse_acl_body(&body);
                }
                break;
            }
            Some(pos) => {
                current = current[..pos].to_string();
            }
            None => break,
        }

        let parent_acl = format!("{}/.acl", current);
        if let Ok((body, _)) = storage.get(&parent_acl).await {
            return parse_acl_body(&body);
        }
    }

    None
}

#[cfg(feature = "solid-pod-embed")]
fn parse_acl_body(body: &[u8]) -> Option<solid_pod_rs::wac::AclDocument> {
    // Try JSON-LD first, then Turtle
    if let Ok(doc) = serde_json::from_slice::<solid_pod_rs::wac::AclDocument>(body) {
        return Some(doc);
    }
    if let Ok(text) = std::str::from_utf8(body) {
        if let Ok(doc) = solid_pod_rs::wac::parse_turtle_acl(text) {
            return Some(doc);
        }
    }
    None
}

// ============================================================================
// Pod Management Endpoints
// ============================================================================

/// Check if a pod exists for the given npub
#[cfg(feature = "solid-pod-embed")]
async fn pod_exists(storage: &Arc<FsBackend>, npub: &str) -> bool {
    let pod_path = format!("/{}/", npub);
    storage.exists(&pod_path).await.unwrap_or(false)
}

/// Create the pod using solid_pod_rs::provision::provision_pod and custom containers
#[cfg(feature = "solid-pod-embed")]
async fn create_pod_with_structure(
    storage: &Arc<FsBackend>,
    npub: &str,
    pubkey: &str,
    pod_base_url: &str,
) -> Result<PodStructure, String> {
    let pod_base = format!("{}/{}", pod_base_url, npub);

    // Use solid_pod_rs provisioning for the core pod structure
    let plan = ProvisionPlan {
        pubkey: pubkey.to_string(),
        display_name: None,
        pod_base: pod_base.clone(),
        containers: vec![
            "/profile/".into(),
            "/ontology/".into(),
            "/ontology/contributions/".into(),
            "/ontology/proposals/".into(),
            "/ontology/annotations/".into(),
            "/preferences/".into(),
            "/inbox/".into(),
        ],
        root_acl: Some(build_pod_root_acl(pubkey, &pod_base)),
        quota_bytes: None,
    };

    // provision_pod writes relative to the storage root. We need the pod
    // under /<npub>/ so we scope by putting the pod path prefix on the
    // storage calls. Since provision_pod writes to /profile/card etc,
    // we need to write our Nostr-specific profile separately.
    match provision_pod(storage.as_ref(), &plan).await {
        Ok(outcome) => {
            info!(
                "Provisioned pod for {}: webid={}, containers={:?}",
                npub, outcome.webid, outcome.containers_created
            );
        }
        Err(e) => {
            // Non-fatal: log and continue with manual structure creation
            warn!(
                "provision_pod partial failure for {}: {} — creating structure manually",
                npub, e
            );
        }
    }

    // Write Nostr-specific WebID profile card with pod-relative paths
    let profile_path = format!("/{}/profile/card", npub);
    let profile_content = format!(
        r#"@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix solid: <http://www.w3.org/ns/solid/terms#> .
@prefix vcard: <http://www.w3.org/2006/vcard/ns#> .
@prefix nostr: <https://github.com/nostr-protocol/nostr#> .

<#me>
    a foaf:Person ;
    solid:oidcIssuer <https://visionclaw.info> ;
    nostr:pubkey "{pubkey}" ;
    nostr:npub "{npub}" ;
    vcard:hasUID <did:nostr:{pubkey}> .
"#,
        pubkey = pubkey,
        npub = npub
    );

    if let Err(e) = storage
        .put(
            &profile_path,
            Bytes::from(profile_content.into_bytes()),
            "text/turtle",
        )
        .await
    {
        warn!("Failed to write WebID profile for {}: {}", npub, e);
    } else {
        info!("Created WebID profile for {}", npub);
    }

    // Ensure all custom directories exist
    let directories = [
        format!("/{}/", npub),
        format!("/{}/profile/", npub),
        format!("/{}/ontology/", npub),
        format!("/{}/ontology/contributions/", npub),
        format!("/{}/ontology/proposals/", npub),
        format!("/{}/ontology/annotations/", npub),
        format!("/{}/preferences/", npub),
        format!("/{}/inbox/", npub),
    ];

    for dir in &directories {
        match storage.create_container(dir).await {
            Ok(_) => debug!("Created/confirmed directory: {}", dir),
            Err(PodError::AlreadyExists(_)) => debug!("Directory already exists: {}", dir),
            Err(e) => warn!("Failed to create directory {}: {}", dir, e),
        }
    }

    // Write WAC ACL for the pod root
    let acl_path = format!("/{}/.acl", npub);
    let acl_doc = build_pod_root_acl(pubkey, &pod_base);
    let acl_body = match serde_json::to_vec(&acl_doc) {
        Ok(b) => b,
        Err(e) => {
            warn!("Failed to serialize ACL: {}", e);
            Vec::new()
        }
    };

    if !acl_body.is_empty() {
        match storage
            .put(&acl_path, Bytes::from(acl_body), "application/ld+json")
            .await
        {
            Ok(_) => info!("Created ACL for pod {}", npub),
            Err(e) => warn!("Failed to create ACL for {}: {}", npub, e),
        }
    }

    Ok(SolidPodState::pod_structure(&pod_base, npub))
}

/// Build the WAC ACL document for a pod root
#[cfg(feature = "solid-pod-embed")]
fn build_pod_root_acl(pubkey: &str, _pod_base: &str) -> solid_pod_rs::wac::AclDocument {
    use solid_pod_rs::wac::{AclAuthorization, AclDocument, IdOrIds, IdRef};

    let owner = AclAuthorization {
        id: Some("#owner".into()),
        r#type: Some("acl:Authorization".into()),
        agent: Some(IdOrIds::Single(IdRef {
            id: format!("did:nostr:{}", pubkey),
        })),
        agent_class: None,
        agent_group: None,
        origin: None,
        access_to: Some(IdOrIds::Single(IdRef { id: "./".into() })),
        default: Some(IdOrIds::Single(IdRef { id: "./".into() })),
        mode: Some(IdOrIds::Multiple(vec![
            IdRef {
                id: "acl:Read".into(),
            },
            IdRef {
                id: "acl:Write".into(),
            },
            IdRef {
                id: "acl:Control".into(),
            },
        ])),
        condition: None,
    };

    let public = AclAuthorization {
        id: Some("#public".into()),
        r#type: Some("acl:Authorization".into()),
        agent: None,
        agent_class: Some(IdOrIds::Single(IdRef {
            id: "foaf:Agent".into(),
        })),
        agent_group: None,
        origin: None,
        access_to: Some(IdOrIds::Single(IdRef { id: "./".into() })),
        default: None,
        mode: Some(IdOrIds::Single(IdRef {
            id: "acl:Read".into(),
        })),
        condition: None,
    };

    AclDocument {
        context: None,
        graph: Some(vec![owner, public]),
    }
}

/// Ensure a pod exists, auto-provisioning if needed
#[cfg(feature = "solid-pod-embed")]
pub async fn ensure_pod_exists(
    state: &SolidPodState,
    npub: &str,
    pubkey: &str,
    pod_base_url: &str,
) -> Result<(bool, PodStructure), String> {
    if pod_exists(&state.storage, npub).await {
        let pod_base = format!("{}/{}", pod_base_url, npub);

        // Backfill missing ACL for existing pods
        let acl_path = format!("/{}/.acl", npub);
        if !state.storage.exists(&acl_path).await.unwrap_or(true) {
            info!("Backfilling missing ACL for existing pod: {}", npub);
            let acl_doc = build_pod_root_acl(pubkey, &pod_base);
            if let Ok(acl_body) = serde_json::to_vec(&acl_doc) {
                match state
                    .storage
                    .put(&acl_path, Bytes::from(acl_body), "application/ld+json")
                    .await
                {
                    Ok(_) => info!("Backfilled ACL for pod {}", npub),
                    Err(e) => warn!("ACL backfill for {} failed: {}", npub, e),
                }
            }
        }

        return Ok((false, SolidPodState::pod_structure(&pod_base, npub)));
    }

    info!("Auto-provisioning pod for user: {}", npub);

    let structure = create_pod_with_structure(&state.storage, npub, pubkey, pod_base_url).await?;

    Ok((true, structure))
}

/// Create a new pod for a user based on their Nostr identity
#[cfg(feature = "solid-pod-embed")]
pub async fn create_pod(
    req: HttpRequest,
    _body: web::Json<CreatePodRequest>,
    state: web::Data<SolidPodState>,
    nostr_service: web::Data<NostrService>,
) -> HttpResponse {
    let user = match get_user_from_request(&req, &nostr_service).await {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(SolidProxyError {
                error: "Authentication required".to_string(),
                details: Some("Valid Nostr session required to create pod".to_string()),
            });
        }
    };

    let npub = &user.npub;
    let pubkey = &user.pubkey;
    let pod_base_url = SolidPodState::pod_base_url(&req);
    info!("Creating pod for user: {}", npub);

    match ensure_pod_exists(&state, npub, pubkey, &pod_base_url).await {
        Ok((created, structure)) => {
            let pod_url = format!("{}/{}/", pod_base_url, npub);
            let status = if created {
                actix_web::http::StatusCode::CREATED
            } else {
                actix_web::http::StatusCode::OK
            };
            HttpResponse::build(status).json(PodCreationResponse {
                pod_url,
                webid: structure.profile.clone(),
                created,
                structure,
            })
        }
        Err(e) => {
            error!("Pod creation failed: {}", e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Pod creation failed".to_string(),
                details: Some(e),
            })
        }
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `create_pod` used to live here; removed now that no route reaches it in the
// feature-off build.

#[derive(Debug, Deserialize)]
pub struct CreatePodRequest {
    /// Optional custom pod name (defaults to npub)
    pub name: Option<String>,
}

/// Check if a pod exists for the current user
#[cfg(feature = "solid-pod-embed")]
pub async fn check_pod_exists(
    req: HttpRequest,
    state: web::Data<SolidPodState>,
    nostr_service: web::Data<NostrService>,
) -> HttpResponse {
    let user = match get_user_from_request(&req, &nostr_service).await {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(SolidProxyError {
                error: "Authentication required".to_string(),
                details: None,
            });
        }
    };

    let pod_base_url = SolidPodState::pod_base_url(&req);
    let exists = pod_exists(&state.storage, &user.npub).await;

    if exists {
        let pod_url = format!("{}/{}/", pod_base_url, user.npub);
        let structure = SolidPodState::pod_structure(&pod_base_url, &user.npub);
        HttpResponse::Ok().json(serde_json::json!({
            "exists": true,
            "pod_url": pod_url,
            "webid": structure.profile,
            "structure": structure
        }))
    } else {
        let pod_url = format!("{}/{}/", pod_base_url, user.npub);
        HttpResponse::Ok().json(serde_json::json!({
            "exists": false,
            "suggested_url": pod_url
        }))
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `check_pod_exists` used to live here; removed for the same reason.

/// Initialize pod for current user (auto-provision if needed)
#[cfg(feature = "solid-pod-embed")]
pub async fn init_pod(
    req: HttpRequest,
    state: web::Data<SolidPodState>,
    nostr_service: web::Data<NostrService>,
) -> HttpResponse {
    let user = match get_user_from_request(&req, &nostr_service).await {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(SolidProxyError {
                error: "Authentication required".to_string(),
                details: Some("Valid Nostr session required to initialize pod".to_string()),
            });
        }
    };

    let npub = &user.npub;
    let pubkey = &user.pubkey;
    let pod_base_url = SolidPodState::pod_base_url(&req);

    debug!("Initializing pod for user: {}", npub);

    match ensure_pod_exists(&state, npub, pubkey, &pod_base_url).await {
        Ok((created, structure)) => {
            let pod_url = format!("{}/{}/", pod_base_url, npub);
            HttpResponse::Ok().json(serde_json::json!({
                "pod_url": pod_url,
                "webid": structure.profile,
                "created": created,
                "structure": structure
            }))
        }
        Err(e) => {
            error!("Pod initialization failed: {}", e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Pod initialization failed".to_string(),
                details: Some(e),
            })
        }
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `init_pod` used to live here; removed for the same reason.

/// Initialize pod from NIP-98 auth (for Solid-first requests)
#[cfg(feature = "solid-pod-embed")]
pub async fn init_pod_nip98(req: HttpRequest, state: web::Data<SolidPodState>) -> HttpResponse {
    let identity = match state.extract_user_identity(&req) {
        Some(id) => id,
        None => {
            return HttpResponse::Unauthorized().json(SolidProxyError {
                error: "NIP-98 authentication required".to_string(),
                details: Some("Valid NIP-98 Authorization header required".to_string()),
            });
        }
    };

    // Convert hex pubkey to npub (bech32)
    let npub = match PublicKey::from_hex(&identity.pubkey) {
        Ok(pk) => match pk.to_bech32() {
            Ok(n) => n,
            Err(e) => {
                error!("Failed to convert pubkey to npub: {}", e);
                return HttpResponse::InternalServerError().json(SolidProxyError {
                    error: "Failed to process public key".to_string(),
                    details: Some(e.to_string()),
                });
            }
        },
        Err(e) => {
            error!("Invalid pubkey in NIP-98 token: {}", e);
            return HttpResponse::BadRequest().json(SolidProxyError {
                error: "Invalid public key".to_string(),
                details: Some(e.to_string()),
            });
        }
    };

    let pod_base_url = SolidPodState::pod_base_url(&req);
    debug!("Initializing pod for NIP-98 user: {}", npub);

    match ensure_pod_exists(&state, &npub, &identity.pubkey, &pod_base_url).await {
        Ok((created, structure)) => {
            let pod_url = format!("{}/{}/", pod_base_url, npub);
            HttpResponse::Ok().json(serde_json::json!({
                "pod_url": pod_url,
                "webid": structure.profile,
                "created": created,
                "structure": structure,
                "npub": npub
            }))
        }
        Err(e) => {
            error!("Pod initialization failed: {}", e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "Pod initialization failed".to_string(),
                details: Some(e),
            })
        }
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `init_pod_nip98` used to live here; removed for the same reason.

/// Get user from request using NIP-98 auth (primary) or session token (fallback)
#[cfg(feature = "solid-pod-embed")]
async fn get_user_from_request(
    req: &HttpRequest,
    nostr_service: &web::Data<NostrService>,
) -> Option<NostrUser> {
    let auth_header = req.headers().get("Authorization")?;
    let auth_str = auth_header.to_str().ok()?;

    // Try NIP-98 first (primary authentication path)
    if auth_str.starts_with("Nostr ") {
        let conn_info = req.connection_info();
        let scheme = req
            .headers()
            .get("X-Forwarded-Proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.scheme());
        let host = req
            .headers()
            .get("X-Forwarded-Host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| conn_info.host());
        let path = req
            .headers()
            .get("X-Forwarded-URI")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_else(|| {
                req.uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            });
        let request_url = format!("{}://{}{}", scheme, host, path);
        let request_method = req.method().as_str();

        match nostr_service
            .verify_nip98_auth(auth_str, &request_url, request_method, None)
            .await
        {
            Ok(user) => return Some(user),
            Err(e) => {
                warn!("NIP-98 auth failed in pod management: {}", e);
                return None;
            }
        }
    }

    // Fall back to Bearer session token (legacy path)
    if auth_str.starts_with("Bearer ") {
        let token = &auth_str[7..];
        nostr_service.get_session(token).await
    } else {
        None
    }
}

// ============================================================================
// WebSocket Handler for Solid-0.1 Notifications
// ============================================================================

use actix::{Actor, ActorContext, StreamHandler};
use actix_web_actors::ws;

/// WebSocket actor for solid-0.1 notifications backed by storage events
///
/// ADR-2066: the `storage_rx` field this actor used to carry (a
/// `Receiver<solid_pod_rs::storage::StorageEvent>`, feature-gated on
/// `solid-pod-embed`) was dead — always initialised to `None`, never fed by any
/// sender, never polled anywhere in `started`/`StreamHandler::handle`. The
/// comment that used to sit above the `Subscribe` arm claiming "storage watch is
/// handled at the handler level" was aspirational, not real wiring. Removed; the
/// actor only tracks subscriptions for filtering, as it always actually did.
pub struct SolidNotificationWs {
    /// User identity for the connection
    user_identity: Option<UserIdentity>,
    /// Subscribed resources
    subscriptions: Vec<String>,
}

impl SolidNotificationWs {
    pub fn new(user_identity: Option<UserIdentity>) -> Self {
        Self {
            user_identity,
            subscriptions: Vec::new(),
        }
    }
}

impl Actor for SolidNotificationWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!(
            "Solid notification WebSocket started for user: {:?}",
            self.user_identity
                .as_ref()
                .map(|u| &u.pubkey[..16.min(u.pubkey.len())])
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("Solid notification WebSocket stopped");
    }
}

/// Message format for solid-0.1 protocol
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SolidNotificationMessage {
    /// Subscribe to resource changes
    #[serde(rename = "sub")]
    Subscribe { resource: String },
    /// Unsubscribe from resource
    #[serde(rename = "unsub")]
    Unsubscribe { resource: String },
    /// Acknowledgment from server
    #[serde(rename = "ack")]
    Ack { resource: String },
    /// Publication notification (resource changed)
    #[serde(rename = "pub")]
    Publish { resource: String },
    /// Ping for keepalive
    #[serde(rename = "ping")]
    Ping,
    /// Pong response
    #[serde(rename = "pong")]
    Pong,
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for SolidNotificationWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(text)) => {
                debug!("Received solid notification message: {}", text);

                match serde_json::from_str::<SolidNotificationMessage>(&text) {
                    Ok(SolidNotificationMessage::Subscribe { resource }) => {
                        info!("Client subscribing to: {}", resource);
                        self.subscriptions.push(resource.clone());

                        // ADR-2066: no storage watcher is registered anywhere for
                        // this resource — there was never any handler-level wiring
                        // feeding storage events back into this actor (the dead
                        // `storage_rx` field this comment used to justify has been
                        // removed). This actor only tracks subscriptions for
                        // client-side filtering; it does not push change events.
                        debug!("Subscription registered for: {}", resource);

                        let ack = SolidNotificationMessage::Ack { resource };
                        if let Ok(json) = serde_json::to_string(&ack) {
                            ctx.text(json);
                        }
                    }
                    Ok(SolidNotificationMessage::Unsubscribe { resource }) => {
                        info!("Client unsubscribing from: {}", resource);
                        self.subscriptions.retain(|r| r != &resource);
                    }
                    Ok(SolidNotificationMessage::Ping) => {
                        let pong = SolidNotificationMessage::Pong;
                        if let Ok(json) = serde_json::to_string(&pong) {
                            ctx.text(json);
                        }
                    }
                    Ok(msg) => {
                        debug!("Received other solid message: {:?}", msg);
                    }
                    Err(e) => {
                        warn!("Failed to parse solid notification message: {}", e);
                    }
                }
            }
            Ok(ws::Message::Ping(msg)) => {
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Binary(_)) => {
                warn!("Binary messages not supported for solid notifications");
            }
            Ok(ws::Message::Close(reason)) => {
                info!("WebSocket close requested: {:?}", reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                warn!("Continuation frames not supported");
            }
            Ok(ws::Message::Nop) => {}
            Err(e) => {
                error!("WebSocket protocol error: {}", e);
                ctx.stop();
            }
        }
    }
}

/// WebSocket handler for solid-0.1 notifications
pub async fn handle_solid_notifications_ws(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<SolidPodState>,
) -> Result<HttpResponse, actix_web::Error> {
    let user_identity = state.extract_user_identity(&req);

    if let Some(ref identity) = user_identity {
        debug!(
            "Solid notifications WebSocket connecting for user: {}...",
            &identity.pubkey[..16.min(identity.pubkey.len())]
        );
    } else {
        debug!("Solid notifications WebSocket connecting (anonymous)");
    }

    let ws_actor = SolidNotificationWs::new(user_identity);
    ws::start(ws_actor, &req, stream)
}

// ============================================================================
// Health Check
// ============================================================================

/// Health check — verifies the storage backend is operational.
#[cfg(feature = "solid-pod-embed")]
pub async fn solid_health_check(state: web::Data<SolidPodState>) -> HttpResponse {
    // Probe the storage by checking if the root exists
    match state.storage.exists("/").await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({
            "status": "healthy",
            "backend": "solid-pod-rs",
            "data_root": state.data_root.display().to_string()
        })),
        Err(e) => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "unhealthy",
            "backend": "solid-pod-rs",
            "error": e.to_string()
        })),
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `solid_health_check` used to live here; removed for the same reason.

// ============================================================================
// DID Resolution
// ============================================================================

/// GET /.well-known/did.json — DID document (did:web method)
#[cfg(feature = "solid-pod-embed")]
async fn handle_did_wellknown(req: HttpRequest, state: web::Data<SolidPodState>) -> HttpResponse {
    // Try to read a stored DID document first
    let did_path = "/.well-known/did.json";
    match state.storage.get(did_path).await {
        Ok((body, meta)) => HttpResponse::Ok()
            .content_type(meta.content_type)
            .body(body),
        Err(PodError::NotFound(_)) => {
            // Generate a minimal did:web document from the server origin
            let pod_base = SolidPodState::pod_base_url(&req);
            let doc = serde_json::json!({
                "@context": ["https://www.w3.org/ns/did/v1"],
                "id": format!("did:web:{}", extract_did_web_host(&req)),
                "service": [{
                    "id": "#solid",
                    "type": "SolidStorage",
                    "serviceEndpoint": pod_base
                }]
            });
            HttpResponse::Ok()
                .content_type("application/did+ld+json")
                .json(doc)
        }
        Err(e) => {
            error!("DID well-known fetch error: {}", e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "DID resolution failed".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `handle_did_wellknown` used to live here; removed for the same reason.

/// GET /did/{tail:.*} — DID resolution (e.g. /did/nostr:<npub>)
#[cfg(feature = "solid-pod-embed")]
async fn handle_did_proxy(
    path: web::Path<String>,
    _req: HttpRequest,
    state: web::Data<SolidPodState>,
) -> HttpResponse {
    let tail = path.into_inner();

    // Parse did:nostr:<pubkey> style paths
    if let Some(pubkey) = tail.strip_prefix("nostr:") {
        // Use solid_pod_rs interop to generate a DID document
        let also_known_as: Vec<String> = Vec::new();
        let doc = solid_pod_rs::interop::did_nostr::did_nostr_document(pubkey, &also_known_as);

        return HttpResponse::Ok()
            .content_type("application/did+ld+json")
            .json(doc);
    }

    // For other DID methods, try to read from storage
    let did_path = format!("/did/{}", tail);
    match state.storage.get(&did_path).await {
        Ok((body, meta)) => HttpResponse::Ok()
            .content_type(meta.content_type)
            .body(body),
        Err(PodError::NotFound(_)) => HttpResponse::NotFound().json(SolidProxyError {
            error: "DID not found".to_string(),
            details: Some(format!("No DID document at /did/{}", tail)),
        }),
        Err(e) => {
            error!("DID resolution error for {}: {}", tail, e);
            HttpResponse::InternalServerError().json(SolidProxyError {
                error: "DID resolution failed".to_string(),
                details: Some(e.to_string()),
            })
        }
    }
}

// ADR-2067: the `#[cfg(not(feature = "solid-pod-embed"))]` 503 stub twin of
// `handle_did_proxy` used to live here; removed for the same reason.

/// Extract the host for did:web from the request
#[cfg(feature = "solid-pod-embed")]
fn extract_did_web_host(req: &HttpRequest) -> String {
    let conn_info = req.connection_info();
    let host = req
        .headers()
        .get("X-Forwarded-Host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| conn_info.host());
    // did:web encodes : as %3A and / as :
    host.replace(':', "%3A")
}

// ============================================================================
// Global Storage Accessor
// ============================================================================

/// Global singleton for the storage backend, set during configure_routes.
/// Used by other handlers (e.g. image_gen_handler) to store resources in pods.
#[cfg(feature = "solid-pod-embed")]
static GLOBAL_STORAGE: std::sync::OnceLock<Arc<FsBackend>> = std::sync::OnceLock::new();

/// Get a reference to the global Solid storage backend.
/// Returns None if solid-pod-embed is disabled or not yet initialized.
#[cfg(feature = "solid-pod-embed")]
pub fn get_global_storage() -> Option<Arc<FsBackend>> {
    GLOBAL_STORAGE.get().cloned()
}

/// Stub when feature is disabled.
#[cfg(not(feature = "solid-pod-embed"))]
pub fn get_global_storage() -> Option<()> {
    None
}

// ============================================================================
// Route Configuration
// ============================================================================

/// Initialise Solid pod state asynchronously. Call from main (async context)
/// before `HttpServer::new`, then pass the returned `web::Data` into
/// `configure_routes_with_state`.
#[cfg(feature = "solid-pod-embed")]
pub async fn init_solid_state() -> web::Data<SolidPodState> {
    let state = SolidPodState::new_async().await;
    let _ = GLOBAL_STORAGE.set(Arc::clone(&state.storage));
    web::Data::new(state)
}

/// Configure Solid routes — all routes use in-process solid-pod-rs storage.
///
/// On `solid-pod-embed`: registers the full route tree with FsBackend.
/// Without the feature (ADR-2067): registers nothing at all — see the
/// `#[cfg(not(feature = "solid-pod-embed"))]` twin of this function below.
#[cfg(feature = "solid-pod-embed")]
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    info!("=== REGISTERING SOLID POD ROUTES (embedded solid-pod-rs) ===");

    // State is pre-initialized via init_solid_state() in main.rs and
    // injected via .app_data() on the HttpServer. The configure function
    // only wires up the route tree — no async init needed here.

    cfg.service(
        web::scope("/solid")
            // Health check endpoint
            .route("/health", web::get().to(solid_health_check))
            // WebSocket endpoint for notifications (solid-0.1 protocol)
            .route(
                "/.notifications",
                web::get().to(handle_solid_notifications_ws),
            )
            // Pod management endpoints
            .route("/pods", web::post().to(create_pod))
            .route("/pods/check", web::get().to(check_pod_exists))
            .route("/pods/init", web::post().to(init_pod))
            .route("/pods/init-nip98", web::post().to(init_pod_nip98))
            // LDP CRUD for all other paths
            .route("/{tail:.*}", web::get().to(handle_solid_proxy))
            .route("/{tail:.*}", web::put().to(handle_solid_proxy))
            .route("/{tail:.*}", web::post().to(handle_solid_proxy))
            .route("/{tail:.*}", web::delete().to(handle_solid_proxy))
            .route("/{tail:.*}", web::head().to(handle_solid_proxy))
            .route(
                "/{tail:.*}",
                web::method(Method::PATCH).to(handle_solid_proxy),
            ),
    )
    // DID document resolution — public endpoints at canonical spec paths.
    .service(web::resource("/.well-known/did.json").route(web::get().to(handle_did_wellknown)))
    .service(web::scope("/did").route("/{tail:.*}", web::get().to(handle_did_proxy)));
}

/// Configure Solid routes when the `solid-pod-embed` feature is disabled.
///
/// ADR-2067: this used to register the entire `/solid` + DID route tree
/// pointed at 503 stub handlers, meaning the feature-off build advertised a
/// full API surface that always failed. It now registers nothing at all —
/// `/solid/*`, `/.well-known/did.json` and `/did/*` all 404 in a feature-off
/// build, matching the actual (absent) capability. `src/main.rs` calls this
/// function unconditionally regardless of the feature flag, so the name and
/// `fn(&mut web::ServiceConfig)` signature are kept identical to the
/// `solid-pod-embed` variant above.
#[cfg(not(feature = "solid-pod-embed"))]
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    let _ = cfg;
    info!("=== SOLID POD ROUTES DISABLED (no solid-pod-embed feature): registering none ===");
}
