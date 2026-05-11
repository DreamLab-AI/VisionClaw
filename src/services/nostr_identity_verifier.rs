//! Nostr-backed [`IdentityVerifier`] for the XR presence handshake.
//!
//! Implements the `(nonce || timestamp_us)` Schnorr verification path described
//! in `docs/xr-godot-threat-model.md` §T-WS-1. Uses `secp256k1` directly (the
//! same crate `nostr-sdk` 0.44 transitively depends on) so we share a single
//! global context and skip the extra layer of `nostr_sdk::Event` synthesis.

use secp256k1::schnorr::Signature;
use secp256k1::{Message, XOnlyPublicKey, SECP256K1};
use sha2::{Digest, Sha256};
use tracing::warn;

use visionclaw_xr_presence::{
    error::RoomError,
    ports::{IdentityVerifier, SignedChallenge},
    types::Did,
};

#[derive(Debug, Default, Clone)]
pub struct NostrIdentityVerifier;

impl NostrIdentityVerifier {
    pub fn new() -> Self {
        Self::default()
    }
}

fn fail(claimed: &str) -> RoomError {
    RoomError::InvalidDid {
        did: format!("did:nostr:{}", claimed),
    }
}

impl IdentityVerifier for NostrIdentityVerifier {
    fn verify_signed_challenge(&self, challenge: &SignedChallenge) -> Result<Did, RoomError> {
        let pubkey_bytes =
            hex::decode(&challenge.claimed_pubkey_hex).map_err(|_| fail(&challenge.claimed_pubkey_hex))?;
        if pubkey_bytes.len() != 32 {
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        let xonly = XOnlyPublicKey::from_slice(&pubkey_bytes)
            .map_err(|_| fail(&challenge.claimed_pubkey_hex))?;

        let sig_bytes = hex::decode(&challenge.signature_hex)
            .map_err(|_| fail(&challenge.claimed_pubkey_hex))?;
        if sig_bytes.len() != 64 {
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        let signature =
            Signature::from_slice(&sig_bytes).map_err(|_| fail(&challenge.claimed_pubkey_hex))?;

        let mut hasher = Sha256::new();
        hasher.update(challenge.nonce);
        hasher.update(challenge.timestamp_us.to_le_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        let message = Message::from_digest(digest);

        if let Err(e) = SECP256K1.verify_schnorr(&signature, &message, &xonly) {
            warn!("schnorr verify failed for {}: {e}", challenge.claimed_pubkey_hex);
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        Did::parse(format!("did:nostr:{}", challenge.claimed_pubkey_hex))
    }
}

/// Permissive verifier that only checks well-formedness — used when the Nostr
/// pipeline is unavailable (CI, integration tests). Documented as a stub by
/// PRD-008 §5.3 so the rest of the service can be exercised end-to-end.
#[derive(Debug, Default, Clone)]
pub struct WellFormedOnlyVerifier;

impl IdentityVerifier for WellFormedOnlyVerifier {
    fn verify_signed_challenge(&self, challenge: &SignedChallenge) -> Result<Did, RoomError> {
        if challenge.signature_hex.len() != 128
            || !challenge
                .signature_hex
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        if challenge.claimed_pubkey_hex.len() != 64
            || !challenge
                .claimed_pubkey_hex
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        Did::parse(format!("did:nostr:{}", challenge.claimed_pubkey_hex))
    }
}
