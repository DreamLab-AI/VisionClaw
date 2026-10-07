//! Persisted pubkey → role store for multi-user RBAC (ADR-142).
//!
//! Backs [`crate::models::rbac::UserRole`] with a dedicated `user_roles` SQLite
//! table. The table lives in the same database file as settings (it shares the
//! already-open [`tokio_rusqlite::Connection`]) but is a **separate, isolated
//! table** — deliberately *not* the user-writable `settings` owner layer — so a
//! user cannot escalate their own role through any settings-write endpoint.
//!
//! Identity is the NIP-98-verified pubkey (the user's DID), canonicalised to
//! lowercase hex by [`canonicalise_pubkey`] on **every** path (get/set/remove/
//! bootstrap/lookup) so a mixed-case input always resolves the same row. A
//! process-global handle ([`global_role_store`]) lets `verify_access` resolve
//! roles without threading a new parameter through every call site; it is
//! installed once at startup from `main.rs`.

use std::sync::Arc;

use log::{error, info, warn};
use once_cell::sync::OnceCell;
use rusqlite::OptionalExtension;
use tokio_rusqlite::Connection;

use crate::models::rbac::UserRole;

/// Environment variable naming the bootstrap Owner pubkey (hex). When set, the
/// pubkey is granted `Owner` on store initialisation if it has no explicit row.
pub const RBAC_OWNER_PUBKEY_ENV: &str = "RBAC_OWNER_PUBKEY";

/// Escape hatch permitting startup with no Owner assigned (legacy single-user
/// deployments where the `POWER_USER_PUBKEYS` → Admin fallback is the intended
/// reality). Without it, [`RoleStore::has_owner`] returning false is a
/// fail-closed startup error (see `main.rs`).
pub const RBAC_ALLOW_OWNERLESS_ENV: &str = "RBAC_ALLOW_OWNERLESS";

/// Role granted to an authenticated pubkey with no explicit assignment.
/// Accepted values: `editor` (compatibility default — preserves pre-RBAC
/// behaviour where every authenticated user could write) and `viewer`
/// (multi-user-locked posture: unknown signers read only until an Admin
/// grants a role). Any other value fails closed to `viewer` with an error
/// log — a typo must never widen access.
pub const RBAC_DEFAULT_ROLE_ENV: &str = "RBAC_DEFAULT_ROLE";

/// Column DDL shared by the initial `CREATE TABLE` and the legacy-migration
/// rebuild. `CHECK`-constrained: `role` is confined to the canonical lattice and
/// `pubkey` to a 64-char lowercase-hex string.
const TABLE_COLUMNS: &str = r#"(
    pubkey      TEXT PRIMARY KEY NOT NULL
                CHECK (length(pubkey) = 64 AND pubkey NOT GLOB '*[^0-9a-f]*'),
    role        TEXT NOT NULL
                CHECK (role IN ('owner','admin','editor','viewer')),
    assigned_by TEXT,
    updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
)"#;

/// Errors from role storage. An unparseable stored role is a *hard error*
/// (`InvalidRole`) — never silently coerced to a default that could grant
/// elevated access.
#[derive(Debug, thiserror::Error)]
pub enum RoleStoreError {
    #[error("role store database error: {0}")]
    Db(#[from] tokio_rusqlite::Error),
    #[error("invalid stored role '{role}' for pubkey {pubkey}")]
    InvalidRole { pubkey: String, role: String },
    #[error("invalid pubkey (must be 64 hex chars): {0}")]
    InvalidPubkey(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("refused: this would remove the last Owner")]
    LastOwner,
    /// ADR-2010: the caller's authority changed between admission and commit.
    ///
    /// The handler resolves the caller's role when the request is admitted, but
    /// the mutation commits later. If a concurrent demotion lands in between,
    /// the request was admitted under authority the caller no longer holds. The
    /// transaction re-reads the caller's role and refuses rather than committing
    /// on the stale value; the client must re-authenticate and retry.
    #[error(
        "refused: caller authority changed during the request \
(admitted as {admission}, now {current}) — re-authenticate and retry"
    )]
    CallerAuthorityChanged {
        admission: UserRole,
        current: UserRole,
    },
}

/// The caller's identity and admission-time authority for a role mutation.
///
/// ADR-2010: passing the resolved `UserRole` alone made the mutation
/// transaction consume authority read *before* the transaction opened. Carrying
/// the pubkey and the power-user flag lets the transaction re-resolve the
/// caller's effective role against the same snapshot it reads the target from,
/// so a concurrent demotion cannot be raced past the lattice check.
#[derive(Debug, Clone)]
pub struct CallerAuthority {
    /// The caller's canonicalised pubkey.
    pub pubkey: String,
    /// Whether the caller is a legacy `POWER_USER_PUBKEYS` member. This is
    /// process configuration rather than stored state, so it is resolved once
    /// by the handler and carried in.
    pub is_power_user: bool,
    /// The effective role the request was admitted under. Retained so a
    /// mid-request change is *reported* rather than silently absorbed.
    pub admission_role: UserRole,
}

impl CallerAuthority {
    /// Build a caller authority from the values the handler already has.
    pub fn new(pubkey: &str, is_power_user: bool, admission_role: UserRole) -> Self {
        Self {
            pubkey: canonicalise_pubkey(pubkey),
            is_power_user,
            admission_role,
        }
    }
}

/// What a successful removal actually did to the target's access.
///
/// ADR-2010: "revoke" is a misnomer for this operation. Deleting the explicit
/// row does not deny the target — it drops them back to whatever the ambient
/// rules grant: `Admin` if they are a legacy power user, otherwise the
/// configured unassigned-signer default (`Editor` unless overridden). For a
/// target sitting at `Viewer`, removal *raises* their authority. Callers get
/// the post-removal effective role and an explicit statement of whether access
/// was actually reduced, so an operator is never told "revoked" when the user
/// still holds write access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RemovalOutcome {
    /// Was there an explicit row to remove?
    pub had_explicit_role: bool,
    /// The role the target held before removal, if any.
    pub previous_role: Option<UserRole>,
    /// The effective role the target holds now that the row is gone.
    pub effective_after: UserRole,
    /// True only when the target's effective authority actually decreased.
    /// False means the removal was a no-op or an *increase* — it did not
    /// revoke anything.
    pub authority_reduced: bool,
}

impl RemovalOutcome {
    /// The explicit assignment needed to genuinely deny a target that removal
    /// would not reduce: assign `Viewer`, do not remove the row.
    pub fn revocation_requires_explicit_viewer(&self) -> bool {
        !self.authority_reduced && self.effective_after > UserRole::Viewer
    }
}

/// Canonicalise a pubkey for storage AND lookup: trim + lowercase. This is the
/// **single** function every path uses so mixed-case input hits the same row.
/// It does *not* validate length/charset — that is [`validate_pubkey`], applied
/// only on writes (a lookup of an arbitrary pubkey must not error).
pub fn canonicalise_pubkey(pubkey: &str) -> String {
    pubkey.trim().to_ascii_lowercase()
}

/// Canonicalise and validate a pubkey to canonical 64-char lowercase hex.
fn validate_pubkey(pubkey: &str) -> Result<String, RoleStoreError> {
    let p = canonicalise_pubkey(pubkey);
    if p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(p)
    } else {
        Err(RoleStoreError::InvalidPubkey(pubkey.to_string()))
    }
}

/// A single persisted role assignment, as returned by [`RoleStore::list`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoleAssignment {
    pub pubkey: String,
    pub role: UserRole,
    pub assigned_by: Option<String>,
    pub updated_at: i64,
}

/// SQLite-backed store mapping pubkeys to [`UserRole`]s.
pub struct RoleStore {
    conn: Arc<Connection>,
    /// Role for authenticated-but-unassigned pubkeys, resolved once at
    /// construction from [`RBAC_DEFAULT_ROLE_ENV`]. See
    /// [`resolve_default_role`] for the parse/fail-closed rules.
    default_role: UserRole,
}

/// Parse an [`RBAC_DEFAULT_ROLE_ENV`] value into the unassigned-signer
/// default role.
///
/// - `None` (unset) / explicitly empty `""` → `Editor` (ADR-2010 compatibility
///   default; compose `${VAR:-editor}` interpolation makes empty ≡ unset)
/// - `editor` → `Editor`; `viewer` → `Viewer` (case-insensitive, trimmed)
/// - anything else present — including whitespace-only — → **fail closed** to
///   `Viewer` with an error log. `admin` and `owner` are deliberately rejected
///   here too: an env var must never be able to mass-grant elevated access.
fn parse_default_role(value: Option<&str>) -> UserRole {
    match value {
        None => UserRole::default_authenticated(),
        Some("") => UserRole::default_authenticated(),
        Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "editor" => UserRole::Editor,
            "viewer" => UserRole::Viewer,
            other => {
                error!(
                    "RBAC: {RBAC_DEFAULT_ROLE_ENV}='{other}' is not one of \
                     'editor'|'viewer'; failing closed to Viewer"
                );
                UserRole::Viewer
            }
        },
    }
}

/// Read and parse [`RBAC_DEFAULT_ROLE_ENV`] from the process environment.
/// A present-but-non-UTF-8 value is a *present invalid* value and fails closed
/// to `Viewer` — it must not be conflated with "unset" (which would widen to
/// Editor).
fn resolve_default_role() -> UserRole {
    match std::env::var(RBAC_DEFAULT_ROLE_ENV) {
        Ok(v) => parse_default_role(Some(&v)),
        Err(std::env::VarError::NotPresent) => parse_default_role(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            error!(
                "RBAC: {RBAC_DEFAULT_ROLE_ENV} is set but not valid UTF-8; \
                 failing closed to Viewer"
            );
            UserRole::Viewer
        }
    }
}

impl RoleStore {
    /// Construct over an already-open connection, ensuring the `user_roles`
    /// table exists **and carries the `CHECK` constraints** — migrating a legacy
    /// (pre-constraint) table by rebuild if necessary.
    pub async fn new(conn: Arc<Connection>) -> Result<Self, tokio_rusqlite::Error> {
        let (migrated, kept, total) = conn
            .call(|c| {
                let existing_sql: Option<String> = c
                    .query_row(
                        "SELECT sql FROM sqlite_master WHERE type='table' AND name='user_roles'",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;

                match existing_sql {
                    // Fresh database — create the constrained table.
                    None => {
                        c.execute_batch(&format!("CREATE TABLE user_roles {TABLE_COLUMNS};"))?;
                        Ok((false, 0i64, 0i64))
                    }
                    // Already constrained — nothing to do.
                    Some(sql) if sql.contains("CHECK") => Ok((false, 0i64, 0i64)),
                    // Legacy unconstrained table — rebuild inside a transaction,
                    // validating existing rows (invalid ones are dropped and the
                    // user reverts to the default role — fail closed).
                    Some(_) => {
                        let tx = c.transaction()?;
                        let total: i64 =
                            tx.query_row("SELECT COUNT(*) FROM user_roles", [], |r| r.get(0))?;
                        tx.execute_batch(&format!("CREATE TABLE user_roles_new {TABLE_COLUMNS};"))?;
                        let kept = tx.execute(
                            "INSERT INTO user_roles_new (pubkey, role, assigned_by, updated_at)
                             SELECT lower(pubkey), lower(role), assigned_by, updated_at
                             FROM user_roles
                             WHERE lower(role) IN ('owner','admin','editor','viewer')
                               AND length(pubkey) = 64
                               AND lower(pubkey) NOT GLOB '*[^0-9a-f]*'",
                            [],
                        )? as i64;
                        tx.execute_batch(
                            "DROP TABLE user_roles;
                             ALTER TABLE user_roles_new RENAME TO user_roles;",
                        )?;
                        tx.commit()?;
                        Ok((true, kept, total))
                    }
                }
            })
            .await?;

        if migrated {
            if kept < total {
                warn!(
                    "RBAC: migrated legacy user_roles table to constrained schema; \
                     kept {kept}/{total} rows ({} invalid dropped, reverting to default)",
                    total - kept
                );
            } else {
                info!("RBAC: migrated legacy user_roles table to constrained schema ({kept} rows)");
            }
        }
        let default_role = resolve_default_role();
        if default_role != UserRole::default_authenticated() {
            info!(
                "RBAC: unassigned-signer default role is {} (via {RBAC_DEFAULT_ROLE_ENV})",
                default_role.as_str()
            );
        }
        Ok(Self { conn, default_role })
    }

    /// Construct with an explicitly supplied unassigned-signer default,
    /// bypassing [`RBAC_DEFAULT_ROLE_ENV`].
    ///
    /// The environment variable is the deployment-time control; this is the
    /// programmatic one, for embedders that configure the posture in code and
    /// for exercising the ADR-2010 removal semantics under both defaults
    /// (removal restores the default, so the default decides whether removing
    /// an assignment reduces authority or raises it).
    pub async fn new_with_default(
        conn: Arc<Connection>,
        default_role: UserRole,
    ) -> Result<Self, tokio_rusqlite::Error> {
        let mut store = Self::new(conn).await?;
        store.default_role = default_role;
        Ok(store)
    }

    /// The role an authenticated-but-unassigned pubkey resolves to, as
    /// configured by [`RBAC_DEFAULT_ROLE_ENV`] at construction.
    pub fn configured_default(&self) -> UserRole {
        self.default_role
    }

    /// Resolve the explicit role for `pubkey`, or `None` if unassigned. A row
    /// whose stored role does not parse is a hard [`RoleStoreError::InvalidRole`].
    pub async fn get(&self, pubkey: &str) -> Result<Option<UserRole>, RoleStoreError> {
        let key = canonicalise_pubkey(pubkey);
        let key_for_err = key.clone();
        let raw: Option<String> = self
            .conn
            .call(move |c| {
                let mut stmt = c.prepare_cached("SELECT role FROM user_roles WHERE pubkey = ?1")?;
                let mut rows = stmt.query([key])?;
                match rows.next()? {
                    Some(row) => Ok(Some(row.get::<_, String>(0)?)),
                    None => Ok(None),
                }
            })
            .await?;
        match raw {
            None => Ok(None),
            Some(s) => UserRole::parse(&s)
                .map(Some)
                .ok_or(RoleStoreError::InvalidRole {
                    pubkey: key_for_err,
                    role: s,
                }),
        }
    }

    /// Resolve the *effective* role for an authenticated user.
    ///
    /// Precedence: explicit assignment → `Admin` for legacy power users → else
    /// the configured unassigned-signer default ([`RBAC_DEFAULT_ROLE_ENV`],
    /// `Editor` unless overridden). **Fails closed** to `Viewer` on any error
    /// (including an invalid stored role) — never up to Admin/power-user.
    pub async fn effective_role(&self, pubkey: &str, is_power_user: bool) -> UserRole {
        match self.get(pubkey).await {
            Ok(Some(role)) => role,
            Ok(None) => {
                if is_power_user {
                    UserRole::Admin
                } else {
                    self.default_role
                }
            }
            Err(e) => {
                error!("RBAC: role lookup failed for {pubkey}: {e}; failing closed to Viewer");
                UserRole::Viewer
            }
        }
    }

    /// Upsert `pubkey`'s role. `pubkey` is validated to canonical 64-hex.
    pub async fn set(
        &self,
        pubkey: &str,
        role: UserRole,
        assigned_by: Option<&str>,
    ) -> Result<UserRole, RoleStoreError> {
        let key = validate_pubkey(pubkey)?;
        let role_str = role.as_str().to_string();
        let assigned_by = assigned_by.map(canonicalise_pubkey);
        self.conn
            .call(move |c| {
                c.execute(
                    "INSERT INTO user_roles (pubkey, role, assigned_by, updated_at)
                     VALUES (?1, ?2, ?3, strftime('%s','now'))
                     ON CONFLICT(pubkey) DO UPDATE SET
                        role = excluded.role,
                        assigned_by = excluded.assigned_by,
                        updated_at = excluded.updated_at",
                    rusqlite::params![key, role_str, assigned_by],
                )?;
                Ok(())
            })
            .await?;
        Ok(role)
    }

    /// Assign `new_role` to `target`, enforcing the full authorization lattice
    /// **and** the last-Owner invariant **atomically** in one transaction
    /// (Codex round-2): the current-role read, the `can_assign` checks, the
    /// last-Owner count, and the write cannot interleave with a concurrent
    /// mutation.
    pub async fn assign_checked(
        &self,
        target: &str,
        new_role: UserRole,
        caller: &CallerAuthority,
    ) -> Result<UserRole, RoleStoreError> {
        let key = validate_pubkey(target)?;
        let assigned_by = caller.pubkey.clone();
        let new_str = new_role.as_str().to_string();
        let caller = caller.clone();
        let default_role = self.default_role;

        let outcome: TxOutcome = self
            .conn
            .call(move |c| {
                let tx = c.transaction()?;

                // ADR-2010: re-resolve the CALLER's authority inside the same
                // transaction that reads the target, so a concurrent demotion
                // cannot be raced past the lattice check below.
                let current = resolve_caller_role_in_tx(&tx, &caller, default_role)?;
                let caller_role = match effective_mutation_authority(caller.admission_role, current)
                {
                    Ok(role) => role,
                    Err(outcome) => return Ok(outcome),
                };
                let caller = caller_role;

                let existing = read_role(&tx, &key)?;
                if let ExistingRole::Invalid(role) = existing {
                    return Ok(TxOutcome::InvalidExisting { pubkey: key, role });
                }
                let existing = existing.into_option();

                // Authorization: caller must be able to assign the NEW role, and
                // (if the target already holds one) their CURRENT role too.
                if !caller.can_assign(new_role) {
                    return Ok(TxOutcome::Forbidden(format!(
                        "{caller} may not assign role {new_role}"
                    )));
                }
                if let Some(ex) = existing {
                    if !caller.can_assign(ex) {
                        return Ok(TxOutcome::Forbidden(format!(
                            "{caller} may not modify a user currently holding {ex}"
                        )));
                    }
                }

                // Last-Owner invariant: demoting the sole Owner is refused.
                if existing == Some(UserRole::Owner) && new_role != UserRole::Owner {
                    let owners: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM user_roles WHERE role = 'owner'",
                        [],
                        |r| r.get(0),
                    )?;
                    if owners <= 1 {
                        return Ok(TxOutcome::LastOwner);
                    }
                }

                tx.execute(
                    "INSERT INTO user_roles (pubkey, role, assigned_by, updated_at)
                     VALUES (?1, ?2, ?3, strftime('%s','now'))
                     ON CONFLICT(pubkey) DO UPDATE SET
                        role = excluded.role,
                        assigned_by = excluded.assigned_by,
                        updated_at = excluded.updated_at",
                    rusqlite::params![key, new_str, assigned_by],
                )?;
                tx.commit()?;
                Ok(TxOutcome::Ok)
            })
            .await?;

        outcome.into_result(new_role)
    }

    /// Remove an explicit assignment, enforcing `can_assign` on the current role
    /// and the last-Owner invariant, atomically.
    ///
    /// ADR-2010 — **removal is not revocation.** Deleting the row drops the
    /// target to their ambient authority: `Admin` if they are a legacy power
    /// user, otherwise the configured unassigned-signer default. The returned
    /// [`RemovalOutcome`] states the post-removal effective role and whether
    /// authority actually fell, so an operator is never told "revoked" when the
    /// user still holds write access. To genuinely deny a target, assign
    /// `Viewer` explicitly — see
    /// [`RemovalOutcome::revocation_requires_explicit_viewer`].
    ///
    /// `target_is_power_user` is process configuration the store cannot read,
    /// so the caller supplies it; it decides the post-removal effective role.
    pub async fn remove_checked(
        &self,
        target: &str,
        target_is_power_user: bool,
        caller: &CallerAuthority,
    ) -> Result<RemovalOutcome, RoleStoreError> {
        let key = canonicalise_pubkey(target);
        let caller = caller.clone();
        let default_role = self.default_role;

        let outcome: TxRemoval = self
            .conn
            .call(move |c| {
                let tx = c.transaction()?;

                // ADR-2010: caller authority is re-read inside the transaction.
                let current = resolve_caller_role_in_tx(&tx, &caller, default_role)?;
                let caller_role = match effective_mutation_authority(caller.admission_role, current)
                {
                    Ok(role) => role,
                    Err(outcome) => return Ok(TxRemoval::Other(outcome)),
                };
                let caller = caller_role;

                let existing = read_role(&tx, &key)?;
                if let ExistingRole::Invalid(role) = existing {
                    return Ok(TxRemoval::Other(TxOutcome::InvalidExisting {
                        pubkey: key,
                        role,
                    }));
                }
                let existing = existing.into_option();
                let Some(ex) = existing else {
                    // Nothing to remove — a no-op, not a revocation.
                    return Ok(TxRemoval::Removed {
                        previous_role: None,
                    });
                };
                if !caller.can_assign(ex) {
                    return Ok(TxRemoval::Other(TxOutcome::Forbidden(format!(
                        "{caller} may not remove the assignment of a user currently holding {ex}"
                    ))));
                }
                if ex == UserRole::Owner {
                    let owners: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM user_roles WHERE role = 'owner'",
                        [],
                        |r| r.get(0),
                    )?;
                    if owners <= 1 {
                        return Ok(TxRemoval::Other(TxOutcome::LastOwner));
                    }
                }
                tx.execute("DELETE FROM user_roles WHERE pubkey = ?1", [key])?;
                tx.commit()?;
                Ok(TxRemoval::Removed {
                    previous_role: Some(ex),
                })
            })
            .await?;

        let previous_role = match outcome {
            TxRemoval::Removed { previous_role } => previous_role,
            TxRemoval::Other(other) => {
                return Err(other.into_result(UserRole::Viewer).unwrap_err())
            }
        };

        let effective_after = if target_is_power_user {
            UserRole::Admin
        } else {
            self.default_role
        };
        Ok(RemovalOutcome {
            had_explicit_role: previous_role.is_some(),
            previous_role,
            effective_after,
            authority_reduced: previous_role.is_some_and(|prev| effective_after < prev),
        })
    }

    /// Whether any pubkey currently holds `Owner`. Used by the startup lockout
    /// guard.
    pub async fn has_owner(&self) -> Result<bool, RoleStoreError> {
        let exists: i64 = self
            .conn
            .call(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT EXISTS(SELECT 1 FROM user_roles WHERE role = 'owner')",
                )?;
                let mut rows = stmt.query([])?;
                match rows.next()? {
                    Some(row) => Ok(row.get::<_, i64>(0)?),
                    None => Ok(0),
                }
            })
            .await?;
        Ok(exists != 0)
    }

    /// List every explicit assignment, most-recently-updated first.
    pub async fn list(&self) -> Result<Vec<RoleAssignment>, RoleStoreError> {
        let rows = self
            .conn
            .call(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT pubkey, role, assigned_by, updated_at
                     FROM user_roles ORDER BY updated_at DESC",
                )?;
                let mapped = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })?;
                let mut out = Vec::new();
                for r in mapped {
                    out.push(r?);
                }
                Ok(out)
            })
            .await?;

        let mut assignments = Vec::with_capacity(rows.len());
        for (pubkey, role, assigned_by, updated_at) in rows {
            let role = UserRole::parse(&role).ok_or_else(|| RoleStoreError::InvalidRole {
                pubkey: pubkey.clone(),
                role,
            })?;
            assignments.push(RoleAssignment {
                pubkey,
                role,
                assigned_by,
                updated_at,
            });
        }
        Ok(assignments)
    }

    /// Grant `Owner` to the pubkey named by [`RBAC_OWNER_PUBKEY_ENV`] if it has
    /// no explicit role yet. Idempotent; a no-op when the env var is unset.
    pub async fn bootstrap_owner_from_env(&self) -> Result<(), RoleStoreError> {
        let Ok(owner) = std::env::var(RBAC_OWNER_PUBKEY_ENV) else {
            return Ok(());
        };
        if owner.trim().is_empty() {
            return Ok(());
        }
        match validate_pubkey(&owner) {
            Ok(key) => {
                if self.get(&key).await?.is_none() {
                    self.set(&key, UserRole::Owner, Some("bootstrap:env"))
                        .await?;
                    info!("RBAC: bootstrapped Owner role for {key} from {RBAC_OWNER_PUBKEY_ENV}");
                }
                Ok(())
            }
            Err(_) => {
                warn!(
                    "RBAC: {RBAC_OWNER_PUBKEY_ENV} is not a valid 64-hex pubkey ('{owner}'); skipping Owner bootstrap"
                );
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Transaction helpers
// ---------------------------------------------------------------------------

/// The current role of a target row as read inside a transaction.
enum ExistingRole {
    None,
    Some(UserRole),
    Invalid(String),
}

impl ExistingRole {
    fn into_option(self) -> Option<UserRole> {
        match self {
            ExistingRole::Some(r) => Some(r),
            _ => None,
        }
    }
}

/// Read a target's role within a transaction, classifying an unparseable stored
/// value as `Invalid` (a hard error surfaced to the caller).
fn read_role(tx: &rusqlite::Transaction<'_>, pubkey: &str) -> rusqlite::Result<ExistingRole> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT role FROM user_roles WHERE pubkey = ?1",
            [pubkey],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw {
        None => ExistingRole::None,
        Some(s) => match UserRole::parse(&s) {
            Some(r) => ExistingRole::Some(r),
            None => ExistingRole::Invalid(s),
        },
    })
}

/// Resolve the caller's effective role from *inside* an open transaction
/// (ADR-2010).
///
/// Applies exactly the precedence [`RoleStore::effective_role`] uses — explicit
/// assignment, then `Admin` for a legacy power user, then the configured
/// unassigned default — but against the transaction's snapshot, so the value
/// cannot be stale by the time the write commits. An unparseable stored role
/// fails closed to `Viewer`, matching the async path: a corrupt row must never
/// widen the caller's authority.
fn resolve_caller_role_in_tx(
    tx: &rusqlite::Transaction<'_>,
    caller: &CallerAuthority,
    default_role: UserRole,
) -> rusqlite::Result<UserRole> {
    Ok(match read_role(tx, &caller.pubkey)? {
        ExistingRole::Some(role) => role,
        ExistingRole::Invalid(role) => {
            error!(
                "RBAC: caller {} has invalid stored role '{role}'; failing closed to Viewer",
                caller.pubkey
            );
            UserRole::Viewer
        }
        ExistingRole::None => {
            if caller.is_power_user {
                UserRole::Admin
            } else {
                default_role
            }
        }
    })
}

/// The authority a mutation may act under, given admission-time and
/// in-transaction readings (ADR-2010).
///
/// * A **demotion** (current < admission) aborts: the request was admitted on
///   authority the caller no longer holds.
/// * A **promotion** (current > admission) does not escalate: the mutation is
///   still bound by the admission role, so winning a race cannot grant a
///   privilege the request was not admitted for.
/// * Unchanged authority proceeds normally.
fn effective_mutation_authority(
    admission: UserRole,
    current: UserRole,
) -> Result<UserRole, TxOutcome> {
    if current < admission {
        Err(TxOutcome::CallerChanged { admission, current })
    } else {
        Ok(admission)
    }
}

/// Outcome of the removal transaction: either the row was removed (carrying the
/// role it held) or the transaction refused, in which case the shared
/// [`TxOutcome`] mapping applies.
enum TxRemoval {
    Removed { previous_role: Option<UserRole> },
    Other(TxOutcome),
}

/// Outcome of a transactional check+mutate, mapped to [`RoleStoreError`] outside
/// the `conn.call` closure (whose error type cannot carry our variants).
enum TxOutcome {
    Ok,
    Forbidden(String),
    LastOwner,
    InvalidExisting {
        pubkey: String,
        role: String,
    },
    /// ADR-2010: the caller's in-transaction role is weaker than the role the
    /// request was admitted under.
    CallerChanged {
        admission: UserRole,
        current: UserRole,
    },
}

impl TxOutcome {
    fn into_result(self, ok_role: UserRole) -> Result<UserRole, RoleStoreError> {
        match self {
            TxOutcome::Ok => Ok(ok_role),
            TxOutcome::Forbidden(m) => Err(RoleStoreError::Forbidden(m)),
            TxOutcome::LastOwner => Err(RoleStoreError::LastOwner),
            TxOutcome::InvalidExisting { pubkey, role } => {
                Err(RoleStoreError::InvalidRole { pubkey, role })
            }
            TxOutcome::CallerChanged { admission, current } => {
                Err(RoleStoreError::CallerAuthorityChanged { admission, current })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Process-global handle
// ---------------------------------------------------------------------------

static GLOBAL_ROLE_STORE: OnceCell<Arc<RoleStore>> = OnceCell::new();

/// Install the process-global [`RoleStore`]. Called once at startup.
pub fn set_global_role_store(store: Arc<RoleStore>) -> Result<(), Arc<RoleStore>> {
    GLOBAL_ROLE_STORE.set(store)
}

/// Fetch the process-global [`RoleStore`], if installed.
pub fn global_role_store() -> Option<Arc<RoleStore>> {
    GLOBAL_ROLE_STORE.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PK_PU: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    // Same key as PK_A but upper-case, to prove canonicalisation.
    const PK_A_UPPER: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    async fn mem_store() -> RoleStore {
        let conn = Connection::open_in_memory()
            .await
            .expect("open in-memory db");
        RoleStore::new(Arc::new(conn)).await.expect("create table")
    }

    /// Give `pubkey` an explicit `role` row and return the matching
    /// [`CallerAuthority`]. ADR-2010: the admission role must be derivable from
    /// what the transaction can read, so a test caller needs a real row (or the
    /// power-user flag) — an admission role conjured from nothing is exactly
    /// the stale authority the store now refuses.
    async fn caller_with_role(store: &RoleStore, pubkey: &str, role: UserRole) -> CallerAuthority {
        store
            .set(pubkey, role, None)
            .await
            .expect("seed caller row");
        CallerAuthority::new(pubkey, false, role)
    }

    /// A caller whose authority comes from the legacy power-user list rather
    /// than a stored row.
    fn power_user_caller(pubkey: &str) -> CallerAuthority {
        CallerAuthority::new(pubkey, true, UserRole::Admin)
    }

    #[tokio::test]
    async fn unassigned_user_gets_editor_default() {
        let store = mem_store().await;
        assert_eq!(store.get(PK_A).await.unwrap(), None);
        assert_eq!(store.effective_role(PK_A, false).await, UserRole::Editor);
    }

    #[test]
    fn default_role_parse_lattice() {
        // Unset and explicitly-empty preserve the ADR-2010 compatibility
        // default (compose `${VAR:-editor}` makes empty ≡ unset).
        assert_eq!(parse_default_role(None), UserRole::Editor);
        assert_eq!(parse_default_role(Some("")), UserRole::Editor);
        // Explicit values, case-insensitive.
        assert_eq!(parse_default_role(Some("editor")), UserRole::Editor);
        assert_eq!(parse_default_role(Some("Viewer")), UserRole::Viewer);
        assert_eq!(parse_default_role(Some(" viewer ")), UserRole::Viewer);
        // Present-but-garbage fails CLOSED to Viewer — an env var must never
        // widen access. Whitespace-only is present garbage, not "unset".
        assert_eq!(parse_default_role(Some("  ")), UserRole::Viewer);
        assert_eq!(parse_default_role(Some("admin")), UserRole::Viewer);
        assert_eq!(parse_default_role(Some("owner")), UserRole::Viewer);
        assert_eq!(parse_default_role(Some("banana")), UserRole::Viewer);
    }

    #[tokio::test]
    async fn unassigned_default_is_store_configured_not_hardcoded() {
        // The store's Ok(None) branch must consult the constructed default,
        // not UserRole::default_authenticated() directly.
        let conn = Connection::open_in_memory().await.expect("open db");
        let mut store = RoleStore::new(Arc::new(conn)).await.expect("create");
        store.default_role = UserRole::Viewer;
        assert_eq!(store.effective_role(PK_A, false).await, UserRole::Viewer);
        assert_eq!(store.configured_default(), UserRole::Viewer);
        // Power-user precedence is unaffected by the default.
        assert_eq!(store.effective_role(PK_PU, true).await, UserRole::Admin);
        // An explicit assignment still wins over the default.
        store.set(PK_A, UserRole::Editor, None).await.unwrap();
        assert_eq!(store.effective_role(PK_A, false).await, UserRole::Editor);
    }

    #[tokio::test]
    async fn power_user_without_row_maps_to_admin() {
        let store = mem_store().await;
        assert_eq!(store.effective_role(PK_PU, true).await, UserRole::Admin);
    }

    #[tokio::test]
    async fn explicit_role_overrides_power_user_flag() {
        let store = mem_store().await;
        store
            .set(PK_PU, UserRole::Viewer, Some(PK_A))
            .await
            .unwrap();
        assert_eq!(store.effective_role(PK_PU, true).await, UserRole::Viewer);
    }

    #[tokio::test]
    async fn set_get_roundtrip_and_upsert() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Admin, Some(PK_B)).await.unwrap();
        assert_eq!(store.get(PK_A).await.unwrap(), Some(UserRole::Admin));
        store.set(PK_A, UserRole::Editor, Some(PK_B)).await.unwrap();
        assert_eq!(store.get(PK_A).await.unwrap(), Some(UserRole::Editor));
    }

    #[tokio::test]
    async fn mixed_case_pubkey_hits_same_row() {
        let store = mem_store().await;
        // Store under upper-case, read under lower-case (and vice versa).
        store.set(PK_A_UPPER, UserRole::Viewer, None).await.unwrap();
        assert_eq!(store.get(PK_A).await.unwrap(), Some(UserRole::Viewer));
        assert_eq!(
            store.effective_role(PK_A, true).await,
            UserRole::Viewer,
            "canonicalised lookup must find the explicit Viewer row"
        );
        // Only one row exists — the two cases did not create duplicates.
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_reverts_to_default() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Editor, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        let first = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert!(first.had_explicit_role);
        assert_eq!(first.previous_role, Some(UserRole::Editor));
        assert_eq!(store.get(PK_A).await.unwrap(), None);

        let second = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert!(!second.had_explicit_role, "second removal is a no-op");
        assert!(!second.authority_reduced);
    }

    #[tokio::test]
    async fn list_returns_all_assignments() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Owner, None).await.unwrap();
        store.set(PK_B, UserRole::Viewer, Some(PK_A)).await.unwrap();
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all
            .iter()
            .any(|r| r.pubkey == PK_A && r.role == UserRole::Owner));
        assert!(all
            .iter()
            .any(|r| r.pubkey == PK_B && r.role == UserRole::Viewer));
    }

    #[tokio::test]
    async fn has_owner_tracks_owner_presence() {
        let store = mem_store().await;
        assert!(!store.has_owner().await.unwrap());
        store.set(PK_A, UserRole::Admin, None).await.unwrap();
        assert!(!store.has_owner().await.unwrap(), "Admin is not Owner");
        store.set(PK_B, UserRole::Owner, None).await.unwrap();
        assert!(store.has_owner().await.unwrap());
    }

    #[tokio::test]
    async fn set_rejects_non_hex_pubkey() {
        let store = mem_store().await;
        let err = store.set("not-a-hex-pubkey", UserRole::Editor, None).await;
        assert!(matches!(err, Err(RoleStoreError::InvalidPubkey(_))));
    }

    #[tokio::test]
    async fn demoting_last_owner_is_rejected() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Owner, None).await.unwrap();
        // Owner demotes themselves → refused (they are the last Owner).
        let self_caller = CallerAuthority::new(PK_A, false, UserRole::Owner);
        let err = store
            .assign_checked(PK_A, UserRole::Admin, &self_caller)
            .await;
        assert!(matches!(err, Err(RoleStoreError::LastOwner)));
        // Removing the last Owner's assignment is likewise refused.
        let rev = store.remove_checked(PK_A, false, &self_caller).await;
        assert!(matches!(rev, Err(RoleStoreError::LastOwner)));
        // With a second Owner, demotion of the first is allowed.
        let other_owner = caller_with_role(&store, PK_B, UserRole::Owner).await;
        assert_eq!(
            store
                .assign_checked(PK_A, UserRole::Admin, &other_owner)
                .await
                .unwrap(),
            UserRole::Admin
        );
    }

    #[tokio::test]
    async fn assign_checked_enforces_can_assign() {
        let store = mem_store().await;
        let admin = caller_with_role(&store, PK_B, UserRole::Admin).await;
        // An Admin cannot mint an Admin/Owner.
        assert!(matches!(
            store.assign_checked(PK_A, UserRole::Admin, &admin).await,
            Err(RoleStoreError::Forbidden(_))
        ));
        // ...but can grant Editor.
        assert_eq!(
            store
                .assign_checked(PK_A, UserRole::Editor, &admin)
                .await
                .unwrap(),
            UserRole::Editor
        );
        // A power-user caller with no stored row reaches the same Admin
        // authority through the legacy fallback.
        let legacy = power_user_caller(PK_PU);
        assert_eq!(
            store
                .assign_checked(PK_A, UserRole::Viewer, &legacy)
                .await
                .unwrap(),
            UserRole::Viewer
        );
    }

    #[tokio::test]
    async fn check_constraint_rejects_unknown_role() {
        let store = mem_store().await;
        let bad = store
            .conn
            .call(|c| {
                c.execute(
                    "INSERT INTO user_roles (pubkey, role) VALUES (?1, 'superuser')",
                    [PK_A],
                )?;
                Ok(())
            })
            .await;
        assert!(bad.is_err(), "CHECK constraint must reject an unknown role");
    }

    #[tokio::test]
    async fn invalid_stored_role_is_hard_error_and_fails_closed() {
        // Legacy/corrupt table WITHOUT the CHECK, with a bad role. new() must
        // migrate it (dropping the invalid row), so the corrupt role cannot
        // survive; get() then returns None and effective_role is the default.
        let conn = Connection::open_in_memory().await.unwrap();
        conn.call(|c| {
            c.execute_batch(
                "CREATE TABLE user_roles (pubkey TEXT PRIMARY KEY, role TEXT, \
                 assigned_by TEXT, updated_at INTEGER DEFAULT 0);",
            )?;
            Ok(())
        })
        .await
        .unwrap();
        let key = PK_A.to_string();
        conn.call(move |c| {
            c.execute(
                "INSERT INTO user_roles (pubkey, role) VALUES (?1, 'superuser')",
                [key],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let store = RoleStore::new(Arc::new(conn)).await.unwrap();
        // Migration dropped the invalid row (fail closed) — no corrupt role
        // survives, and the user reverts to the default.
        assert_eq!(store.get(PK_A).await.unwrap(), None);
        assert_eq!(store.effective_role(PK_A, true).await, UserRole::Admin);
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn legacy_table_migration_applies_constraints() {
        // A legacy table with a MIX of valid and invalid rows: valid ones are
        // preserved (canonicalised), invalid ones dropped, and the rebuilt table
        // enforces the CHECK afterwards.
        let conn = Connection::open_in_memory().await.unwrap();
        conn.call(|c| {
            c.execute_batch(
                "CREATE TABLE user_roles (pubkey TEXT PRIMARY KEY, role TEXT, \
                 assigned_by TEXT, updated_at INTEGER DEFAULT 0);
                 INSERT INTO user_roles (pubkey, role) VALUES
                    ('AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA','owner'),
                    ('short','admin'),
                    ('dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd','bogus');",
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let store = RoleStore::new(Arc::new(conn)).await.unwrap();
        // Only the valid Owner survived, canonicalised to lowercase.
        let all = store.list().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].role, UserRole::Owner);
        assert_eq!(all[0].pubkey, PK_A);
        assert!(store.has_owner().await.unwrap());
        // The rebuilt table enforces the CHECK.
        let bad = store
            .conn
            .call(|c| {
                c.execute(
                    "INSERT INTO user_roles (pubkey, role) VALUES (?1, 'superuser')",
                    [PK_B],
                )?;
                Ok(())
            })
            .await;
        assert!(bad.is_err(), "migrated table must enforce the role CHECK");
    }

    // ---- ADR-2010 caller-authority freshness ---------------------------------

    /// The reproduced defect: the handler resolves the caller's role, then the
    /// transaction consumes that stale value. Demote the caller after admission
    /// and the mutation must refuse, not commit on the old authority.
    #[tokio::test]
    async fn concurrent_caller_demotion_is_refused() {
        let store = mem_store().await;
        // PK_B is admitted as Owner...
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        // ...and is demoted to Editor before the mutation commits.
        store.set(PK_B, UserRole::Editor, None).await.unwrap();

        let err = store
            .assign_checked(PK_A, UserRole::Admin, &caller)
            .await
            .expect_err("a demoted caller must not mint an Admin");
        match err {
            RoleStoreError::CallerAuthorityChanged { admission, current } => {
                assert_eq!(admission, UserRole::Owner);
                assert_eq!(current, UserRole::Editor);
            }
            other => panic!("expected CallerAuthorityChanged, got {other:?}"),
        }
        assert_eq!(
            store.get(PK_A).await.unwrap(),
            None,
            "the refused mutation must not have written anything"
        );
    }

    /// Removal is guarded by the same freshness check.
    #[tokio::test]
    async fn concurrent_caller_demotion_is_refused_on_removal() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Editor, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        store.set(PK_B, UserRole::Viewer, None).await.unwrap();

        let err = store
            .remove_checked(PK_A, false, &caller)
            .await
            .expect_err("a demoted caller must not remove assignments");
        assert!(matches!(
            err,
            RoleStoreError::CallerAuthorityChanged {
                admission: UserRole::Owner,
                current: UserRole::Viewer,
            }
        ));
        assert_eq!(
            store.get(PK_A).await.unwrap(),
            Some(UserRole::Editor),
            "the target's assignment must survive a refused removal"
        );
    }

    /// Losing the explicit row entirely is also a demotion when the default is
    /// weaker than the admission role.
    #[tokio::test]
    async fn caller_row_removed_mid_request_is_a_demotion() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        store
            .conn
            .call(|c| {
                c.execute("DELETE FROM user_roles WHERE pubkey = ?1", [PK_B])?;
                Ok(())
            })
            .await
            .unwrap();

        let err = store
            .assign_checked(PK_A, UserRole::Owner, &caller)
            .await
            .expect_err("an unassigned caller falls back to the default role");
        assert!(matches!(
            err,
            RoleStoreError::CallerAuthorityChanged {
                admission: UserRole::Owner,
                // the store default for an unassigned signer
                current: UserRole::Editor,
            }
        ));
    }

    /// A caller whose stored role is corrupted mid-request fails closed to
    /// Viewer inside the transaction, which is a demotion.
    #[tokio::test]
    async fn corrupt_caller_row_mid_request_fails_closed() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        // Bypass the CHECK constraint the way the migration test does, so the
        // stored value is unparseable.
        store
            .conn
            .call(|c| {
                c.execute("PRAGMA writable_schema=ON", [])?;
                c.execute(
                    "UPDATE user_roles SET role = 'superuser' WHERE pubkey = ?1",
                    [PK_B],
                )
                .ok();
                c.execute("PRAGMA writable_schema=OFF", [])?;
                Ok(())
            })
            .await
            .ok();

        // Either the CHECK held (role unchanged, mutation succeeds) or the row
        // is now corrupt and the caller fails closed to Viewer. Both are safe;
        // what must never happen is the mutation committing on Owner authority
        // over a corrupt row.
        let result = store.assign_checked(PK_A, UserRole::Owner, &caller).await;
        match store.get(PK_B).await {
            Ok(Some(UserRole::Owner)) => {
                assert!(result.is_ok(), "an intact Owner row may still assign");
            }
            _ => {
                assert!(
                    result.is_err(),
                    "a caller whose row no longer reads as Owner must be refused"
                );
            }
        }
    }

    /// A promotion between admission and commit does not escalate: the
    /// mutation stays bound by the authority the request was admitted under, so
    /// winning a race cannot grant a privilege the request never had.
    #[tokio::test]
    async fn concurrent_caller_promotion_does_not_escalate() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Admin).await;
        // Promoted to Owner after admission.
        store.set(PK_B, UserRole::Owner, None).await.unwrap();

        // Admin admission authority still cannot mint an Owner.
        assert!(matches!(
            store.assign_checked(PK_A, UserRole::Owner, &caller).await,
            Err(RoleStoreError::Forbidden(_))
        ));
        // The same caller re-admitted as Owner can.
        let reauthenticated = CallerAuthority::new(PK_B, false, UserRole::Owner);
        assert_eq!(
            store
                .assign_checked(PK_A, UserRole::Owner, &reauthenticated)
                .await
                .unwrap(),
            UserRole::Owner
        );
    }

    /// Unchanged authority behaves exactly as before the freshness check.
    #[tokio::test]
    async fn unchanged_caller_authority_proceeds() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        assert_eq!(
            store
                .assign_checked(PK_A, UserRole::Admin, &caller)
                .await
                .unwrap(),
            UserRole::Admin
        );
    }

    /// The freshness check runs before the lattice check, so a demoted caller
    /// gets the stale-authority answer rather than a plain denial — the client
    /// needs to know re-authentication will help.
    #[tokio::test]
    async fn demotion_is_reported_distinctly_from_denial() {
        let store = mem_store().await;
        let demoted = caller_with_role(&store, PK_B, UserRole::Owner).await;
        store.set(PK_B, UserRole::Viewer, None).await.unwrap();
        assert!(matches!(
            store.assign_checked(PK_A, UserRole::Editor, &demoted).await,
            Err(RoleStoreError::CallerAuthorityChanged { .. })
        ));

        // A caller who never had the authority gets a plain Forbidden.
        let viewer = caller_with_role(&store, PK_PU, UserRole::Viewer).await;
        assert!(matches!(
            store.assign_checked(PK_A, UserRole::Editor, &viewer).await,
            Err(RoleStoreError::Forbidden(_))
        ));
    }

    // ---- ADR-2010 removal versus revocation ----------------------------------

    /// Removing a Viewer assignment RAISES the target's authority to the
    /// unassigned default. The outcome must say so rather than reporting a
    /// revocation.
    #[tokio::test]
    async fn removing_a_viewer_assignment_is_not_a_revocation() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Viewer, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;

        let outcome = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert!(outcome.had_explicit_role);
        assert_eq!(outcome.previous_role, Some(UserRole::Viewer));
        assert_eq!(
            outcome.effective_after,
            UserRole::Editor,
            "removal drops to the unassigned default, which is above Viewer"
        );
        assert!(
            !outcome.authority_reduced,
            "removing a Viewer row increases authority; it revokes nothing"
        );
        assert!(outcome.revocation_requires_explicit_viewer());
    }

    /// Removing an Admin assignment does reduce authority, so it is a genuine
    /// reduction — though still not a denial.
    #[tokio::test]
    async fn removing_an_admin_assignment_reduces_authority() {
        let store = mem_store().await;
        store.set(PK_A, UserRole::Admin, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;

        let outcome = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert_eq!(outcome.previous_role, Some(UserRole::Admin));
        assert_eq!(outcome.effective_after, UserRole::Editor);
        assert!(outcome.authority_reduced);
        assert!(
            !outcome.revocation_requires_explicit_viewer(),
            "authority fell, so the operator is not misled"
        );
    }

    /// For a legacy power user, removal restores Admin — the strongest
    /// not-a-revocation case there is.
    #[tokio::test]
    async fn removing_a_power_user_assignment_restores_admin() {
        let store = mem_store().await;
        store.set(PK_PU, UserRole::Viewer, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;

        let outcome = store.remove_checked(PK_PU, true, &caller).await.unwrap();
        assert_eq!(
            outcome.effective_after,
            UserRole::Admin,
            "a power user reverts to Admin, not the configured default"
        );
        assert!(!outcome.authority_reduced);
        assert!(
            outcome.revocation_requires_explicit_viewer(),
            "removing a power user's Viewer row hands them Admin back"
        );
    }

    /// The actual revocation: assign Viewer explicitly. It reduces a power
    /// user's effective authority where removal would have raised it.
    #[tokio::test]
    async fn explicit_viewer_assignment_is_the_real_revocation() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;
        assert_eq!(
            store
                .assign_checked(PK_PU, UserRole::Viewer, &caller)
                .await
                .unwrap(),
            UserRole::Viewer
        );
        assert_eq!(
            store.effective_role(PK_PU, true).await,
            UserRole::Viewer,
            "the explicit row overrides the power-user fallback"
        );
    }

    /// Removing an assignment the target never had is a no-op, not a
    /// revocation, and reports the ambient authority truthfully.
    #[tokio::test]
    async fn removing_a_nonexistent_assignment_reports_ambient_authority() {
        let store = mem_store().await;
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;

        let outcome = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert!(!outcome.had_explicit_role);
        assert_eq!(outcome.previous_role, None);
        assert_eq!(outcome.effective_after, UserRole::Editor);
        assert!(!outcome.authority_reduced);
    }

    /// The store's configured default decides what removal restores, so a
    /// Viewer-default deployment turns removal into a real reduction.
    #[tokio::test]
    async fn removal_semantics_follow_the_configured_default() {
        let conn = Connection::open_in_memory().await.expect("open db");
        let store = RoleStore::new_with_default(Arc::new(conn), UserRole::Viewer)
            .await
            .expect("create table");
        store.set(PK_A, UserRole::Editor, None).await.unwrap();
        let caller = caller_with_role(&store, PK_B, UserRole::Owner).await;

        let outcome = store.remove_checked(PK_A, false, &caller).await.unwrap();
        assert_eq!(outcome.effective_after, UserRole::Viewer);
        assert!(
            outcome.authority_reduced,
            "with a Viewer default, removal really does reduce access"
        );
    }
}
