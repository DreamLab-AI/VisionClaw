//! NIP-98 HTTP Authentication for Solid Server Integration
//!
//! Generates Nostr events for HTTP authentication as defined in:
//! - NIP-98: https://nips.nostr.com/98
//! - JIP-0001: https://github.com/JavaScriptSolidServer/jips/blob/main/jip-0001.md
//!
//! Authorization header format: "Nostr <base64-encoded-event>"

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::debug;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use thiserror::Error;

/// NIP-98 HTTP Auth event kind (references RFC 7235)
const HTTP_AUTH_KIND: u16 = 27235;

/// Errors from NIP-98 operations
#[derive(Debug, Error)]
pub enum Nip98Error {
    #[error("Failed to create Nostr keys: {0}")]
    KeyCreation(String),
    #[error("Failed to build event: {0}")]
    EventBuild(String),
    #[error("Failed to sign event: {0}")]
    EventSign(String),
    #[error("Failed to serialize event: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// NIP-98 event structure for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nip98Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: i64,
    pub kind: u16,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

/// Configuration for generating NIP-98 tokens
#[derive(Debug, Clone)]
pub struct Nip98Config {
    /// Target URL for the request
    pub url: String,
    /// HTTP method (GET, POST, PUT, DELETE, PATCH, HEAD)
    pub method: String,
    /// Optional request body for payload hash
    pub body: Option<String>,
}

/// Generate a NIP-98 authentication token for a request
/// Returns a base64-encoded Nostr event that can be used in the
/// Authorization header as: `Authorization: Nostr <token>`
/// # Arguments
/// * `keys` - The Nostr Keys (secret key) to sign with
/// * `config` - Configuration for the NIP-98 request
/// # Returns
/// Base64-encoded event string
pub fn generate_nip98_token(keys: &Keys, config: &Nip98Config) -> Result<String, Nip98Error> {
    // Build tags
    let mut tags: Vec<Tag> = vec![
        Tag::custom(TagKind::Custom("u".into()), vec![config.url.clone()]),
        Tag::custom(
            TagKind::Custom("method".into()),
            vec![config.method.to_uppercase()],
        ),
    ];

    // Add payload hash if body is provided
    if let Some(body) = &config.body {
        let hash = compute_payload_hash(body);
        tags.push(Tag::custom(TagKind::Custom("payload".into()), vec![hash]));
    }

    // Build the event
    let event = EventBuilder::new(Kind::Custom(HTTP_AUTH_KIND), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|e| Nip98Error::EventSign(e.to_string()))?;

    // Convert to our serialization format
    let nip98_event = Nip98Event {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_u64() as i64,
        kind: HTTP_AUTH_KIND,
        tags: event
            .tags
            .iter()
            .map(|t| t.as_slice().iter().map(|s| s.to_string()).collect())
            .collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    };

    // Serialize to JSON and base64 encode
    let json = serde_json::to_string(&nip98_event)?;
    let token = BASE64.encode(json.as_bytes());

    debug!(
        "Generated NIP-98 token for {} {} (pubkey: {}...)",
        config.method,
        config.url,
        &nip98_event.pubkey[..16]
    );

    Ok(token)
}

/// Generate NIP-98 token from hex secret key
/// # Arguments
/// * `secret_key_hex` - 64-character hex secret key
/// * `config` - Configuration for the NIP-98 request
pub fn generate_nip98_token_from_hex(
    secret_key_hex: &str,
    config: &Nip98Config,
) -> Result<String, Nip98Error> {
    let secret_key =
        SecretKey::from_hex(secret_key_hex).map_err(|e| Nip98Error::KeyCreation(e.to_string()))?;
    let keys = Keys::new(secret_key);
    generate_nip98_token(&keys, config)
}

/// Compute SHA256 hash of payload for the 'payload' tag
fn compute_payload_hash(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Build the Authorization header value
/// # Arguments
/// * `token` - The base64-encoded NIP-98 token
/// # Returns
/// Full header value: "Nostr <token>"
pub fn build_auth_header(token: &str) -> String {
    format!("Nostr {}", token)
}

/// Extract pubkey from a NIP-98 token (for validation/logging)
pub fn extract_pubkey_from_token(token: &str) -> Option<String> {
    let decoded = BASE64.decode(token).ok()?;
    let json_str = String::from_utf8(decoded).ok()?;
    let event: Nip98Event = serde_json::from_str(&json_str).ok()?;
    Some(event.pubkey)
}

/// Maximum age for NIP-98 tokens (60 seconds).
///
/// Replay resistance is a two-layer scheme, not the window alone:
///  1. **Freshness window** — the token's `created_at` must fall within
///     ±`TOKEN_MAX_AGE_SECONDS` of the server clock (see the symmetric check in
///     [`validate_nip98_token`]). This bounds how long a captured token is worth
///     replaying at all. Clients with clock skew beyond this threshold receive
///     401s and must re-authenticate.
///  2. **Single-use cache** — after full validation succeeds, the event id is
///     atomically recorded in a process-wide cache (see [`REPLAY_CACHE`]); a
///     second presentation of the same id within its validity window is rejected
///     with [`Nip98ValidationError::TokenReplayed`]. Layer 1 alone cannot stop a
///     replay inside the 60s window — this layer closes that gap.
const TOKEN_MAX_AGE_SECONDS: i64 = 60;

/// Time-to-live for a spent event id in the replay cache.
///
/// Set to `2 × TOKEN_MAX_AGE_SECONDS` so an entry outlives the entire window in
/// which its token could still pass the freshness check (a token created at
/// `now + TOKEN_MAX_AGE_SECONDS` remains valid until `now + 2×`). Once an id is
/// older than this it can no longer be replayed successfully anyway (the
/// freshness check would reject it), so the entry is safe to prune.
const REPLAY_CACHE_TTL: Duration = Duration::from_secs(2 * TOKEN_MAX_AGE_SECONDS as u64);

/// Soft cap on cache size that triggers an opportunistic prune on insert.
/// The map is small in practice (bounded by request rate × TTL), so an O(n)
/// sweep is cheap; this only guards against pathological growth.
const REPLAY_CACHE_PRUNE_THRESHOLD: usize = 4096;

/// Hard capacity ceiling. Even after pruning every expired entry, if the live
/// set still sits at this many ids we refuse to record more and fail the claim
/// with [`Nip98ValidationError::ReplayCacheFull`].
///
/// Rationale (fail-closed under flood): an attacker who can mint cheap, valid
/// signatures would otherwise grow the map at `request-rate × TTL` unbounded.
/// Rejecting new auth when full is safe — today every `Nip98ValidationError`
/// surfaces through the callers' generic auth-failure path (401); the distinct
/// variant exists so a future caller can map capacity exhaustion to 503
/// without disturbing the replay semantics. We must **not** evict the oldest
/// live entry to make room: that would let a flooder purge a genuine,
/// still-valid id from the cache and re-enable the very replay this layer
/// exists to prevent. Bounded memory beats availability here.
const REPLAY_CACHE_MAX_ENTRIES: usize = 100_000;

/// Process-wide single-use cache mapping a spent NIP-98 event id to the
/// **monotonic** [`Instant`] at which it was first accepted. Guarded by a
/// `Mutex` so the check-and-insert in [`validate_nip98_token`] is atomic (no
/// TOCTOU).
///
/// `Instant` (not wall-clock) is deliberate: a backward system-clock step must
/// never extend an entry's lifetime or stall pruning. Wall-clock Unix time is
/// used only for the event-freshness window, which legitimately tracks the
/// signer's declared `created_at`.
///
/// **Single-process deployment invariant:** this cache is process-local by
/// design. Replay protection does **not** span replicas — horizontal scaling
/// requires shared storage (e.g. Redis) or sticky routing so every presentation
/// of a given token lands on the same process. See
/// `docs/SECURITY-profiles.md` invariant 4.
const REPLAY_CACHE_MAX_PER_PUBKEY: usize = 1_000;
#[derive(Default)]
struct ReplayState {
    events: HashMap<String, Instant>,
    signers: HashMap<String, VecDeque<Instant>>,
}
static REPLAY_CACHE: OnceLock<Mutex<ReplayState>> = OnceLock::new();
fn replay_cache() -> &'static Mutex<ReplayState> {
    REPLAY_CACHE.get_or_init(|| Mutex::new(ReplayState::default()))
}

// Called only after signature, freshness and request binding verification.
fn claim_signed_id(event_id: &str, pubkey: &str, now: Instant) -> Result<(), Nip98ValidationError> {
    let mut state = replay_cache().lock().unwrap_or_else(|p| p.into_inner());
    admit_signed_in(
        &mut state,
        event_id,
        pubkey,
        now,
        REPLAY_CACHE_MAX_PER_PUBKEY,
    )
}
fn admit_signed_in(
    state: &mut ReplayState,
    event_id: &str,
    pubkey: &str,
    now: Instant,
    limit: usize,
) -> Result<(), Nip98ValidationError> {
    // Expired signers must not accumulate indefinitely. Entries are created
    // only after a successful global-cache claim, bounding this map as well.
    state.signers.retain(|_, times| {
        times
            .back()
            .is_some_and(|t| now.saturating_duration_since(*t) < REPLAY_CACHE_TTL)
    });
    if let Some(times) = state.signers.get_mut(pubkey) {
        while times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= REPLAY_CACHE_TTL)
        {
            times.pop_front();
        }
        if times.len() >= limit {
            return Err(Nip98ValidationError::PubkeyAdmissionExceeded);
        }
    }
    claim_in(&mut state.events, event_id, now, REPLAY_CACHE_MAX_ENTRIES)?;
    state
        .signers
        .entry(pubkey.to_string())
        .or_default()
        .push_back(now);
    Ok(())
}

/// Atomically claim an event id as spent.
///
/// Returns:
///  - `Ok(())` if the id had not been seen (or its prior entry has expired) and
///    is now recorded;
///  - `Err(TokenReplayed)` if a still-live entry already exists;
///  - `Err(ReplayCacheFull)` if the cache is at its hard capacity even after
///    pruning expired entries.
///
/// The whole check-prune-cap-insert sequence runs under the cache lock, so two
/// concurrent validations of the same token cannot both win. `now` is a
/// monotonic reference instant (injected for testability); production passes
/// `Instant::now()`.
#[cfg(test)]
fn claim_event_id(event_id: &str, now: Instant) -> Result<(), Nip98ValidationError> {
    let mut cache = replay_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    claim_in(&mut cache.events, event_id, now, REPLAY_CACHE_MAX_ENTRIES)
}

/// Lock-free inner claim logic operating on an explicit map and capacity so the
/// cap boundary is unit-testable without flooding the process-global cache.
/// Callers must hold whatever lock guards `cache` for the whole call.
fn claim_in(
    cache: &mut HashMap<String, Instant>,
    event_id: &str,
    now: Instant,
    max_entries: usize,
) -> Result<(), Nip98ValidationError> {
    // A prior entry only counts as a replay while it is still within its TTL.
    // An expired entry is treated as absent and overwritten below.
    // `saturating_duration_since` clamps to zero if `now` precedes `seen_at`
    // (belt-and-braces; monotonic instants should never go backwards).
    if let Some(&seen_at) = cache.get(event_id) {
        if now.saturating_duration_since(seen_at) < REPLAY_CACHE_TTL {
            return Err(Nip98ValidationError::TokenReplayed);
        }
    }

    // Opportunistic O(n) prune of expired entries once the map grows large.
    // Clamped to the capacity so the cap check below never fires while expired
    // entries could have been reclaimed (matters when `max_entries` is small,
    // e.g. under test; in production the threshold is well below the cap).
    if cache.len() >= REPLAY_CACHE_PRUNE_THRESHOLD.min(max_entries) {
        cache.retain(|_, &mut seen_at| now.saturating_duration_since(seen_at) < REPLAY_CACHE_TTL);
    }

    // Hard capacity guard, checked AFTER pruning expired entries so a transient
    // burst that has since aged out does not wedge the cache. If we are still at
    // the ceiling every remaining entry is live — fail closed rather than evict.
    if cache.len() >= max_entries {
        return Err(Nip98ValidationError::ReplayCacheFull);
    }

    cache.insert(event_id.to_string(), now);
    Ok(())
}

/// Result of NIP-98 token validation
#[derive(Debug, Clone)]
pub struct Nip98ValidationResult {
    pub pubkey: String,
    pub url: String,
    pub method: String,
    pub created_at: i64,
    pub payload_hash: Option<String>,
}

/// Errors specific to token validation
#[derive(Debug, Error)]
pub enum Nip98ValidationError {
    #[error("Invalid base64 encoding")]
    InvalidBase64,
    #[error("Invalid UTF-8 in token")]
    InvalidUtf8,
    #[error("Invalid JSON structure: {0}")]
    InvalidJson(String),
    #[error("Invalid event kind: expected {HTTP_AUTH_KIND}, got {0}")]
    InvalidKind(u16),
    #[error("Token expired: created {0}s ago (max {TOKEN_MAX_AGE_SECONDS}s). Please check your system clock is synchronized.")]
    TokenExpired(i64),
    #[error("Token from the future: created {0}s ahead (tolerance {TOKEN_MAX_AGE_SECONDS}s). Please check your system clock is synchronized.")]
    TokenFromFuture(i64),
    #[error("Missing required tag: {0}")]
    MissingTag(String),
    #[error("URL mismatch: expected {expected}, got {actual}")]
    UrlMismatch { expected: String, actual: String },
    #[error("Method mismatch: expected {expected}, got {actual}")]
    MethodMismatch { expected: String, actual: String },
    #[error("Payload hash mismatch")]
    PayloadHashMismatch,
    #[error(
        "Token carries no payload tag but the route requires the body to be bound to the token"
    )]
    PayloadHashMissing,
    #[error("Token carries a payload tag but the route accepts no request body")]
    UnexpectedPayloadHash,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Failed to verify event: {0}")]
    VerificationFailed(String),
    #[error("Token replayed: this NIP-98 event id has already been used")]
    TokenReplayed,
    #[error("Replay cache at capacity: refusing new authentication (retry shortly)")]
    ReplayCacheFull,
    #[error("NIP-98 per-pubkey admission limit exceeded; retry after the replay window")]
    PubkeyAdmissionExceeded,
}

/// Validate a NIP-98 token from an Authorization header
/// # Arguments
/// * `token` - The base64-encoded token (without "Nostr " prefix)
/// * `expected_url` - The URL the request was made to
/// * `expected_method` - The HTTP method used
/// * `request_body` - Optional request body for payload verification
/// # Returns
/// Validation result with pubkey and metadata, or validation error
/// How a route binds its request body to the NIP-98 token (ADR-2002).
///
/// The original validator compared the payload hash **only when both a body and
/// a `payload` tag were present**, so a token minted without the tag
/// authenticated any body at all — the signature covered the URL and method but
/// not what was being sent. Whether that is acceptable is a property of the
/// route, not of the validator, so the route now declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BodyBinding {
    /// The route takes no request body. A token carrying a `payload` tag is
    /// rejected: it was minted for a different request.
    NoBody,
    /// The route takes a body and the token **must** bind it. A body with no
    /// `payload` tag is rejected. This is the default, and the right choice for
    /// every mutating route.
    #[default]
    Required,
    /// The route takes an optional body; when one is supplied the tag must
    /// match, but a body with no tag is tolerated. Retained for the legacy
    /// callers that pass a body through a shared helper which cannot yet say
    /// whether the route needs it bound. New routes must not choose this.
    RequiredWhenTagged,
}

/// Validate a NIP-98 token with the route's declared body binding (ADR-2002).
///
/// [`validate_nip98_token`] delegates here with [`BodyBinding::RequiredWhenTagged`],
/// preserving the pre-ADR-2002 behaviour for existing callers; a route that
/// mutates state should call this directly with [`BodyBinding::Required`].
pub fn validate_nip98_token_bound(
    token: &str,
    expected_url: &str,
    expected_method: &str,
    request_body: Option<&str>,
    binding: BodyBinding,
) -> Result<Nip98ValidationResult, Nip98ValidationError> {
    validate_nip98_token_inner(token, expected_url, expected_method, request_body, binding)
}

pub fn validate_nip98_token(
    token: &str,
    expected_url: &str,
    expected_method: &str,
    request_body: Option<&str>,
) -> Result<Nip98ValidationResult, Nip98ValidationError> {
    validate_nip98_token_inner(
        token,
        expected_url,
        expected_method,
        request_body,
        BodyBinding::RequiredWhenTagged,
    )
}

fn validate_nip98_token_inner(
    token: &str,
    expected_url: &str,
    expected_method: &str,
    request_body: Option<&str>,
    binding: BodyBinding,
) -> Result<Nip98ValidationResult, Nip98ValidationError> {
    // Decode base64
    let decoded = BASE64
        .decode(token)
        .map_err(|_| Nip98ValidationError::InvalidBase64)?;

    let json_str = String::from_utf8(decoded).map_err(|_| Nip98ValidationError::InvalidUtf8)?;

    // Parse the event
    let nip98_event: Nip98Event = serde_json::from_str(&json_str)
        .map_err(|e| Nip98ValidationError::InvalidJson(e.to_string()))?;

    // Verify event kind
    if nip98_event.kind != HTTP_AUTH_KIND {
        return Err(Nip98ValidationError::InvalidKind(nip98_event.kind));
    }

    // Check timestamp against a symmetric tolerance window. The token's
    // `created_at` must fall within ±TOKEN_MAX_AGE_SECONDS of the server clock:
    // too far in the past is a stale/replayed token, too far in the future is a
    // forged or clock-skewed token. Matching the forum verifier's abs_diff gate.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_secs() as i64;
    let age = now - nip98_event.created_at;

    if age > TOKEN_MAX_AGE_SECONDS {
        // created_at is too far in the past — stale or replayed.
        return Err(Nip98ValidationError::TokenExpired(age));
    }

    if age < -TOKEN_MAX_AGE_SECONDS {
        // created_at is too far in the future — forged or badly skewed clock.
        return Err(Nip98ValidationError::TokenFromFuture(-age));
    }

    // Extract and validate tags
    let mut url: Option<String> = None;
    let mut method: Option<String> = None;
    let mut payload_hash: Option<String> = None;

    for tag in &nip98_event.tags {
        if tag.len() >= 2 {
            match tag[0].as_str() {
                "u" => url = Some(tag[1].clone()),
                "method" => method = Some(tag[1].clone()),
                "payload" => payload_hash = Some(tag[1].clone()),
                _ => {}
            }
        }
    }

    let url = url.ok_or_else(|| Nip98ValidationError::MissingTag("u".to_string()))?;
    let method = method.ok_or_else(|| Nip98ValidationError::MissingTag("method".to_string()))?;

    // Validate URL matches (normalize for comparison)
    // The client may sign with a relative path (e.g. /solid/pods/init) while
    // the server sees the full URL after nginx rewrites /solid/ → /api/solid/.
    // We compare paths flexibly: strip the /api prefix from the expected URL
    // and allow relative-path tokens to match the server-side full URL.
    if !urls_match(expected_url, &url) {
        return Err(Nip98ValidationError::UrlMismatch {
            expected: expected_url.to_string(),
            actual: url,
        });
    }

    // Validate method matches
    if method.to_uppercase() != expected_method.to_uppercase() {
        return Err(Nip98ValidationError::MethodMismatch {
            expected: expected_method.to_string(),
            actual: method,
        });
    }

    // ADR-2002 — body binding, per the route's declared policy.
    match (binding, request_body, payload_hash.as_deref()) {
        // No body expected: a payload tag means the token was minted for some
        // other request, so it is not evidence of intent for this one.
        (BodyBinding::NoBody, _, Some(_)) => {
            return Err(Nip98ValidationError::UnexpectedPayloadHash)
        }
        (BodyBinding::NoBody, _, None) => {}

        // Body must be bound: an absent tag leaves the body unauthenticated.
        (BodyBinding::Required, Some(body), Some(token_hash)) => {
            if compute_payload_hash(body) != token_hash {
                return Err(Nip98ValidationError::PayloadHashMismatch);
            }
        }
        (BodyBinding::Required, Some(_), None) => {
            return Err(Nip98ValidationError::PayloadHashMissing)
        }
        // No body supplied on a body-binding route: the tag, if present, has
        // nothing to match, which means the token belongs to another request.
        (BodyBinding::Required, None, Some(_)) => {
            return Err(Nip98ValidationError::PayloadHashMismatch)
        }
        (BodyBinding::Required, None, None) => {}

        // Legacy tolerance: compare only when both halves are present.
        (BodyBinding::RequiredWhenTagged, Some(body), Some(token_hash)) => {
            if compute_payload_hash(body) != token_hash {
                return Err(Nip98ValidationError::PayloadHashMismatch);
            }
        }
        (BodyBinding::RequiredWhenTagged, _, _) => {}
    }

    // Verify the Nostr event signature
    let nostr_event = Event::from_json(&json_str)
        .map_err(|e| Nip98ValidationError::VerificationFailed(e.to_string()))?;

    nostr_event
        .verify()
        .map_err(|_| Nip98ValidationError::InvalidSignature)?;

    // Single-use enforcement (layer 2 of replay resistance). Only claim the id
    // once every other check has passed, so a malformed/forged token can never
    // burn a legitimate id. The claim keys on a monotonic `Instant` (not the
    // wall-clock `now` used for the freshness window) so a backward clock step
    // cannot extend entry lifetimes. This check-and-insert is atomic under the
    // cache lock, closing the replay gap the ±60s window leaves open.
    claim_signed_id(&nip98_event.id, &nip98_event.pubkey, Instant::now())?;

    debug!(
        "Validated NIP-98 token for {} {} (pubkey: {}...)",
        method,
        url,
        &nip98_event.pubkey[..16.min(nip98_event.pubkey.len())]
    );

    Ok(Nip98ValidationResult {
        pubkey: nip98_event.pubkey,
        url,
        method,
        created_at: nip98_event.created_at,
        payload_hash,
    })
}

/// Parse Authorization header and extract NIP-98 token
/// # Arguments
/// * `auth_header` - Full Authorization header value (e.g., "Nostr <base64>")
/// # Returns
/// The base64 token portion if valid Nostr auth, None otherwise
pub fn parse_auth_header(auth_header: &str) -> Option<&str> {
    let trimmed = auth_header.trim();
    if trimmed.starts_with("Nostr ") {
        Some(trimmed.strip_prefix("Nostr ")?.trim())
    } else {
        None
    }
}

/// Normalize URL for comparison (remove trailing slashes, lowercase scheme/host)
fn normalize_url(url: &str) -> String {
    let mut normalized = url.trim().to_string();

    // Remove trailing slash for comparison
    while normalized.ends_with('/') && normalized.len() > 1 {
        normalized.pop();
    }

    // Lowercase the scheme and host portion
    if let Some(idx) = normalized.find("://") {
        let (scheme, rest) = normalized.split_at(idx);
        let rest = &rest[3..]; // Skip "://"

        if let Some(path_idx) = rest.find('/') {
            let host = &rest[..path_idx];
            let path = &rest[path_idx..];
            normalized = format!(
                "{}://{}{}",
                scheme.to_lowercase(),
                host.to_lowercase(),
                path
            );
        } else {
            normalized = format!("{}://{}", scheme.to_lowercase(), rest.to_lowercase());
        }
    }

    normalized
}

/// Browser WebSocket upgrades cannot set Authorization. Carry the signed event
/// as an unpadded URL-safe subprotocol token; only the neutral `visionclaw`
/// protocol is selected in the response. Never put credentials in the URL.
pub fn websocket_auth_header(protocols: &str) -> Result<Option<String>, Nip98ValidationError> {
    let tokens: Vec<&str> = protocols
        .split(',')
        .map(str::trim)
        .filter_map(|p| p.strip_prefix("nostr."))
        .collect();
    if tokens.is_empty() {
        return Ok(None);
    }
    if tokens.len() != 1 || tokens[0].len() > 16_384 {
        return Err(Nip98ValidationError::InvalidBase64);
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(tokens[0])
        .map_err(|_| Nip98ValidationError::InvalidBase64)?;
    Ok(Some(format!("Nostr {}", BASE64.encode(bytes))))
}

/// Extract host and path from a URL.  Returns `(Some(host), path)` for
/// absolute URLs and `(None, path)` for relative paths.
fn extract_host_and_path(url: &str) -> (Option<&str>, &str) {
    if let Some(idx) = url.find("://") {
        let after_scheme = &url[idx + 3..];
        if let Some(path_idx) = after_scheme.find('/') {
            (Some(&after_scheme[..path_idx]), &after_scheme[path_idx..])
        } else {
            (Some(after_scheme), "/")
        }
    } else {
        // Relative path — no host
        (None, url)
    }
}

/// Compare two URLs flexibly for NIP-98 validation.
///
/// Handles two real-world cases:
///  1. Client signs with a relative path (`/solid/pods/init`), server expects
///     the full URL after nginx rewrites it to `/api/solid/pods/init`.
///  2. Client signs with the public absolute URL, server sees the internal one.
///
/// Security: when both URLs are absolute, hosts MUST match (case-insensitive)
/// before we fall through to path-only comparison.  This prevents a token
/// signed for `https://evil.com/solid/x` from matching requests to our server.
fn urls_match(expected: &str, actual: &str) -> bool {
    let norm_expected = normalize_url(expected);
    let norm_actual = normalize_url(actual);

    // 1. Direct full-URL match (fast path)
    if norm_expected == norm_actual {
        return true;
    }

    let (expected_host, expected_path) = extract_host_and_path(&norm_expected);
    let (actual_host, actual_path) = extract_host_and_path(&norm_actual);

    // 2. If both are absolute, hosts must match before we compare paths.
    //    Only skip the host check when one side is a relative path (no host).
    if let (Some(eh), Some(ah)) = (expected_host, actual_host) {
        if !eh.eq_ignore_ascii_case(ah) {
            return false;
        }
    }

    // 3. Direct path match
    if expected_path == actual_path {
        return true;
    }

    // 4. Handle nginx /solid/ → /api/solid/ rewrite:
    //    expected (server-side) = /api/solid/pods/init
    //    actual   (client-side) = /solid/pods/init
    if let Some(stripped) = expected_path.strip_prefix("/api") {
        if stripped == actual_path {
            return true;
        }
    }

    // 5. Reverse: client sent /api/..., server sees without prefix
    if let Some(stripped) = actual_path.strip_prefix("/api") {
        if stripped == expected_path {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn websocket_signed_carrier_is_single_use_and_url_bound() {
        let keys = Keys::generate();
        let token = generate_nip98_token(
            &keys,
            &Nip98Config {
                url: "https://example.test/wss".into(),
                method: "GET".into(),
                body: None,
            },
        )
        .unwrap();
        let carrier = token
            .trim_end_matches('=')
            .replace('+', "-")
            .replace('/', "_");
        let header = websocket_auth_header(&format!("visionclaw, nostr.{carrier}"))
            .unwrap()
            .unwrap();
        let raw = parse_auth_header(&header).unwrap();
        assert!(validate_nip98_token(raw, "https://other.test/wss", "GET", None).is_err());
        assert!(validate_nip98_token(raw, "https://example.test/wss", "GET", None).is_ok());
        assert!(matches!(
            validate_nip98_token(raw, "https://example.test/wss", "GET", None),
            Err(Nip98ValidationError::TokenReplayed)
        ));
        assert!(websocket_auth_header(&format!("nostr.{carrier},nostr.{carrier}")).is_err());
    }

    #[test]
    fn signer_admission_preserves_other_signers_and_replay_entries() {
        let mut state = ReplayState::default();
        let now = Instant::now();
        admit_signed_in(&mut state, "a1", "a", now, 2).unwrap();
        admit_signed_in(&mut state, "a2", "a", now, 2).unwrap();
        assert!(matches!(
            admit_signed_in(&mut state, "a3", "a", now, 2),
            Err(Nip98ValidationError::PubkeyAdmissionExceeded)
        ));
        admit_signed_in(&mut state, "b1", "b", now, 2).unwrap();
        assert_eq!(state.events.len(), 3);
        assert!(state.events.contains_key("a1"));
        admit_signed_in(&mut state, "a3", "a", now + REPLAY_CACHE_TTL, 2).unwrap();
    }

    #[test]
    fn test_generate_nip98_token() {
        let keys = Keys::generate();
        let config = Nip98Config {
            url: "http://localhost:3030/pods/test/".to_string(),
            method: "GET".to_string(),
            body: None,
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        assert!(!token.is_empty());

        // Verify we can extract the pubkey
        let pubkey = extract_pubkey_from_token(&token).expect("Failed to extract pubkey");
        assert_eq!(pubkey, keys.public_key().to_hex());
    }

    #[test]
    fn test_generate_nip98_token_with_body() {
        let keys = Keys::generate();
        let config = Nip98Config {
            url: "http://localhost:3030/pods/test/data.jsonld".to_string(),
            method: "PUT".to_string(),
            body: Some(r#"{"@context": "https://schema.org"}"#.to_string()),
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        assert!(!token.is_empty());
    }

    #[test]
    fn test_payload_hash() {
        let body = r#"{"test": "data"}"#;
        let hash = compute_payload_hash(body);
        assert_eq!(hash.len(), 64); // SHA256 hex is 64 chars
    }

    #[test]
    fn test_build_auth_header() {
        let token = "dGVzdA==";
        let header = build_auth_header(token);
        assert_eq!(header, "Nostr dGVzdA==");
    }

    #[test]
    fn test_parse_auth_header() {
        assert_eq!(parse_auth_header("Nostr abc123"), Some("abc123"));
        assert_eq!(parse_auth_header("  Nostr xyz  "), Some("xyz"));
        assert_eq!(parse_auth_header("Bearer abc123"), None);
        assert_eq!(parse_auth_header("nostr abc123"), None); // case sensitive
    }

    #[test]
    fn test_urls_match_direct() {
        assert!(urls_match(
            "http://localhost:3000/api/solid/pods/init",
            "http://localhost:3000/api/solid/pods/init"
        ));
    }

    #[test]
    fn test_urls_match_api_prefix_strip() {
        // Server sees /api/solid/..., client signed /solid/...
        assert!(urls_match(
            "http://localhost:3001/api/solid/pods/init",
            "http://localhost:3001/solid/pods/init"
        ));
    }

    #[test]
    fn test_urls_match_relative_path() {
        // Client signs relative path, server has full URL
        assert!(urls_match(
            "http://localhost:3001/api/solid/pods/init",
            "/solid/pods/init"
        ));
    }

    #[test]
    fn test_urls_match_rejects_different_host() {
        // CRITICAL: token signed for evil.com must NOT match our server
        assert!(!urls_match(
            "https://visionclaw.info/api/solid/pods/init",
            "https://evil.com/api/solid/pods/init"
        ));
        assert!(!urls_match(
            "https://visionclaw.info/solid/pods/init",
            "https://evil.com/solid/pods/init"
        ));
    }

    #[test]
    fn test_urls_match_case_insensitive_host() {
        assert!(urls_match(
            "https://VisionClaw.INFO/solid/pods",
            "https://visionclaw.info/solid/pods"
        ));
    }

    #[test]
    fn test_urls_match_relative_vs_absolute_allowed() {
        // Relative path has no host — should still match via path comparison
        assert!(urls_match(
            "https://visionclaw.info/api/solid/pods/init",
            "/solid/pods/init"
        ));
        // But the reverse should also work
        assert!(urls_match(
            "/solid/pods/init",
            "https://visionclaw.info/api/solid/pods/init"
        ));
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("HTTP://LOCALHOST:3030/pods/test/"),
            "http://localhost:3030/pods/test"
        );
        assert_eq!(
            normalize_url("https://Example.COM/path"),
            "https://example.com/path"
        );
        assert_eq!(normalize_url("http://a.com///"), "http://a.com");
    }

    #[test]
    fn test_validate_nip98_token_valid() {
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/test/";
        let method = "GET";
        let config = Nip98Config {
            url: url.to_string(),
            method: method.to_string(),
            body: None,
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        let result = validate_nip98_token(&token, url, method, None);

        assert!(result.is_ok(), "Validation failed: {:?}", result.err());
        let validation = result.unwrap();
        assert_eq!(validation.pubkey, keys.public_key().to_hex());
        assert_eq!(validation.method.to_uppercase(), method);
    }

    #[test]
    fn test_validate_nip98_token_with_payload() {
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/test/data.jsonld";
        let method = "PUT";
        let body = r#"{"@context": "https://schema.org"}"#;
        let config = Nip98Config {
            url: url.to_string(),
            method: method.to_string(),
            body: Some(body.to_string()),
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        let result = validate_nip98_token(&token, url, method, Some(body));

        assert!(result.is_ok(), "Validation failed: {:?}", result.err());
        let validation = result.unwrap();
        assert!(validation.payload_hash.is_some());
    }

    #[test]
    fn test_validate_nip98_token_url_mismatch() {
        let keys = Keys::generate();
        let config = Nip98Config {
            url: "http://localhost:3030/pods/alice/".to_string(),
            method: "GET".to_string(),
            body: None,
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        let result = validate_nip98_token(&token, "http://localhost:3030/pods/bob/", "GET", None);

        assert!(matches!(
            result,
            Err(Nip98ValidationError::UrlMismatch { .. })
        ));
    }

    #[test]
    fn test_validate_nip98_token_method_mismatch() {
        let keys = Keys::generate();
        let config = Nip98Config {
            url: "http://localhost:3030/pods/test/".to_string(),
            method: "GET".to_string(),
            body: None,
        };

        let token = generate_nip98_token(&keys, &config).expect("Failed to generate token");
        let result = validate_nip98_token(&token, "http://localhost:3030/pods/test/", "POST", None);

        assert!(matches!(
            result,
            Err(Nip98ValidationError::MethodMismatch { .. })
        ));
    }

    #[test]
    fn test_validate_nip98_token_invalid_base64() {
        let result = validate_nip98_token("not-valid-base64!!!", "http://test.com", "GET", None);
        assert!(matches!(result, Err(Nip98ValidationError::InvalidBase64)));
    }

    /// Build a base64-encoded NIP-98 token whose `created_at` is offset from the
    /// current clock by `offset_secs` (negative = past, positive = future).  The
    /// event is genuinely signed so it passes signature verification, letting us
    /// exercise the timestamp window in isolation.
    fn signed_token_with_offset(keys: &Keys, url: &str, method: &str, offset_secs: i64) -> String {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_secs() as i64
            + offset_secs;

        let tags: Vec<Tag> = vec![
            Tag::custom(TagKind::Custom("u".into()), vec![url.to_string()]),
            Tag::custom(
                TagKind::Custom("method".into()),
                vec![method.to_uppercase()],
            ),
        ];

        let event = EventBuilder::new(Kind::Custom(HTTP_AUTH_KIND), "")
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at as u64))
            .sign_with_keys(keys)
            .expect("failed to sign event");

        let nip98_event = Nip98Event {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_u64() as i64,
            kind: HTTP_AUTH_KIND,
            tags: event
                .tags
                .iter()
                .map(|t| t.as_slice().iter().map(|s| s.to_string()).collect())
                .collect(),
            content: event.content.clone(),
            sig: event.sig.to_string(),
        };

        let json = serde_json::to_string(&nip98_event).expect("serialize");
        BASE64.encode(json.as_bytes())
    }

    #[test]
    fn test_validate_nip98_token_future_dated_rejected() {
        // A token created well beyond the future tolerance window must be
        // rejected. This guards against forged or clock-skewed tokens that an
        // earlier one-sided age check would have silently accepted.
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/test/";
        let method = "GET";

        let token = signed_token_with_offset(&keys, url, method, TOKEN_MAX_AGE_SECONDS + 120);
        let result = validate_nip98_token(&token, url, method, None);

        assert!(
            matches!(result, Err(Nip98ValidationError::TokenFromFuture(_))),
            "expected TokenFromFuture, got {:?}",
            result
        );
    }

    #[test]
    fn test_validate_nip98_token_within_future_tolerance_accepted() {
        // A token a few seconds in the future (within tolerance) is legitimate
        // clock skew and must still validate, confirming the window is symmetric
        // rather than simply rejecting all future timestamps.
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/test/";
        let method = "GET";

        let token = signed_token_with_offset(&keys, url, method, TOKEN_MAX_AGE_SECONDS / 2);
        let result = validate_nip98_token(&token, url, method, None);

        assert!(result.is_ok(), "validation failed: {:?}", result.err());
    }

    #[test]
    fn test_replay_same_token_rejected() {
        // A fully-valid token must succeed exactly once. Presenting the identical
        // event a second time within its validity window is a replay and must be
        // rejected by the single-use cache, even though every other check (fresh
        // timestamp, matching URL/method, valid signature) still passes.
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/replay/";
        let method = "GET";
        let config = Nip98Config {
            url: url.to_string(),
            method: method.to_string(),
            body: None,
        };

        let token = generate_nip98_token(&keys, &config).expect("failed to generate token");

        let first = validate_nip98_token(&token, url, method, None);
        assert!(first.is_ok(), "first use should succeed: {:?}", first.err());

        let second = validate_nip98_token(&token, url, method, None);
        assert!(
            matches!(second, Err(Nip98ValidationError::TokenReplayed)),
            "second use must be rejected as replay, got {second:?}"
        );
    }

    #[test]
    fn test_claim_event_id_distinct_ids_unaffected() {
        // Distinct event ids never collide: claiming one must not block another.
        let now = Instant::now();
        let id_a = "distinct-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let id_b = "distinct-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

        assert!(claim_event_id(id_a, now).is_ok(), "first id should be free");
        assert!(
            claim_event_id(id_b, now).is_ok(),
            "a different id must be unaffected by the first"
        );
        // Re-claiming the first within its TTL is still a replay.
        assert!(matches!(
            claim_event_id(id_a, now + Duration::from_secs(1)),
            Err(Nip98ValidationError::TokenReplayed)
        ));
    }

    #[test]
    fn test_claim_event_id_expires_after_window() {
        // An entry older than the cache TTL is treated as absent: the id becomes
        // claimable again. (In practice the freshness window would already reject
        // such a stale token, but the cache must not leak entries forever.)
        // Uses monotonic `Instant` offsets — a backward wall-clock step cannot
        // perturb this.
        let now = Instant::now();
        let id = "expiring-cccccccccccccccccccccccccccccccccccccccccccccccccc";

        assert!(
            claim_event_id(id, now).is_ok(),
            "initial claim should succeed"
        );

        // Within the TTL → still a replay.
        assert!(matches!(
            claim_event_id(id, now + REPLAY_CACHE_TTL - Duration::from_secs(1)),
            Err(Nip98ValidationError::TokenReplayed)
        ));

        // At/after the TTL boundary → the old entry has expired, so it is
        // accepted again (and re-recorded at the new timestamp).
        assert!(
            claim_event_id(id, now + REPLAY_CACHE_TTL).is_ok(),
            "expired entry must be reclaimable"
        );
    }

    #[test]
    fn test_claim_in_cap_boundary_fails_closed() {
        // Pin the hard-cap semantics on an ISOLATED map (not the process-global
        // cache) so this test neither floods other tests nor needs 100k inserts.
        let now = Instant::now();
        let mut cache: HashMap<String, Instant> = HashMap::new();
        let cap = 2;

        assert!(claim_in(&mut cache, "cap-id-1", now, cap).is_ok());
        assert!(claim_in(&mut cache, "cap-id-2", now, cap).is_ok());

        // At capacity with only LIVE entries: a fresh id must be rejected
        // (fail closed), never admitted by evicting a live entry.
        assert!(matches!(
            claim_in(&mut cache, "cap-id-3", now, cap),
            Err(Nip98ValidationError::ReplayCacheFull)
        ));
        assert_eq!(cache.len(), cap, "rejected claim must not grow the map");
        assert!(
            cache.contains_key("cap-id-1") && cache.contains_key("cap-id-2"),
            "live entries must never be evicted to admit a new claim"
        );

        // A replay of an already-cached id still reports TokenReplayed (the
        // replay check precedes the capacity check).
        assert!(matches!(
            claim_in(&mut cache, "cap-id-1", now + Duration::from_secs(1), cap),
            Err(Nip98ValidationError::TokenReplayed)
        ));

        // Once the resident entries expire, pruning frees capacity and new
        // claims are admitted again.
        let after_ttl = now + REPLAY_CACHE_TTL;
        assert!(
            claim_in(&mut cache, "cap-id-3", after_ttl, cap).is_ok(),
            "expired entries must free capacity for new claims"
        );
    }

    #[test]
    fn test_claim_event_id_atomic_under_concurrency() {
        // Race N threads claiming the SAME id through a barrier so they collide
        // as tightly as the scheduler allows. Exactly one must win; the rest must
        // see TokenReplayed. Proves the check-and-insert is atomic (no TOCTOU).
        use std::sync::{Arc, Barrier};

        const THREADS: usize = 16;
        let id = "concurrent-dddddddddddddddddddddddddddddddddddddddddddddddd";
        let now = Instant::now();
        let barrier = Arc::new(Barrier::new(THREADS));

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    claim_event_id(id, now)
                })
            })
            .collect();

        let mut ok = 0usize;
        let mut replayed = 0usize;
        for h in handles {
            match h.join().expect("thread panicked") {
                Ok(()) => ok += 1,
                Err(Nip98ValidationError::TokenReplayed) => replayed += 1,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }

        assert_eq!(ok, 1, "exactly one claim must win, got {ok}");
        assert_eq!(
            replayed,
            THREADS - 1,
            "all other claims must be rejected as replays, got {replayed}"
        );
    }

    /// Corrupt the `sig` field of a signed token while preserving its event id
    /// (the Nostr id is the hash of the *unsigned* fields, so flipping the
    /// signature does not change it). Lets us prove a signature failure carrying
    /// id X does not consume X.
    fn token_with_broken_signature(keys: &Keys, url: &str, method: &str) -> (String, String) {
        let config = Nip98Config {
            url: url.to_string(),
            method: method.to_string(),
            body: None,
        };
        let token = generate_nip98_token(keys, &config).expect("generate");
        let decoded = BASE64.decode(&token).expect("decode");
        let mut event: Nip98Event =
            serde_json::from_str(&String::from_utf8(decoded).expect("utf8")).expect("parse");
        let id = event.id.clone();

        // Flip the leading hex nibble of the signature — still valid hex, but no
        // longer a signature over this event, so `verify()` fails.
        let mut sig_chars: Vec<char> = event.sig.chars().collect();
        sig_chars[0] = if sig_chars[0] == '0' { '1' } else { '0' };
        event.sig = sig_chars.into_iter().collect();

        let json = serde_json::to_string(&event).expect("serialize");
        (BASE64.encode(json.as_bytes()), id)
    }

    #[test]
    fn test_invalid_signature_does_not_burn_id() {
        // A tampered signature must fail with InvalidSignature AND leave the id
        // unclaimed, so the legitimate holder can still use their token.
        let keys = Keys::generate();
        let url = "http://localhost:3030/pods/burn-sig/";
        let method = "GET";

        let (bad_token, id) = token_with_broken_signature(&keys, url, method);
        let bad = validate_nip98_token(&bad_token, url, method, None);
        assert!(
            matches!(bad, Err(Nip98ValidationError::InvalidSignature)),
            "tampered token must fail signature check, got {bad:?}"
        );

        // The id must still be free — prove it via a direct claim.
        assert!(
            claim_event_id(&id, Instant::now()).is_ok(),
            "a failed-signature validation must not consume the event id"
        );
    }

    #[test]
    fn test_failed_checks_do_not_burn_id() {
        // URL-, method-, and freshness-failures all short-circuit before the
        // single-use claim, so none of them may consume the token's id. Each
        // sub-case uses a FRESH keypair: two NIP-98 events with identical content
        // signed in the same wall-clock second hash to the same event id, which
        // would otherwise cross-contaminate the cases.
        let url = "http://localhost:3030/pods/burn-checks/";
        let method = "GET";
        let config = Nip98Config {
            url: url.to_string(),
            method: method.to_string(),
            body: None,
        };

        // URL mismatch: fails before the claim, so the correct-URL retry succeeds
        // and only then is the id consumed (a subsequent replay is rejected).
        let keys_url = Keys::generate();
        let token = generate_nip98_token(&keys_url, &config).expect("generate");
        assert!(matches!(
            validate_nip98_token(&token, "http://localhost:3030/pods/other/", method, None),
            Err(Nip98ValidationError::UrlMismatch { .. })
        ));
        assert!(
            validate_nip98_token(&token, url, method, None).is_ok(),
            "url-mismatch failure must not consume the id"
        );
        assert!(
            matches!(
                validate_nip98_token(&token, url, method, None),
                Err(Nip98ValidationError::TokenReplayed)
            ),
            "the id must be consumed only by the successful validation"
        );

        // Method mismatch on a fresh keypair.
        let keys_method = Keys::generate();
        let token2 = generate_nip98_token(&keys_method, &config).expect("generate");
        let id2 = extract_id(&token2);
        assert!(matches!(
            validate_nip98_token(&token2, url, "POST", None),
            Err(Nip98ValidationError::MethodMismatch { .. })
        ));
        assert!(
            claim_event_id(&id2, Instant::now()).is_ok(),
            "method-mismatch failure must not consume the id"
        );

        // Stale timestamp — a token far in the past fails freshness before the
        // claim, so its id stays free.
        let keys_stale = Keys::generate();
        let stale =
            signed_token_with_offset(&keys_stale, url, method, -(TOKEN_MAX_AGE_SECONDS + 120));
        let id_stale = extract_id(&stale);
        assert!(matches!(
            validate_nip98_token(&stale, url, method, None),
            Err(Nip98ValidationError::TokenExpired(_))
        ));
        assert!(
            claim_event_id(&id_stale, Instant::now()).is_ok(),
            "stale-timestamp failure must not consume the id"
        );
    }

    /// Decode a token and return its event id (test helper).
    fn extract_id(token: &str) -> String {
        let decoded = BASE64.decode(token).expect("decode");
        let event: Nip98Event =
            serde_json::from_str(&String::from_utf8(decoded).expect("utf8")).expect("parse");
        event.id
    }

    // ---- ADR-2002: exact freshness / TTL boundaries ------------------------

    /// A fresh map for the boundary tests, so the process-global cache is never
    /// touched and the tests can run in any order.
    fn fresh_cache() -> HashMap<String, Instant> {
        HashMap::new()
    }

    /// The TTL boundary is exact and half-open: an entry is a replay for
    /// strictly less than `REPLAY_CACHE_TTL`, and free at exactly the TTL.
    #[test]
    fn ttl_boundary_is_exact_and_half_open() {
        let base = Instant::now();
        let mut cache = fresh_cache();
        claim_in(&mut cache, "id", base, REPLAY_CACHE_MAX_ENTRIES).expect("first claim");

        // One nanosecond before the TTL: still a replay.
        let just_inside = base + REPLAY_CACHE_TTL - Duration::from_nanos(1);
        assert!(
            matches!(
                claim_in(&mut cache, "id", just_inside, REPLAY_CACHE_MAX_ENTRIES),
                Err(Nip98ValidationError::TokenReplayed)
            ),
            "an entry one nanosecond short of the TTL is still live"
        );

        // Exactly at the TTL: expired, so the id is claimable again.
        let at_ttl = base + REPLAY_CACHE_TTL;
        assert!(
            claim_in(&mut cache, "id", at_ttl, REPLAY_CACHE_MAX_ENTRIES).is_ok(),
            "an entry at exactly the TTL has expired"
        );
    }

    /// Re-claiming refreshes the entry, so the TTL runs from the latest claim
    /// rather than the first.
    #[test]
    fn a_reclaim_restarts_the_ttl() {
        let base = Instant::now();
        let mut cache = fresh_cache();
        claim_in(&mut cache, "id", base, REPLAY_CACHE_MAX_ENTRIES).unwrap();
        let expired = base + REPLAY_CACHE_TTL;
        claim_in(&mut cache, "id", expired, REPLAY_CACHE_MAX_ENTRIES).unwrap();

        // The TTL now runs from `expired`, not from `base`.
        assert!(matches!(
            claim_in(
                &mut cache,
                "id",
                expired + REPLAY_CACHE_TTL - Duration::from_nanos(1),
                REPLAY_CACHE_MAX_ENTRIES
            ),
            Err(Nip98ValidationError::TokenReplayed)
        ));
    }

    /// A backward monotonic step (which should be impossible, but the code
    /// guards for it) must never shorten an entry's life.
    #[test]
    fn a_backward_clock_step_does_not_expire_an_entry() {
        let base = Instant::now() + REPLAY_CACHE_TTL * 2;
        let mut cache = fresh_cache();
        claim_in(&mut cache, "id", base, REPLAY_CACHE_MAX_ENTRIES).unwrap();

        // `now` before `seen_at`: saturating_duration_since clamps to zero, so
        // the entry is still live and the claim is refused.
        let stepped_back = base - REPLAY_CACHE_TTL;
        assert!(matches!(
            claim_in(&mut cache, "id", stepped_back, REPLAY_CACHE_MAX_ENTRIES),
            Err(Nip98ValidationError::TokenReplayed)
        ));
    }

    /// The TTL is exactly twice the freshness window, so an id stays claimed for
    /// the whole period in which its token could still pass the freshness check.
    #[test]
    fn ttl_covers_the_entire_freshness_window() {
        assert_eq!(
            REPLAY_CACHE_TTL,
            Duration::from_secs(2 * TOKEN_MAX_AGE_SECONDS as u64),
            "the TTL must outlive the widest window a token can be fresh in"
        );
    }

    /// The capacity ceiling is exact: the last slot is usable, the next is not,
    /// and a full cache fails closed rather than evicting a live entry.
    #[test]
    fn capacity_boundary_is_exact_and_fails_closed() {
        let base = Instant::now();
        let mut cache = fresh_cache();
        for i in 0..3 {
            claim_in(&mut cache, &format!("id-{i}"), base, 3).expect("within capacity");
        }
        assert_eq!(cache.len(), 3);

        // The next distinct id has nowhere to go.
        assert!(matches!(
            claim_in(&mut cache, "id-overflow", base, 3),
            Err(Nip98ValidationError::ReplayCacheFull)
        ));

        // A full cache still recognises a replay — capacity exhaustion must not
        // open a hole in the single-use guarantee.
        assert!(matches!(
            claim_in(&mut cache, "id-0", base, 3),
            Err(Nip98ValidationError::TokenReplayed)
        ));

        // And it must not have evicted the live entry to make room.
        assert_eq!(cache.len(), 3);
        assert!(cache.contains_key("id-0"));
    }

    /// Once entries expire, the space they occupied becomes usable again.
    #[test]
    fn expired_entries_free_capacity() {
        let base = Instant::now();
        let mut cache = fresh_cache();
        for i in 0..3 {
            claim_in(&mut cache, &format!("id-{i}"), base, 3).unwrap();
        }
        assert!(matches!(
            claim_in(&mut cache, "new", base, 3),
            Err(Nip98ValidationError::ReplayCacheFull)
        ));

        let later = base + REPLAY_CACHE_TTL;
        assert!(
            claim_in(&mut cache, "new", later, 3).is_ok(),
            "expired entries must be pruned to make room"
        );
    }

    // ---- ADR-2002: route-specific body binding -----------------------------

    /// Build a signed token, optionally binding a body.
    fn token_for(keys: &Keys, url: &str, method: &str, body: Option<&str>) -> String {
        generate_nip98_token(
            keys,
            &Nip98Config {
                url: url.to_string(),
                method: method.to_string(),
                body: body.map(str::to_string),
            },
        )
        .expect("token")
    }

    /// The reproduced gap: a token with no `payload` tag authenticated ANY
    /// body, because the comparison only ran when both halves were present.
    /// `BodyBinding::Required` closes it.
    #[test]
    fn required_binding_rejects_an_unbound_body() {
        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let unbound = token_for(&keys, url, "POST", None);

        // The legacy policy accepts it — this is the behaviour the ADR names.
        assert!(
            validate_nip98_token(&unbound, url, "POST", Some("{\"drop\":\"everything\"}")).is_ok(),
            "the legacy policy is the reproduced gap"
        );

        // The route-declared policy refuses it.
        assert!(matches!(
            validate_nip98_token_bound(
                &unbound,
                url,
                "POST",
                Some("{\"drop\":\"everything\"}"),
                BodyBinding::Required,
            ),
            Err(Nip98ValidationError::PayloadHashMissing)
        ));
    }

    /// A correctly bound body passes under the strict policy.
    #[test]
    fn required_binding_accepts_a_matching_body() {
        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let body = "{\"value\":1}";
        let bound = token_for(&keys, url, "POST", Some(body));

        assert!(
            validate_nip98_token_bound(&bound, url, "POST", Some(body), BodyBinding::Required)
                .is_ok()
        );
    }

    /// A substituted body is refused even though the token itself is valid.
    #[test]
    fn required_binding_rejects_a_substituted_body() {
        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let bound = token_for(&keys, url, "POST", Some("{\"value\":1}"));

        assert!(matches!(
            validate_nip98_token_bound(
                &bound,
                url,
                "POST",
                Some("{\"value\":999}"),
                BodyBinding::Required
            ),
            Err(Nip98ValidationError::PayloadHashMismatch)
        ));
    }

    /// A body-binding route handed a token whose tag has no body to match is
    /// refused: the token belongs to a different request.
    #[test]
    fn required_binding_rejects_a_tag_without_a_body() {
        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let bound = token_for(&keys, url, "POST", Some("{\"value\":1}"));

        assert!(matches!(
            validate_nip98_token_bound(&bound, url, "POST", None, BodyBinding::Required),
            Err(Nip98ValidationError::PayloadHashMismatch)
        ));
    }

    /// A no-body route refuses a token that carries a payload tag.
    #[test]
    fn no_body_binding_rejects_a_payload_tag() {
        let keys = Keys::generate();
        let url = "https://example.org/api/read";
        let bound = token_for(&keys, url, "GET", Some("{\"value\":1}"));

        assert!(matches!(
            validate_nip98_token_bound(&bound, url, "GET", None, BodyBinding::NoBody),
            Err(Nip98ValidationError::UnexpectedPayloadHash)
        ));

        // A tagless token is exactly what a no-body route expects.
        let plain = token_for(&keys, url, "GET", None);
        assert!(validate_nip98_token_bound(&plain, url, "GET", None, BodyBinding::NoBody).is_ok());
    }

    /// The legacy policy remains what the existing callers get, so the change
    /// is additive rather than a silent behavioural break.
    #[test]
    fn the_default_policy_is_strict_and_legacy_callers_are_unchanged() {
        // The DEFAULT for a newly written route is the strict one...
        assert_eq!(BodyBinding::default(), BodyBinding::Required);

        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let bound = token_for(&keys, url, "POST", Some("{\"a\":1}"));
        // Mismatch is still caught under the legacy policy.
        assert!(matches!(
            validate_nip98_token(&bound, url, "POST", Some("{\"a\":2}")),
            Err(Nip98ValidationError::PayloadHashMismatch)
        ));
    }

    /// Every binding policy claims the event id exactly once, so declaring a
    /// stricter policy does not weaken the single-use guarantee.
    #[test]
    fn a_rejected_binding_does_not_burn_the_event_id() {
        let keys = Keys::generate();
        let url = "https://example.org/api/mutate";
        let unbound = token_for(&keys, url, "POST", None);

        // Rejected on the binding policy, before the claim.
        assert!(validate_nip98_token_bound(
            &unbound,
            url,
            "POST",
            Some("body"),
            BodyBinding::Required
        )
        .is_err());

        // The id was never spent, so a correctly bound use still succeeds.
        assert!(
            validate_nip98_token_bound(&unbound, url, "POST", None, BodyBinding::Required).is_ok(),
            "a binding rejection must not consume the token"
        );
    }
}
