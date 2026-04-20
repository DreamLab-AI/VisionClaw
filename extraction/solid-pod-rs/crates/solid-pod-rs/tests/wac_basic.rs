//! WAC smoke tests.

use solid_pod_rs::wac::{evaluate_access, AccessMode, AclDocument};

fn doc_from(json: &str) -> AclDocument {
    serde_json::from_str(json).expect("parse ACL JSON-LD")
}

#[test]
fn specific_agent_read_grants_that_agent() {
    let doc = doc_from(
        r##"{
            "@context": {"acl": "http://www.w3.org/ns/auth/acl#"},
            "@graph": [{
                "@id": "#alice",
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/private/note"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"##,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/private/note",
        AccessMode::Read,
        None,));
}

#[test]
fn agent_without_grant_is_denied() {
    let doc = doc_from(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/private/note"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:mallory"),
        "/private/note",
        AccessMode::Read,
        None,));
}

#[test]
fn public_foaf_agent_read_allows_anonymous() {
    let doc = doc_from(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "foaf:Agent"},
                "acl:accessTo": {"@id": "/public/index"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        None,
        "/public/index",
        AccessMode::Read,
        None,));
}

#[test]
fn container_default_inherits_to_children() {
    let doc = doc_from(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "foaf:Agent"},
                "acl:default": {"@id": "/public"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        None,
        "/public/file.txt",
        AccessMode::Read,
        None,));
    assert!(evaluate_access(
        Some(&doc),
        None,
        "/public/nested/deep.txt",
        AccessMode::Read,
        None,));
}

#[test]
fn write_implies_append() {
    let doc = doc_from(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:owner"},
                "acl:accessTo": {"@id": "/inbox/"},
                "acl:mode": {"@id": "acl:Write"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:owner"),
        "/inbox/",
        AccessMode::Append,
        None,));
}

#[test]
fn no_acl_denies_by_default() {
    assert!(!evaluate_access(None, None, "/whatever", AccessMode::Read, None));
    assert!(!evaluate_access(
        None,
        Some("did:nostr:anyone"),
        "/whatever",
        AccessMode::Write,
        None,));
}
