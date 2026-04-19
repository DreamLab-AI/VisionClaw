//! Server-as-identity for VisionClaw.
//!
//! Implements ADR-050 §"Server-as-identity" and the UQ5 ratification decision:
//! VisionClaw owns a dedicated Nostr keypair that signs server-issued events —
//! migration approvals, bridge promotions, bead provenance stamps, and audit
//! records. Third parties can verify authenticity by resolving the server's
//! public key via `GET /api/server/identity`.
//!
//! Event kinds (server-signed):
//! - 30023 : migration approval (Long-form Content, NIP-23 addressable)
//! - 30100 : bridge-to promotion
//! - 30200 : bead provenance stamp
//! - 30300 : opaque audit record
//!
//! Secret loading precedence:
//! 1. `SERVER_NOSTR_PRIVKEY` (64-char hex) — canonical
//! 2. if absent and `SERVER_NOSTR_AUTO_GENERATE=true` — ephemeral dev key
//! 3. otherwise — error (fail fast in production)
//!
//! The secret is never logged and never returned in any API response; only the
//! public key (hex and npub) is exposed. `nostr_sdk::Keys` manages the secret's
//! in-memory lifetime via its internal `SecretKey` wrapper.
//!
//! Broadcast strategy: `sign_and_broadcast` attempts delivery to every URL in
//! `NOSTR_RELAY_URLS` but never blocks the caller on relay health. Per-relay
//! errors are logged and swallowed so that a stamped/approved event is always
//! returned to the caller (signature-first semantics).

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use nostr_sdk::prelude::*;

/// Parameterised-replaceable migration approval (NIP-23 style).
pub const KIND_MIGRATION_APPROVAL: u16 = 30023;
/// Bridge-to promotion event (KG → OWL declared equivalence).
pub const KIND_BRIDGE_PROMOTION: u16 = 30100;
/// Bead provenance stamp — one server-witnessed signature per payload.
pub const KIND_BEAD_STAMP: u16 = 30200;
/// Opaque operator audit record.
pub const KIND_AUDIT_RECORD: u16 = 30300;

/// All kinds this server is willing to sign. Advertised at
/// `GET /api/server/identity` for client-side sanity checking.
pub const SUPPORTED_KINDS: &[u16] = &[
    KIND_MIGRATION_APPROVAL,
    KIND_BRIDGE_PROMOTION,
    KIND_BEAD_STAMP,
    KIND_AUDIT_RECORD,
];

/// Server-owned Nostr identity.
///
/// Cloneable-via-`Arc` singleton; construct once with `from_env` at startup,
/// then share by injecting `Arc<ServerIdentity>` into actors and handlers.
pub struct ServerIdentity {
    keys: Keys,
    relay_urls: Vec<String>,
}

impl ServerIdentity {
    /// Load identity from environment.
    ///
    /// Env vars:
    /// - `SERVER_NOSTR_PRIVKEY`         — 64-char hex secret (required in prod)
    /// - `SERVER_NOSTR_AUTO_GENERATE`   — `"true"` to auto-generate in dev
    /// - `NOSTR_RELAY_URLS`             — comma-separated `wss://` / `ws://`
    ///
    /// Fails fast when no key material is available and auto-generate is off.
    pub fn from_env() -> Result<Self> {
        let keys = Self::load_keys_from_env()?;
        let relay_urls = Self::load_relays_from_env();

        // Log the pubkey (safe) so operators can verify the identity at boot.
        let pubkey_hex = keys.public_key().to_hex();
        let pubkey_npub = keys
            .public_key()
            .to_bech32()
            .unwrap_or_else(|_| pubkey_hex.clone());
        info!(
            "[ServerIdentity] Loaded server Nostr identity: pubkey_hex={} npub={} relays={}",
            pubkey_hex,
            pubkey_npub,
            relay_urls.len()
        );

        Ok(Self { keys, relay_urls })
    }

    fn load_keys_from_env() -> Result<Keys> {
        match std::env::var("SERVER_NOSTR_PRIVKEY") {
            Ok(hex) if !hex.trim().is_empty() => {
                let trimmed = hex.trim();
                let secret_key = SecretKey::from_hex(trimmed).map_err(|e| {
                    // Never include the key material itself in the error.
                    anyhow!("SERVER_NOSTR_PRIVKEY is not a valid 64-char hex secret: {e}")
                })?;
                Ok(Keys::new(secret_key))
            }
            _ => {
                let auto = std::env::var("SERVER_NOSTR_AUTO_GENERATE")
                    .map(|v| v.trim().eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                if !auto {
                    return Err(anyhow!(
                        "SERVER_NOSTR_PRIVKEY is unset and SERVER_NOSTR_AUTO_GENERATE is not 'true'. \
                         Set SERVER_NOSTR_PRIVKEY to a 64-char hex secret, or set \
                         SERVER_NOSTR_AUTO_GENERATE=true for ephemeral dev mode."
                    ));
                }
                let keys = Keys::generate();
                warn!(
                    "[ServerIdentity] SERVER_NOSTR_AUTO_GENERATE=true — generated ephemeral dev \
                     key (pubkey={}). Do NOT use this mode in production; the secret lives only \
                     in memory and every restart yields a new identity.",
                    keys.public_key().to_hex()
                );
                Ok(keys)
            }
        }
    }

    fn load_relays_from_env() -> Vec<String> {
        match std::env::var("NOSTR_RELAY_URLS") {
            Ok(raw) => raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| {
                    let ok = s.starts_with("wss://") || s.starts_with("ws://");
                    if !s.is_empty() && !ok {
                        warn!(
                            "[ServerIdentity] Ignoring NOSTR_RELAY_URLS entry without \
                             ws(s):// scheme: {s}"
                        );
                    }
                    ok
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Hex-encoded public key (64 chars). Safe to expose.
    pub fn pubkey_hex(&self) -> String {
        self.keys.public_key().to_hex()
    }

    /// Bech32 `npub1…` encoding (NIP-19). Safe to expose.
    pub fn pubkey_npub(&self) -> String {
        self.keys
            .public_key()
            .to_bech32()
            .unwrap_or_else(|_| self.pubkey_hex())
    }

    /// Configured relay URLs. Safe to expose.
    pub fn relay_urls(&self) -> &[String] {
        &self.relay_urls
    }

    /// Signed-event kinds this server is willing to emit. Safe to expose.
    pub fn supported_kinds(&self) -> &'static [u16] {
        SUPPORTED_KINDS
    }

    /// Build and sign a server-owned Nostr event. Does not broadcast.
    ///
    /// `kind` should be one of `SUPPORTED_KINDS`; we do not enforce that
    /// constraint inside this helper so that future ADRs can add new kinds
    /// without touching this module, but callers in the actor layer should
    /// stay on the enumerated set.
    pub async fn sign_event(
        &self,
        kind: u16,
        content: String,
        tags: Vec<Tag>,
    ) -> Result<Event> {
        let event = EventBuilder::new(Kind::Custom(kind), content)
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| anyhow!("failed to sign server event (kind={kind}): {e}"))?;
        Ok(event)
    }

    /// Sign an event and attempt to broadcast it to every configured relay.
    ///
    /// Returns the signed `Event` regardless of relay success — signature-first
    /// semantics: a caller minting a migration approval wants the audit record
    /// even if the relay is unreachable. Broadcast failures are logged.
    pub async fn sign_and_broadcast(
        &self,
        kind: u16,
        content: String,
        tags: Vec<Tag>,
    ) -> Result<Event> {
        let event = self.sign_event(kind, content, tags).await?;

        if self.relay_urls.is_empty() {
            warn!(
                "[ServerIdentity] No NOSTR_RELAY_URLS configured; event {} (kind={}) signed \
                 but not broadcast",
                event.id, kind
            );
            return Ok(event);
        }

        // Build a transient client. The `Keys` signer is required even for a
        // send-only flow because `Client::new(signer)` drives NIP-42 handshakes
        // and signed subscription filters when necessary.
        let client = Client::new(self.keys.clone());
        for url in &self.relay_urls {
            match client.add_relay(url).await {
                Ok(_) => {}
                Err(e) => warn!(
                    "[ServerIdentity] add_relay({url}) failed: {e}; skipping this relay"
                ),
            }
        }
        client.connect().await;

        match client.send_event(&event).await {
            Ok(output) => {
                info!(
                    "[ServerIdentity] Broadcast event {} (kind={}) to {} relay(s); failed {}",
                    event.id,
                    kind,
                    output.success.len(),
                    output.failed.len()
                );
                for (url, reason) in &output.failed {
                    warn!(
                        "[ServerIdentity] relay {url} rejected event {}: {reason}",
                        event.id
                    );
                }
            }
            Err(e) => {
                // Never fail the caller — they asked for a signed record, they got one.
                error!(
                    "[ServerIdentity] send_event failed for {} (kind={}): {e}",
                    event.id, kind
                );
            }
        }

        // Best-effort disconnect — infallible in nostr-sdk 0.43.
        client.disconnect().await;

        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construction must succeed when the auto-generate flag is set,
    /// even with no configured relays.
    #[test]
    fn auto_generate_yields_valid_identity() {
        std::env::remove_var("SERVER_NOSTR_PRIVKEY");
        std::env::set_var("SERVER_NOSTR_AUTO_GENERATE", "true");
        std::env::remove_var("NOSTR_RELAY_URLS");

        let id = ServerIdentity::from_env().expect("auto-generated identity");
        assert_eq!(id.pubkey_hex().len(), 64);
        assert!(id.pubkey_npub().starts_with("npub1"));
        assert!(id.relay_urls().is_empty());
        assert_eq!(id.supported_kinds(), SUPPORTED_KINDS);
    }

    /// When neither a key nor the auto-generate flag is present, startup fails.
    #[test]
    fn missing_key_without_auto_generate_fails() {
        std::env::remove_var("SERVER_NOSTR_PRIVKEY");
        std::env::set_var("SERVER_NOSTR_AUTO_GENERATE", "false");
        let err = ServerIdentity::from_env().unwrap_err().to_string();
        assert!(err.contains("SERVER_NOSTR_PRIVKEY"));
    }
}
