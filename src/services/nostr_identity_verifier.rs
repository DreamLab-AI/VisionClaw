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
        Self
    }
}

fn fail(claimed: &str) -> RoomError {
    RoomError::InvalidDid {
        did: format!("did:nostr:{}", claimed),
    }
}

impl IdentityVerifier for NostrIdentityVerifier {
    fn verify_signed_challenge(&self, challenge: &SignedChallenge) -> Result<Did, RoomError> {
        let pubkey_bytes = hex::decode(&challenge.claimed_pubkey_hex)
            .map_err(|_| fail(&challenge.claimed_pubkey_hex))?;
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
            warn!(
                "schnorr verify failed for {}: {e}",
                challenge.claimed_pubkey_hex
            );
            return Err(fail(&challenge.claimed_pubkey_hex));
        }
        Did::parse(format!("did:nostr:{}", challenge.claimed_pubkey_hex))
    }
}

/// Verify a claimed spawn-payload `did:nostr` against a signed challenge from the
/// agent, per ADR-130 Decision 6 (COM-14 verify-before-trust). Two gates, in order:
///
///  1. **Well-formedness (ADR-125 I1).** `payload_did` must round-trip through the
///     canonical `uri::did_nostr()` primitive; a non-DID or malformed claim is
///     rejected before any crypto runs.
///  2. **Control of the key (ADR-125 I3-safe).** The BIP-340 Schnorr signature over
///     `(nonce || timestamp_us)` is verified against `event.pubkey`
///     (`challenge.claimed_pubkey_hex`) by `verifier` — the raw event pubkey, never
///     a resolved DID-document verificationMethod — and the derived
///     `did:nostr:{event.pubkey}` must equal `payload_did`.
///
/// Returns the verified [`Did`] only when both gates pass. A signature failure or a
/// derived-DID/payload mismatch yields [`RoomError::InvalidDid`]; a node that does
/// not pass is not trusted and cannot receive a 31402 (WP-1, DDD invariant 2).
pub fn verify_did_matches_challenge(
    payload_did: &str,
    challenge: &SignedChallenge,
    verifier: &dyn IdentityVerifier,
) -> Result<Did, RoomError> {
    // Gate 1 — the payload DID is a canonical `did:nostr` (I1). Reject anything
    // that is not a `ParsedUri::DidNostr` before spending a crypto verify.
    let payload_pubkey = match crate::uri::parse(payload_did) {
        Ok(crate::uri::ParsedUri::DidNostr { pubkey }) => pubkey,
        _ => {
            return Err(RoomError::InvalidDid {
                did: payload_did.to_string(),
            })
        }
    };

    // Gate 2 — the challenge proves control of `event.pubkey` (I3-safe: the verifier
    // reads the raw pubkey, never a DID-document verificationMethod).
    let verified = verifier.verify_signed_challenge(challenge)?;

    // The proven key must be exactly the one the spawn payload claims.
    if verified.pubkey_hex() != payload_pubkey {
        warn!(
            "did:nostr mismatch: payload claims {payload_pubkey}, challenge proved {}",
            verified.pubkey_hex()
        );
        return Err(RoomError::InvalidDid {
            did: payload_did.to_string(),
        });
    }
    Ok(verified)
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

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{Keypair, Secp256k1, SecretKey};

    /// Build a valid [`SignedChallenge`] for `sk` over `(nonce || ts)`, returning
    /// the challenge and the x-only pubkey hex the payload DID must match.
    fn signed(sk: &SecretKey, nonce: [u8; 32], ts: u64) -> (SignedChallenge, String) {
        let secp = Secp256k1::new();
        let keypair = Keypair::from_secret_key(&secp, sk);
        let (xonly, _parity) = keypair.x_only_public_key();
        let pk_hex = hex::encode(xonly.serialize());

        let mut hasher = Sha256::new();
        hasher.update(nonce);
        hasher.update(ts.to_le_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        let message = Message::from_digest(digest);
        let sig = secp.sign_schnorr_no_aux_rand(&message, &keypair);

        let challenge = SignedChallenge {
            nonce,
            timestamp_us: ts,
            claimed_pubkey_hex: pk_hex.clone(),
            signature_hex: hex::encode(sig.as_ref()),
        };
        (challenge, pk_hex)
    }

    #[test]
    fn accepts_matching_did_over_real_schnorr_signature() {
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let (challenge, pk_hex) = signed(&sk, [7u8; 32], 1_720_000_000_000_000);
        let payload_did = format!("did:nostr:{pk_hex}");

        let verified =
            verify_did_matches_challenge(&payload_did, &challenge, &NostrIdentityVerifier::new())
                .expect("a valid signature over a matching payload DID must verify");
        assert_eq!(verified.as_str(), payload_did);
        assert_eq!(verified.pubkey_hex(), pk_hex);
    }

    #[test]
    fn rejects_payload_did_for_a_different_key() {
        // The signature is valid for key A, but the payload claims key B's DID —
        // the spoof the register's identity-blind finding forbids (Decision 6).
        let sk_a = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let sk_b = SecretKey::from_slice(&[0x22; 32]).unwrap();
        let (challenge_a, _) = signed(&sk_a, [3u8; 32], 42);

        let secp = Secp256k1::new();
        let (xonly_b, _) = Keypair::from_secret_key(&secp, &sk_b).x_only_public_key();
        let payload_did_b = format!("did:nostr:{}", hex::encode(xonly_b.serialize()));

        let err = verify_did_matches_challenge(
            &payload_did_b,
            &challenge_a,
            &NostrIdentityVerifier::new(),
        )
        .unwrap_err();
        assert!(matches!(err, RoomError::InvalidDid { .. }));
    }

    #[test]
    fn rejects_tampered_signature() {
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let (mut challenge, pk_hex) = signed(&sk, [9u8; 32], 100);
        // Flip a signature byte — BIP-340 verify must fail even though the payload
        // DID matches the claimed pubkey.
        let mut bytes = hex::decode(&challenge.signature_hex).unwrap();
        bytes[10] ^= 0xff;
        challenge.signature_hex = hex::encode(bytes);

        let payload_did = format!("did:nostr:{pk_hex}");
        let err =
            verify_did_matches_challenge(&payload_did, &challenge, &NostrIdentityVerifier::new())
                .unwrap_err();
        assert!(matches!(err, RoomError::InvalidDid { .. }));
    }

    #[test]
    fn rejects_non_did_payload_before_crypto() {
        // Gate 1 rejects a payload that is not a did:nostr at all (I1), regardless
        // of an otherwise-valid challenge.
        let sk = SecretKey::from_slice(&[0x11; 32]).unwrap();
        let (challenge, _) = signed(&sk, [1u8; 32], 1);
        let err = verify_did_matches_challenge(
            "urn:visionclaw:room:sha256-12-deadbeef0011",
            &challenge,
            &NostrIdentityVerifier::new(),
        )
        .unwrap_err();
        assert!(matches!(err, RoomError::InvalidDid { .. }));
    }
}
