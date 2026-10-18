//! Cross-cutting HTTP pieces for the main port: isolation headers, the static
//! desktop bundle with SPA fallback, and the routes Codescape deliberately does
//! not serve.
//!
//! The headers wrap the whole `App`, so API responses and 404s carry them too.
//! `Cross-Origin-Opener-Policy: same-origin` with
//! `Cross-Origin-Embedder-Policy: require-corp` makes the page
//! `crossOriginIsolated`, which the client's SharedArrayBuffer position path
//! requires.

use std::path::PathBuf;
use std::sync::Arc;

use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::{DefaultHeaders, Next};
use actix_web::{web, HttpRequest, HttpResponse};

use crate::auth::Verifier;

/// App data carrying the request verifier for [`auth_gate`].
#[derive(Clone)]
pub struct VerifierData {
    /// The verifier.
    pub verifier: Arc<dyn Verifier>,
    /// How the absolute URL NIP-98 signs is rebuilt: `--public-origin`, the
    /// forwarded origin under `--trust-proxy`, or the bind-derived default; never
    /// the client-controlled `Host` header.
    pub public: crate::proxy::PublicOrigin,
}

/// Per-address rate limit on `/api/*` for [`api_rate_gate`].
pub struct ApiRateData {
    /// The buckets.
    pub buckets: crate::auth::IpBuckets,
    /// Which address a request is charged to (TCP peer, or the trusted forwarded hop).
    pub proxy: crate::proxy::ProxyTrust,
}

/// Middleware (with `from_fn`): at most 20 requests per second (burst 40) per
/// client address on `/api/*` (the TCP peer, or the last `X-Forwarded-For` hop
/// under `--trust-proxy`); 429 JSON beyond. Register before
/// [`isolation_headers`] so the 429 carries them.
pub async fn api_rate_gate(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, actix_web::Error> {
    if req.path().starts_with("/api/") {
        let limiter = req.app_data::<web::Data<ApiRateData>>();
        let ip = limiter.and_then(|l| {
            l.proxy
                .client_ip(req.headers(), req.peer_addr().map(|a| a.ip()))
        });
        if let (Some(l), Some(ip)) = (limiter, ip) {
            if !l.buckets.try_take(ip) {
                let resp = HttpResponse::TooManyRequests()
                    .insert_header(("Retry-After", "1"))
                    .json(serde_json::json!({"error": "rate limited"}));
                return Ok(req.into_response(resp).map_into_right_body());
            }
        }
    }
    Ok(next.call(req).await?.map_into_left_body())
}

/// The absolute URL a verifier should check: the public origin plus the request target.
pub fn public_url(public_origin: &str, uri: &actix_web::http::Uri) -> String {
    let target = uri.path_and_query().map_or("/", |p| p.as_str());
    format!("{}{target}", public_origin.trim_end_matches('/'))
}

/// Middleware (use with `actix_web::middleware::from_fn`): verify every `/api/` request
/// with the [`VerifierData`] registered on the app, answering 401 JSON on failure.
/// WebSocket routes verify inside their handlers (the first-message envelope).
///
/// Register it *before* [`isolation_headers`] (`.wrap(from_fn(auth_gate)).wrap(isolation_headers())`)
/// so the headers layer is outermost and a 401 carries them too.
pub async fn auth_gate(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, actix_web::Error> {
    if req.path().starts_with("/api/") {
        if let Some(v) = req.app_data::<web::Data<VerifierData>>() {
            let auth = req
                .headers()
                .get(actix_web::http::header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string);
            let peer = req.peer_addr().map(|a| a.ip());
            let url = v.public.url_for(req.headers(), peer, req.uri());
            if let Err(e) = v
                .verifier
                .verify_http(req.method().as_str(), &url, auth.as_deref())
            {
                let resp =
                    HttpResponse::Unauthorized().json(serde_json::json!({"error": e.to_string()}));
                return Ok(req.into_response(resp).map_into_right_body());
            }
        }
    }
    Ok(next.call(req).await?.map_into_left_body())
}

/// The three isolation headers, added to every response that does not set them itself.
pub fn isolation_headers() -> DefaultHeaders {
    DefaultHeaders::new()
        .add(("Cross-Origin-Opener-Policy", "same-origin"))
        .add(("Cross-Origin-Embedder-Policy", "require-corp"))
        .add(("Cross-Origin-Resource-Policy", "same-origin"))
        .add(("X-Content-Type-Options", "nosniff"))
}

/// JSON 404.
pub fn not_found_json(what: &str) -> HttpResponse {
    HttpResponse::NotFound().json(serde_json::json!({ "error": "not found", "path": what }))
}

async fn not_served(req: HttpRequest) -> HttpResponse {
    not_found_json(req.path())
}

/// Knowledge-work routes the XR boot sequence probes; Codescape answers 404 (ADR-2143 §3).
pub const NOT_SERVED: &[&str] = &[
    "/api/memory-cloud",
    "/api/analytics",
    "/api/graph/fold",
    "/api/canary",
];

/// Register explicit 404s for [`NOT_SERVED`] (before the static fallback can claim them).
pub fn configure_not_served(cfg: &mut web::ServiceConfig) {
    for p in NOT_SERVED {
        cfg.route(p, web::route().to(not_served));
        cfg.route(&format!("{p}/{{tail:.*}}"), web::route().to(not_served));
    }
}

/// `true` for paths that must never fall back to `index.html`.
pub fn is_backend_path(path: &str) -> bool {
    path == "/api"
        || path.starts_with("/api/")
        || path.starts_with("/ws")
        || path.starts_with("/ingest/")
}

/// Serve `dir` at `/` with SPA fallback to `index.html`; backend paths get JSON 404.
/// Without a directory every unmatched path is a JSON 404. Register last.
pub fn configure_static(cfg: &mut web::ServiceConfig, dir: Option<PathBuf>) {
    match dir {
        Some(dir) => {
            let index = dir.join("index.html");
            let files = actix_files::Files::new("/", dir)
                .index_file("index.html")
                .prefer_utf8(true)
                .default_handler(actix_web::dev::fn_service(move |req: ServiceRequest| {
                    let index = index.clone();
                    async move {
                        let (req, _) = req.into_parts();
                        if is_backend_path(req.path()) {
                            let res = not_found_json(req.path());
                            return Ok(ServiceResponse::new(req, res));
                        }
                        match actix_files::NamedFile::open_async(&index).await {
                            Ok(f) => {
                                let res = f.into_response(&req);
                                Ok(ServiceResponse::new(req, res))
                            }
                            Err(_) => {
                                let res = not_found_json(req.path());
                                Ok(ServiceResponse::new(req, res))
                            }
                        }
                    }
                }));
            cfg.service(files);
        }
        None => {
            cfg.default_service(web::route().to(not_served));
        }
    }
}

#[cfg(test)]
mod tests {
    use actix_web::{test, App};

    use super::*;

    fn has_isolation<B>(res: &ServiceResponse<B>) {
        let h = res.headers();
        assert_eq!(h.get("cross-origin-opener-policy").unwrap(), "same-origin");
        assert_eq!(
            h.get("cross-origin-embedder-policy").unwrap(),
            "require-corp"
        );
        assert_eq!(
            h.get("cross-origin-resource-policy").unwrap(),
            "same-origin"
        );
    }

    #[actix_web::test]
    async fn static_spa_and_404s_all_carry_isolation_headers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>codescape</html>").unwrap();
        std::fs::write(dir.path().join("app.js"), "console.log(1)").unwrap();
        let app = test::init_service(
            App::new()
                .wrap(isolation_headers())
                .route(
                    "/api/health",
                    web::get()
                        .to(|| async { HttpResponse::Ok().json(serde_json::json!({"ok":true})) }),
                )
                .configure(configure_not_served)
                .configure(|c| configure_static(c, Some(dir.path().to_path_buf()))),
        )
        .await;

        for (path, status, body_has) in [
            ("/", 200, "codescape"),
            ("/app.js", 200, "console"),
            ("/graph/deep/link", 200, "codescape"),
            ("/api/health", 200, "ok"),
            ("/api/memory-cloud/query", 404, "not found"),
            ("/api/analytics/cycles", 404, "not found"),
            ("/api/graph/fold", 404, "not found"),
            ("/api/canary/observe/1", 404, "not found"),
            ("/api/unknown", 404, "not found"),
        ] {
            let res =
                test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
            assert_eq!(res.status().as_u16(), status, "{path}");
            has_isolation(&res);
            let body = test::read_body(res).await;
            assert!(String::from_utf8_lossy(&body).contains(body_has), "{path}");
        }
    }

    struct DenyAll;
    impl Verifier for DenyAll {
        fn name(&self) -> &'static str {
            "deny"
        }
        fn verify_http(
            &self,
            _: &str,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::auth::Principal, crate::auth::AuthError> {
            Err(crate::auth::AuthError::Missing)
        }
        fn verify_ws_envelope(
            &self,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<crate::auth::Principal, crate::auth::AuthError> {
            Err(crate::auth::AuthError::Missing)
        }
    }

    #[actix_web::test]
    async fn api_rate_limit_is_per_peer_and_keeps_isolation_headers() {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(ApiRateData {
                    buckets: crate::auth::IpBuckets::new(1, 2),
                    proxy: crate::proxy::ProxyTrust::off(),
                }))
                .wrap(actix_web::middleware::from_fn(api_rate_gate))
                .wrap(isolation_headers())
                .route(
                    "/api/health",
                    web::get().to(|| async { HttpResponse::Ok().finish() }),
                ),
        )
        .await;
        let call = |ip: &str| {
            test::TestRequest::get()
                .uri("/api/health")
                .peer_addr(format!("{ip}:5000").parse().unwrap())
                .to_request()
        };
        let mut codes = vec![];
        for _ in 0..3 {
            codes.push(
                test::call_service(&app, call("10.0.0.1"))
                    .await
                    .status()
                    .as_u16(),
            );
        }
        assert_eq!(codes, vec![200, 200, 429]);
        let res = test::call_service(&app, call("10.0.0.1")).await;
        has_isolation(&res);
        assert_eq!(
            test::call_service(&app, call("10.0.0.2"))
                .await
                .status()
                .as_u16(),
            200
        );
    }

    #[actix_web::test]
    async fn api_rate_limit_keys_on_forwarded_hop_only_when_trusted() {
        async fn codes(proxy: crate::proxy::ProxyTrust) -> Vec<u16> {
            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(ApiRateData {
                        buckets: crate::auth::IpBuckets::new(1, 1),
                        proxy,
                    }))
                    .wrap(actix_web::middleware::from_fn(api_rate_gate))
                    .route(
                        "/api/health",
                        web::get().to(|| async { HttpResponse::Ok().finish() }),
                    ),
            )
            .await;
            let mut out = vec![];
            // Two clients behind one proxy peer.
            for client in ["192.168.2.50", "192.168.2.51"] {
                let req = test::TestRequest::get()
                    .uri("/api/health")
                    .peer_addr("172.18.0.2:5000".parse().unwrap())
                    .insert_header(("X-Forwarded-For", client))
                    .to_request();
                out.push(test::call_service(&app, req).await.status().as_u16());
            }
            out
        }
        assert_eq!(
            codes(crate::proxy::ProxyTrust::off()).await,
            vec![200, 429],
            "trust off: both are the proxy peer"
        );
        assert_eq!(
            codes(crate::proxy::ProxyTrust::new(true, "").unwrap()).await,
            vec![200, 200],
            "trust on: charged to each forwarded client"
        );
    }

    #[::core::prelude::v1::test]
    fn public_url_ignores_the_host_header() {
        let uri: actix_web::http::Uri = "/api/graph/search?q=x".parse().unwrap();
        assert_eq!(
            public_url("https://codescape.lan/", &uri),
            "https://codescape.lan/api/graph/search?q=x"
        );
    }

    #[actix_web::test]
    async fn auth_rejections_still_carry_isolation_headers() {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(VerifierData {
                    verifier: Arc::new(DenyAll),
                    public: crate::proxy::PublicOrigin::new(
                        None,
                        "http://localhost:4100".into(),
                        crate::proxy::ProxyTrust::off(),
                    ),
                }))
                .wrap(actix_web::middleware::from_fn(auth_gate))
                .wrap(isolation_headers())
                .route(
                    "/api/health",
                    web::get().to(|| async { HttpResponse::Ok().finish() }),
                )
                .configure(|c| configure_static(c, None)),
        )
        .await;
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/health").to_request(),
        )
        .await;
        assert_eq!(res.status().as_u16(), 401);
        has_isolation(&res);
        // Non-API paths are not gated by the HTTP verifier.
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/index.html").to_request(),
        )
        .await;
        assert_eq!(res.status().as_u16(), 404);
        has_isolation(&res);
    }

    #[actix_web::test]
    async fn without_static_dir_everything_unknown_is_json_404() {
        let app = test::init_service(
            App::new()
                .wrap(isolation_headers())
                .configure(|c| configure_static(c, None)),
        )
        .await;
        let res = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;
        assert_eq!(res.status().as_u16(), 404);
        has_isolation(&res);
    }

    #[::core::prelude::v1::test]
    fn backend_paths() {
        assert!(is_backend_path("/api/x"));
        assert!(is_backend_path("/wss"));
        assert!(is_backend_path("/ws/events"));
        assert!(!is_backend_path("/apiary"));
        assert!(!is_backend_path("/graph"));
    }
}
