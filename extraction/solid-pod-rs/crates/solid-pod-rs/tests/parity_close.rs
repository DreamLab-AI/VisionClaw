//! Sprint 3 parity-close integration tests.
//!
//! Covers the 19 rows flipped from partial/missing to present:
//!
//! - Turtle-serialized ACL documents (parse + resolver fallback).
//! - If-Match / If-None-Match conditional requests.
//! - Range requests on binary resources.
//! - JSON Patch (RFC 6902).
//! - OPTIONS surfacing Allow / Accept-Patch / Accept-Ranges.
//! - WebID-OIDC discovery (`solid:oidcIssuer`).
//! - `.well-known/solid` discovery doc.
//! - WebFinger integration.
//! - NIP-05 verification.
//! - Provisioning (`.provision` endpoint) + admin override.
//! - Quota enforcement.
//! - Dev-mode session bypass.

use bytes::Bytes;
use solid_pod_rs::{
    apply_json_patch, check_admin_override, dev_session, evaluate_preconditions,
    extract_oidc_issuer, generate_webid_html_with_issuer, nip05_document, options_for,
    parse_range_header, parse_turtle_acl, provision_pod, serialize_turtle_acl,
    slice_range, verify_nip05, webfinger_response, well_known_solid, AccessMode,
    ConditionalOutcome, ProvisionPlan, QuotaTracker,
};
use solid_pod_rs::storage::memory::MemoryBackend;
use solid_pod_rs::storage::Storage;
use solid_pod_rs::wac::{evaluate_access, AclResolver, StorageAclResolver};

// ---------------------------------------------------------------------------
// Turtle ACL
// ---------------------------------------------------------------------------

#[tokio::test]
async fn turtle_acl_resolver_reads_ttl_sidecar() {
    let pod = std::sync::Arc::new(MemoryBackend::new());
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .

        <#public> a acl:Authorization ;
            acl:agentClass foaf:Agent ;
            acl:accessTo </> ;
            acl:default </> ;
            acl:mode acl:Read .
    "#;
    pod.put(
        "/.acl",
        Bytes::copy_from_slice(ttl.as_bytes()),
        "text/turtle",
    )
    .await
    .unwrap();

    let resolver = StorageAclResolver::new(pod.clone());
    let doc = resolver.find_effective_acl("/foo").await.unwrap().unwrap();
    assert!(evaluate_access(Some(&doc), None, "/foo", AccessMode::Read, None));
}

#[test]
fn turtle_acl_round_trip_preserves_modes() {
    let ttl = r#"
        @prefix acl: <http://www.w3.org/ns/auth/acl#> .

        <#owner> a acl:Authorization ;
            acl:agent <did:nostr:owner> ;
            acl:accessTo </> ;
            acl:mode acl:Read, acl:Write, acl:Control .
    "#;
    let doc = parse_turtle_acl(ttl).unwrap();
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:owner"),
        "/",
        AccessMode::Write
    ,
        None));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:owner"),
        "/",
        AccessMode::Control
    ,
        None));

    // Re-serialise and re-parse: modes survive.
    let out = serialize_turtle_acl(&doc);
    let doc2 = parse_turtle_acl(&out).unwrap();
    assert!(evaluate_access(
        Some(&doc2),
        Some("did:nostr:owner"),
        "/",
        AccessMode::Write
    ,
        None));
}

// ---------------------------------------------------------------------------
// Conditional requests
// ---------------------------------------------------------------------------

#[test]
fn if_match_concurrent_update_fails_with_412() {
    let outcome = evaluate_preconditions("PUT", Some("current"), Some("\"stale\""), None);
    assert_eq!(outcome, ConditionalOutcome::PreconditionFailed);
}

#[test]
fn if_none_match_star_creates_only_when_absent() {
    let absent = evaluate_preconditions("PUT", None, None, Some("*"));
    assert_eq!(absent, ConditionalOutcome::Proceed);
    let exists = evaluate_preconditions("PUT", Some("e"), None, Some("*"));
    assert_eq!(exists, ConditionalOutcome::PreconditionFailed);
}

#[test]
fn if_none_match_on_get_304() {
    let outcome = evaluate_preconditions("GET", Some("e1"), None, Some("\"e1\""));
    assert_eq!(outcome, ConditionalOutcome::NotModified);
}

// ---------------------------------------------------------------------------
// Range requests
// ---------------------------------------------------------------------------

#[test]
fn range_slice_matches_parsed_bounds() {
    let body = b"0123456789";
    let r = parse_range_header(Some("bytes=2-5"), body.len() as u64)
        .unwrap()
        .unwrap();
    let slice = slice_range(body, r);
    assert_eq!(slice, b"2345");
}

#[test]
fn range_suffix_returns_tail() {
    let body = b"hello-world";
    let r = parse_range_header(Some("bytes=-5"), body.len() as u64)
        .unwrap()
        .unwrap();
    assert_eq!(slice_range(body, r), b"world");
}

#[test]
fn range_unsatisfiable_yields_error() {
    let err = parse_range_header(Some("bytes=999-1000"), 10);
    assert!(err.is_err());
}

// ---------------------------------------------------------------------------
// JSON Patch
// ---------------------------------------------------------------------------

#[test]
fn json_patch_move_op_reshapes_document() {
    let mut v = serde_json::json!({ "a": 1, "b": 2 });
    let patch = serde_json::json!([
        { "op": "move", "from": "/a", "path": "/c" }
    ]);
    apply_json_patch(&mut v, &patch).unwrap();
    assert!(v.get("a").is_none());
    assert_eq!(v["c"], 1);
}

#[test]
fn json_patch_copy_duplicates_value() {
    let mut v = serde_json::json!({ "src": { "n": 42 } });
    let patch = serde_json::json!([
        { "op": "copy", "from": "/src", "path": "/dest" }
    ]);
    apply_json_patch(&mut v, &patch).unwrap();
    assert_eq!(v["src"]["n"], 42);
    assert_eq!(v["dest"]["n"], 42);
}

#[test]
fn json_patch_array_append_with_dash() {
    let mut v = serde_json::json!({ "items": [1, 2] });
    let patch = serde_json::json!([
        { "op": "add", "path": "/items/-", "value": 3 }
    ]);
    apply_json_patch(&mut v, &patch).unwrap();
    assert_eq!(v["items"], serde_json::json!([1, 2, 3]));
}

// ---------------------------------------------------------------------------
// OPTIONS response
// ---------------------------------------------------------------------------

#[test]
fn options_advertises_accept_patch_and_ranges() {
    let r = options_for("/x");
    assert_eq!(r.accept_ranges, "bytes");
    assert!(r.accept_patch.contains("n3"));
    assert!(r.accept_patch.contains("sparql-update"));
    assert!(r.accept_patch.contains("json-patch"));
    assert!(r.allow.contains(&"OPTIONS"));
}

// ---------------------------------------------------------------------------
// WebID-OIDC discovery
// ---------------------------------------------------------------------------

#[test]
fn webid_with_issuer_round_trips_issuer() {
    let html = generate_webid_html_with_issuer(
        "abc",
        Some("Alice"),
        "https://pods.example.com",
        Some("https://op.example"),
    );
    let iss = extract_oidc_issuer(html.as_bytes()).unwrap();
    assert_eq!(iss.as_deref(), Some("https://op.example"));
}

// ---------------------------------------------------------------------------
// Discovery documents
// ---------------------------------------------------------------------------

#[test]
fn well_known_solid_embeds_storage_and_issuer() {
    let d = well_known_solid("https://pod.example/", "https://op.example");
    assert!(d.storage.ends_with('/'));
    assert_eq!(d.solid_oidc_issuer, "https://op.example");
    assert!(d.webfinger.is_some());
}

#[test]
fn webfinger_acct_lookup_returns_links() {
    let j = webfinger_response(
        "acct:alice@pod.example",
        "https://pod.example",
        "https://pod.example/profile/card#me",
    )
    .unwrap();
    let rels: Vec<_> = j.links.iter().map(|l| l.rel.as_str()).collect();
    assert!(rels.iter().any(|r| r == &"http://www.w3.org/ns/solid#webid"));
    assert!(rels.iter().any(|r| r == &"http://www.w3.org/ns/pim/space#storage"));
}

// ---------------------------------------------------------------------------
// NIP-05
// ---------------------------------------------------------------------------

#[test]
fn nip05_verify_happy_path() {
    let mut names = std::collections::HashMap::new();
    names.insert("alice".to_string(), "a".repeat(64));
    let doc = nip05_document(names);
    assert_eq!(verify_nip05("alice@p", &doc).unwrap(), "a".repeat(64));
}

// ---------------------------------------------------------------------------
// Quota
// ---------------------------------------------------------------------------

#[test]
fn quota_rejects_over_limit_writes() {
    let q = QuotaTracker::new(Some(1024));
    q.reserve(512).unwrap();
    q.reserve(400).unwrap();
    let err = q.reserve(200).unwrap_err();
    assert!(matches!(
        err,
        solid_pod_rs::PodError::PreconditionFailed(_)
    ));
}

// ---------------------------------------------------------------------------
// Provisioning + admin override + dev session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn provision_pod_creates_webid_and_containers() {
    let pod = MemoryBackend::new();
    let plan = ProvisionPlan {
        pubkey: "0123".into(),
        display_name: Some("Alice".into()),
        pod_base: "https://pod.example".into(),
        containers: vec!["/media/".into(), "/docs/".into()],
        root_acl: None,
        quota_bytes: Some(10_000),
    };
    let outcome = provision_pod(&pod, &plan).await.unwrap();
    assert!(outcome.webid.contains("/profile/card#me"));
    assert_eq!(outcome.quota_bytes, Some(10_000));
    // WebID profile exists.
    assert!(pod.exists("/profile/card").await.unwrap());
}

#[test]
fn admin_override_rejects_length_mismatch() {
    assert!(check_admin_override(Some("abc"), Some("abcd")).is_none());
    assert!(check_admin_override(Some("abcd"), Some("abcd")).is_some());
}

#[test]
fn dev_session_default_is_not_admin() {
    let s = dev_session("https://x/profile#me", false);
    assert!(!s.is_admin);
}
