//! Central RBAC gate for the `/api` scope (ADR-142).
//!
//! Ports the intent of the reference `enterprise_auth::RequireRole` middleware
//! (`sprint-3/jss-cut-scaffold`) into `main`, but bound to the NIP-98-verified
//! pubkey rather than a spoofable `X-Enterprise-Role` header, and applied
//! *centrally* to the whole `/api` scope instead of per-route. This closes the
//! "15+ endpoints missing auth" gap (CQRS gap analysis / AUTH-001) in one place
//! without editing every handler's `configure` fn.
//!
//! ## Policy
//!
//! Path/role matching is **segment-aware** (whole `/`-delimited segments), so
//! `/api/administrator` does NOT inherit `/api/admin` policy and `/api/health-x`
//! does NOT inherit the public allowlist.
//!
//! - **Public allowlist** (any method, no auth): the auth endpoints themselves
//!   (`/api/auth/*`) — you cannot require a session to *create* one — plus
//!   `/api/client-logs` and the liveness/health probes.
//! - **`/api/admin/*`** → `Admin` for *every* method.
//! - **Safe methods** (GET/HEAD/OPTIONS) elsewhere → gated at `ReadOnly`
//!   (any authenticated user) by default. Because the current single-operator
//!   deployment serves anonymous graph reads, `RBAC_PUBLIC_READS=1` (default on,
//!   but explicit and visible) keeps those reads public; set it to `0` to
//!   require authentication on every read.
//! - **Mutating methods** (POST/PUT/PATCH/DELETE) elsewhere →
//!   `AccessLevel::WriteSettings` under `/api/settings`, otherwise
//!   `AccessLevel::Authenticated` (any Editor+).
//!
//! ## Enforcement mode
//!
//! `RBAC_GATE_MODE=report` turns denials into logs instead of 401/403. Because a
//! stray env var must never silently disable auth in production, report mode
//! **refuses to activate** unless the build has `debug_assertions` OR
//! `RBAC_REPORT_MODE_ACK` equals today's UTC date (`YYYY-MM-DD`). While active it
//! logs at `error` level, both at startup and on every request it waves through.

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    web, Error, HttpMessage, HttpResponse,
};
use chrono::Utc;
use futures_util::future::LocalBoxFuture;
use log::{debug, error, warn};
use std::future::{ready, Ready};
use std::rc::Rc;

use crate::config::security_profile::{
    report_mode_acknowledged, report_mode_requested, BuildIdentity, EnvSnapshot,
};
use crate::services::nostr_service::NostrService;
use crate::utils::auth::{verify_access, AccessLevel};

/// Public path prefixes, expressed as whole-segment lists (matched against the
/// full request path, which includes the `/api` scope prefix).
const PUBLIC_SEGMENT_PREFIXES: &[&[&str]] = &[
    &["api", "auth"], // login / verify / refresh / logout — self-authenticating
    &["api", "client-logs"],
    &["api", "health"],
    &["api", "healthz"],
    &["api", "readyz"],
];

/// Split a path into non-empty `/`-delimited segments.
/// The path the router matches. Actix decodes percent-escapes (all but the
/// protected `/`, `%` and `+`) before matching routes, while `req.path()` is
/// the raw URI, so classifying the raw path let `/api/%73ettings/...` route
/// to the settings handlers under the graph-write level, and `/api/%61dmin`
/// skip the Admin rule (estate register, 2026-10-18 security review).
fn routed_path(req: &ServiceRequest) -> &str {
    req.match_info().as_str()
}

fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Does `path`'s leading segments equal `prefix` (segment-wise)?
fn has_segment_prefix(path_segs: &[&str], prefix: &[&str]) -> bool {
    path_segs.len() >= prefix.len() && path_segs[..prefix.len()] == *prefix
}

/// Whether the gate should actually deny, or merely log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GateMode {
    Enforce,
    Report,
}

impl GateMode {
    /// Resolve the mode, refusing to honour `report` unless it is explicitly
    /// acknowledged (debug build, or `RBAC_REPORT_MODE_ACK` = today's UTC date).
    fn from_env() -> Self {
        let env = EnvSnapshot::from_process();
        if !report_mode_requested(&env) {
            return GateMode::Enforce;
        }
        if Self::report_acknowledged() {
            error!(
                "RBAC_GATE_MODE=report is ACTIVE — /api auth denials are being LOGGED, not enforced. \
                 This must not be used in production."
            );
            GateMode::Report
        } else {
            error!(
                "RBAC_GATE_MODE=report requested but NOT acknowledged — refusing to disable auth. \
                 Set RBAC_REPORT_MODE_ACK={} (today's UTC date) on a debug build to enable it. \
                 Falling back to enforce.",
                Utc::now().format("%Y-%m-%d")
            );
            GateMode::Enforce
        }
    }

    /// ADR-2012: the dated-acknowledgement rule has exactly one implementation,
    /// in `config::security_profile`, shared with the boot-time profile
    /// assertion so the gate and the boot check cannot drift apart. Note that
    /// the boot assertion rejects report mode outright in a production
    /// artefact — reaching this code in such a build means the process should
    /// already have exited.
    fn report_acknowledged() -> bool {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        report_mode_acknowledged(
            &EnvSnapshot::from_process(),
            BuildIdentity::current(),
            &today,
        )
    }
}

/// Whether anonymous safe-method reads are permitted. **Fails closed**: absence
/// of the flag means reads require authentication (Codex round-2 — the absence
/// of a security flag must never widen access). The current single-operator
/// deployment sets `RBAC_PUBLIC_READS=1` explicitly in its env to keep anonymous
/// reads, so the behaviour is visible rather than a hidden structural default.
fn public_reads_enabled() -> bool {
    std::env::var("RBAC_PUBLIC_READS")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Compute the minimum [`AccessLevel`] required for `method` + `path`, or `None`
/// when the route is public. `public_reads` toggles anonymous safe-method reads.
/// Pure function — unit-tested below.
pub(crate) fn required_level(
    method: &actix_web::http::Method,
    path: &str,
    public_reads: bool,
) -> Option<AccessLevel> {
    let segs = segments(path);

    if PUBLIC_SEGMENT_PREFIXES
        .iter()
        .any(|p| has_segment_prefix(&segs, p))
    {
        return None;
    }
    // `whoami` reports the caller's own role and power-user flag, so any
    // signed-in user may read it; the handler verifies the caller itself.
    // Without this the Admin rule below refused it to everyone but admins,
    // and the client could never learn a power user's status (ER-26).
    if *method == actix_web::http::Method::GET && segs == ["api", "admin", "rbac", "whoami"] {
        return Some(AccessLevel::Authenticated);
    }
    // The admin surface is sensitive for reads and writes alike.
    if has_segment_prefix(&segs, &["api", "admin"]) {
        return Some(AccessLevel::Admin);
    }
    // Safe reads: public when the flag is on, else any authenticated user.
    if method.is_safe() {
        return if public_reads {
            None
        } else {
            Some(AccessLevel::ReadOnly)
        };
    }
    // Mutations: settings writes need the settings-write level; everything else
    // needs at least Editor. NB: the requirement is `WriteGraph`, not
    // `Authenticated` — `AccessLevel::has_permission` treats a required
    // `Authenticated` as "any authenticated user" (a Viewer would pass), whereas
    // `WriteGraph` is satisfied by Editor(→Authenticated)/Admin but NOT by a
    // Viewer(→ReadOnly). This is what actually denies Viewer writes.
    if has_segment_prefix(&segs, &["api", "settings"]) {
        Some(AccessLevel::WriteSettings)
    } else {
        Some(AccessLevel::WriteGraph)
    }
}

/// Actix `Transform` enforcing [`required_level`] across the wrapped scope.
pub struct RbacGate {
    mode: GateMode,
    public_reads: bool,
}

impl RbacGate {
    /// Construct from env: `RBAC_GATE_MODE` (default `enforce`) and
    /// `RBAC_PUBLIC_READS` (default on).
    pub fn from_env() -> Self {
        let mode = GateMode::from_env();
        let public_reads = public_reads_enabled();
        debug!(
            "RbacGate initialised: mode={:?}, public_reads={}",
            mode, public_reads
        );
        if public_reads {
            warn!(
                "RbacGate: anonymous /api reads ENABLED via RBAC_PUBLIC_READS=1 — reads bypass auth"
            );
        } else {
            debug!("RbacGate: anonymous reads DISABLED (default) — every /api read requires auth");
        }
        Self { mode, public_reads }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RbacGate
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = RbacGateMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RbacGateMiddleware {
            service: Rc::new(service),
            mode: self.mode,
            public_reads: self.public_reads,
        }))
    }
}

pub struct RbacGateMiddleware<S> {
    service: Rc<S>,
    mode: GateMode,
    public_reads: bool,
}

impl<S, B> Service<ServiceRequest> for RbacGateMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<actix_web::body::BoxBody>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let svc = self.service.clone();
        let mode = self.mode;

        let required = required_level(req.method(), routed_path(&req), self.public_reads);

        let Some(level) = required else {
            // Public route — pass straight through.
            return Box::pin(async move {
                let resp = svc.call(req).await?;
                Ok(resp.map_into_boxed_body())
            });
        };

        Box::pin(async move {
            let nostr_service = match req.app_data::<web::Data<NostrService>>() {
                Some(service) => service.clone(),
                None => {
                    warn!("RbacGate: NostrService missing from app data");
                    if mode == GateMode::Enforce {
                        let resp = HttpResponse::Unauthorized().body("Unauthorized");
                        return Ok(req.into_response(resp).map_into_boxed_body());
                    }
                    let resp = svc.call(req).await?;
                    return Ok(resp.map_into_boxed_body());
                }
            };

            match verify_access(req.request(), &nostr_service, level.clone()).await {
                Ok(pubkey) => {
                    // Expose the authenticated pubkey to downstream handlers.
                    req.extensions_mut()
                        .insert(crate::middleware::auth::AuthenticatedUser { pubkey });
                    let resp = svc.call(req).await?;
                    Ok(resp.map_into_boxed_body())
                }
                Err(deny_response) => {
                    if mode == GateMode::Enforce {
                        Ok(req.into_response(deny_response).map_into_boxed_body())
                    } else {
                        // Continuous error-level logging while report mode is active.
                        error!(
                            "RBAC_GATE[report] would DENY {} {} (needs {:?}) — allowed because auth is NOT enforced",
                            req.method(),
                            req.path(),
                            level
                        );
                        let resp = svc.call(req).await?;
                        Ok(resp.map_into_boxed_body())
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::Method;

    // Default deployment: public reads on.
    fn lvl(m: Method, p: &str) -> Option<AccessLevel> {
        required_level(&m, p, true)
    }

    #[test]
    fn auth_endpoints_are_public() {
        assert_eq!(lvl(Method::POST, "/api/auth/nostr"), None);
        assert_eq!(lvl(Method::POST, "/api/auth/nostr/verify"), None);
        assert_eq!(lvl(Method::DELETE, "/api/auth/nostr"), None);
        assert_eq!(lvl(Method::POST, "/api/client-logs"), None);
        assert_eq!(lvl(Method::GET, "/api/healthz"), None);
    }

    /// The gate wraps the `/api` scope in `main.rs`, not the app root. Inside
    /// a scope middleware `match_info().as_str()` must still be the full,
    /// decoded path; were it scope-relative, nothing would start with `api`
    /// and every route would classify as public.
    #[actix_web::test]
    async fn routed_path_is_the_full_decoded_path_inside_the_api_scope() {
        use actix_web::{dev::Service, test, web, App, HttpResponse};
        let app = test::init_service(
            App::new().service(
                web::scope("/api")
                    .wrap_fn(|req, srv| {
                        let seen = routed_path(&req).to_string();
                        let fut = srv.call(req);
                        async move {
                            let mut res = fut.await?;
                            res.headers_mut().insert(
                                actix_web::http::header::HeaderName::from_static("x-routed"),
                                actix_web::http::header::HeaderValue::from_str(&seen).unwrap(),
                            );
                            Ok(res)
                        }
                    })
                    .route("/settings/physics", web::put().to(HttpResponse::Ok)),
            ),
        )
        .await;
        let req = test::TestRequest::put()
            .uri("/api/%73ettings/physics")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success(), "the encoded path must route");
        assert_eq!(
            resp.headers().get("x-routed").unwrap(),
            "/api/settings/physics"
        );
    }

    #[test]
    fn the_gate_classifies_the_path_the_router_matches() {
        use actix_web::test::TestRequest;
        for (raw, routed) in [
            ("/api/%73ettings/physics", "/api/settings/physics"),
            ("/api/%61dmin/rbac/users", "/api/admin/rbac/users"),
            ("/api/admin/rbac/%77hoami", "/api/admin/rbac/whoami"),
        ] {
            let req = TestRequest::with_uri(raw).to_srv_request();
            assert_eq!(routed_path(&req), routed, "router view of {raw}");
        }
        let lvl = |m: Method, raw: &str| {
            let req = TestRequest::with_uri(raw)
                .method(m.clone())
                .to_srv_request();
            required_level(&m, routed_path(&req), true)
        };
        assert_eq!(
            lvl(Method::PUT, "/api/%73ettings/physics"),
            lvl(Method::PUT, "/api/settings/physics")
        );
        assert_eq!(
            lvl(Method::GET, "/api/%61dmin/rbac/users"),
            Some(AccessLevel::Admin)
        );
        // The raw path classified these differently: the bug being fixed.
        assert_ne!(
            required_level(&Method::PUT, "/api/%73ettings/physics", true),
            required_level(&Method::PUT, "/api/settings/physics", true)
        );
        assert_ne!(
            required_level(&Method::GET, "/api/%61dmin/rbac/users", true),
            Some(AccessLevel::Admin)
        );
        // An encoded slash stays encoded for the router too, so it cannot split
        // a segment to dodge a prefix rule.
        let req = TestRequest::with_uri("/api/admin%2Frbac/users").to_srv_request();
        assert!(routed_path(&req).contains("%2F"));
    }

    #[test]
    fn whoami_is_open_to_any_signed_in_user_and_nothing_else_is() {
        let lvl = |m: Method, p: &str| required_level(&m, p, true);
        assert_eq!(
            lvl(Method::GET, "/api/admin/rbac/whoami"),
            Some(AccessLevel::Authenticated)
        );
        assert_eq!(
            lvl(Method::POST, "/api/admin/rbac/whoami"),
            Some(AccessLevel::Admin)
        );
        assert_eq!(
            lvl(Method::GET, "/api/admin/rbac/whoami/x"),
            Some(AccessLevel::Admin)
        );
        assert_eq!(
            lvl(Method::GET, "/api/admin/rbac/users"),
            Some(AccessLevel::Admin)
        );
    }

    #[test]
    fn admin_surface_requires_admin_for_all_methods() {
        assert_eq!(
            lvl(Method::GET, "/api/admin/rbac/users"),
            Some(AccessLevel::Admin)
        );
        assert_eq!(
            lvl(Method::PUT, "/api/admin/rbac/users/abc/role"),
            Some(AccessLevel::Admin)
        );
    }

    #[test]
    fn segment_matching_does_not_leak_prefixes() {
        // /api/administrator must NOT inherit /api/admin's Admin policy — as a
        // non-admin write it falls to the default mutation level.
        assert_eq!(
            lvl(Method::POST, "/api/administrator/x"),
            Some(AccessLevel::WriteGraph)
        );
        // ...and as a read, it's a normal (public-when-enabled) read.
        assert_eq!(lvl(Method::GET, "/api/administrator/x"), None);
        // /api/health-x must NOT inherit the public health allowlist.
        assert_eq!(
            lvl(Method::POST, "/api/health-x"),
            Some(AccessLevel::WriteGraph)
        );
    }

    #[test]
    fn reads_public_only_when_flag_enabled() {
        // Flag ON (explicit RBAC_PUBLIC_READS=1): anonymous reads allowed.
        assert_eq!(lvl(Method::GET, "/api/graph/data"), None);
        assert_eq!(lvl(Method::HEAD, "/api/bots/status"), None);
        // Flag OFF (default / absent): the same reads require authentication —
        // absence of the security flag must NOT widen access.
        assert_eq!(
            required_level(&Method::GET, "/api/graph/data", false),
            Some(AccessLevel::ReadOnly)
        );
        // Admin reads stay Admin regardless of the public-reads flag.
        assert_eq!(
            required_level(&Method::GET, "/api/admin/rbac/users", false),
            Some(AccessLevel::Admin)
        );
    }

    #[test]
    fn writes_are_gated_at_write_graph_not_mere_authenticated() {
        // WriteGraph (not Authenticated) so a Viewer(→ReadOnly) is denied while
        // an Editor(→Authenticated) passes.
        assert_eq!(
            lvl(Method::POST, "/api/graph/update"),
            Some(AccessLevel::WriteGraph)
        );
        assert_eq!(
            lvl(Method::DELETE, "/api/ontology/classes/x"),
            Some(AccessLevel::WriteGraph)
        );
        // The lattice check that makes this bite: a Viewer cannot satisfy it.
        assert!(!AccessLevel::ReadOnly.has_permission(&AccessLevel::WriteGraph));
        assert!(AccessLevel::Authenticated.has_permission(&AccessLevel::WriteGraph));
    }

    #[test]
    fn settings_writes_require_write_settings() {
        assert_eq!(
            lvl(Method::PUT, "/api/settings/physics"),
            Some(AccessLevel::WriteSettings)
        );
        // But reading settings stays open (public reads on).
        assert_eq!(lvl(Method::GET, "/api/settings/all"), None);
    }
}
