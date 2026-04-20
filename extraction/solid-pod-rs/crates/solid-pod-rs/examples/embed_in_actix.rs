//! Embed `solid-pod-rs` as a sub-scope inside an existing actix-web app.
//!
//! This example demonstrates the production pattern: you have an
//! existing API (`/api/*`, `/health`, …) and you want to mount a
//! Solid pod under `/pod/*` while sharing application state (the
//! authenticator, the storage handle, …) with the rest of the app.
//!
//! Run with:
//! ```bash
//! cargo run --example embed_in_actix -p solid-pod-rs
//! ```
//!
//! Expected output:
//! ```text
//! embed_in_actix listening on http://127.0.0.1:8766
//!   /health                    — app health check (no auth)
//!   /api/whoami                — uses shared SharedAuth state
//!   /pod/{any-resource-path}   — full Solid pod mounted here
//! ```
//!
//! Try:
//! ```bash
//! curl -i http://127.0.0.1:8766/health
//! curl -i -X PUT --data 'hi' -H 'Content-Type: text/plain' \
//!      http://127.0.0.1:8766/pod/notes/first.txt
//! curl -i http://127.0.0.1:8766/pod/notes/first.txt
//! curl -i http://127.0.0.1:8766/pod/notes/    # container listing
//! ```

use std::sync::Arc;

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use bytes::Bytes;
use solid_pod_rs::{
    auth::nip98,
    ldp::{self, LdpContainerOps},
    storage::{memory::MemoryBackend, Storage},
    wac, PodError,
};

/// Shared auth + storage state. Both the host app (`/api/*`) and the
/// embedded pod (`/pod/*`) read from the same handle, so anything the
/// host authenticates is visible to the pod's WAC evaluator via
/// `did:nostr:<pubkey>` URIs.
#[derive(Clone)]
struct SharedState {
    storage: Arc<dyn Storage>,
    /// Arbitrary app-level config the host owns; not used by the pod.
    app_name: String,
}

// ---------------------------------------------------------------------------
// Host-app handlers
// ---------------------------------------------------------------------------

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

async fn whoami(req: HttpRequest, state: web::Data<SharedState>) -> HttpResponse {
    let pk = extract_pubkey(&req).await;
    HttpResponse::Ok().json(serde_json::json!({
        "app": state.app_name,
        "authenticated": pk.is_some(),
        "pubkey": pk,
    }))
}

// ---------------------------------------------------------------------------
// Pod handlers (mounted under /pod)
// ---------------------------------------------------------------------------

fn pod_subpath(req: &HttpRequest) -> String {
    // The pod is mounted at `/pod`, so strip that prefix. The pod's
    // internal paths always start with `/`.
    let full = req.uri().path();
    let tail = full.strip_prefix("/pod").unwrap_or(full);
    if tail.is_empty() {
        "/".to_string()
    } else {
        tail.to_string()
    }
}

async fn pod_get(
    req: HttpRequest,
    state: web::Data<SharedState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = pod_subpath(&req);
    let pk = extract_pubkey(&req).await;
    let agent_uri = pk.as_ref().map(|pk| format!("did:nostr:{pk}"));
    let wac_allow = wac::wac_allow_header(None, agent_uri.as_deref(), &path);

    if ldp::is_container(&path) {
        let v = state
            .storage
            .container_representation(&path)
            .await
            .map_err(to_actix)?;
        let mut rsp = HttpResponse::Ok().json(v);
        let _ = rsp.headers_mut().insert(
            actix_web::http::header::CONTENT_TYPE,
            actix_web::http::header::HeaderValue::from_static("application/ld+json"),
        );
        if let Ok(v) = actix_web::http::header::HeaderValue::from_str(&wac_allow) {
            rsp.headers_mut().insert(
                actix_web::http::header::HeaderName::from_static("wac-allow"),
                v,
            );
        }
        return Ok(rsp);
    }

    match state.storage.get(&path).await {
        Ok((body, meta)) => {
            let mut rsp = HttpResponse::Ok().body(body.to_vec());
            let _ = rsp.headers_mut().insert(
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::HeaderValue::from_str(&meta.content_type)
                    .unwrap_or_else(|_| {
                        actix_web::http::header::HeaderValue::from_static(
                            "application/octet-stream",
                        )
                    }),
            );
            if let Ok(v) = actix_web::http::header::HeaderValue::from_str(&wac_allow) {
                rsp.headers_mut().insert(
                    actix_web::http::header::HeaderName::from_static("wac-allow"),
                    v,
                );
            }
            Ok(rsp)
        }
        Err(PodError::NotFound(_)) => Ok(HttpResponse::NotFound().finish()),
        Err(e) => Err(to_actix(e)),
    }
}

async fn pod_put(
    req: HttpRequest,
    body: web::Bytes,
    state: web::Data<SharedState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = pod_subpath(&req);
    if ldp::is_container(&path) {
        return Ok(HttpResponse::MethodNotAllowed().body("cannot PUT to a container"));
    }
    let ct = req
        .headers()
        .get(actix_web::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let meta = state
        .storage
        .put(&path, Bytes::from(body.to_vec()), ct)
        .await
        .map_err(to_actix)?;
    let mut rsp = HttpResponse::Created().finish();
    if let Ok(etag) =
        actix_web::http::header::HeaderValue::from_str(&format!("\"{}\"", meta.etag))
    {
        rsp.headers_mut()
            .insert(actix_web::http::header::ETAG, etag);
    }
    Ok(rsp)
}

async fn pod_delete(
    req: HttpRequest,
    state: web::Data<SharedState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = pod_subpath(&req);
    match state.storage.delete(&path).await {
        Ok(()) => Ok(HttpResponse::NoContent().finish()),
        Err(PodError::NotFound(_)) => Ok(HttpResponse::NotFound().finish()),
        Err(e) => Err(to_actix(e)),
    }
}

// ---------------------------------------------------------------------------
// Auth helper (shared across host app + pod)
// ---------------------------------------------------------------------------

async fn extract_pubkey(req: &HttpRequest) -> Option<String> {
    let header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;
    let url = format!(
        "http://{}{}",
        req.connection_info().host(),
        req.uri().path()
    );
    nip98::verify(header, &url, req.method().as_str(), None)
        .await
        .ok()
}

fn to_actix(e: PodError) -> actix_web::Error {
    actix_web::error::ErrorInternalServerError(e.to_string())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let storage: Arc<dyn Storage> = Arc::new(MemoryBackend::new());
    let state = SharedState {
        storage,
        app_name: "host-app".to_string(),
    };

    eprintln!("embed_in_actix listening on http://127.0.0.1:8766");
    eprintln!("  /health                    — app health check (no auth)");
    eprintln!("  /api/whoami                — uses shared SharedAuth state");
    eprintln!("  /pod/{{any-resource-path}}   — full Solid pod mounted here");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            // Host app routes — live alongside the pod.
            .route("/health", web::get().to(health))
            .service(
                web::scope("/api")
                    .route("/whoami", web::get().to(whoami)),
            )
            // Pod is mounted as a sub-scope — a real deployment would
            // wrap this in a middleware that enforces WAC via
            // `evaluate_access`, rejecting requests that don't pass.
            .service(
                web::scope("/pod")
                    .route("/{tail:.*}", web::get().to(pod_get))
                    .route("/{tail:.*}", web::head().to(pod_get))
                    .route("/{tail:.*}", web::put().to(pod_put))
                    .route("/{tail:.*}", web::delete().to(pod_delete)),
            )
    })
    .bind(("127.0.0.1", 8766))?
    .run()
    .await
}
