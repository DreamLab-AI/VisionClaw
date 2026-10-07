//! Admin RBAC management endpoints (ADR-142).
//!
//! NIP-98-authenticated surface for inspecting and assigning per-pubkey roles.
//! Mounted under `/api/admin/rbac`. All routes verify the caller's Nostr
//! identity; management routes additionally require the caller to hold `Admin`
//! or `Owner`, and enforce [`UserRole::can_assign`] so an Admin can never mint
//! Admins or Owners (only an Owner can).
//!
//! | Method | Path                                | Min role | Purpose                    |
//! |--------|-------------------------------------|----------|----------------------------|
//! | GET    | `/api/admin/rbac/whoami`            | any auth | caller's own resolved role |
//! | GET    | `/api/admin/rbac/users`             | Admin    | list explicit assignments  |
//! | PUT    | `/api/admin/rbac/users/{pubkey}/role` | Admin  | assign a role              |
//! | DELETE | `/api/admin/rbac/users/{pubkey}/role` | Admin  | revert to default role     |

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::models::rbac::UserRole;
use crate::services::nostr_service::NostrService;
use crate::services::role_store::{
    global_role_store, CallerAuthority, RoleAssignment, RoleStore, RoleStoreError,
};
use crate::utils::auth::{verify_access, verify_admin, AccessLevel};

#[derive(Serialize)]
struct WhoamiResponse {
    pubkey: String,
    role: UserRole,
    is_power_user: bool,
}

#[derive(Serialize)]
struct UserRoleView {
    pubkey: String,
    role: UserRole,
    assigned_by: Option<String>,
    updated_at: i64,
}

impl From<RoleAssignment> for UserRoleView {
    fn from(a: RoleAssignment) -> Self {
        Self {
            pubkey: a.pubkey,
            role: a.role,
            assigned_by: a.assigned_by,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Serialize)]
struct UsersResponse {
    users: Vec<UserRoleView>,
}

/// Body of `PUT /api/admin/rbac/users/{pubkey}/role`.
#[derive(Deserialize)]
pub struct AssignRoleRequest {
    role: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

/// Fetch the process-global role store or a 503 if RBAC is not initialised.
fn store_or_unavailable() -> Result<std::sync::Arc<RoleStore>, HttpResponse> {
    global_role_store().ok_or_else(|| {
        HttpResponse::ServiceUnavailable().json(ErrorBody {
            error: "RBAC role store not initialised".to_string(),
        })
    })
}

/// GET /api/admin/rbac/whoami — the caller's own resolved role. Any
/// authenticated user may call it (useful for the client to render role-gated
/// UI). Returns 401/403 via `verify_access` when unauthenticated.
pub async fn whoami(req: HttpRequest, nostr: web::Data<NostrService>) -> impl Responder {
    let pubkey = match verify_access(&req, &nostr, AccessLevel::Authenticated).await {
        Ok(pk) => pk,
        Err(resp) => return resp,
    };
    let is_power_user = nostr.is_power_user(&pubkey).await;
    let role = match store_or_unavailable() {
        Ok(store) => store.effective_role(&pubkey, is_power_user).await,
        Err(resp) => return resp,
    };
    HttpResponse::Ok().json(WhoamiResponse {
        pubkey,
        role,
        is_power_user,
    })
}

/// GET /api/admin/rbac/users — list explicit role assignments (Admin+).
pub async fn list_users(req: HttpRequest, nostr: web::Data<NostrService>) -> impl Responder {
    // verify_admin resolves the caller's role through the store (ADR-142),
    // so an Editor/Viewer is rejected with 403 here.
    if let Err(resp) = verify_admin(&req, &nostr).await {
        return resp;
    }
    let store = match store_or_unavailable() {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match store.list().await {
        Ok(list) => HttpResponse::Ok().json(UsersResponse {
            users: list.into_iter().map(UserRoleView::from).collect(),
        }),
        Err(e) => {
            warn!("RBAC list_users failed: {e}");
            HttpResponse::InternalServerError().json(ErrorBody {
                error: format!("failed to list roles: {e}"),
            })
        }
    }
}

/// PUT /api/admin/rbac/users/{pubkey}/role — assign a role (Admin+, gated by
/// `can_assign`).
pub async fn assign_role(
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<AssignRoleRequest>,
    nostr: web::Data<NostrService>,
) -> impl Responder {
    let caller = match verify_admin(&req, &nostr).await {
        Ok(pk) => pk,
        Err(resp) => return resp,
    };
    let target_pubkey = path.into_inner();
    let Some(new_role) = UserRole::parse(&body.role) else {
        return HttpResponse::BadRequest().json(ErrorBody {
            error: format!(
                "unknown role '{}': expected one of owner|admin|editor|viewer",
                body.role
            ),
        });
    };

    let store = match store_or_unavailable() {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    // Resolve the caller's own role at admission; the full lattice check, the
    // current-role guard AND the last-Owner invariant are enforced atomically
    // inside `assign_checked` (single SQLite transaction — no TOCTOU).
    // ADR-2010: the store re-reads the caller's role inside that transaction
    // and refuses if it has weakened since admission, so a concurrent demotion
    // cannot be raced past the lattice check.
    let caller_is_power = nostr.is_power_user(&caller).await;
    let caller_role = store.effective_role(&caller, caller_is_power).await;
    let authority = CallerAuthority::new(&caller, caller_is_power, caller_role);
    match store
        .assign_checked(&target_pubkey, new_role, &authority)
        .await
    {
        Ok(role) => {
            info!("RBAC: {caller} assigned {role} to {target_pubkey}");
            HttpResponse::Ok().json(UserRoleView {
                pubkey: target_pubkey,
                role,
                assigned_by: Some(caller),
                updated_at: 0, // client re-reads via GET /users for the stored ts
            })
        }
        Err(e) => role_error_response("assign", &target_pubkey, e),
    }
}

/// DELETE /api/admin/rbac/users/{pubkey}/role — revert a user to the default
/// role (Admin+, gated by `can_assign` on the user's current role).
pub async fn revoke_role(
    req: HttpRequest,
    path: web::Path<String>,
    nostr: web::Data<NostrService>,
) -> impl Responder {
    let caller = match verify_admin(&req, &nostr).await {
        Ok(pk) => pk,
        Err(resp) => return resp,
    };
    let target_pubkey = path.into_inner();
    let store = match store_or_unavailable() {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    // Lattice guard + last-Owner invariant enforced atomically in the store,
    // with the caller's authority re-read inside that transaction (ADR-2010).
    let caller_is_power = nostr.is_power_user(&caller).await;
    let caller_role = store.effective_role(&caller, caller_is_power).await;
    let authority = CallerAuthority::new(&caller, caller_is_power, caller_role);
    let target_is_power = nostr.is_power_user(&target_pubkey).await;
    match store
        .remove_checked(&target_pubkey, target_is_power, &authority)
        .await
    {
        Ok(outcome) => {
            // ADR-2010: removal is not revocation. Say plainly what happened to
            // the target's access rather than implying it was denied.
            info!(
                "RBAC: {caller} removed the explicit role for {target_pubkey} \
(existed={}, previous={:?}, now={}, authority_reduced={})",
                outcome.had_explicit_role,
                outcome.previous_role,
                outcome.effective_after,
                outcome.authority_reduced
            );
            if outcome.revocation_requires_explicit_viewer() {
                warn!(
                    "RBAC: removing {target_pubkey}'s assignment did NOT revoke access — \
they now hold {} by default; assign 'viewer' explicitly to deny",
                    outcome.effective_after
                );
            }
            HttpResponse::Ok().json(serde_json::json!({
                "pubkey": target_pubkey,
                "reverted_to": outcome.effective_after,
                "had_explicit_role": outcome.had_explicit_role,
                "previous_role": outcome.previous_role,
                "authority_reduced": outcome.authority_reduced,
                "access_revoked": outcome.authority_reduced,
                "note": if outcome.revocation_requires_explicit_viewer() {
                    "removal restored default authority; assign 'viewer' explicitly to deny access"
                } else {
                    "explicit assignment removed"
                },
            }))
        }
        Err(e) => role_error_response("remove", &target_pubkey, e),
    }
}

/// Map a [`RoleStoreError`] to the appropriate HTTP response for the admin
/// surface: 403 for authorization, 409 for the last-Owner invariant, 400 for a
/// malformed pubkey, 500 for storage/corruption errors.
fn role_error_response(op: &str, target: &str, e: RoleStoreError) -> HttpResponse {
    use crate::services::role_store::RoleStoreError as E;
    match e {
        E::Forbidden(msg) => {
            warn!("RBAC {op} {target} denied: {msg}");
            HttpResponse::Forbidden().json(ErrorBody { error: msg })
        }
        E::LastOwner => {
            warn!("RBAC {op} {target} refused: would remove the last Owner");
            HttpResponse::Conflict().json(ErrorBody {
                error: "refused: this would remove the last Owner".to_string(),
            })
        }
        E::InvalidPubkey(pk) => HttpResponse::BadRequest().json(ErrorBody {
            error: format!("invalid pubkey: {pk}"),
        }),
        // ADR-2010: the caller was demoted between admission and commit. This
        // is a stale-request conflict, not a permanent denial — the client
        // re-authenticates and retries, and the retry gets a plain 403 if the
        // demotion stands.
        E::CallerAuthorityChanged { admission, current } => {
            warn!(
                "RBAC {op} {target} refused: caller authority changed mid-request \
(admitted as {admission}, now {current})"
            );
            HttpResponse::Conflict().json(ErrorBody {
                error: format!(
                    "caller authority changed during the request (admitted as {admission}, \
now {current}); re-authenticate and retry"
                ),
            })
        }
        other => {
            warn!("RBAC {op} {target} failed: {other}");
            HttpResponse::InternalServerError().json(ErrorBody {
                error: format!("failed to {op} role: {other}"),
            })
        }
    }
}

/// Register the RBAC admin routes under `/api/admin/rbac`.
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin/rbac")
            .route("/whoami", web::get().to(whoami))
            .route("/users", web::get().to(list_users))
            .route("/users/{pubkey}/role", web::put().to(assign_role))
            .route("/users/{pubkey}/role", web::delete().to(revoke_role)),
    );
}
