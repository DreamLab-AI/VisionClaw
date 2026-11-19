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

use std::collections::HashMap;
use std::sync::RwLock;

/// Hard ceiling on a binding's lifetime, whatever the configured maximum.
pub const SESSION_KEY_TTL_CAP_SECS: u64 = 3600;

/// Default for the `sessionKeyMaxTtlSecs` setting.
pub const SESSION_KEY_DEFAULT_MAX_TTL_SECS: u64 = 3600;

/// Upper bound on live bindings; past it new binds fail closed.
const MAX_BINDINGS: usize = 50_000;

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
}

/// In-memory table of session-key bindings.
#[derive(Debug)]
pub struct SessionKeyStore {
    max_ttl_secs: u64,
    bindings: RwLock<HashMap<String, SessionBinding>>,
}

impl SessionKeyStore {
    /// A store whose bindings live at most `max_ttl_secs` (clamped to
    /// [`SESSION_KEY_TTL_CAP_SECS`], and to at least one second).
    pub fn new(max_ttl_secs: u64) -> Self {
        Self {
            max_ttl_secs: max_ttl_secs.clamp(1, SESSION_KEY_TTL_CAP_SECS),
            bindings: RwLock::new(HashMap::new()),
        }
    }

    /// The effective maximum lifetime.
    pub fn max_ttl_secs(&self) -> u64 {
        self.max_ttl_secs
    }

    /// Bind `session_pubkey` to `principal` for `min(requested, max)` seconds.
    ///
    /// Re-binding by the same principal replaces the entry (absolute expiry
    /// restarts from the new bind, which needed a fresh real signature). A key
    /// bound to a different principal is refused.
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
        let mut table = self.bindings.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = table.get(&session_pubkey) {
            if existing.expires_at > now && existing.principal != principal {
                return Err(BindError::BoundToAnotherPrincipal);
            }
        }
        if table.len() >= MAX_BINDINGS {
            table.retain(|_, b| b.expires_at > now);
            if table.len() >= MAX_BINDINGS {
                return Err(BindError::TableFull);
            }
        }
        table.insert(session_pubkey, binding.clone());
        Ok(binding)
    }

    /// Resolve `signer_pubkey` at `now`.
    pub fn lookup(&self, signer_pubkey: &str, now: i64) -> SessionLookup {
        let table = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        match table.get(&signer_pubkey.to_ascii_lowercase()) {
            Some(b) if b.expires_at > now => SessionLookup::Live(b.clone()),
            _ => SessionLookup::Unbound,
        }
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
}
