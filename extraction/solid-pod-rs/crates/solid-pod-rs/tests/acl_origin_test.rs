//! F4 — `acl:origin` enforcement (WAC §4.3).
//!
//! Closes the shared gap described in
//! `docs/design/jss-parity/03-wac-enforcement-context.md`. Each test
//! maps onto a single acceptance criterion F4a..F4k.

use solid_pod_rs::wac::{
    check_origin, evaluate_access, parse_turtle_acl, AccessMode, AclDocument, Origin,
    OriginDecision, OriginPattern,
};

// ---------------------------------------------------------------------------
// F4a / F4b — Origin parsing canonicalises default ports
// ---------------------------------------------------------------------------

#[test]
fn f4a_origin_strips_default_https_port() {
    let origin = Origin::parse("https://example.org:443/foo").expect("parse");
    assert_eq!(origin.as_str(), "https://example.org");
}

#[test]
fn f4a_origin_strips_default_http_port() {
    let origin = Origin::parse("http://example.org:80/foo").expect("parse");
    assert_eq!(origin.as_str(), "http://example.org");
}

#[test]
fn f4b_origin_preserves_non_default_port() {
    let origin = Origin::parse("https://example.org:8443/foo").expect("parse");
    assert_eq!(origin.as_str(), "https://example.org:8443");
}

// ---------------------------------------------------------------------------
// F4c / F4d / F4e — OriginPattern matching
// ---------------------------------------------------------------------------

#[test]
fn f4c_pattern_exact_match() {
    let pattern = OriginPattern::parse("https://example.org").expect("parse");
    let match_origin = Origin::parse("https://example.org").unwrap();
    let mismatch_origin = Origin::parse("https://evil.example").unwrap();
    assert!(pattern.matches(&match_origin));
    assert!(!pattern.matches(&mismatch_origin));
}

#[test]
fn f4c_pattern_exact_is_port_sensitive() {
    let pattern = OriginPattern::parse("https://example.org:8443").expect("parse");
    assert!(pattern.matches(&Origin::parse("https://example.org:8443").unwrap()));
    assert!(!pattern.matches(&Origin::parse("https://example.org").unwrap()));
}

#[test]
fn f4d_pattern_wildcard_subdomain_match() {
    let pattern = OriginPattern::parse("https://*.example.org").expect("parse");
    assert!(pattern.matches(&Origin::parse("https://app.example.org").unwrap()));
    assert!(pattern.matches(&Origin::parse("https://a.b.example.org").unwrap()));
    // Same suffix on a different scheme must not match.
    assert!(!pattern.matches(&Origin::parse("http://app.example.org").unwrap()));
    // Bare apex must not match a wildcard subdomain pattern.
    assert!(!pattern.matches(&Origin::parse("https://example.org").unwrap()));
}

#[test]
fn f4e_pattern_global_wildcard() {
    let pattern = OriginPattern::parse("*").expect("parse");
    assert!(matches!(pattern, OriginPattern::Any));
    assert!(pattern.matches(&Origin::parse("https://anything.test").unwrap()));
    assert!(pattern.matches(&Origin::parse("http://localhost:3000").unwrap()));
}

#[test]
fn f4e_pattern_rejects_trailing_slash() {
    // Invariant 5: canonical origins only — trailing slashes malformed.
    assert!(OriginPattern::parse("https://example.org/").is_none());
}

// ---------------------------------------------------------------------------
// F4f..F4i — check_origin over an AclDocument
// ---------------------------------------------------------------------------

fn doc_without_origin() -> AclDocument {
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .
        <#public> a acl:Authorization ;
            acl:agentClass foaf:Agent ;
            acl:accessTo </> ;
            acl:mode acl:Read .
    "#;
    parse_turtle_acl(ttl).unwrap()
}

fn doc_with_single_origin() -> AclDocument {
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        <#r> a acl:Authorization ;
            acl:agent <did:nostr:alice> ;
            acl:origin <https://app.example> ;
            acl:accessTo </data> ;
            acl:mode acl:Read .
    "#;
    parse_turtle_acl(ttl).unwrap()
}

#[test]
fn f4f_no_policies_yields_no_policy_set() {
    let doc = doc_without_origin();
    let decision = check_origin(&doc, None);
    assert_eq!(decision, OriginDecision::NoPolicySet);
    let decision = check_origin(&doc, Some(&Origin::parse("https://app.example").unwrap()));
    assert_eq!(decision, OriginDecision::NoPolicySet);
}

#[test]
fn f4g_matching_origin_permitted() {
    let doc = doc_with_single_origin();
    let origin = Origin::parse("https://app.example").unwrap();
    assert_eq!(check_origin(&doc, Some(&origin)), OriginDecision::Permitted);
}

#[test]
fn f4h_mismatched_origin_rejected() {
    let doc = doc_with_single_origin();
    let origin = Origin::parse("https://evil.example").unwrap();
    assert_eq!(
        check_origin(&doc, Some(&origin)),
        OriginDecision::RejectedMismatch,
    );
}

#[test]
fn f4i_no_origin_header_with_policies_rejected() {
    let doc = doc_with_single_origin();
    assert_eq!(check_origin(&doc, None), OriginDecision::RejectedNoOrigin);
}

// ---------------------------------------------------------------------------
// F4j — evaluate_access end-to-end with origin gate
// ---------------------------------------------------------------------------

#[cfg(feature = "acl-origin")]
#[test]
fn f4j_evaluate_access_denies_on_origin_mismatch() {
    let doc = doc_with_single_origin();
    let wrong = Origin::parse("https://evil.example").unwrap();
    let right = Origin::parse("https://app.example").unwrap();

    // Agent / mode / accessTo all match — only origin should matter.
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/data",
        AccessMode::Read,
        Some(&wrong),
    ));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/data",
        AccessMode::Read,
        Some(&right),
    ));
    // No Origin header + rule has origin patterns → deny.
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/data",
        AccessMode::Read,
        None,
    ));
}

#[cfg(feature = "acl-origin")]
#[test]
fn f4j_control_mode_bypasses_origin_gate() {
    // Invariant 4: Control mode bypasses the origin gate so owners can
    // always fix a misconfigured ACL.
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        <#r> a acl:Authorization ;
            acl:agent <did:nostr:owner> ;
            acl:origin <https://app.example> ;
            acl:accessTo </c> ;
            acl:mode acl:Control .
    "#;
    let doc = parse_turtle_acl(ttl).unwrap();
    let wrong = Origin::parse("https://evil.example").unwrap();
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:owner"),
        "/c",
        AccessMode::Control,
        Some(&wrong),
    ));
}

// ---------------------------------------------------------------------------
// F4k — Backward compatibility: ACLs without acl:origin behave as v0.3.x
// ---------------------------------------------------------------------------

#[test]
fn f4k_legacy_acl_without_origin_unchanged_by_feature_flag() {
    // Whether the feature is on or off, an ACL with zero `acl:origin`
    // triples must produce identical grants regardless of the origin
    // the caller supplies.
    let doc = doc_without_origin();
    let some_origin = Origin::parse("https://any.test").unwrap();

    assert!(evaluate_access(Some(&doc), None, "/", AccessMode::Read, None));
    assert!(evaluate_access(
        Some(&doc),
        None,
        "/",
        AccessMode::Read,
        Some(&some_origin),
    ));
    // Write still denied — origin gate must not accidentally widen permissions.
    assert!(!evaluate_access(
        Some(&doc),
        None,
        "/",
        AccessMode::Write,
        Some(&some_origin),
    ));
}

#[test]
fn f4k_from_url_extracts_origin() {
    let url = url::Url::parse("https://app.example:8443/path/to?x=1#frag").unwrap();
    let origin = Origin::from_url(&url).unwrap();
    assert_eq!(origin.as_str(), "https://app.example:8443");
}
