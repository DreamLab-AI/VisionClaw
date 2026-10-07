//! Nostr (BIP-340 Schnorr) [`Signer`] for the XR presence challenge handshake.
//!
//! The server's [`NostrIdentityVerifier`](../../../src/services/nostr_identity_verifier.rs)
//! verifies a Schnorr signature over `SHA256(nonce || timestamp_us.to_le_bytes())`
//! against the client's x-only pubkey. This impl produces exactly that, using
//! the same `secp256k1` crate (0.29) the server depends on, so signatures
//! interop byte-for-byte.

// gdext's #[godot_api] expands to closures returning its own CallError
// (176 bytes); that generated code is outside this crate's control.
#![allow(clippy::result_large_err)]

use secp256k1::schnorr::Signature;
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use sha2::{Digest, Sha256};

use visionclaw_xr_presence::ports::SignedChallenge;
use visionclaw_xr_presence::Did;

use crate::ports::{Signer, SignerError};

#[cfg(not(test))]
use godot::prelude::*;

pub struct NostrSigner {
    secp: Secp256k1<secp256k1::All>,
    keypair: Keypair,
    pubkey_hex: String,
    did: Did,
}

impl NostrSigner {
    /// Build from a 32-byte secret key in lowercase hex (64 chars).
    pub fn from_secret_hex(secret_hex: &str) -> Result<Self, SignerError> {
        let secp = Secp256k1::new();
        let bytes = hex::decode(secret_hex.trim())
            .map_err(|e| SignerError::Sign(format!("secret hex decode: {e}")))?;
        let sk = SecretKey::from_slice(&bytes)
            .map_err(|e| SignerError::Sign(format!("secret key: {e}")))?;
        let keypair = Keypair::from_secret_key(&secp, &sk);
        let (xonly, _parity) = keypair.x_only_public_key();
        let pubkey_hex = hex::encode(xonly.serialize());
        let did = Did::parse(format!("did:nostr:{pubkey_hex}"))
            .map_err(|e| SignerError::Sign(format!("did parse: {e}")))?;
        Ok(Self {
            secp,
            keypair,
            pubkey_hex,
            did,
        })
    }

    /// Generate a fresh random keypair.
    pub fn generate() -> Self {
        let secp = Secp256k1::new();
        let (sk, _pk) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());
        Self::from_secret_hex(&hex::encode(sk.secret_bytes()))
            .expect("freshly generated secret key is always valid")
    }

    /// 32-byte secret in lowercase hex — persist this to reuse the identity.
    pub fn secret_hex(&self) -> String {
        hex::encode(self.keypair.secret_bytes())
    }

    fn challenge_digest(nonce: &[u8; 32], timestamp_us: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(nonce);
        hasher.update(timestamp_us.to_le_bytes());
        hasher.finalize().into()
    }

    /// Build a NIP-98 HTTP-auth event (kind 27235) for `url`/`method`, signed
    /// with this identity, and return its standard-alphabet base64 encoding.
    /// Both the WebSocket envelope and the HTTP `Authorization` header wrap this
    /// same signed event, so the two auth paths interop byte-for-byte with the
    /// server's `verify_nip98_auth` decoder.
    fn nip98_event_b64(&self, url: &str, method: &str) -> String {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        let tags = serde_json::json!([["u", url], ["method", method]]);
        let content = "";

        // NIP-01 canonical id preimage: [0, pubkey, created_at, kind, tags, content]
        // serialised compactly (serde_json emits no whitespace and performs the
        // required string escaping).
        let preimage = serde_json::json!([
            0,
            self.pubkey_hex,
            created_at,
            NIP98_HTTP_AUTH_KIND,
            tags,
            content
        ]);
        let id_bytes: [u8; 32] = Sha256::digest(preimage.to_string().as_bytes()).into();
        let message = Message::from_digest(id_bytes);
        let sig: Signature = self.secp.sign_schnorr_no_aux_rand(&message, &self.keypair);

        let event = serde_json::json!({
            "id": hex::encode(id_bytes),
            "pubkey": self.pubkey_hex,
            "created_at": created_at,
            "kind": NIP98_HTTP_AUTH_KIND,
            "tags": tags,
            "content": content,
            "sig": hex::encode(sig.as_ref()),
        });
        base64_encode(event.to_string().as_bytes())
    }

    /// Build a NIP-98 HTTP-auth event and wrap it in the server's WebSocket
    /// `{"type":"authenticate","event":"<base64(event)>"}` envelope. Unlocks
    /// mutating `/wss` messages (node drag/pin) for this session.
    pub fn nip98_authenticate_json(&self, url: &str, method: &str) -> String {
        let event_b64 = self.nip98_event_b64(url, method);
        serde_json::json!({ "type": "authenticate", "event": event_b64 }).to_string()
    }

    /// Build the `Authorization` header value for a NIP-98-authenticated HTTP
    /// request to `url` with `method`. The `Nostr ` scheme prefix is exactly
    /// what the server's `auth_extractor` matches (`auth_extractor.rs:109`), so
    /// the Godot intervention POST authenticates against the same power-user
    /// gate the desktop control centre uses. `method` must be upper-case
    /// (`POST`) and `url` the fully-qualified request URL the server will
    /// reconstruct, or the NIP-98 `u`/`method` tag check fails.
    pub fn nip98_http_authorization(&self, url: &str, method: &str) -> String {
        format!("Nostr {}", self.nip98_event_b64(url, method))
    }
}

/// NIP-98 HTTP-auth event kind.
const NIP98_HTTP_AUTH_KIND: u32 = 27_235;

/// Standard-alphabet base64 with padding (matches the server's decoder).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

impl Signer for NostrSigner {
    fn pubkey_hex(&self) -> String {
        self.pubkey_hex.clone()
    }

    fn sign_challenge(
        &self,
        nonce: &[u8; 32],
        timestamp_us: u64,
    ) -> Result<SignedChallenge, SignerError> {
        let digest = Self::challenge_digest(nonce, timestamp_us);
        let message = Message::from_digest(digest);
        let sig: Signature = self.secp.sign_schnorr_no_aux_rand(&message, &self.keypair);
        Ok(SignedChallenge {
            nonce: *nonce,
            timestamp_us,
            claimed_pubkey_hex: self.pubkey_hex.clone(),
            signature_hex: hex::encode(sig.as_ref()),
        })
    }

    fn did(&self) -> Result<Did, SignerError> {
        Ok(self.did.clone())
    }
}

// --- Godot node --------------------------------------------------------------

/// GDScript-facing Nostr HTTP authenticator. Holds the session identity and
/// mints per-request NIP-98 `Authorization` headers so the in-headset
/// intervention panel can POST a signed broker decision to the same
/// power-user-gated `/api/broker/cases/{id}/decide` route the desktop control
/// centre uses (PRD-023 WP-9 M2 / COM-18). The secret key never crosses the
/// GDExtension boundary — only signed, single-use headers do.
#[cfg(not(test))]
#[derive(GodotClass)]
#[class(no_init, base = RefCounted)]
pub struct NostrAuth {
    signer: NostrSigner,
    base: Base<RefCounted>,
}

#[cfg(not(test))]
#[godot_api]
impl NostrAuth {
    /// Build from a 32-byte secret in lowercase hex. An empty or malformed hex
    /// yields a fresh ephemeral identity rather than failing — the panel always
    /// has a signing key, and an ephemeral operator simply is not a power user
    /// server-side (the decide POST 401s), which is the honest outcome, not a
    /// crash. Reuse the same secret the graph socket authenticates with so the
    /// decision is attributed to one identity.
    #[func]
    fn create(secret_hex: GString) -> Gd<Self> {
        let s = secret_hex.to_string();
        let signer = if s.trim().is_empty() {
            NostrSigner::generate()
        } else {
            NostrSigner::from_secret_hex(&s).unwrap_or_else(|_| NostrSigner::generate())
        };
        Gd::from_init_fn(|base| Self { signer, base })
    }

    /// The `Authorization` header value (`Nostr <base64(event)>`) for a NIP-98
    /// request to `url` with `method` (upper-case). `url` must be the
    /// fully-qualified request URL the server reconstructs, or the tag check
    /// fails server-side.
    #[func]
    fn nip98_header(&self, url: GString, method: GString) -> GString {
        GString::from(
            self.signer
                .nip98_http_authorization(&url.to_string(), &method.to_string()),
        )
    }

    /// This identity's x-only pubkey hex — the `broker_pubkey` attribution field
    /// on the decide body (HITL provenance, `enrichment_proposals_handler.rs`).
    #[func]
    fn pubkey_hex(&self) -> GString {
        GString::from(self.signer.pubkey_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{XOnlyPublicKey, SECP256K1};

    #[test]
    fn roundtrip_secret_hex_is_stable() {
        let s = NostrSigner::generate();
        let hex = s.secret_hex();
        let s2 = NostrSigner::from_secret_hex(&hex).unwrap();
        assert_eq!(s.pubkey_hex(), s2.pubkey_hex());
    }

    #[test]
    fn pubkey_is_64_hex_chars() {
        let s = NostrSigner::generate();
        assert_eq!(s.pubkey_hex().len(), 64);
        assert!(s.pubkey_hex().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn did_matches_pubkey() {
        let s = NostrSigner::generate();
        let did = s.did().unwrap();
        assert_eq!(did.as_str(), format!("did:nostr:{}", s.pubkey_hex()));
    }

    #[test]
    fn signature_verifies_against_server_message_construction() {
        // Mirror NostrIdentityVerifier exactly: SHA256(nonce || ts_le) then schnorr verify.
        let s = NostrSigner::generate();
        let nonce = [7u8; 32];
        let ts: u64 = 1_700_000_000_000_000;
        let sc = s.sign_challenge(&nonce, ts).unwrap();

        let digest = NostrSigner::challenge_digest(&nonce, ts);
        let message = Message::from_digest(digest);
        let pk_bytes = hex::decode(&sc.claimed_pubkey_hex).unwrap();
        let xonly = XOnlyPublicKey::from_slice(&pk_bytes).unwrap();
        let sig_bytes = hex::decode(&sc.signature_hex).unwrap();
        assert_eq!(sig_bytes.len(), 64);
        let sig = Signature::from_slice(&sig_bytes).unwrap();
        assert!(SECP256K1.verify_schnorr(&sig, &message, &xonly).is_ok());
    }

    #[test]
    fn signature_is_128_hex_chars() {
        let s = NostrSigner::generate();
        let sc = s.sign_challenge(&[0u8; 32], 1).unwrap();
        assert_eq!(sc.signature_hex.len(), 128);
    }

    #[test]
    fn rejects_bad_secret_hex() {
        assert!(NostrSigner::from_secret_hex("nothex").is_err());
        assert!(NostrSigner::from_secret_hex("00").is_err());
    }

    /// Decode standard base64 (test-only inverse of `base64_encode`).
    fn base64_decode(s: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let val = |c: u8| ALPHABET.iter().position(|&a| a == c).unwrap() as u32;
        let mut out = Vec::new();
        let chars: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
        for chunk in chars.chunks(4) {
            let mut n = 0u32;
            for (i, &c) in chunk.iter().enumerate() {
                n |= val(c) << (18 - 6 * i);
            }
            out.push((n >> 16) as u8);
            if chunk.len() > 2 {
                out.push((n >> 8) as u8);
            }
            if chunk.len() > 3 {
                out.push(n as u8);
            }
        }
        out
    }

    #[test]
    fn nip98_event_is_well_formed_and_signature_verifies() {
        let s = NostrSigner::generate();
        let url = "ws://localhost:4000/wss?token=abc";
        let envelope = s.nip98_authenticate_json(url, "GET");

        let env: serde_json::Value = serde_json::from_str(&envelope).unwrap();
        assert_eq!(env["type"], "authenticate");
        let event_json = String::from_utf8(base64_decode(env["event"].as_str().unwrap())).unwrap();
        let event: serde_json::Value = serde_json::from_str(&event_json).unwrap();

        assert_eq!(event["kind"], 27235);
        assert_eq!(event["pubkey"].as_str().unwrap(), s.pubkey_hex());
        assert_eq!(event["tags"][0][0], "u");
        assert_eq!(event["tags"][0][1], url);
        assert_eq!(event["tags"][1][1], "GET");
        assert_eq!(event["content"], "");

        // Recompute the NIP-01 id preimage and confirm both the id and the
        // schnorr signature verify against the embedded pubkey.
        let preimage = serde_json::json!([
            0,
            event["pubkey"],
            event["created_at"],
            event["kind"],
            event["tags"],
            event["content"]
        ]);
        let id_bytes: [u8; 32] = Sha256::digest(preimage.to_string().as_bytes()).into();
        assert_eq!(hex::encode(id_bytes), event["id"].as_str().unwrap());

        let message = Message::from_digest(id_bytes);
        let xonly =
            XOnlyPublicKey::from_slice(&hex::decode(event["pubkey"].as_str().unwrap()).unwrap())
                .unwrap();
        let sig =
            Signature::from_slice(&hex::decode(event["sig"].as_str().unwrap()).unwrap()).unwrap();
        assert!(SECP256K1.verify_schnorr(&sig, &message, &xonly).is_ok());
    }

    #[test]
    fn http_authorization_carries_a_verifiable_nip98_event() {
        let s = NostrSigner::generate();
        let url = "http://localhost:4000/api/broker/cases/case-7/decide";
        let header = s.nip98_http_authorization(url, "POST");

        // The scheme prefix is exactly what auth_extractor.rs:109 matches.
        let b64 = header
            .strip_prefix("Nostr ")
            .expect("HTTP NIP-98 header must use the `Nostr ` scheme");

        let event_json = String::from_utf8(base64_decode(b64)).unwrap();
        let event: serde_json::Value = serde_json::from_str(&event_json).unwrap();
        assert_eq!(event["kind"], 27235);
        assert_eq!(event["pubkey"].as_str().unwrap(), s.pubkey_hex());
        assert_eq!(event["tags"][0][0], "u");
        assert_eq!(event["tags"][0][1], url);
        assert_eq!(event["tags"][1][1], "POST");

        // Same id + signature integrity the server enforces.
        let preimage = serde_json::json!([
            0,
            event["pubkey"],
            event["created_at"],
            event["kind"],
            event["tags"],
            event["content"]
        ]);
        let id_bytes: [u8; 32] = Sha256::digest(preimage.to_string().as_bytes()).into();
        assert_eq!(hex::encode(id_bytes), event["id"].as_str().unwrap());
        let message = Message::from_digest(id_bytes);
        let xonly =
            XOnlyPublicKey::from_slice(&hex::decode(event["pubkey"].as_str().unwrap()).unwrap())
                .unwrap();
        let sig =
            Signature::from_slice(&hex::decode(event["sig"].as_str().unwrap()).unwrap()).unwrap();
        assert!(SECP256K1.verify_schnorr(&sig, &message, &xonly).is_ok());
    }

    /// The server decodes the header with `nostr::Event::from_json` and checks
    /// it with `Event::verify` (`src/utils/nip98.rs`). Prove the headset's
    /// event passes that exact library, for the URL shapes the HUD signs: a
    /// LAN host with a non-default port and a query string with `&` and `,`.
    #[test]
    fn http_authorization_verifies_under_the_servers_nostr_crate() {
        use nostr::JsonUtil;

        let s = NostrSigner::generate();
        for (url, method) in [
            (
                "http://trust.lan:3001/api/settings/physics?graph=knowledge",
                "PUT",
            ),
            (
                "http://trust.lan:3001/api/graph/fold?level=2&pinned=a,b",
                "GET",
            ),
            ("https://visionclaw.example.org/api/layout/mode", "POST"),
        ] {
            let header = s.nip98_http_authorization(url, method);
            let b64 = header.strip_prefix("Nostr ").unwrap();
            let json = String::from_utf8(base64_decode(b64)).unwrap();
            let event = nostr::Event::from_json(&json).expect("server decoder parses the event");
            event.verify().expect("id and Schnorr signature verify");
            assert_eq!(event.kind, nostr::Kind::HttpAuth);
            assert_eq!(event.pubkey.to_hex(), s.pubkey_hex());
            let tags: Vec<Vec<String>> = event.tags.iter().map(|t| t.as_slice().to_vec()).collect();
            assert!(
                tags.contains(&vec!["u".to_string(), url.to_string()]),
                "{tags:?}"
            );
            assert!(
                tags.contains(&vec!["method".to_string(), method.to_string()]),
                "{tags:?}"
            );
        }
    }

    #[test]
    fn base64_round_trips_all_pad_lengths() {
        for input in [&b""[..], b"a", b"ab", b"abc", b"abcd", b"hello world!"] {
            let enc = base64_encode(input);
            assert_eq!(
                base64_decode(&enc),
                input,
                "round-trip failed for {input:?}"
            );
        }
    }
}
