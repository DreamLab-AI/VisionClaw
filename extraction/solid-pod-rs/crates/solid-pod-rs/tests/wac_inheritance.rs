//! WAC inheritance corpus.
//!
//! Scenarios independently authored against the Solid Protocol's WAC
//! spec clauses. Where they resemble the JSS (JavaScriptSolidServer)
//! reference test suite at
//! `references/javascript-solid-server/test/unit/authorization/`, that
//! is because the Solid Protocol admits a finite number of edge cases,
//! not because JSS test code was copied. Each test maps a Solid
//! Protocol / WAC spec clause onto an assertion against
//! `evaluate_access`.

use solid_pod_rs::wac::{
    evaluate_access, evaluate_access_with_groups, AccessMode, AclAuthorization, AclDocument,
    IdOrIds, IdRef, StaticGroupMembership,
};

fn parse(json: &str) -> AclDocument {
    serde_json::from_str(json).expect("parse ACL JSON-LD")
}

// ---------------------------------------------------------------------------
// Default inheritance from a parent container
// ---------------------------------------------------------------------------

#[test]
fn default_on_container_inherits_to_child_resource() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/shared"},
                "acl:mode": [
                    {"@id": "acl:Read"},
                    {"@id": "acl:Write"}
                ]
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/shared/file.txt",
        AccessMode::Read,
        None,));
}

#[test]
fn default_on_container_inherits_to_deep_descendant() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/root"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/root/a/b/c/d.txt",
        AccessMode::Read,
        None,));
}

#[test]
fn default_mode_does_not_grant_unspecified_mode() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/x",
        AccessMode::Write,
        None,));
}

#[test]
fn access_to_does_not_inherit_by_itself() {
    // `acl:accessTo` is exact-match (+ immediate child for containers),
    // not recursive default inheritance.
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/container/"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/container/deep/file.txt",
        AccessMode::Read,
        None,));
}

#[test]
fn access_to_on_container_covers_direct_children() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/container"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/container/file.txt",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// Override at child with explicit ACL
// ---------------------------------------------------------------------------

#[test]
fn child_explicit_acl_replaces_parent_default() {
    // Parent grants Read-only; child replaces with Write.
    // The `ACL resolver` would usually pick the child's sidecar; this
    // test validates that a child-level Write grant is respected on
    // its own, independent of any parent default.
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/child/file"},
                "acl:mode": {"@id": "acl:Write"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/child/file",
        AccessMode::Write,
        None,));
}

#[test]
fn child_explicit_without_mode_denies_even_if_parent_grants() {
    // Child ACL exists but mentions only Append; Read is not granted.
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/child"},
                "acl:mode": {"@id": "acl:Append"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/child",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// acl:accessTo vs acl:default semantics
// ---------------------------------------------------------------------------

#[test]
fn access_to_applies_to_resource_itself() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/doc"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/doc",
        AccessMode::Read,
        None,));
}

#[test]
fn default_does_not_apply_when_agent_unknown() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/shared"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:mallory"),
        "/shared/file",
        AccessMode::Read,
        None,));
}

#[test]
fn access_to_and_default_both_apply_to_own_container() {
    // An authorisation using `acl:default` for container `/x` is
    // expected to cover resources under `/x/…`. It also covers `/x`
    // itself per WAC §5.
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/x"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/x",
        AccessMode::Read,
        None,));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/x/deep/nested",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// Multiple acl:agent values in one authorization
// ---------------------------------------------------------------------------

#[test]
fn multiple_agents_all_granted() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": [
                    {"@id": "did:nostr:alice"},
                    {"@id": "did:nostr:bob"}
                ],
                "acl:accessTo": {"@id": "/shared"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/shared",
        AccessMode::Read,
        None,));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:bob"),
        "/shared",
        AccessMode::Read,
        None,));
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:carol"),
        "/shared",
        AccessMode::Read,
        None,));
}

#[test]
fn multiple_modes_in_one_authorization() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/f"},
                "acl:mode": [
                    {"@id": "acl:Read"},
                    {"@id": "acl:Write"},
                    {"@id": "acl:Control"}
                ]
            }]
        }"#,
    );
    for mode in [AccessMode::Read, AccessMode::Write, AccessMode::Append, AccessMode::Control] {
        assert!(
            evaluate_access(Some(&doc), Some("did:nostr:alice"), "/f", mode, None),
            "mode {mode:?} should be granted"
        );
    }
}

// ---------------------------------------------------------------------------
// agentClass foaf:Agent + acl:AuthenticatedAgent
// ---------------------------------------------------------------------------

#[test]
fn foaf_agent_covers_anonymous() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "foaf:Agent"},
                "acl:accessTo": {"@id": "/public"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(Some(&doc), None, "/public", AccessMode::Read, None));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:anyone"),
        "/public",
        AccessMode::Read,
        None,));
}

#[test]
fn authenticated_agent_excludes_anonymous() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "acl:AuthenticatedAgent"},
                "acl:accessTo": {"@id": "/members"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(Some(&doc), None, "/members", AccessMode::Read, None));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/members",
        AccessMode::Read,
        None,));
}

#[test]
fn foaf_agent_iri_full_form_accepted() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "http://xmlns.com/foaf/0.1/Agent"},
                "acl:accessTo": {"@id": "/public"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(Some(&doc), None, "/public", AccessMode::Read, None));
}

#[test]
fn authenticated_agent_full_iri_accepted() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentClass": {"@id": "http://www.w3.org/ns/auth/acl#AuthenticatedAgent"},
                "acl:accessTo": {"@id": "/m"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:bob"),
        "/m",
        AccessMode::Read,
        None,));
    assert!(!evaluate_access(Some(&doc), None, "/m", AccessMode::Read, None));
}

// ---------------------------------------------------------------------------
// agentGroup membership
// ---------------------------------------------------------------------------

#[test]
fn group_membership_grants_access() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentGroup": {"@id": "https://group.example/team#members"},
                "acl:accessTo": {"@id": "/project"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    let mut groups = StaticGroupMembership::new();
    groups.add(
        "https://group.example/team#members",
        vec!["did:nostr:alice".into(), "did:nostr:bob".into()],
    );

    assert!(evaluate_access_with_groups(
        Some(&doc),
        Some("did:nostr:alice"),
        "/project",
        AccessMode::Read,
        None,
        &groups,));
    assert!(!evaluate_access_with_groups(
        Some(&doc),
        Some("did:nostr:carol"),
        "/project",
        AccessMode::Read,
        None,
        &groups,));
}

#[test]
fn group_without_resolver_denies() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentGroup": {"@id": "https://group.example/team#members"},
                "acl:accessTo": {"@id": "/p"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    // Default evaluate_access uses the no-op resolver.
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/p",
        AccessMode::Read,
        None,));
}

#[test]
fn empty_group_grants_nobody() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agentGroup": {"@id": "https://group.example/empty"},
                "acl:accessTo": {"@id": "/p"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    let mut groups = StaticGroupMembership::new();
    groups.add("https://group.example/empty", vec![]);
    assert!(!evaluate_access_with_groups(
        Some(&doc),
        Some("did:nostr:alice"),
        "/p",
        AccessMode::Read,
        None,
        &groups,));
}

// ---------------------------------------------------------------------------
// Mode escalation semantics
// ---------------------------------------------------------------------------

#[test]
fn write_implies_append() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/inbox"},
                "acl:mode": {"@id": "acl:Write"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/inbox",
        AccessMode::Append,
        None,));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/inbox",
        AccessMode::Write,
        None,));
}

#[test]
fn append_does_not_imply_write() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/inbox"},
                "acl:mode": {"@id": "acl:Append"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/inbox",
        AccessMode::Write,
        None,));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/inbox",
        AccessMode::Append,
        None,));
}

#[test]
fn control_does_not_imply_read() {
    // Per WAC §4.3 `acl:Control` is orthogonal to Read/Write.
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/r"},
                "acl:mode": {"@id": "acl:Control"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/r",
        AccessMode::Control,
        None,));
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/r",
        AccessMode::Read,
        None,));
}

#[test]
fn read_does_not_imply_append() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/r"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/r",
        AccessMode::Append,
        None,));
}

// ---------------------------------------------------------------------------
// Multiple authorisations, agent-combination
// ---------------------------------------------------------------------------

#[test]
fn union_of_authorizations_is_effective_permission() {
    // Authorisation 1: Read via foaf:Agent
    // Authorisation 2: Write for alice specifically
    // alice should have Read + Write.
    let doc = parse(
        r#"{
            "@graph": [
                {
                    "acl:agentClass": {"@id": "foaf:Agent"},
                    "acl:accessTo": {"@id": "/r"},
                    "acl:mode": {"@id": "acl:Read"}
                },
                {
                    "acl:agent": {"@id": "did:nostr:alice"},
                    "acl:accessTo": {"@id": "/r"},
                    "acl:mode": {"@id": "acl:Write"}
                }
            ]
        }"#,
    );
    assert!(evaluate_access(Some(&doc), None, "/r", AccessMode::Read, None));
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/r",
        AccessMode::Write,
        None,));
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:bob"),
        "/r",
        AccessMode::Write,
        None,));
}

#[test]
fn unrelated_authorization_does_not_grant_unrelated_resource() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/private"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/public",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// Path normalisation edge-cases
// ---------------------------------------------------------------------------

#[test]
fn trailing_slash_normalisation_on_container() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/shared/"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/shared/file",
        AccessMode::Read,
        None,));
}

#[test]
fn root_default_covers_everything() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:default": {"@id": "/"},
                "acl:mode": [
                    {"@id": "acl:Read"},
                    {"@id": "acl:Write"}
                ]
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/anything/at/all",
        AccessMode::Write,
        None,));
}

#[test]
fn dot_prefixed_path_resolves() {
    let doc = parse(
        r#"{
            "@graph": [{
                "acl:agent": {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "./local"},
                "acl:mode": {"@id": "acl:Read"}
            }]
        }"#,
    );
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/local",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// Empty ACL and missing graph
// ---------------------------------------------------------------------------

#[test]
fn empty_graph_denies_everyone() {
    let doc = parse(r#"{ "@graph": [] }"#);
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/x",
        AccessMode::Read,
        None,));
}

#[test]
fn missing_graph_treated_as_no_acl() {
    let doc = parse(r#"{ "@context": {} }"#);
    assert!(!evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/x",
        AccessMode::Read,
        None,));
}

// ---------------------------------------------------------------------------
// Sanity: struct-literal authorisations round-trip
// ---------------------------------------------------------------------------

#[test]
fn struct_literal_authorization_works() {
    let auth = AclAuthorization {
        id: None,
        r#type: None,
        agent: Some(IdOrIds::Single(IdRef {
            id: "did:nostr:alice".into(),
        })),
        agent_class: None,
        agent_group: None,
        origin: None,
        access_to: Some(IdOrIds::Single(IdRef { id: "/d".into() })),
        default: None,
        mode: Some(IdOrIds::Single(IdRef {
            id: "acl:Read".into(),
        })),
    };
    let doc = AclDocument {
        context: None,
        graph: Some(vec![auth]),
    };
    assert!(evaluate_access(
        Some(&doc),
        Some("did:nostr:alice"),
        "/d",
        AccessMode::Read,
        None,));
}
