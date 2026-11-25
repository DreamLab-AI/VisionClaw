//! Vocabulary v3 page headers through the domain parser (ADR-VG-010..013).
//!
//! Which key publishes a page depends on the vault and the vocabulary version
//! (`Rules`), exactly as ontomark's `is_public_under` decides it:
//! `visibility: public` for a working page under vocabulary 3 or later,
//! `public: true` alone everywhere else. A page that is migrated from the first
//! form to the second must stay published, and a re-render must never resolve
//! a conflict by deleting a key.

use visionclaw_domain::vault::{parse, render_page, Rules};

const V2: &str = "---\ntype: Note\npublic: true\nstatus: stable\n---\nbody\n";
const V3: &str = "---\ntype: Note\nvisibility: public\nstatus: stable\n---\nbody\n";
const V3_SHARED: &str = "---\ntype: Note\nvisibility: shared\nstatus: stable\n---\nbody\n";
const WORKING_V2: Rules = Rules::PublicFlag;
const WORKING_V3: Rules = Rules::Visibility;

#[test]
fn a_migrated_page_stays_published_and_in_the_graph() {
    // Before the migration the vault is vocabulary 2; after it, 3.
    let (before, after) = (parse(V2, WORKING_V2), parse(V3, WORKING_V3));
    assert!(before.is_publishable() && after.is_publishable());
    assert!(before.is_kg_included() && after.is_kg_included());
    assert_eq!(before.inclusion_reason(), after.inclusion_reason());
}

#[test]
fn the_reviews_probe_pages_are_private_under_vocabulary_2() {
    // `visibility` is not a key of vocabulary 2: it must never publish a page.
    for page in [
        "---\ntype: Note\npublic: false\nvisibility: public\n---\n",
        "---\ntype: Note\nvisibility: public\n---\n",
        V3,
    ] {
        let meta = parse(page, WORKING_V2);
        assert!(!meta.is_kg_included(), "{page}");
        assert!(!meta.is_publishable(), "{page}");
    }
    // ...and a knowledge page ignores it under vocabulary 3 too.
    assert!(!parse(V3, Rules::for_path("knowledge/A.md", Some(3))).is_kg_included());
    assert!(!parse(V3, Rules::for_path("A.md", Some(3))).is_kg_included());
    assert!(parse(V3, Rules::for_path("working/A.md", Some(3))).is_kg_included());
}

#[test]
fn shared_is_not_public_and_a_missing_key_is_not_public() {
    assert!(!parse(V3_SHARED, WORKING_V3).is_kg_included());
    assert!(!parse("---\ntype: Note\n---\n", WORKING_V3).is_kg_included());
    // An unmigrated `public: true` is not public under vocabulary 3.
    assert!(!parse(V2, WORKING_V3).is_kg_included());
}

#[test]
fn an_unknown_visibility_is_not_public() {
    for v in ["Public", "world", "true", "1"] {
        let page = format!("---\ntype: Note\nvisibility: {v}\n---\n");
        assert!(!parse(&page, WORKING_V3).is_kg_included(), "{v}");
    }
}

#[test]
fn under_vocabulary_3_a_conflict_fails_closed_and_is_reported() {
    let meta = parse(
        "---\ntype: Note\npublic: true\nvisibility: shared\n---\n",
        WORKING_V3,
    );
    assert!(!meta.is_kg_included());
    assert!(meta.visibility_problem.is_some());
    let meta = parse(
        "---\ntype: Note\npublic: false\nvisibility: public\n---\n",
        WORKING_V3,
    );
    assert!(
        !meta.is_kg_included(),
        "fails closed, ontomark differs here"
    );
    assert!(!meta.is_publishable());
    assert!(meta.visibility_problem.is_some());
}

#[test]
fn explicit_public_false_is_only_the_public_key_under_vocabulary_2() {
    // ADR-2014's class-marker interaction: `visibility: shared` is not an
    // explicit `public: false` when `visibility` is not a key of the vault.
    let page = "---\nowl-class: mv:Foo\nvisibility: shared\n---\n";
    assert!(!parse(page, WORKING_V2).public_declared_false);
    assert!(parse(page, WORKING_V3).public_declared_false);
    let page = "---\nowl-class: mv:Foo\npublic: false\n---\n";
    assert!(parse(page, WORKING_V2).public_declared_false);
}

#[test]
fn a_migrated_page_with_a_class_marker_keeps_the_adr_2014_semantics() {
    // m0002 gives every non-public working page an explicit `visibility:
    // shared`. That is the same declaration as `public: false` (ADR-2014): the
    // formal class is still admitted to the graph (it is structural), is not
    // publishable, and is labelled as included despite the explicit "no".
    use visionclaw_domain::vault::InclusionReason::{FormalClass, FormalClassDespitePublicFalse};
    for visibility in ["shared", "private"] {
        let page = format!("---\ntype: Note\nowl-class: mv:Foo\nvisibility: {visibility}\n---\n");
        let migrated = parse(&page, WORKING_V3);
        assert!(
            migrated.is_kg_included() && !migrated.is_publishable(),
            "{visibility}"
        );
        assert_eq!(migrated.inclusion_reason(), FormalClassDespitePublicFalse);
        // Before migration the same intent was `public: false`: same answers.
        let before = parse(
            "---\ntype: Note\nowl-class: mv:Foo\npublic: false\n---\n",
            WORKING_V2,
        );
        assert_eq!(
            (
                before.is_kg_included(),
                before.is_publishable(),
                before.inclusion_reason()
            ),
            (true, false, FormalClassDespitePublicFalse)
        );
        // Under vocabulary 2 `visibility` is not a key: no explicit "no" there.
        let v2 = parse(&page, WORKING_V2);
        assert_eq!(v2.inclusion_reason(), FormalClass);
        assert!(v2.is_kg_included() && !v2.is_publishable());
    }
    // A migrated PUBLIC page with a class marker is public and formal.
    let page = "---\ntype: Note\nowl-class: mv:Foo\nvisibility: public\n---\n";
    assert!(parse(page, WORKING_V3).is_publishable());
}

#[test]
fn rendering_a_v3_page_does_not_bring_public_back() {
    let meta = parse(V3, WORKING_V3);
    let out = render_page(&meta, "body\n");
    assert!(out.contains("visibility: public"), "{out}");
    assert!(!out.contains("public: "), "{out}");
    assert!(parse(&out, WORKING_V3).is_kg_included());
}

fn keys_of(page: &str) -> (Option<serde_yaml::Value>, Option<serde_yaml::Value>) {
    let yaml = page.split("---\n").nth(1).unwrap();
    let map: serde_yaml::Mapping = serde_yaml::from_str(yaml).unwrap();
    let get = |k: &str| map.get(serde_yaml::Value::String(k.to_string())).cloned();
    (get("public"), get("visibility"))
}

#[test]
fn a_render_keeps_both_keys_of_a_conflicting_page() {
    use serde_yaml::Value::{Bool, String as S};
    for (src, public, visibility) in [
        // The direction that used to turn a private page into a public one.
        (
            "---\ntype: Note\npublic: false\nvisibility: public\n---\n",
            false,
            "public",
        ),
        (
            "---\ntype: Note\npublic: true\nvisibility: shared\n---\n",
            true,
            "shared",
        ),
        (
            "---\ntype: Note\npublic: true\nvisibility: private\n---\n",
            true,
            "private",
        ),
    ] {
        let meta = parse(src, WORKING_V3);
        assert!(meta.visibility_problem.is_some(), "{src}");
        let out = render_page(&meta, "body\n");
        let (p, v) = keys_of(&out);
        assert_eq!(
            (p, v),
            (Some(Bool(public)), Some(S(visibility.to_string()))),
            "{out}"
        );
        // The conflict is still there to be seen after the render.
        let again = parse(&out, WORKING_V3);
        assert_eq!(again.visibility_problem, meta.visibility_problem, "{out}");
        assert!(!again.is_kg_included(), "{out}");
    }
}

#[test]
fn a_render_never_derives_one_key_from_the_other() {
    use serde_yaml::Value::{Bool, String as S};
    // Under vocabulary 2, `visibility` is not a key: it is written back as
    // authored and never rewritten from `public`.
    let meta = parse(
        "---\ntype: Note\npublic: true\nvisibility: shared\n---\n",
        WORKING_V2,
    );
    let (p, v) = keys_of(&render_page(&meta, "x\n"));
    assert_eq!((p, v), (Some(Bool(true)), Some(S("shared".into()))));
    // `visibility` alone gains no `public`; `public` alone gains no `visibility`.
    let (p, v) = keys_of(&render_page(&parse(V3, WORKING_V3), "x\n"));
    assert_eq!((p, v), (None, Some(S("public".into()))));
    let (p, v) = keys_of(&render_page(&parse(V2, WORKING_V2), "x\n"));
    assert_eq!((p, v), (Some(Bool(true)), None));
    // A consistent pair keeps both keys.
    let both = "---\ntype: Note\npublic: true\nvisibility: public\n---\n";
    let (p, v) = keys_of(&render_page(&parse(both, WORKING_V3), "x\n"));
    assert_eq!((p, v), (Some(Bool(true)), Some(S("public".into()))));
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
    let meta = parse(&src, WORKING_V3);
    assert!(meta.stamp_problems.is_empty());
    let out = render_page(&meta, "body\n");
    let (a, b) = (
        parse(&src, WORKING_V3).raw.unwrap(),
        parse(&out, WORKING_V3).raw.unwrap(),
    );
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
    let meta = parse(src, WORKING_V3);
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
