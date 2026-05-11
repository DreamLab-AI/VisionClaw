use actix_web::{web, HttpRequest, HttpResponse};
use log::warn;
use std::sync::{Arc, Once};
use tracing::{debug, info};
use uuid::Uuid;
use crate::services::metrics::MetricsRegistry;
use crate::services::nostr_service::NostrService;

use solid_pod_rs::auth::lws_cid::{LwsCidVerifier, ProfileFetcher};
use solid_pod_rs::auth::self_signed::{ProofEnvelope, SelfSignedVerifier};

/// Best-effort lookup of the shared metrics registry.
/// Returns `None` if no registry is installed (e.g. unit tests) — call-sites
/// should be tolerant of that and skip observation.
fn metrics_of(req: &HttpRequest) -> Option<&Arc<MetricsRegistry>> {
    req.app_data::<web::Data<Arc<MetricsRegistry>>>()
        .map(|d| d.get_ref())
}

// ---------------------------------------------------------------------------
// LWS-CID self-signed JWT support (W3C Controlled Identifier Document v1.0)
// ---------------------------------------------------------------------------

/// HTTP-based profile fetcher for LWS-CID JWT verification.
///
/// Resolves the signer's WebID profile document via HTTPS. The profile
/// contains the `verificationMethod` public key used to validate the
/// JWT signature. Uses `reqwest` with a 10-second timeout and 256 KiB
/// size cap (matching solid-pod-rs's `MAX_PROFILE_SIZE`).
struct HttpProfileFetcher {
    client: reqwest::Client,
}

impl HttpProfileFetcher {
    fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }
}

#[async_trait::async_trait]
impl ProfileFetcher for HttpProfileFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .client
            .get(url)
            .header("Accept", "application/ld+json, application/json")
            .send()
            .await
            .map_err(|e| format!("profile fetch failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("profile fetch HTTP {}", resp.status()));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("profile body read: {e}"))?;
        if bytes.len() > 256 * 1024 {
            return Err("profile exceeds 256 KiB".into());
        }
        Ok(bytes.to_vec())
    }
}

/// Build a singleton `LwsCidVerifier` backed by `HttpProfileFetcher`.
///
/// The verifier is constructed once per process (lazy static) and shared
/// across all requests. It is `Send + Sync` and reference-counted.
fn lws_cid_verifier() -> &'static LwsCidVerifier {
    use std::sync::OnceLock;
    static VERIFIER: OnceLock<LwsCidVerifier> = OnceLock::new();
    VERIFIER.get_or_init(|| {
        let fetcher = Arc::new(HttpProfileFetcher::new());
        LwsCidVerifier::new(fetcher)
    })
}

/// Attempt LWS-CID self-signed JWT verification on a Bearer token.
///
/// Returns `Ok(Some(did))` when the token is a valid LWS-CID JWT,
/// `Ok(None)` when the token is not in LWS-CID format (so the caller
/// should try the next auth method), or `Err(msg)` when the format
/// matches but verification fails.
async fn verify_lws_cid_bearer(
    token: &str,
    method: &str,
    uri: &str,
) -> Result<Option<String>, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let envelope = ProofEnvelope {
        proof: token,
        method,
        uri,
        now_unix: now,
        expected_subject_hint: None,
    };

    match lws_cid_verifier().verify(&envelope).await {
        Ok(Some(subject)) => Ok(Some(subject.did)),
        Ok(None) => Ok(None),
        Err(e) => Err(format!("LWS-CID verification failed: {e}")),
    }
}

/// Fail-closed `APP_ENV` probe (ADR-055 H3).
///
/// Returns `true` when the env var is absent OR set to `"production"`.
/// This flips the pre-H3 default of "missing → non-production" (which
/// silently enabled dev-mode bypasses in unconfigured deployments) to
/// "missing → production" so the safe default is the restrictive one.
///
/// The first call with an unset `APP_ENV` emits a single startup WARN so
/// operators who intended to run in development mode notice and set the
/// variable explicitly.
fn is_production() -> bool {
    static WARN_ONCE: Once = Once::new();
    match std::env::var("APP_ENV") {
        Ok(v) => v == "production",
        Err(_) => {
            WARN_ONCE.call_once(|| {
                warn!(
                    "APP_ENV unset — defaulting to production for safety; \
                     set APP_ENV=development for dev-mode bypasses"
                );
            });
            true
        }
    }
}

/// Scoped permission levels for RBAC.
///
/// The hierarchy (from least to most privileged):
///   ReadOnly < WriteGraph < WriteSettings < Admin
///
/// Legacy mappings for backward compatibility:
///   - `Authenticated` maps to `ReadOnly + WriteGraph`
///   - `PowerUser` maps to `Admin`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessLevel {
    /// Legacy: any authenticated user (maps to ReadOnly + WriteGraph)
    Authenticated,
    /// Legacy: power user (maps to Admin)
    PowerUser,
    /// Can read graph data and settings, no mutations
    ReadOnly,
    /// Can mutate graph data (create/update/delete nodes and edges)
    WriteGraph,
    /// Can modify application settings
    WriteSettings,
    /// Full administrative access (includes all permissions)
    Admin,
    /// Caller may be unauthenticated; handler branches on presence.
    ///
    /// Not a permission gate — used only with `RequireAuth::optional()`.
    /// Signed requests are verified normally; unsigned requests pass through
    /// with an empty-string pubkey marker. Malformed NIP-98 headers still
    /// yield 401.
    Optional,
}

impl AccessLevel {
    /// Check whether this access level satisfies the `required` permission.
    ///
    /// The mapping is:
    /// - `ReadOnly`: satisfied by ReadOnly, WriteGraph, WriteSettings, Admin, Authenticated, PowerUser
    /// - `WriteGraph`: satisfied by WriteGraph, Admin, Authenticated, PowerUser
    /// - `WriteSettings`: satisfied by WriteSettings, Admin, PowerUser
    /// - `Admin`: satisfied by Admin, PowerUser
    /// - `Authenticated`: satisfied by Authenticated, PowerUser, WriteGraph, WriteSettings, Admin, ReadOnly
    /// - `PowerUser`: satisfied by PowerUser, Admin
    pub fn has_permission(&self, required: &AccessLevel) -> bool {
        use AccessLevel::*;
        match required {
            // `Optional` is a special marker, not a gate: any level (including
            // a hypothetical "no auth" caller) satisfies it. The actual
            // presence-vs-absence branching happens in `verify_access`.
            Optional => true,
            ReadOnly => true, // every authenticated level can read
            Authenticated => true, // same as ReadOnly for permission checks
            WriteGraph => matches!(self, WriteGraph | Admin | Authenticated | PowerUser),
            WriteSettings => matches!(self, WriteSettings | Admin | PowerUser),
            Admin => matches!(self, Admin | PowerUser),
            PowerUser => matches!(self, Admin | PowerUser),
        }
    }
}

/// Returns true when `NIP98_OPTIONAL_AUTH=true` is set.
/// When false (default), `AccessLevel::Optional` is transparently promoted
/// to `AccessLevel::Authenticated` so scopes wrapped with
/// `RequireAuth::optional()` behave identically to
/// `RequireAuth::authenticated()`. This is the sprint-level rollback lever.
fn optional_auth_enabled() -> bool {
    std::env::var("NIP98_OPTIONAL_AUTH")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

/// Legacy entry-point used by the middleware layer where the request body is
/// not yet buffered. The NIP-98 call runs without body-hash binding — handler-
/// level callers with access to the buffered body should prefer
/// [`verify_access_with_body`] (ADR-055 B3).
pub async fn verify_access(
    req: &HttpRequest,
    nostr_service: &NostrService,
    required_level: AccessLevel,
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, required_level, None).await
}

/// Verify access with an optional request body. When `body` is `Some(..)`, the
/// NIP-98 path binds the signature to a SHA-256 of the raw bytes (ADR-055 B3)
/// so a captured token cannot be replayed against a different payload.
pub async fn verify_access_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    required_level: AccessLevel,
    body: Option<&[u8]>,
) -> Result<String, HttpResponse> {
    // Feature-flag downgrade: when disabled, Optional behaves as Authenticated.
    let required_level = match required_level {
        AccessLevel::Optional if !optional_auth_enabled() => AccessLevel::Authenticated,
        other => other,
    };

    let request_id = req
        .headers()
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&Uuid::new_v4().to_string())
        .to_string();

    // Detect whether any Authorization or legacy Nostr session header is
    // present. Only used to decide whether `Optional` falls through to the
    // anonymous branch (no headers at all) or runs normal verification (any
    // auth attempt must succeed — we do not silently demote invalid
    // signatures to anonymous).
    let any_auth_attempt = req.headers().get("Authorization").is_some()
        || req.headers().get("X-Nostr-Pubkey").is_some()
        || req.headers().get("X-Nostr-Token").is_some();

    // --- Optional auth: anonymous short-circuit ---
    // Scope wrapped with `RequireAuth::optional()` + no auth headers at all
    // → pass through with an empty pubkey. Handlers distinguish anonymous
    // vs signed via `pubkey.is_empty()`. Any auth attempt (valid or
    // malformed) still goes through verification below.
    if required_level == AccessLevel::Optional && !any_auth_attempt {
        debug!(
            request_id = %request_id,
            "Optional auth: no headers — passing through as anonymous"
        );
        if let Some(m) = metrics_of(req) {
            m.auth_anonymous_total.inc();
        }
        return Ok(String::new());
    }

    // --- Dev-mode session bypass ---
    // Mirrors `settings::auth_extractor` so enterprise panels behind
    // `RequireAuth::authenticated()` work in dev too. Only active when
    // APP_ENV != "production" AND the client sends `Bearer dev-session-token`
    // plus an `X-Nostr-Pubkey`. The pubkey is trusted as-is in dev — never
    // accept this branch without the APP_ENV gate.
    if let Some(auth_value) = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
    {
        if auth_value == "Bearer dev-session-token" {
            // ADR-055 H3: fail-closed — missing APP_ENV now means production.
            if !is_production() {
                let pubkey = req
                    .headers()
                    .get("X-Nostr-Pubkey")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("dev-user")
                    .to_string();
                debug!(
                    request_id = %request_id,
                    pubkey = %pubkey,
                    "Dev-mode bypass accepted (APP_ENV != production)"
                );
                // Dev users have PowerUser-equivalent access so enterprise panels
                // (mesh-metrics, connectors, policy) open without extra setup.
                let user_level = AccessLevel::Admin;
                if user_level.has_permission(&required_level) {
                    return Ok(pubkey);
                }
                // Somehow required level is higher than Admin — fall through.
            }
        }
    }

    // --- LWS-CID self-signed JWT auth (W3C Controlled Identifier) ---
    //
    // Accepts `Authorization: Bearer <JWT>` where the JWT is a compact
    // JWS with a `kid` pointing to a URL#fragment (WebID profile
    // verificationMethod). The LWS-CID verifier resolves the kid,
    // fetches the profile, and validates the ES256K / ES256 / EdDSA
    // signature.
    //
    // Returns Ok(None) for non-LWS-CID Bearer tokens (falls through to
    // the legacy path below). Returns Err for tokens that look like
    // LWS-CID but fail verification (fail-closed).
    if let Some(auth_value) = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
    {
        if let Some(bearer_token) = auth_value.strip_prefix("Bearer ") {
            // Skip dev-session-token (handled above) and empty tokens.
            if bearer_token != "dev-session-token" && !bearer_token.is_empty() {
                let conn_info = req.connection_info();
                let scheme = req.headers()
                    .get("X-Forwarded-Proto")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_else(|| conn_info.scheme());
                let host = req.headers()
                    .get("X-Forwarded-Host")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_else(|| conn_info.host());
                let uri = format!(
                    "{}://{}{}",
                    scheme,
                    host,
                    req.uri()
                        .path_and_query()
                        .map(|pq| pq.as_str())
                        .unwrap_or("/")
                );
                let method = req.method().as_str();

                match verify_lws_cid_bearer(bearer_token, method, &uri).await {
                    Ok(Some(did)) => {
                        info!(
                            request_id = %request_id,
                            did = %did,
                            "LWS-CID JWT auth successful"
                        );
                        if let Some(m) = metrics_of(req) {
                            m.auth_nip98_success_total.inc();
                        }
                        // LWS-CID authenticated users get Authenticated level;
                        // power-user promotion requires NIP-98 or session.
                        let user_level = AccessLevel::Authenticated;
                        if user_level.has_permission(&required_level) {
                            return Ok(did);
                        } else {
                            warn!(
                                "LWS-CID user {} with level {:?} lacks required {:?}",
                                did, user_level, required_level
                            );
                            return Err(HttpResponse::Forbidden()
                                .body("Insufficient permissions for this operation"));
                        }
                    }
                    Ok(None) => {
                        // Not an LWS-CID token — fall through to NIP-98 / legacy.
                        debug!(
                            request_id = %request_id,
                            "Bearer token is not LWS-CID format, trying other auth methods"
                        );
                    }
                    Err(e) => {
                        // Format matched but verification failed — reject.
                        warn!("[{}] LWS-CID JWT verification failed: {}", request_id, e);
                        if let Some(m) = metrics_of(req) {
                            m.auth_nip98_failure_total.inc();
                        }
                        return Err(
                            HttpResponse::Unauthorized().body(format!("LWS-CID auth failed: {e}"))
                        );
                    }
                }
            }
        }
    }

    // --- NIP-98 Schnorr auth (primary path) ---
    if let Some(auth_value) = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
    {
        if auth_value.starts_with("Nostr ") {
            // Behind a TLS-terminating proxy, connection_info returns internal
            // scheme/host; prefer X-Forwarded-* headers from the proxy.
            let conn_info = req.connection_info();
            let scheme = req.headers()
                .get("X-Forwarded-Proto")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.scheme());
            let host = req.headers()
                .get("X-Forwarded-Host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_else(|| conn_info.host());
            let url = format!(
                "{}://{}{}",
                scheme,
                host,
                req.uri()
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            );
            let method = req.method().as_str();

            // ADR-055 B3: bind the NIP-98 signature to the request body via
            // SHA-256 so a captured token cannot be replayed against a
            // different payload. Middleware callers pass `None` because the
            // body isn't buffered yet at that layer; handler-level callers
            // use `verify_access_with_body` to close the loop.
            //
            // The verifier treats the body as UTF-8 text (JSON, RDF/TTL,
            // form-encoded — all UTF-8) and computes the payload hash the
            // same way the client did. Binary bodies are rare on our
            // authenticated surface; a lossy conversion preserves the hash
            // equivalence class for legitimate traffic and still fails the
            // replay test because tampered bodies produce a different hash.
            let body_as_str = body.map(|b| String::from_utf8_lossy(b).into_owned());
            let body_ref: Option<&str> = body_as_str.as_deref();

            match nostr_service
                .verify_nip98_auth(auth_value, &url, method, body_ref)
                .await
            {
                Ok(user) => {
                    info!(
                        request_id = %request_id,
                        pubkey = %user.pubkey,
                        "NIP-98 auth successful"
                    );
                    if let Some(m) = metrics_of(req) {
                        m.auth_nip98_success_total.inc();
                    }
                    // Determine the user's effective access level
                    let user_level = if user.is_power_user {
                        AccessLevel::Admin
                    } else {
                        AccessLevel::Authenticated
                    };
                    if user_level.has_permission(&required_level) {
                        return Ok(user.pubkey);
                    } else {
                        warn!(
                            "User {} with level {:?} lacks required {:?}",
                            user.pubkey, user_level, required_level
                        );
                        return Err(HttpResponse::Forbidden()
                            .body("Insufficient permissions for this operation"));
                    }
                }
                Err(e) => {
                    warn!("[{}] NIP-98 validation failed: {}", request_id, e);
                    if let Some(m) = metrics_of(req) {
                        m.auth_nip98_failure_total.inc();
                    }
                    return Err(
                        HttpResponse::Unauthorized().body(format!("NIP-98 auth failed: {}", e))
                    );
                }
            }
        }
    }

    // --- Legacy path: X-Nostr-Pubkey + X-Nostr-Token ---
    //
    // Retained for development ergonomics (browser extensions, fixture
    // scripts) but is an unsigned bearer-style flow without NIP-98's
    // Schnorr/body-hash/URL-binding guarantees. Rejected outright in
    // production so no regression path can re-enable it.
    // ADR-055 H3: fail-closed — missing APP_ENV now means production, so the
    // legacy unsigned bearer-token path is disabled unless APP_ENV is
    // explicitly set to "development".
    if is_production() {
        warn!(
            "[{}] Legacy X-Nostr-Pubkey auth rejected in production; use NIP-98",
            request_id
        );
        return Err(HttpResponse::Unauthorized()
            .body("Legacy session auth not available in production. Use NIP-98."));
    }

    // Request is entering the legacy (non-NIP-98) path — observed for ADR
    // rollback visibility.
    if let Some(m) = metrics_of(req) {
        m.auth_legacy_fallback_total.inc();
    }

    let pubkey = match req.headers().get("X-Nostr-Pubkey") {
        Some(value) => value.to_str().unwrap_or("").to_string(),
        None => {
            warn!("Missing Nostr pubkey in request headers");
            debug!(
                request_id = %request_id,
                "Authentication failed - missing pubkey header"
            );
            return Err(HttpResponse::Forbidden().body("Authentication required"));
        }
    };

    let token = match req.headers().get("X-Nostr-Token") {
        Some(value) => value.to_str().unwrap_or("").to_string(),
        None => {
            warn!("Missing Nostr token in request headers");
            debug!(
                request_id = %request_id,
                has_pubkey = true,
                "Authentication failed - missing token header"
            );
            return Err(HttpResponse::Forbidden().body("Authentication required"));
        }
    };

    debug!(
        request_id = %request_id,
        has_pubkey = !pubkey.is_empty(),
        has_token = !token.is_empty(),
        pubkey_prefix = %&pubkey.chars().take(8).collect::<String>(),
        "Authentication headers extracted"
    );

    if !nostr_service.validate_session(&pubkey, &token).await {
        warn!("Invalid or expired session for user {}", pubkey);
        debug!(
            request_id = %request_id,
            pubkey = %pubkey,
            "Session validation failed"
        );
        return Err(HttpResponse::Unauthorized().body("Invalid or expired session"));
    }

    info!(
        request_id = %request_id,
        pubkey = %pubkey,
        "Session validated successfully"
    );

    // Determine the user's effective access level from their role
    let is_power = nostr_service.is_power_user(&pubkey).await;
    let user_level = if is_power {
        AccessLevel::Admin
    } else {
        AccessLevel::Authenticated
    };

    if user_level.has_permission(&required_level) {
        debug!(
            request_id = %request_id,
            pubkey = %pubkey,
            user_level = ?user_level,
            required_level = ?required_level,
            "Access granted"
        );
        Ok(pubkey)
    } else {
        warn!(
            "User {} with level {:?} lacks required {:?}",
            pubkey, user_level, required_level
        );
        debug!(
            request_id = %request_id,
            pubkey = %pubkey,
            "Access denied - insufficient permissions"
        );
        Err(HttpResponse::Forbidden().body("Insufficient permissions for this operation"))
    }
}

// Helper function for handlers that require power user access
pub async fn verify_power_user(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::PowerUser).await
}

// Helper function for handlers that require authentication
pub async fn verify_authenticated(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::Authenticated).await
}

// Helper function for handlers that require read-only access
pub async fn verify_read_only(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::ReadOnly).await
}

// Helper function for handlers that require graph write access
pub async fn verify_write_graph(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::WriteGraph).await
}

// Helper function for handlers that require settings write access
pub async fn verify_write_settings(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::WriteSettings).await
}

// Helper function for handlers that require admin access
pub async fn verify_admin(
    req: &HttpRequest,
    nostr_service: &NostrService,
) -> Result<String, HttpResponse> {
    verify_access(req, nostr_service, AccessLevel::Admin).await
}

// ---------------------------------------------------------------------------
// Body-bound helpers (ADR-055 B3)
// ---------------------------------------------------------------------------
//
// These variants accept the buffered request body and forward it to the
// NIP-98 verifier so the signature binds to the payload. Handler-level code
// with access to `web::Bytes` should prefer these over the body-less
// helpers above.

/// Authenticated scope with body-hash binding.
pub async fn verify_authenticated_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    body: &[u8],
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, AccessLevel::Authenticated, Some(body)).await
}

/// Read-only scope with body-hash binding.
pub async fn verify_read_only_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    body: &[u8],
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, AccessLevel::ReadOnly, Some(body)).await
}

/// Graph-write scope with body-hash binding.
pub async fn verify_write_graph_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    body: &[u8],
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, AccessLevel::WriteGraph, Some(body)).await
}

/// Settings-write scope with body-hash binding.
pub async fn verify_write_settings_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    body: &[u8],
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, AccessLevel::WriteSettings, Some(body)).await
}

/// Admin scope with body-hash binding.
pub async fn verify_admin_with_body(
    req: &HttpRequest,
    nostr_service: &NostrService,
    body: &[u8],
) -> Result<String, HttpResponse> {
    verify_access_with_body(req, nostr_service, AccessLevel::Admin, Some(body)).await
}