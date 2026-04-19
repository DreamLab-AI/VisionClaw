use actix_web::{HttpRequest, HttpResponse};
use log::warn;
use tracing::{debug, info};
use uuid::Uuid;
use crate::services::nostr_service::NostrService;

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
    /// Unlike the other variants, `Optional` is not a permission gate. When
    /// used with `RequireAuth::optional()`, the middleware accepts both
    /// signed and anonymous callers: signed requests are verified normally
    /// (providing a pubkey for caller-aware filtering), and unsigned
    /// requests pass through with an empty-string pubkey marker. Handlers
    /// inspect the pubkey and decide what to return. Malformed NIP-98
    /// headers are still rejected (401) — `Optional` never bypasses
    /// validation, only missing headers.
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
            // `Optional` is a special marker, not a gate: any level
            // (including a hypothetical "no auth" caller) satisfies it.
            // The presence-vs-absence branching happens in `verify_access`.
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

/// Returns true when the feature flag `NIP98_OPTIONAL_AUTH=true` is set.
/// When false (default), `AccessLevel::Optional` is transparently promoted
/// to `AccessLevel::Authenticated` so scopes wrapped with
/// `RequireAuth::optional()` behave identically to `RequireAuth::authenticated()`.
/// This is the sprint-level rollback lever.
fn optional_auth_enabled() -> bool {
    std::env::var("NIP98_OPTIONAL_AUTH")
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

pub async fn verify_access(
    req: &HttpRequest,
    nostr_service: &NostrService,
    required_level: AccessLevel,
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
    let has_auth_header = req.headers().get("Authorization").is_some();
    let has_legacy_pubkey = req.headers().get("X-Nostr-Pubkey").is_some();
    let has_legacy_token = req.headers().get("X-Nostr-Token").is_some();
    let any_auth_attempt = has_auth_header || has_legacy_pubkey || has_legacy_token;

    // --- Optional auth: anonymous short-circuit ---
    // Scope wrapped with `RequireAuth::optional()` + no auth headers at all
    // → pass through with an empty pubkey. Handlers distinguish anonymous
    // vs signed via `pubkey.is_empty()`. Any auth attempt (valid or
    // malformed) must still go through verification below.
    if required_level == AccessLevel::Optional && !any_auth_attempt {
        debug!(
            request_id = %request_id,
            "Optional auth: no headers — passing through as anonymous"
        );
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
            let is_production = std::env::var("APP_ENV")
                .map(|v| v == "production")
                .unwrap_or(false);
            if !is_production {
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

            match nostr_service
                .verify_nip98_auth(auth_value, &url, method, None)
                .await
            {
                Ok(user) => {
                    info!(
                        request_id = %request_id,
                        pubkey = %user.pubkey,
                        "NIP-98 auth successful"
                    );
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
    {
        let is_production = std::env::var("APP_ENV")
            .map(|v| v == "production")
            .unwrap_or(false);
        if is_production {
            warn!(
                "[{}] Legacy X-Nostr-Pubkey auth rejected in production; use NIP-98",
                request_id
            );
            return Err(HttpResponse::Unauthorized()
                .body("Legacy session auth not available in production. Use NIP-98."));
        }
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