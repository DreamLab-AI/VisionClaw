//! WAC evaluator performance benchmarks.
//!
//! Three scenarios exercise the hot path (`evaluate_access` /
//! `evaluate_access_with_groups`):
//!
//! 1. **Simple authorisation** — a single authorisation rule granting
//!    one agent one mode on one resource.
//! 2. **Inherited authorisation** — a resource 10 containers deep
//!    inherits its ACL from the root via `acl:default`.
//! 3. **Group membership** — an authorisation gated on
//!    `acl:agentGroup`, resolved against a 1000-member in-memory
//!    `StaticGroupMembership`.
//!
//! Run with:
//! ```bash
//! cargo bench -p solid-pod-rs --bench wac_eval_bench
//! ```
//!
//! Targets (release, x86_64):
//!
//! | Scenario              | Expected |
//! |-----------------------|----------|
//! | Simple (1 rule)       | ~1-3 µs  |
//! | 10-deep inherited     | ~2-5 µs  |
//! | Group (1000 members)  | ~3-8 µs  |

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use solid_pod_rs::wac::{
    evaluate_access, evaluate_access_with_groups, AccessMode, AclDocument,
    StaticGroupMembership,
};

fn simple_doc() -> AclDocument {
    serde_json::from_str(
        r##"{
            "@context": {"acl": "http://www.w3.org/ns/auth/acl#"},
            "@graph": [{
                "@id": "#rule-1",
                "acl:agent":    {"@id": "did:nostr:alice"},
                "acl:accessTo": {"@id": "/private/doc"},
                "acl:mode":     {"@id": "acl:Read"}
            }]
        }"##,
    )
    .unwrap()
}

fn inherited_doc() -> AclDocument {
    // Single default rule at the root — the evaluator must match a
    // 10-segment path against the default IRI.
    serde_json::from_str(
        r##"{
            "@graph": [{
                "@id": "#root-default",
                "acl:agentClass": {"@id": "foaf:Agent"},
                "acl:default":    {"@id": "/"},
                "acl:mode":       {"@id": "acl:Read"}
            }]
        }"##,
    )
    .unwrap()
}

fn group_doc() -> AclDocument {
    serde_json::from_str(
        r##"{
            "@graph": [{
                "@id": "#team",
                "acl:agentGroup": {"@id": "https://pod.example/groups/team#members"},
                "acl:accessTo":   {"@id": "/team/roadmap"},
                "acl:mode":       {"@id": "acl:Read"}
            }]
        }"##,
    )
    .unwrap()
}

fn bench_simple(c: &mut Criterion) {
    let doc = simple_doc();
    c.bench_function("wac_simple_1_rule", |b| {
        b.iter(|| {
            let allowed = evaluate_access(
                black_box(Some(&doc)),
                black_box(Some("did:nostr:alice")),
                black_box("/private/doc"),
                black_box(AccessMode::Read),
                None,);
            debug_assert!(allowed);
            black_box(allowed);
        });
    });
}

fn bench_inherited(c: &mut Criterion) {
    let doc = inherited_doc();
    let deep_path =
        "/a/b/c/d/e/f/g/h/i/j/leaf";
    c.bench_function("wac_inherited_10_deep", |b| {
        b.iter(|| {
            let allowed = evaluate_access(
                black_box(Some(&doc)),
                black_box(None),
                black_box(deep_path),
                black_box(AccessMode::Read),
                None,);
            debug_assert!(allowed);
            black_box(allowed);
        });
    });
}

fn bench_group(c: &mut Criterion) {
    let doc = group_doc();
    let mut membership = StaticGroupMembership::new();
    let members: Vec<String> = (0..1000)
        .map(|i| format!("did:nostr:member-{i:04}"))
        .collect();
    membership.add(
        "https://pod.example/groups/team#members",
        members.clone(),
    );
    // A member near the end of the list — worst-case linear scan.
    let agent = "did:nostr:member-0999";

    c.bench_function("wac_group_1k_members", |b| {
        b.iter(|| {
            let allowed = evaluate_access_with_groups(
                black_box(Some(&doc)),
                black_box(Some(agent)),
                black_box("/team/roadmap"),
                black_box(AccessMode::Read),
                None,
                black_box(&membership),);
            debug_assert!(allowed);
            black_box(allowed);
        });
    });
}

criterion_group!(benches, bench_simple, bench_inherited, bench_group);
criterion_main!(benches);
