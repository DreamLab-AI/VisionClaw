//! `acl:origin` value objects and enforcement helpers (WAC §4.3 / F4).
//!
//! Implements the Origin gate described in
//! `docs/design/jss-parity/03-wac-enforcement-context.md`. The gate runs
//! **after** the existing agent / agent-class / mode / accessTo checks;
//! if any authorisation in the effective ACL declares `acl:origin`
//! triples, the request's `Origin` header must match one of them.
//!
//! This module is strictly additive: consumers that never pass an
//! `Origin` value object observe no behavioural change, because an
//! ACL with zero `acl:origin` triples yields [`OriginDecision::NoPolicySet`].
//!
//! # Ubiquitous language
//!
//! - **Origin**: RFC 6454 web origin, canonicalised as
//!   `scheme://host[:port]` with default ports (80/443) elided.
//! - **OriginPattern**: a rule's declared origin list entry; exact
//!   origin, wildcard subdomain (`https://*.example.org`), or global
//!   wildcard (`*`). Global wildcard disables the gate for that rule.
//! - **Origin gate**: the additional check that runs after agent matching.

use std::collections::HashSet;

use url::Url;

use crate::wac::AclAuthorization;

// ---------------------------------------------------------------------------
// Origin — canonicalised `scheme://host[:port]`
// ---------------------------------------------------------------------------

/// Canonicalised web origin per RFC 6454.
///
/// The internal representation is a lowercased, default-port-stripped
/// serialisation of the form `scheme://host` or `scheme://host:port`.
/// Paths, queries and fragments are discarded; only the tuple
/// `(scheme, host, port)` is preserved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Origin(String);

impl Origin {
    /// Parse a string into a canonical [`Origin`].
    ///
    /// Accepts raw origin forms (`https://example.org`,
    /// `https://example.org:8443`) as well as full URLs; in the latter
    /// case path/query/fragment are discarded.
    ///
    /// Returns `None` if the input is not a parseable URL, has no host,
    /// or uses a scheme without a hierarchical origin (e.g. `data:`).
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        let url = Url::parse(trimmed).ok()?;
        Self::from_url(&url)
    }

    /// Extract a canonical [`Origin`] from a parsed URL.
    ///
    /// Returns `None` for opaque-origin schemes (schemes without a host
    /// such as `data:`, `javascript:`, `file:` without host).
    pub fn from_url(url: &Url) -> Option<Self> {
        let scheme = url.scheme().to_ascii_lowercase();
        let host = url.host_str()?.to_ascii_lowercase();
        let port = url.port(); // None when the URL uses the default port
        let serialised = match port {
            None => format!("{scheme}://{host}"),
            Some(p) => format!("{scheme}://{host}:{p}"),
        };
        Some(Origin(serialised))
    }

    /// Canonical serialised form (e.g. `https://example.org`,
    /// `https://example.org:8443`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// OriginPattern — exact / wildcard-subdomain / global wildcard
// ---------------------------------------------------------------------------

/// A rule-declared origin pattern.
///
/// Three forms are supported:
///
/// - **Exact**: `https://example.org` — matches only that origin.
/// - **Wildcard subdomain**: `https://*.example.org` — matches any
///   single-or-multi-level subdomain of `example.org` on the same
///   scheme. Does **not** match the bare `example.org`.
/// - **Global wildcard**: `*` — matches any origin. Equivalent to
///   "origin gate effectively disabled for this rule". Discouraged;
///   requires explicit opt-in (the caller must write `*` literally).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginPattern {
    /// Exact origin match.
    Exact(Origin),
    /// Wildcard subdomain: scheme + suffix (e.g. `https` + `example.org`).
    Wildcard { scheme: String, suffix: String },
    /// Global wildcard (`*`). Matches any origin.
    Any,
}

impl OriginPattern {
    /// Parse a pattern string.
    ///
    /// - `"*"` → [`OriginPattern::Any`]
    /// - `"https://*.example.org"` → [`OriginPattern::Wildcard`]
    /// - `"https://example.org"` / `"https://example.org:8443"` →
    ///   [`OriginPattern::Exact`]
    ///
    /// Returns `None` for malformed input (missing scheme, empty host,
    /// trailing slashes, etc). The invariant in the DDD doc is that
    /// only canonical origins are stored; the exact-origin branch uses
    /// [`Origin::parse`] for strict validation.
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed == "*" {
            return Some(OriginPattern::Any);
        }
        // Wildcard subdomain: scheme://*.suffix[:port]
        if let Some(rest) = trimmed.strip_prefix("https://*.") {
            return Self::parse_wildcard("https", rest);
        }
        if let Some(rest) = trimmed.strip_prefix("http://*.") {
            return Self::parse_wildcard("http", rest);
        }
        // Exact origin — must round-trip through Origin::parse.
        // Reject trailing slashes to match the DDD invariant
        // "canonical scheme://host[:port]" only.
        if trimmed.ends_with('/') {
            return None;
        }
        let origin = Origin::parse(trimmed)?;
        // Reject if Origin::parse canonicalised away user-supplied
        // content (e.g. user supplied a path we quietly stripped).
        let lc = trimmed.to_ascii_lowercase();
        if origin.as_str() != lc {
            return None;
        }
        Some(OriginPattern::Exact(origin))
    }

    fn parse_wildcard(scheme: &str, suffix_part: &str) -> Option<Self> {
        // suffix_part is e.g. "example.org" or "example.org:8443".
        if suffix_part.is_empty() {
            return None;
        }
        // Basic host-shape validation: reject whitespace, empty labels,
        // stray wildcard characters in the suffix itself.
        if suffix_part.contains(char::is_whitespace) || suffix_part.contains('*') {
            return None;
        }
        // Reject trailing slash / path segments.
        if suffix_part.contains('/') {
            return None;
        }
        Some(OriginPattern::Wildcard {
            scheme: scheme.to_string(),
            suffix: suffix_part.to_ascii_lowercase(),
        })
    }

    /// Test whether a request origin matches this pattern.
    pub fn matches(&self, origin: &Origin) -> bool {
        match self {
            OriginPattern::Any => true,
            OriginPattern::Exact(expected) => expected == origin,
            OriginPattern::Wildcard { scheme, suffix } => {
                // Rebuild expected tuple from the request origin.
                let serialised = origin.as_str();
                let (req_scheme, req_rest) = match serialised.split_once("://") {
                    Some(v) => v,
                    None => return false,
                };
                if req_scheme != scheme {
                    return false;
                }
                // req_rest is host[:port]; match if host ends with
                // ".{suffix}" with at least one non-empty label in
                // front. Ports are not part of the "which suffix"
                // question so they are stripped before comparison.
                let req_host = match req_rest.split_once(':') {
                    Some((h, _)) => h,
                    None => req_rest,
                };
                let pattern_suffix = match suffix.split_once(':') {
                    Some((h, _)) => h,
                    None => suffix.as_str(),
                };
                let needle = format!(".{pattern_suffix}");
                req_host.ends_with(&needle) && req_host.len() > needle.len()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extraction from ACL rules + aggregate-level decision
// ---------------------------------------------------------------------------

/// Extract `acl:origin` patterns from a single authorisation.
///
/// Malformed entries are silently dropped (matches the forgiving
/// posture of [`super::parse_turtle_acl`]); strict validation happens
/// at write time, not read time.
pub fn extract_origin_patterns(auth: &AclAuthorization) -> Vec<OriginPattern> {
    let mut out = Vec::new();
    if let Some(ids) = &auth.origin {
        for id in iter_ids(ids) {
            if let Some(p) = OriginPattern::parse(id) {
                out.push(p);
            }
        }
    }
    out
}

fn iter_ids(ids: &crate::wac::IdOrIds) -> Vec<&str> {
    match ids {
        crate::wac::IdOrIds::Single(r) => vec![r.id.as_str()],
        crate::wac::IdOrIds::Multiple(v) => v.iter().map(|r| r.id.as_str()).collect(),
    }
}

/// Origin-gate decision for a request against an ACL document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginDecision {
    /// No authorisation in the ACL declares `acl:origin`. Permissive:
    /// backward-compatible with pre-F4 ACLs.
    NoPolicySet,
    /// Request origin matches at least one authorisation's pattern.
    Permitted,
    /// Policies exist and the request origin does not match any of them.
    RejectedMismatch,
    /// Policies exist and the request carries no `Origin` header.
    RejectedNoOrigin,
}

/// Check whether the request origin is permitted by any authorisation's
/// origin patterns in the supplied ACL document.
///
/// Semantics:
///
/// - If no authorisation declares `acl:origin` → [`OriginDecision::NoPolicySet`]
///   (gate inactive; backward compatible).
/// - If any authorisation declares patterns and the request origin
///   matches at least one → [`OriginDecision::Permitted`].
/// - If patterns exist but the request origin doesn't match any →
///   [`OriginDecision::RejectedMismatch`].
/// - If patterns exist and no `Origin` header was supplied →
///   [`OriginDecision::RejectedNoOrigin`].
///
/// This check is ACL-document-wide, matching the doc's integration
/// semantics for v0.4.0; rule-level short-circuiting is performed by
/// the evaluator when combined with agent matching.
pub fn check_origin(
    acl: &crate::wac::AclDocument,
    request_origin: Option<&Origin>,
) -> OriginDecision {
    let graph = match acl.graph.as_ref() {
        Some(g) => g,
        None => return OriginDecision::NoPolicySet,
    };
    let mut any_patterns = false;
    let mut matched = false;
    // Deduplicate patterns via a HashSet of their canonical string forms
    // to avoid quadratic work when the same origin is repeated across
    // many rules. Using Vec would also work; HashSet documents intent.
    let mut seen: HashSet<String> = HashSet::new();
    for auth in graph {
        for pattern in extract_origin_patterns(auth) {
            let key = pattern_key(&pattern);
            if !seen.insert(key) {
                continue;
            }
            any_patterns = true;
            if let Some(req) = request_origin {
                if pattern.matches(req) {
                    matched = true;
                }
            }
        }
    }
    if !any_patterns {
        OriginDecision::NoPolicySet
    } else if matched {
        OriginDecision::Permitted
    } else if request_origin.is_none() {
        OriginDecision::RejectedNoOrigin
    } else {
        OriginDecision::RejectedMismatch
    }
}

fn pattern_key(p: &OriginPattern) -> String {
    match p {
        OriginPattern::Any => "*".to_string(),
        OriginPattern::Exact(o) => format!("exact:{}", o.as_str()),
        OriginPattern::Wildcard { scheme, suffix } => {
            format!("wild:{scheme}://*.{suffix}")
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (module-local; broader integration lives in tests/acl_origin_test.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_parse_strips_default_https_port() {
        let o = Origin::parse("https://example.org:443/foo").unwrap();
        assert_eq!(o.as_str(), "https://example.org");
    }

    #[test]
    fn origin_parse_preserves_non_default_port() {
        let o = Origin::parse("https://example.org:8443/foo").unwrap();
        assert_eq!(o.as_str(), "https://example.org:8443");
    }

    #[test]
    fn origin_parse_lowercases_host_and_scheme() {
        let o = Origin::parse("HTTPS://Example.ORG").unwrap();
        assert_eq!(o.as_str(), "https://example.org");
    }

    #[test]
    fn origin_parse_rejects_empty_and_opaque() {
        assert!(Origin::parse("").is_none());
        assert!(Origin::parse("not a url").is_none());
        assert!(Origin::parse("data:text/plain,hello").is_none());
    }

    #[test]
    fn pattern_any_matches_everything() {
        let any = OriginPattern::parse("*").unwrap();
        assert!(any.matches(&Origin::parse("https://example.org").unwrap()));
        assert!(any.matches(&Origin::parse("http://foo.test:9000").unwrap()));
    }

    #[test]
    fn pattern_exact_requires_canonical_input() {
        assert!(OriginPattern::parse("https://example.org/").is_none());
        let p = OriginPattern::parse("https://example.org").unwrap();
        match p {
            OriginPattern::Exact(o) => assert_eq!(o.as_str(), "https://example.org"),
            _ => panic!("expected Exact"),
        }
    }

    #[test]
    fn pattern_wildcard_rejects_bare_apex() {
        let p = OriginPattern::parse("https://*.example.org").unwrap();
        assert!(!p.matches(&Origin::parse("https://example.org").unwrap()));
        assert!(p.matches(&Origin::parse("https://app.example.org").unwrap()));
        assert!(p.matches(&Origin::parse("https://a.b.example.org:8443").unwrap()));
    }
}
