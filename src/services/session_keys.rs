//! Bound browser session keys (ADR-2205).
//!
//! A browser signs one NIP-98 token with the user's real signer to bind an
//! ephemeral secp256k1 key. Later requests are signed by that key and resolve
//! to the bound *principal*. This module is the in-memory binding table; the
//! verification and policy live in [`NostrService`](super::nostr_service) and
//! [`crate::utils::auth`].
//!
//! Every lookup takes `now` so expiry is deterministic under test. The
//! lifetime is absolute: a binding is never renewed.

use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

/// Hard ceiling on a binding's lifetime, whatever the configured maximum.
pub const SESSION_KEY_TTL_CAP_SECS: u64 = 3600;

/// Default for the `sessionKeyMaxTtlSecs` setting.
pub const SESSION_KEY_DEFAULT_MAX_TTL_SECS: u64 = 3600;

/// Upper bound on live bindings across all principals; past it new binds fail closed.
const MAX_BINDINGS: usize = 50_000;

/// Live bindings one principal may hold; a further bind evicts its oldest.
pub const MAX_BINDINGS_PER_PRINCIPAL: usize = 8;

/// Binds one principal may make per [`BIND_RATE_WINDOW_SECS`].
pub const BIND_RATE_MAX: usize = 10;

/// Length of the per-principal bind rate-limit window.
pub const BIND_RATE_WINDOW_SECS: i64 = 60;

/// Minimum gap between whole-table sweeps of expired entries.
const SWEEP_INTERVAL_SECS: i64 = 60;

/// What a session key may do on behalf of its principal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionScope {
    /// Everything the principal can do except the deny list in
    /// [`crate::utils::auth::session_key_denied`].
    Standard,
}

/// One live binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBinding {
    /// The user's real pubkey (hex) the session key acts for.
    pub principal: String,
    /// Unix seconds after which the binding is dead.
    pub expires_at: i64,
    /// Permitted scope.
    pub scope: SessionScope,
}

/// Result of resolving a signer pubkey against the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionLookup {
    /// A live binding.
    Live(SessionBinding),
    /// No live binding (never bound, expired, or lost to a restart).
    Unbound,
}

/// Why a bind was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum BindError {
    /// The session pubkey is not 64 lowercase/uppercase hex characters.
    InvalidSessionPubkey,
    /// The session key equals the principal.
    SessionKeyIsPrincipal,
    /// The session key is already bound to a different principal.
    BoundToAnotherPrincipal,
    /// The table is full.
    TableFull,
    /// Too many binds from this principal in the current window.
    RateLimited,
}

#[derive(Debug, Default)]
struct Table {
    bindings: HashMap<String, SessionBinding>,
    /// Session pubkeys per principal, oldest bind first.
    by_principal: HashMap<String, Vec<String>>,
    /// Bind times per principal inside the rate window.
    recent_binds: HashMap<String, VecDeque<i64>>,
    last_sweep: i64,
}

impl Table {
    fn remove_key(&mut self, session_pubkey: &str) -> Option<SessionBinding> {
        let binding = self.bindings.remove(session_pubkey)?;
        if let Some(keys) = self.by_principal.get_mut(&binding.principal) {
            keys.retain(|k| k != session_pubkey);
            if keys.is_empty() {
                self.by_principal.remove(&binding.principal);
            }
        }
        Some(binding)
    }

    /// Drop this principal's expired entries (cheap: at most the per-principal cap).
    fn prune_principal(&mut self, principal: &str, now: i64) {
        let expired: Vec<String> = self
            .by_principal
            .get(principal)
            .into_iter()
            .flatten()
            .filter(|k| self.bindings.get(*k).is_none_or(|b| b.expires_at <= now))
            .cloned()
            .collect();
        for key in expired {
            self.remove_key(&key);
        }
    }

    /// Whole-table sweep, at most once per [`SWEEP_INTERVAL_SECS`].
    fn sweep_if_due(&mut self, now: i64) {
        if now - self.last_sweep < SWEEP_INTERVAL_SECS {
            return;
        }
        self.last_sweep = now;
        let expired: Vec<String> = self
            .bindings
            .iter()
            .filter(|(_, b)| b.expires_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            self.remove_key(&key);
        }
        self.recent_binds.retain(|_, times| {
            while times
                .front()
                .is_some_and(|t| now - t >= BIND_RATE_WINDOW_SECS)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
    }
}

/// In-memory table of session-key bindings.
///
/// `lookup` takes only a read lock and never scans. Expired entries are
/// removed on the bind path: the binding principal's own on every bind, and the
/// whole table at most once a minute.
#[derive(Debug)]
pub struct SessionKeyStore {
    max_ttl_secs: u64,
    table: RwLock<Table>,
}

impl SessionKeyStore {
    /// A store whose bindings live at most `max_ttl_secs` (clamped to
    /// [`SESSION_KEY_TTL_CAP_SECS`], and to at least one second).
    pub fn new(max_ttl_secs: u64) -> Self {
        Self {
            max_ttl_secs: max_ttl_secs.clamp(1, SESSION_KEY_TTL_CAP_SECS),
            table: RwLock::new(Table::default()),
        }
    }

    /// The effective maximum lifetime.
    pub fn max_ttl_secs(&self) -> u64 {
        self.max_ttl_secs
    }

    /// Bind `session_pubkey` to `principal` for `min(requested, max)` seconds.
    ///
    /// At most [`BIND_RATE_MAX`] binds per principal per window; at most
    /// [`MAX_BINDINGS_PER_PRINCIPAL`] live keys per principal (the oldest is
    /// evicted). Re-binding the same key by the same principal replaces the
    /// entry and restarts its absolute expiry (that needed a fresh real
    /// signature). A key bound to a different principal is refused.
    pub fn bind(
        &self,
        session_pubkey: &str,
        principal: &str,
        requested_ttl_secs: u64,
        now: i64,
    ) -> Result<SessionBinding, BindError> {
        if session_pubkey.len() != 64 || !session_pubkey.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BindError::InvalidSessionPubkey);
        }
        let session_pubkey = session_pubkey.to_ascii_lowercase();
        if session_pubkey.eq_ignore_ascii_case(principal) {
            return Err(BindError::SessionKeyIsPrincipal);
        }
        let ttl = requested_ttl_secs.clamp(1, self.max_ttl_secs);
        let binding = SessionBinding {
            principal: principal.to_string(),
            expires_at: now.saturating_add(ttl as i64),
            scope: SessionScope::Standard,
        };
        let mut table = self.table.write().unwrap_or_else(|e| e.into_inner());

        let times = table.recent_binds.entry(principal.to_string()).or_default();
        while times
            .front()
            .is_some_and(|t| now - t >= BIND_RATE_WINDOW_SECS)
        {
            times.pop_front();
        }
        if times.len() >= BIND_RATE_MAX {
            return Err(BindError::RateLimited);
        }

        if let Some(existing) = table.bindings.get(&session_pubkey) {
            if existing.expires_at > now && existing.principal != principal {
                return Err(BindError::BoundToAnotherPrincipal);
            }
        }
        table.sweep_if_due(now);
        table.prune_principal(principal, now);
        // A replaced entry frees its own slot before the caps are applied.
        table.remove_key(&session_pubkey);
        if table.bindings.len() >= MAX_BINDINGS {
            return Err(BindError::TableFull);
        }
        while table
            .by_principal
            .get(principal)
            .is_some_and(|keys| keys.len() >= MAX_BINDINGS_PER_PRINCIPAL)
        {
            let oldest = table.by_principal[principal][0].clone();
            table.remove_key(&oldest);
        }

        table
            .bindings
            .insert(session_pubkey.clone(), binding.clone());
        table
            .by_principal
            .entry(principal.to_string())
            .or_default()
            .push(session_pubkey);
        table
            .recent_binds
            .entry(principal.to_string())
            .or_default()
            .push_back(now);
        Ok(binding)
    }

    /// Resolve `signer_pubkey` at `now`. Read lock only; never scans or prunes.
    pub fn lookup(&self, signer_pubkey: &str, now: i64) -> SessionLookup {
        let table = self.table.read().unwrap_or_else(|e| e.into_inner());
        match table.bindings.get(&signer_pubkey.to_ascii_lowercase()) {
            Some(b) if b.expires_at > now => SessionLookup::Live(b.clone()),
            _ => SessionLookup::Unbound,
        }
    }

    /// Drop every binding for `principal`; returns how many were removed.
    pub fn revoke_principal(&self, principal: &str) -> usize {
        let mut table = self.table.write().unwrap_or_else(|e| e.into_inner());
        let keys = table.by_principal.remove(principal).unwrap_or_default();
        for key in &keys {
            table.bindings.remove(key);
        }
        keys.len()
    }

    /// Drop one binding; returns whether it existed.
    pub fn revoke_key(&self, session_pubkey: &str) -> bool {
        let mut table = self.table.write().unwrap_or_else(|e| e.into_inner());
        table
            .remove_key(&session_pubkey.to_ascii_lowercase())
            .is_some()
    }

    /// Number of entries held (live or not yet swept).
    pub fn len(&self) -> usize {
        self.table
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .bindings
            .len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000;
    const S: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const P: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const Q: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn ttl_is_min_of_requested_and_setting_and_never_above_the_cap() {
        let store = SessionKeyStore::new(600);
        assert_eq!(store.bind(S, P, 60, NOW).unwrap().expires_at, NOW + 60);
        assert_eq!(store.bind(S, P, 9999, NOW).unwrap().expires_at, NOW + 600);
        assert_eq!(
            SessionKeyStore::new(86_400).max_ttl_secs(),
            SESSION_KEY_TTL_CAP_SECS
        );
    }

    #[test]
    fn lookup_is_live_until_expiry_then_unbound_and_never_slides() {
        let store = SessionKeyStore::new(600);
        store.bind(S, P, 100, NOW).unwrap();
        assert!(matches!(store.lookup(S, NOW + 99), SessionLookup::Live(b) if b.principal == P));
        // Using the key does not extend it.
        assert_eq!(store.lookup(S, NOW + 100), SessionLookup::Unbound);
        assert_eq!(store.lookup(S, NOW + 101), SessionLookup::Unbound);
    }

    #[test]
    fn an_unbound_key_resolves_to_nothing() {
        let store = SessionKeyStore::new(600);
        assert_eq!(store.lookup(S, NOW), SessionLookup::Unbound);
    }

    #[test]
    fn a_live_key_cannot_be_taken_over_by_another_principal() {
        let store = SessionKeyStore::new(600);
        store.bind(S, P, 100, NOW).unwrap();
        assert_eq!(
            store.bind(S, Q, 100, NOW + 1),
            Err(BindError::BoundToAnotherPrincipal)
        );
        // Same principal may re-bind; after expiry anyone may.
        assert!(store.bind(S, P, 100, NOW + 1).is_ok());
        assert!(store.bind(S, Q, 100, NOW + 500).is_ok());
    }

    #[test]
    fn malformed_and_self_bound_keys_are_refused() {
        let store = SessionKeyStore::new(600);
        assert_eq!(
            store.bind("xyz", P, 10, NOW),
            Err(BindError::InvalidSessionPubkey)
        );
        assert_eq!(
            store.bind(P, P, 10, NOW),
            Err(BindError::SessionKeyIsPrincipal)
        );
    }

    fn key(n: u32) -> String {
        format!("{n:064x}")
    }

    #[test]
    fn a_principal_holds_at_most_the_cap_and_the_oldest_is_evicted() {
        let store = SessionKeyStore::new(600);
        for i in 0..MAX_BINDINGS_PER_PRINCIPAL as u32 + 2 {
            // Spread across windows so the rate limit is not what stops us.
            store
                .bind(
                    &key(i + 1),
                    P,
                    500,
                    NOW + i as i64 * BIND_RATE_WINDOW_SECS / 8,
                )
                .unwrap();
        }
        let at = NOW + 20;
        assert_eq!(store.len(), MAX_BINDINGS_PER_PRINCIPAL);
        assert_eq!(store.lookup(&key(1), at), SessionLookup::Unbound);
        assert_eq!(store.lookup(&key(2), at), SessionLookup::Unbound);
        assert!(matches!(store.lookup(&key(3), at), SessionLookup::Live(_)));
        assert!(matches!(store.lookup(&key(10), at), SessionLookup::Live(_)));
    }

    #[test]
    fn another_principals_keys_are_not_evicted_by_the_cap() {
        let store = SessionKeyStore::new(600);
        store.bind(S, Q, 500, NOW).unwrap();
        for i in 0..MAX_BINDINGS_PER_PRINCIPAL as u32 + 3 {
            store.bind(&key(i + 1), P, 500, NOW + i as i64 * 7).unwrap();
        }
        assert!(matches!(store.lookup(S, NOW + 30), SessionLookup::Live(_)));
    }

    #[test]
    fn binds_are_rate_limited_per_principal_per_window() {
        let store = SessionKeyStore::new(600);
        for i in 0..BIND_RATE_MAX as u32 {
            store.bind(&key(i + 1), P, 500, NOW).unwrap();
        }
        assert_eq!(
            store.bind(&key(99), P, 500, NOW + 1),
            Err(BindError::RateLimited)
        );
        // Other principals are unaffected; the window then reopens.
        assert!(store.bind(&key(98), Q, 500, NOW + 1).is_ok());
        assert!(store
            .bind(&key(99), P, 500, NOW + BIND_RATE_WINDOW_SECS)
            .is_ok());
    }

    #[test]
    fn expired_entries_are_swept_off_the_lookup_path() {
        let store = SessionKeyStore::new(600);
        store.bind(S, Q, 10, NOW).unwrap();
        // Lookup never prunes, even after expiry.
        assert_eq!(store.lookup(S, NOW + 100), SessionLookup::Unbound);
        assert_eq!(store.len(), 1);
        // A later bind by someone else sweeps the table once the interval is due.
        store.bind(&key(1), P, 10, NOW + 100).unwrap();
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn revoking_drops_one_key_or_all_of_a_principals_keys() {
        let store = SessionKeyStore::new(600);
        store.bind(S, P, 500, NOW).unwrap();
        store.bind(&key(1), P, 500, NOW).unwrap();
        store.bind(&key(2), Q, 500, NOW).unwrap();
        assert!(store.revoke_key(S));
        assert!(!store.revoke_key(S));
        assert_eq!(store.revoke_principal(P), 1);
        assert_eq!(store.lookup(&key(1), NOW), SessionLookup::Unbound);
        assert!(matches!(store.lookup(&key(2), NOW), SessionLookup::Live(_)));
    }
}
