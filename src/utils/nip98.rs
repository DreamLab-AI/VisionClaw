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
        tags.push(Tag::custom(
            TagKind::Custom("payload".into()),
            vec![hash],
        ));
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
    let secret_key = SecretKey::from_hex(secret_key_hex)
        .map_err(|e| Nip98Error::KeyCreation(e.to_string()))?;
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

/// Maximum age for NIP-98 tokens (5 minutes to accommodate clock skew)
/// Must match JSS's TIMESTAMP_TOLERANCE (60s). Tokens 61-300s old passed our
/// check but were rejected by JSS, causing 401s on legitimate requests.
const TOKEN_MAX_AGE_SECONDS: i64 = 60;

/// Result of NIP-98 token validation
#[derive(Debug, Clone)]
pub struct Nip98ValidationResult {
    pub event_id: String,
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
    #[error("Missing required tag: {0}")]
    MissingTag(String),
    #[error("URL mismatch: expected {expected}, got {actual}")]
    UrlMismatch { expected: String, actual: String },
    #[error("Method mismatch: expected {expected}, got {actual}")]
    MethodMismatch { expected: String, actual: String },
    #[error("Payload hash mismatch")]
    PayloadHashMismatch,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Failed to verify event: {0}")]
    VerificationFailed(String),
    #[error("NIP-98 token already used (event {0})")]
    TokenReplayed(String),
}

/// Validate a NIP-98 token from an Authorization header
/// # Arguments
/// * `token` - The base64-encoded token (without "Nostr " prefix)
/// * `expected_url` - The URL the request was made to
/// * `expected_method` - The HTTP method used
/// * `request_body` - Optional request body for payload verification
/// # Returns
/// Validation result with pubkey and metadata, or validation error
pub fn validate_nip98_token(
    token: &str,
    expected_url: &str,
    expected_method: &str,
    request_body: Option<&str>,
) -> Result<Nip98ValidationResult, Nip98ValidationError> {
    // Decode base64
    let decoded = BASE64
        .decode(token)
        .map_err(|_| Nip98ValidationError::InvalidBase64)?;

    let json_str =
        String::from_utf8(decoded).map_err(|_| Nip98ValidationError::InvalidUtf8)?;

    // Parse the event
    let nip98_event: Nip98Event = serde_json::from_str(&json_str)
        .map_err(|e| Nip98ValidationError::InvalidJson(e.to_string()))?;

    // Verify event kind
    if nip98_event.kind != HTTP_AUTH_KIND {
        return Err(Nip98ValidationError::InvalidKind(nip98_event.kind));
    }

    // Check timestamp (60 second window)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_secs() as i64;
    let age = now - nip98_event.created_at;

    if age > TOKEN_MAX_AGE_SECONDS {
        return Err(Nip98ValidationError::TokenExpired(age));
    }

    // Also reject tokens from the future (clock skew protection)
    if age < -TOKEN_MAX_AGE_SECONDS {
        return Err(Nip98ValidationError::TokenExpired(age));
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

    // Validate payload hash if body provided
    if let Some(body) = request_body {
        let computed_hash = compute_payload_hash(body);
        if let Some(ref token_hash) = payload_hash {
            if &computed_hash != token_hash {
                return Err(Nip98ValidationError::PayloadHashMismatch);
            }
        }
    }

    // Verify the Nostr event signature
    let nostr_event = Event::from_json(&json_str)
        .map_err(|e| Nip98ValidationError::VerificationFailed(e.to_string()))?;

    nostr_event
        .verify()
        .map_err(|_| Nip98ValidationError::InvalidSignature)?;

    debug!(
        "Validated NIP-98 token for {} {} (pubkey: {}...)",
        method,
        url,
        &nip98_event.pubkey[..16.min(nip98_event.pubkey.len())]
    );

    Ok(Nip98ValidationResult {
        event_id: nip98_event.id,
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
            normalized = format!("{}://{}{}", scheme.to_lowercase(), host.to_lowercase(), path);
        } else {
            normalized = format!("{}://{}", scheme.to_lowercase(), rest.to_lowercase());
        }
    }

    normalized
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
            "https://visionflow.info/api/solid/pods/init",
            "https://evil.com/api/solid/pods/init"
        ));
        assert!(!urls_match(
            "https://visionflow.info/solid/pods/init",
            "https://evil.com/solid/pods/init"
        ));
    }

    #[test]
    fn test_urls_match_case_insensitive_host() {
        assert!(urls_match(
            "https://VisionFlow.INFO/solid/pods",
            "https://visionflow.info/solid/pods"
        ));
    }

    #[test]
    fn test_urls_match_relative_vs_absolute_allowed() {
        // Relative path has no host — should still match via path comparison
        assert!(urls_match(
            "https://visionflow.info/api/solid/pods/init",
            "/solid/pods/init"
        ));
        // But the reverse should also work
        assert!(urls_match(
            "/solid/pods/init",
            "https://visionflow.info/api/solid/pods/init"
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

        assert!(matches!(result, Err(Nip98ValidationError::UrlMismatch { .. })));
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
}
