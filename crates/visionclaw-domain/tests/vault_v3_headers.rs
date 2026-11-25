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
fn contradictory_keys_fail_closed_and_are_reported() {
    let page = "---\ntype: Note\npublic: true\nvisibility: shared\n---\n";
    let meta = parse(page);
    assert!(!meta.is_kg_included());
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
