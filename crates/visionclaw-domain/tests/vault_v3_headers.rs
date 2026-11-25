//! Vocabulary v3 page headers through the domain parser (ADR-VG-010..013).
//!
//! The vault is being migrated from vocabulary 2 to 3 after this ships, so
//! every reader must give the same answer for a page before and after the
//! migration: `public: true` and `visibility: public` are the same fact, and
//! dropping `public` from a migrated page must not unpublish it.

use visionclaw_domain::vault::{parse, render_page};

const V2: &str = "---\ntype: Note\npublic: true\nstatus: stable\n---\nbody\n";
const V3: &str = "---\ntype: Note\nvisibility: public\nstatus: stable\n---\nbody\n";
const V3_SHARED: &str = "---\ntype: Note\nvisibility: shared\nstatus: stable\n---\nbody\n";

#[test]
fn a_migrated_page_stays_published_and_in_the_graph() {
    let (before, after) = (parse(V2), parse(V3));
    assert!(before.is_publishable() && after.is_publishable());
    assert!(before.is_kg_included() && after.is_kg_included());
    assert_eq!(before.inclusion_reason(), after.inclusion_reason());
}

#[test]
fn shared_is_not_public_and_a_missing_key_is_not_public() {
    assert!(!parse(V3_SHARED).is_kg_included());
    assert!(!parse("---\ntype: Note\n---\n").is_kg_included());
}

#[test]
fn an_unknown_visibility_is_not_public() {
    for v in ["Public", "world", "true", "1"] {
        let page = format!("---\ntype: Note\nvisibility: {v}\n---\n");
        assert!(!parse(&page).is_kg_included(), "{v}");
    }
}

#[test]
fn visibility_decides_where_the_keys_disagree_and_the_problem_is_reported() {
    let meta = parse("---\ntype: Note\npublic: true\nvisibility: shared\n---\n");
    assert!(!meta.is_kg_included());
    assert!(meta.visibility_problem.is_some());
    let meta = parse("---\ntype: Note\npublic: false\nvisibility: public\n---\n");
    assert!(meta.is_kg_included(), "ontomark publishes by visibility");
    assert!(meta.visibility_problem.is_some());
}

#[test]
fn rendering_a_v3_page_does_not_bring_public_back() {
    let meta = parse(V3);
    let out = render_page(&meta, "body\n");
    assert!(out.contains("visibility: public"), "{out}");
    assert!(!out.contains("public: "), "{out}");
    assert!(parse(&out).is_kg_included());
}

// ---- stamps, sources and the rest of a v3 header survive a round trip --------

const B41: &str = "b41654017f6850b13857d19d8ae0e3f88f1365600cab0321e8101c9e92682f7a";

fn v3_header() -> String {
    format!(
        "---\ntype: Brief\nstatus: draft\nvisibility: public\npage_resource: urn:visionflow:page:abc\n\
valid-from: '2026-11-01T00:00:00Z'\n\
generated:\n  by: did:nostr:{B41}\n  via: process:podcast-promote/1.0\n  at: '2026-11-01T00:00:00Z'\n\
verified:\n  - by: did:nostr:{B41}\n    at: '2026-11-02T00:00:00Z'\n    scope: body\n\
supersedes:\n  - resource: '[[Old]]'\n    reason: corrected\n    at: '2026-11-03T00:00:00Z'\n\
sources:\n  - id: origin\n    resource: '[[Ep]]'\n    rel: origin\n  - id: d\n    resource: '[[B]]'\n    rel: derived-from\n    locator: 'sha256-v1:{h}'\n\
statements:\n  - predicate: requires\n    object: '[[X]]'\n---\nbody\n",
        h = "0123456789abcdef".repeat(4)
    )
}

#[test]
fn a_v3_header_round_trips_with_nothing_lost() {
    let src = v3_header();
    let meta = parse(&src);
    assert!(meta.stamp_problems.is_empty());
    let out = render_page(&meta, "body\n");
    let (a, b) = (parse(&src).raw.unwrap(), parse(&out).raw.unwrap());
    for key in a.keys() {
        assert_eq!(
            a.get(key),
            b.get(key),
            "{key:?} changed in the round trip\n{out}"
        );
    }
    assert_eq!(a.len(), b.len(), "{out}");
    assert!(!out.contains("public:"), "{out}");
}

#[test]
fn an_unreadable_stamp_is_reported_and_written_back_as_authored() {
    let src = "---\ntype: Note\nvisibility: shared\ngenerated:\n  by: 'did:nostr:not-hex'\n  at: '2026-11-01T00:00:00Z'\nverified:\n  - by: who:knows\n    at: '2026-11-02T00:00:00Z'\n---\n";
    let meta = parse(src);
    let fields: Vec<&str> = meta
        .stamp_problems
        .iter()
        .map(|p| p.field.as_str())
        .collect();
    assert_eq!(fields, ["generated", "verified[0]"]);
    let out = render_page(&meta, "");
    assert!(
        out.contains("did:nostr:not-hex") && out.contains("who:knows"),
        "{out}"
    );
}

#[test]
fn a_page_built_in_code_still_writes_public() {
    let meta = visionclaw_domain::vault::PageMeta {
        public: true,
        ..Default::default()
    };
    assert!(render_page(&meta, "").contains("public: true"));
}
