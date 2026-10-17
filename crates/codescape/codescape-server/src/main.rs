//! Codescape server binary.
//!
//! Boots every `--repo` (interner load or epoch bump, extract, publish),
//! starts the ingest and layout threads, serves the main port (API, WebSockets,
//! static client) and, when a token file is configured, the presence-ingest
//! listener. SIGTERM or Ctrl-C stops both listeners gracefully, stops the
//! worker threads, and persists ids and positions.

#![forbid(unsafe_code)]

mod engine;
mod frames;
mod ingest_sink;
mod queries;
mod routes;
mod state;

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpServer};
use clap::Parser;
use codescape_collab::PathMap;
use codescape_server::auth::{build_verifier, IngestToken};
use codescape_server::config::{check_environment, Config};
use codescape_server::egress::EgressAllowlist;
use codescape_server::http::{
    api_rate_gate, auth_gate, configure_not_served, configure_static, isolation_headers,
    ApiRateData, VerifierData,
};
use codescape_server::ingest_api::{self, IngestState};
use codescape_server::origin::OriginPolicy;
use codescape_server::persist::ActorIds;
use codescape_server::{now_ms, VERSION};

use crate::engine::RepoEngine;
use crate::state::AppState;

/// Upper bound on main-port worker threads (WebSocket tasks are async; more threads buy nothing).
const MAX_WORKERS: usize = 8;

fn main() -> ExitCode {
    // Before anything else, including argument parsing: a wired memory store refuses boot.
    if let Err(e) = check_environment(|k| std::env::var_os(k)) {
        eprintln!("codescape-server: {e}");
        return ExitCode::from(2);
    }
    let cfg = Config::parse();
    if let Err(e) = cfg.validate() {
        eprintln!("codescape-server: {e}");
        return ExitCode::from(2);
    }
    let filter = tracing_subscriber::EnvFilter::try_new(&cfg.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match actix_web::rt::System::new().block_on(run(cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %format!("{e:#}"), "codescape-server failed");
            ExitCode::FAILURE
        }
    }
}

fn load_actor_ids(cfg: &Config) -> ActorIds {
    let path = cfg.data_dir.join("actors.bin");
    match std::fs::read(&path) {
        Ok(b) => ActorIds::from_bytes(&b, 0).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "actors.bin unreadable; starting a new table");
            ActorIds::new(0)
        }),
        Err(_) => ActorIds::new(0),
    }
}

async fn run(cfg: Config) -> anyhow::Result<()> {
    let verifier = build_verifier(cfg.require_auth)?;
    let egress = EgressAllowlist::from_urls(cfg.egress_urls())?;
    let origins = OriginPolicy::new(&cfg.allowed_origins)?;
    let path_map = match cfg.path_map.as_deref() {
        Some(s) => PathMap::parse(s).map_err(|e| anyhow::anyhow!("CODESCAPE_PATH_MAP: {e}"))?,
        None => PathMap::new(),
    };
    // Fail closed: a configured but unreadable or empty token refuses boot.
    let ingest_token = cfg
        .ingest_token_file
        .as_deref()
        .map(IngestToken::from_file)
        .transpose()?;
    std::fs::create_dir_all(&cfg.data_dir)?;
    for r in &cfg.repos {
        anyhow::ensure!(
            r.root.is_dir(),
            "repo `{}`: {} is not a directory",
            r.label,
            r.root.display()
        );
    }
    tracing::info!(
        version = VERSION,
        repos = cfg.repos.len(),
        egress = egress.len(),
        auth = verifier.name(),
        "codescape-server starting"
    );

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut repos = Vec::new();
    let mut threads = Vec::new();
    for spec in &cfg.repos {
        let (cfg2, spec2) = (cfg.clone(), spec.clone());
        let (engine, t) = web::block(move || RepoEngine::open(&cfg2, &spec2)).await??;
        threads.extend(engine.spawn(t, shutdown.clone()));
        repos.push(engine);
    }

    let cfg = Arc::new(cfg);
    let st = web::Data::new(AppState::new(
        cfg.clone(),
        repos,
        egress,
        verifier,
        path_map,
        load_actor_ids(&cfg),
        origins,
        shutdown.clone(),
    ));

    let main_state = st.clone();
    let static_dir = cfg.static_dir.clone();
    let verifier_data = web::Data::new(VerifierData {
        verifier: st.verifier.clone(),
        public_origin: cfg.public_origin(),
    });
    let api_rate = web::Data::new(ApiRateData(codescape_server::auth::IpBuckets::api()));
    let main_server = HttpServer::new(move || {
        // Last wrap is outermost: isolation headers cover even an auth rejection.
        App::new()
            .app_data(main_state.clone())
            .app_data(verifier_data.clone())
            .app_data(api_rate.clone())
            .app_data(web::JsonConfig::default().limit(64 * 1024))
            .wrap(from_fn(auth_gate))
            .wrap(from_fn(api_rate_gate))
            .wrap(isolation_headers())
            .configure(routes::configure)
            .configure(configure_not_served)
            .configure(|c| configure_static(c, static_dir.clone()))
    })
    .disable_signals()
    .shutdown_timeout(5)
    .workers(
        std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(MAX_WORKERS),
    )
    .bind((cfg.bind.as_str(), cfg.port))?
    .run();
    let main_handle = main_server.handle();
    tracing::info!(addr = %format!("{}:{}", cfg.bind, cfg.port), "main listener up");

    let ingest_server = match ingest_token {
        Some(token) => {
            let sink: Arc<dyn ingest_api::ActivitySink> =
                Arc::new(ingest_sink::EngineSink::new(st.clone()));
            let istate = web::Data::new(IngestState::new(token, sink));
            let server = HttpServer::new(move || {
                App::new().configure(|c| ingest_api::configure(c, istate.clone()))
            })
            .disable_signals()
            .shutdown_timeout(2)
            .workers(2)
            .bind((cfg.bind.as_str(), cfg.ingest_port))?
            .run();
            tracing::info!(addr = %format!("{}:{}", cfg.bind, cfg.ingest_port), "ingest listener up");
            Some(server)
        }
        None => {
            tracing::info!("no CODESCAPE_INGEST_TOKEN_FILE: presence ingest listener disabled");
            None
        }
    };
    let ingest_handle = ingest_server.as_ref().map(|s| s.handle());

    // Floor hold cap.
    let floor_state = st.clone();
    actix_web::rt::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(1));
        loop {
            t.tick().await;
            let changed = floor_state.floor.lock().tick(now_ms());
            if let Some(s) = changed {
                floor_state.broadcast_floor(s);
            }
        }
    });

    // Graceful shutdown on SIGTERM / Ctrl-C.
    let sd = shutdown.clone();
    actix_web::rt::spawn(async move {
        let term = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(_) => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = term => tracing::info!("SIGTERM: shutting down"),
            _ = tokio::signal::ctrl_c() => tracing::info!("interrupt: shutting down"),
        }
        sd.store(true, Ordering::Relaxed);
        if let Some(h) = ingest_handle {
            h.stop(true).await;
        }
        main_handle.stop(true).await;
    });

    match ingest_server {
        Some(i) => {
            let (a, b) = futures_util::future::join(main_server, i).await;
            a?;
            b?;
        }
        None => main_server.await?,
    }
    shutdown.store(true, Ordering::Relaxed);
    web::block(move || {
        for t in threads {
            let _ = t.join();
        }
    })
    .await?;
    tracing::info!("codescape-server stopped");
    Ok(())
}
