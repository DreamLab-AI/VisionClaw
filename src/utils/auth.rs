use crate::services::nostr_service::NostrService;
use actix_web::{HttpRequest, HttpResponse};
use log::warn;
use tracing::{debug, info};
use uuid::Uuid;

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
            ReadOnly => true,      // every authenticated level can read
            Authenticated => true, // same as ReadOnly for permission checks
            WriteGraph => matches!(self, WriteGraph | Admin | Authenticated | PowerUser),
            WriteSettings => matches!(self, WriteSettings | Admin | PowerUser),
            Admin => matches!(self, Admin | PowerUser),
            PowerUser => matches!(self, Admin | PowerUser),
        }
    }
}

/// Resolve a caller's effective [`AccessLevel`] from their persisted RBAC role
/// (ADR-142). When the process-global [`RoleStore`](crate::services::role_store)
/// is installed, the pubkey's four-tier role (Owner/Admin/Editor/Viewer) drives
/// the level; otherwise we fall back to the legacy binary mapping
/// (`is_power_user` → Admin, else Authenticated) so nothing regresses in
/// contexts that never initialised the store (e.g. unit tests).
async fn resolve_access_level(pubkey: &str, is_power_user: bool) -> AccessLevel {
    match crate::services::role_store::global_role_store() {
        Some(store) => store
            .effective_role(pubkey, is_power_user)
            .await
            .to_access_level(),
        None => {
            if is_power_user {
                AccessLevel::Admin
            } else {
                AccessLevel::Authenticated
            }
        }
    }
}

/// Effective [`AccessLevel`] of a pubkey whose request signature has
/// **already** been verified upstream (by `RbacGate` or `RequireAuth`, which
/// leave an `AuthenticatedUser` in the request extensions).
///
/// NIP-98 tokens are single-use, so a layered check must not verify the same
/// `Authorization` header twice; [`verify_access`] calls this instead whenever
/// the request already carries a verified identity. Handlers may also call it
/// directly. The dev-mode sentinel principal resolves to `Admin`, mirroring
/// the bypass in [`verify_access`].
pub async fn effective_access_level(pubkey: &str, nostr_service: &NostrService) -> AccessLevel {
    if pubkey == DEV_MODE_PUBKEY && dev_full_bypass_active() {
        return AccessLevel::Admin;
    }
    let is_power = nostr_service.is_power_user(pubkey).await;
    resolve_access_level(pubkey, is_power).await
}

/// Synthetic principal returned by the LAN-local dev-mode bypass. Not a real
/// Nostr pubkey — it is a clearly-labelled sentinel so provenance/audit rows are
/// unambiguous about which writes came in unauthenticated on a dev headset.
pub const DEV_MODE_PUBKEY: &str = "dev-mode-local-admin";

/// Whether the **LAN-local full bypass** is active: every request is treated as
/// an authenticated dev admin, with NO NIP-98 signature, dev-token header, or
/// peer-origin check. This is the "desktop dev mode" for a 100%-local headset
/// (e.g. the HP over the rail) where per-request Nostr signing is pure friction
/// and the LAN itself is the trust boundary.
///
/// Triple-gated exactly like the loopback dev-token path, minus the peer check:
///   1. **Compile-time** — the reading code exists only in
///      `debug_assertions`/`dev-auth` builds; the release stub returns `false`
///      and `main::enforce_release_env_hygiene` *hard-fails at boot* if
///      `VISIONCLAW_DEV_MODE` is even present (ADR-06 §D11).
///   2. **Runtime opt-in** — off unless `VISIONCLAW_DEV_MODE=1` (or `true`).
///
/// Deliberately peer-agnostic: behind Docker port-publishing the backend sees
/// the bridge-gateway SNAT address, not the real HP, so a loopback/CIDR gate
/// cannot express "trust my LAN headset". The compile-gate + release-refusal is
/// what keeps this from ever reaching production.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
pub fn dev_full_bypass_active() -> bool {
    std::env::var("VISIONCLAW_DEV_MODE")
        .map(|v| {
            let t = v.trim();
            t == "1" || t.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// Release-build stub: the full-bypass codepath is absent from the binary, so it
/// can never fire regardless of env. (Belt-and-braces with the boot refusal.)
#[cfg(not(any(debug_assertions, feature = "dev-auth")))]
#[inline(always)]
pub fn dev_full_bypass_active() -> bool {
    false
}

/// Whether the dev-session-token bypass may fire for a request from `peer`.
/// Requires the explicit `DEV_AUTH_LOOPBACK=1` opt-in AND a loopback peer
/// address. This is the **single** gate every dev-token acceptance path routes
/// through (REST extractor, middleware, WS handshake), so no path can accept the
/// literal token ungated. Compiled only into dev/`dev-auth` builds.
#[cfg(any(debug_assertions, feature = "dev-auth"))]
pub fn dev_bypass_permitted_for_addr(peer: Option<std::net::SocketAddr>) -> bool {
    let opt_in = std::env::var("DEV_AUTH_LOOPBACK")
        .map(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !opt_in {
        return false;
    }
    match peer {
        Some(addr) => addr.ip().is_loopback(),
        // No peer address (e.g. some proxy configs) — fail closed.
        None => false,
    }
}

/// Convenience wrapper resolving the peer address from an [`HttpRequest`].
#[cfg(any(debug_assertions, feature = "dev-auth"))]
pub fn dev_bypass_permitted(req: &HttpRequest) -> bool {
    dev_bypass_permitted_for_addr(req.peer_addr())
}

/// The absolute URL a NIP-98 `u` tag must name for `req`.
///
/// Behind a proxy, `connection_info` would report the internal scheme and host,
/// so the proxy's `X-Forwarded-Proto` / `X-Forwarded-Host` win. The host must
/// carry the port the client dialled: a headset on the LAN signs
/// `http://host:3001/...`, and a host stripped of its port can never match that
/// `u` tag (`urls_match` compares host and path; the scheme is not decisive).
/// The production nginx therefore forwards `X-Forwarded-Host $http_host`
/// (owner decision 2026-10-02, Q3). Every NIP-98 entry point — `verify_access`,
/// the settings extractor and the `/wss` authenticate frame — uses this one
/// reconstruction so they cannot drift apart.
pub fn nip98_request_url(req: &HttpRequest) -> String {
    let conn_info = req.connection_info();
    let header = |name: &str| {
        req.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
    };
    let scheme = header("X-Forwarded-Proto").unwrap_or_else(|| conn_info.scheme());
    let host = header("X-Forwarded-Host").unwrap_or_else(|| conn_info.host());
    format!(
        "{}://{}{}",
        scheme,
        host,
        req.uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/")
    )
}

pub async fn verify_access(
    req: &HttpRequest,
    nostr_service: &NostrService,
    required_level: AccessLevel,
) -> Result<String, HttpResponse> {
    let request_id = req
        .headers()
        .get("X-Request-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&Uuid::new_v4().to_string())
        .to_string();

    // --- LAN-local full dev bypass (dev builds only, VISIONCLAW_DEV_MODE=1) ---
    //
    // Peer-agnostic: grants an authenticated dev-admin identity to EVERY request
    // with no signature/token/peer check, so a 100%-local headset (the HP over
    // the rail) has zero auth friction. This is the single REST chokepoint —
    // both `RbacGate` (/api) and `RequireAuth` delegate here — so one early
    // return covers every REST write door. Compile-gated + boot-refused in
    // release (see `dev_full_bypass_active` and `enforce_release_env_hygiene`).
    if dev_full_bypass_active() {
        debug!(
            request_id = %request_id,
            "dev-mode: VISIONCLAW_DEV_MODE full bypass — granting {:?} as {}",
            required_level,
            DEV_MODE_PUBKEY
        );
        return Ok(DEV_MODE_PUBKEY.to_string());
    }

    // --- Dev bypass (dev builds only, loopback + explicit opt-in) ---
    //
    // Codex review [HIGH]: `Bearer dev-session-token` previously satisfied the
    // whole lattice (incl. Admin) with an arbitrary `X-Nostr-Pubkey`, in any
    // debug build, from any peer. It is now triple-gated: compile-time
    // (`debug_assertions`/`dev-auth`), an explicit runtime opt-in
    // (`DEV_AUTH_LOOPBACK=1`), AND the request must originate from a loopback
    // peer address. A remote attacker on a debug deployment can no longer reach
    // it even if they guess the header.
    #[cfg(any(debug_assertions, feature = "dev-auth"))]
    {
        if let Some(auth_value) = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
        {
            if auth_value == "Bearer dev-session-token" {
                if dev_bypass_permitted(req) {
                    let pubkey = req
                        .headers()
                        .get("X-Nostr-Pubkey")
                        .and_then(|h| h.to_str().ok())
                        .unwrap_or("dev-user")
                        .to_string();
                    debug!(
                        "dev-auth: Bearer dev-session-token accepted (loopback + DEV_AUTH_LOOPBACK) for {pubkey}"
                    );
                    return Ok(pubkey);
                } else {
                    warn!(
                        "dev-auth: rejected dev-session-token — requires DEV_AUTH_LOOPBACK=1 and a loopback peer (peer={:?})",
                        req.peer_addr()
                    );
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
            // NIP-98 event ids are single-use. When an outer layer (`RbacGate`
            // over `/api`, or an enclosing `RequireAuth`) has already verified
            // this request's token, it left the signer in the request
            // extensions — which only server-side middleware can populate.
            // Verifying the same header again would be rejected as a replay,
            // so reuse that identity and check only the level required here.
            if let Some(identity) = crate::middleware::auth::get_authenticated_user(req) {
                let user_level = effective_access_level(&identity.pubkey, nostr_service).await;
                if user_level.has_permission(&required_level) {
                    debug!(
                        request_id = %request_id,
                        pubkey = %identity.pubkey,
                        "NIP-98 identity reused from an outer verification"
                    );
                    return Ok(identity.pubkey);
                }
                warn!(
                    "User {} with level {:?} lacks required {:?}",
                    identity.pubkey, user_level, required_level
                );
                return Err(
                    HttpResponse::Forbidden().body("Insufficient permissions for this operation")
                );
            }

            let url = nip98_request_url(req);
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
                    // Determine the user's effective access level from their
                    // persisted RBAC role (ADR-142), falling back to the legacy
                    // power-user mapping when the store is absent.
                    let user_level = resolve_access_level(&user.pubkey, user.is_power_user).await;
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
    let pubkey = match req.headers().get("X-Nostr-Pubkey") {
        Some(value) => value.to_str().unwrap_or("").to_string(),
        None => {
            warn!("Missing Nostr pubkey in request headers");
            debug!(
                request_id = %request_id,
                "Authentication failed - missing pubkey header"
            );
            return Err(HttpResponse::Unauthorized().body("Authentication required"));
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
            return Err(HttpResponse::Unauthorized().body("Authentication required"));
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

    // Determine the user's effective access level from their persisted RBAC
    // role (ADR-142), falling back to the legacy power-user mapping.
    let is_power = nostr_service.is_power_user(&pubkey).await;
    let user_level = resolve_access_level(&pubkey, is_power).await;

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

#[cfg(all(test, any(debug_assertions, feature = "dev-auth")))]
mod dev_bypass_tests {
    use super::dev_bypass_permitted;
    use actix_web::test::TestRequest;

    /// Loopback + opt-in permits; non-loopback OR missing opt-in refuses. Cases
    /// run sequentially in one test because they mutate a shared process env var
    /// (parallel tests would race on it).
    #[test]
    fn dev_bypass_requires_loopback_and_optin() {
        let loopback = "127.0.0.1:5000".parse().unwrap();
        let remote = "203.0.113.7:5000".parse().unwrap();

        // Opt-in set: loopback allowed, remote refused.
        std::env::set_var("DEV_AUTH_LOOPBACK", "1");
        let req_lo = TestRequest::default().peer_addr(loopback).to_http_request();
        assert!(
            dev_bypass_permitted(&req_lo),
            "loopback + opt-in must permit"
        );
        let req_rem = TestRequest::default().peer_addr(remote).to_http_request();
        assert!(
            !dev_bypass_permitted(&req_rem),
            "remote peer must be refused even with opt-in"
        );

        // Opt-in unset: even loopback is refused.
        std::env::remove_var("DEV_AUTH_LOOPBACK");
        let req_lo2 = TestRequest::default().peer_addr(loopback).to_http_request();
        assert!(
            !dev_bypass_permitted(&req_lo2),
            "loopback without opt-in must be refused"
        );
    }

    /// VISIONCLAW_DEV_MODE gates the LAN-local full bypass and is peer-agnostic:
    /// only `1`/`true` arm it; anything else (incl. unset) is off. Sequential —
    /// mutates a shared process env var.
    #[test]
    fn dev_full_bypass_respects_env_flag() {
        use super::dev_full_bypass_active;

        std::env::remove_var("VISIONCLAW_DEV_MODE");
        assert!(!dev_full_bypass_active(), "unset must be off");

        std::env::set_var("VISIONCLAW_DEV_MODE", "1");
        assert!(dev_full_bypass_active(), "=1 must arm");

        std::env::set_var("VISIONCLAW_DEV_MODE", "true");
        assert!(dev_full_bypass_active(), "=true must arm");

        std::env::set_var("VISIONCLAW_DEV_MODE", " TRUE ");
        assert!(
            dev_full_bypass_active(),
            "whitespace/case-insensitive must arm"
        );

        std::env::set_var("VISIONCLAW_DEV_MODE", "0");
        assert!(!dev_full_bypass_active(), "=0 must be off");

        std::env::set_var("VISIONCLAW_DEV_MODE", "yes");
        assert!(!dev_full_bypass_active(), "non-1/true must be off");

        std::env::remove_var("VISIONCLAW_DEV_MODE");
    }
}

#[cfg(test)]
mod access_level_tests {
    use super::AccessLevel::*;

    /// S2 core invariant: a regular NIP-98 user resolves to `Authenticated`,
    /// which must satisfy WriteGraph but MUST NOT satisfy PowerUser/Admin. This
    /// is what makes the power_user escalations on destructive graph/ontology
    /// routes actually deny ordinary authenticated callers.
    #[test]
    fn authenticated_user_cannot_reach_power_user_or_admin() {
        assert!(Authenticated.has_permission(&ReadOnly));
        assert!(Authenticated.has_permission(&WriteGraph));
        assert!(Authenticated.has_permission(&Authenticated));
        // The escalation boundary:
        assert!(!Authenticated.has_permission(&PowerUser));
        assert!(!Authenticated.has_permission(&Admin));
        assert!(!Authenticated.has_permission(&WriteSettings));
    }

    /// A power user (mapped to Admin) satisfies every level, including the
    /// power_user-gated destructive routes.
    #[test]
    fn admin_power_user_satisfies_everything() {
        for required in [
            ReadOnly,
            WriteGraph,
            WriteSettings,
            Admin,
            PowerUser,
            Authenticated,
        ] {
            assert!(
                Admin.has_permission(&required),
                "Admin must satisfy {:?}",
                required
            );
            assert!(
                PowerUser.has_permission(&required),
                "PowerUser must satisfy {:?}",
                required
            );
        }
    }

    #[test]
    fn read_only_cannot_write() {
        assert!(ReadOnly.has_permission(&ReadOnly));
        assert!(!ReadOnly.has_permission(&WriteGraph));
        assert!(!ReadOnly.has_permission(&WriteSettings));
        assert!(!ReadOnly.has_permission(&Admin));
        assert!(!ReadOnly.has_permission(&PowerUser));
    }
}

/// NIP-98 behind the profile proxies (owner decision 2026-10-02, Q1 and Q3).
///
/// Under the prod profile the headset reaches the backend through nginx — on the
/// LAN at `http://<host>:3001`, or through the Cloudflare tunnel at
/// `https://<domain>`. The client signs exactly the URL it dialled. These tests
/// read the nginx configs the images bake (`Dockerfile.production:236`,
/// `Dockerfile.unified:306`), apply each `location`'s `proxy_set_header`
/// directives to a client `Host`, rebuild the URL with [`nip98_request_url`],
/// and require the signed token to validate.
#[cfg(test)]
mod nip98_proxy_tests {
    use super::nip98_request_url;
    use crate::utils::nip98::{generate_nip98_token, validate_nip98_token, Nip98Config};
    use actix_web::test::TestRequest;
    use nostr_sdk::prelude::Keys;

    const PROD_NGINX: &str = include_str!("../../nginx.production.conf");
    const DEV_NGINX: &str = include_str!("../../nginx.dev.conf");

    /// The body of the first `location` block whose header line starts with
    /// `marker`, found by brace matching.
    fn location_block<'a>(conf: &'a str, marker: &str) -> &'a str {
        let start = conf
            .find(marker)
            .unwrap_or_else(|| panic!("nginx config has no `{marker}` block"));
        let open = start + conf[start..].find('{').expect("location opens");
        let mut depth = 0usize;
        for (i, c) in conf[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &conf[open + 1..open + i];
                    }
                }
                _ => {}
            }
        }
        panic!("`{marker}` block never closes");
    }

    /// The headers nginx sends upstream for the auth-relevant names, given the
    /// `Host` the client sent. Only the variables these directives use are
    /// modelled; an unmodelled one fails the test so a config change is seen.
    fn upstream_headers(block: &str, client_host: &str) -> Vec<(String, String)> {
        let host_no_port = client_host.split(':').next().unwrap().to_ascii_lowercase();
        block
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.strip_prefix("proxy_set_header "))
            .filter_map(|rest| {
                let rest = rest.split('#').next().unwrap().trim().trim_end_matches(';');
                let (name, value) = rest.split_once(char::is_whitespace)?;
                let name = name.trim();
                if !["host", "x-forwarded-host", "x-forwarded-proto"]
                    .contains(&name.to_ascii_lowercase().as_str())
                {
                    return None;
                }
                let value = match value.trim() {
                    "$host" => host_no_port.clone(),
                    "$http_host" => client_host.to_string(),
                    "$scheme" => "http".to_string(),
                    // The dev /api block passes through whatever an outer proxy
                    // sent; a LAN headset sends none.
                    "$http_x_forwarded_proto" => String::new(),
                    v if !v.contains('$') => v.to_string(),
                    v => panic!("unmodelled nginx variable `{v}` on {name}"),
                };
                Some((name.to_string(), value))
            })
            .collect()
    }

    /// Sign `client_url` as the headset does, pass it through `marker`'s
    /// location in `conf`, and validate it as the backend does.
    fn signed_request_validates(
        conf: &str,
        marker: &str,
        client_url: &str,
        client_host: &str,
        path_and_query: &str,
        method: &str,
    ) -> Result<(), String> {
        let token = generate_nip98_token(
            &Keys::generate(),
            &Nip98Config {
                url: client_url.to_string(),
                method: method.to_string(),
                body: None,
            },
        )
        .map_err(|e| e.to_string())?;

        let mut req = TestRequest::default()
            .uri(path_and_query)
            .insert_header(("Host", client_host));
        for (name, value) in upstream_headers(location_block(conf, marker), client_host) {
            if !value.is_empty() {
                req = req.insert_header((name, value));
            }
        }
        let expected = nip98_request_url(&req.to_http_request());
        validate_nip98_token(&token, &expected, method, None)
            .map(|_| ())
            .map_err(|e| format!("{e} (server rebuilt `{expected}`)"))
    }

    const PROD_API: &str = "location ^~ /api/ {";
    const PROD_WS: &str = "location ~ ^/(wss|ws/speech|ws/mcp-relay|ws/hybrid-status)$ {";

    #[test]
    fn prod_lan_headset_physics_put_validates_through_nginx() {
        signed_request_validates(
            PROD_NGINX,
            PROD_API,
            "http://trust.lan:3001/api/settings/physics?graph=knowledge",
            "trust.lan:3001",
            "/api/settings/physics?graph=knowledge",
            "PUT",
        )
        .unwrap();
    }

    #[test]
    fn prod_lan_headset_wss_authenticate_validates_through_nginx() {
        signed_request_validates(
            PROD_NGINX,
            PROD_WS,
            "ws://trust.lan:3001/wss",
            "trust.lan:3001",
            "/wss",
            "GET",
        )
        .unwrap();
    }

    #[test]
    fn prod_tunnel_headset_physics_put_validates_through_nginx() {
        signed_request_validates(
            PROD_NGINX,
            PROD_API,
            "https://visionclaw.example.org/api/settings/physics?graph=knowledge",
            "visionclaw.example.org",
            "/api/settings/physics?graph=knowledge",
            "PUT",
        )
        .unwrap();
    }

    #[test]
    fn dev_nginx_api_and_wss_keep_the_dialled_port() {
        signed_request_validates(
            DEV_NGINX,
            "location ^~ /api/ {",
            "http://dev.lan:3001/api/layout/mode",
            "dev.lan:3001",
            "/api/layout/mode",
            "POST",
        )
        .unwrap();
        signed_request_validates(
            DEV_NGINX,
            "location /wss {",
            "ws://dev.lan:3001/wss",
            "dev.lan:3001",
            "/wss",
            "GET",
        )
        .unwrap();
    }

    /// A token signed for another host must still fail: forwarding the port
    /// must not loosen the `u`-tag host binding.
    #[test]
    fn token_for_another_host_is_still_rejected() {
        let err = signed_request_validates(
            PROD_NGINX,
            PROD_API,
            "http://evil.example:3001/api/settings/physics?graph=knowledge",
            "trust.lan:3001",
            "/api/settings/physics?graph=knowledge",
            "PUT",
        )
        .unwrap_err();
        assert!(err.contains("URL mismatch"), "{err}");
    }
}
