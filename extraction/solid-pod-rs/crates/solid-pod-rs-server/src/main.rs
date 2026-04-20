//! `solid-pod-rs-server` — drop-in JSS replacement binary.
//!
//! Thin actix-web shell over [`solid_pod_rs`]. Enforces the F7 library-
//! server split described in ADR-056 §D3 and
//! [`docs/design/jss-parity/06-library-surface-context.md`]:
//!
//! - Library (`solid-pod-rs`) owns protocol semantics (LDP, WAC, WebID,
//!   Notifications, OIDC, NIP-98, security primitives, config schema).
//! - This binary owns the HTTP transport, tokio runtime, CLI, config
//!   loader wiring, and signal handling.
//!
//! ## Runtime
//!
//! 1. Parse CLI flags via [`clap`] (config path, log level, port
//!    override).
//! 2. Initialise tracing (stderr JSON or pretty depending on env).
//! 3. Load [`ServerConfig`] via the F6 [`ConfigLoader`]:
//!    Defaults → file → env (JSS_*).
//! 4. Build the storage backend from `config.storage`.
//! 5. Construct [`AppState`] and bind [`actix_web::HttpServer`] to
//!    `config.server.host:config.server.port`.
//! 6. Wait for SIGTERM / SIGINT and shut down gracefully.

use std::sync::Arc;

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use anyhow::Context;
use bytes::Bytes;
use clap::Parser;
use solid_pod_rs::{
    auth::nip98,
    config::{ConfigLoader, ServerConfig, StorageBackendConfig},
    ldp::{self, LdpContainerOps},
    storage::{fs::FsBackend, memory::MemoryBackend, Storage},
    wac, PodError,
};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// JSS-compatible Solid Pod server, implemented in Rust.
#[derive(Debug, Parser)]
#[command(
    name = "solid-pod-rs-server",
    version,
    about = "Drop-in JSS replacement — Solid Pod server binary",
    long_about = None,
)]
struct Cli {
    /// Path to a JSS-compatible `config.json` file. Optional: the
    /// loader still runs with defaults + env if absent.
    #[arg(long, short = 'c', env = "JSS_CONFIG")]
    config: Option<String>,

    /// Override `server.host` from config / env.
    #[arg(long)]
    host: Option<String>,

    /// Override `server.port` from config / env.
    #[arg(long, short = 'p')]
    port: Option<u16>,

    /// Tracing filter directive. Defaults to `info` if unset.
    ///
    /// Examples: `debug`, `solid_pod_rs=debug,info`.
    #[arg(long, env = "RUST_LOG")]
    log: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

/// Actix-web shared state. Wraps the library's typed primitives behind
/// an `Arc<dyn Storage>` so handlers remain transport-thin.
#[derive(Clone)]
struct AppState {
    storage: Arc<dyn Storage>,
}

// ---------------------------------------------------------------------------
// Storage construction
// ---------------------------------------------------------------------------

/// Materialise a boxed [`Storage`] from a [`StorageBackendConfig`]
/// snapshot. `s3` is accepted at the config layer but deferred here:
/// the `s3-backend` feature lives in the library crate and is not
/// enabled by default for the server binary.
async fn build_storage(cfg: &StorageBackendConfig) -> anyhow::Result<Arc<dyn Storage>> {
    match cfg {
        StorageBackendConfig::Fs { root } => {
            info!(backend = "fs", root = %root, "initialising storage");
            let fs = FsBackend::new(root.as_str())
                .await
                .with_context(|| format!("initialise FS backend at {root}"))?;
            Ok(Arc::new(fs))
        }
        StorageBackendConfig::Memory => {
            info!(backend = "memory", "initialising storage (ephemeral)");
            Ok(Arc::new(MemoryBackend::new()))
        }
        StorageBackendConfig::S3 { bucket, region, .. } => {
            anyhow::bail!(
                "storage.type=s3 requested (bucket={bucket}, region={region}) but this \
                 binary was built without the `s3-backend` feature. Rebuild with \
                 `--features solid-pod-rs/s3-backend` or use fs/memory storage."
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Auth helper — shared across handlers
// ---------------------------------------------------------------------------

/// Attempt NIP-98 bearer verification; returns the pubkey on success.
/// Any failure is treated as "no authenticated agent" — WAC will apply
/// public-agent rules.
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

// ---------------------------------------------------------------------------
// Handler utilities
// ---------------------------------------------------------------------------

fn set_link_headers(rsp: &mut HttpResponse, path: &str) {
    let links = ldp::link_headers(path).join(", ");
    if let Ok(value) = actix_web::http::header::HeaderValue::from_str(&links) {
        rsp.headers_mut().insert(
            actix_web::http::header::HeaderName::from_static("link"),
            value,
        );
    }
}

fn set_wac_allow(rsp: &mut HttpResponse, header_value: &str) {
    if let Ok(v) = actix_web::http::header::HeaderValue::from_str(header_value) {
        rsp.headers_mut().insert(
            actix_web::http::header::HeaderName::from_static("wac-allow"),
            v,
        );
    }
}

fn to_actix(e: PodError) -> actix_web::Error {
    match e {
        PodError::NotFound(_) => actix_web::error::ErrorNotFound(e.to_string()),
        _ => actix_web::error::ErrorInternalServerError(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Handlers (GET/HEAD/PUT/DELETE)
// ---------------------------------------------------------------------------

async fn handle_get(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = req.uri().path().to_string();
    let auth_pk = extract_pubkey(&req).await;
    let agent_uri = auth_pk.as_ref().map(|pk| format!("did:nostr:{pk}"));
    let wac_allow = wac::wac_allow_header(None, agent_uri.as_deref(), &path);

    if ldp::is_container(&path) {
        let v = state
            .storage
            .container_representation(&path)
            .await
            .map_err(to_actix)?;
        let mut rsp = HttpResponse::Ok().json(v);
        rsp.headers_mut().insert(
            actix_web::http::header::CONTENT_TYPE,
            actix_web::http::header::HeaderValue::from_static("application/ld+json"),
        );
        set_wac_allow(&mut rsp, &wac_allow);
        set_link_headers(&mut rsp, &path);
        return Ok(rsp);
    }

    match state.storage.get(&path).await {
        Ok((body, meta)) => {
            let mut rsp = HttpResponse::Ok().body(body.to_vec());
            rsp.headers_mut().insert(
                actix_web::http::header::CONTENT_TYPE,
                actix_web::http::header::HeaderValue::from_str(&meta.content_type).unwrap_or_else(
                    |_| {
                        actix_web::http::header::HeaderValue::from_static(
                            "application/octet-stream",
                        )
                    },
                ),
            );
            if let Ok(etag) = actix_web::http::header::HeaderValue::from_str(&format!(
                "\"{}\"",
                meta.etag
            )) {
                rsp.headers_mut()
                    .insert(actix_web::http::header::ETAG, etag);
            }
            set_wac_allow(&mut rsp, &wac_allow);
            set_link_headers(&mut rsp, &path);
            Ok(rsp)
        }
        Err(PodError::NotFound(_)) => Ok(HttpResponse::NotFound().finish()),
        Err(e) => Err(to_actix(e)),
    }
}

async fn handle_put(
    req: HttpRequest,
    body: web::Bytes,
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = req.uri().path().to_string();
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
    set_link_headers(&mut rsp, &path);
    Ok(rsp)
}

async fn handle_delete(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = req.uri().path().to_string();
    match state.storage.delete(&path).await {
        Ok(()) => Ok(HttpResponse::NoContent().finish()),
        Err(PodError::NotFound(_)) => Ok(HttpResponse::NotFound().finish()),
        Err(e) => Err(to_actix(e)),
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Tracing — honour --log > RUST_LOG > "info".
    let filter = cli
        .log
        .clone()
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_target(true)
        .init();

    // F6 layered config load.
    let mut loader = ConfigLoader::new().with_defaults();
    if let Some(path) = cli.config.as_deref() {
        loader = loader.with_file(path);
    }
    let mut cfg: ServerConfig = loader
        .with_env()
        .load()
        .await
        .context("load server config")?;

    // CLI flag overrides (highest precedence per F6 semantics).
    if let Some(host) = cli.host.clone() {
        cfg.server.host = host;
    }
    if let Some(port) = cli.port {
        cfg.server.port = port;
    }
    cfg.validate().map_err(anyhow::Error::msg)?;

    let host = cfg.server.host.clone();
    let port = cfg.server.port;
    let bind_addr = format!("{host}:{port}");

    // Materialise storage + app state.
    let storage = build_storage(&cfg.storage).await?;
    let state = AppState { storage };

    // Warn about features the operator asked for but this build can't serve.
    if !cfg.auth.oidc_enabled {
        warn!("auth.oidc_enabled=false — DPoP / OIDC routes disabled");
    }

    info!(%bind_addr, "solid-pod-rs-server starting");

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .route("/{tail:.*}", web::get().to(handle_get))
            .route("/{tail:.*}", web::head().to(handle_get))
            .route("/{tail:.*}", web::put().to(handle_put))
            .route("/{tail:.*}", web::delete().to(handle_delete))
    })
    .bind(&bind_addr)
    .with_context(|| format!("bind {bind_addr}"))?
    .shutdown_timeout(30)
    .run();

    let server_handle = server.handle();

    // Supervisor task for SIGTERM / SIGINT → graceful shutdown.
    let shutdown = tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("SIGINT received — initiating graceful shutdown");
            }
            _ = terminate_signal() => {
                info!("SIGTERM received — initiating graceful shutdown");
            }
        }
        server_handle.stop(true).await;
    });

    server.await.context("HTTP server exited with error")?;
    let _ = shutdown.await;
    info!("solid-pod-rs-server stopped cleanly");
    Ok(())
}

/// Resolves when the process receives SIGTERM on Unix. On other
/// platforms, this future is pending forever.
#[cfg(unix)]
async fn terminate_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    if let Ok(mut stream) = signal(SignalKind::terminate()) {
        stream.recv().await;
    } else {
        std::future::pending::<()>().await;
    }
}

#[cfg(not(unix))]
async fn terminate_signal() {
    std::future::pending::<()>().await;
}
