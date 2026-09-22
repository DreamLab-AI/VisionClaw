//! Fixture-driven gate check, runnable independently of the server crate.
//!
//! `tests/vault_gate_test.rs` in the root package is the canonical ADR-2040
//! integration test: it drives the fixtures through the real
//! `KnowledgeGraphParser` as well as the gate. This twin covers the gate half
//! only, against the same fixture files, so the EXP-V01–V03 verdicts stay
//! verifiable while the server crate is being changed elsewhere in the
//! workspace. If the two ever disagree, the root test is authoritative.

use std::path::PathBuf;

use visionclaw_domain::vault::{self, PageFormat};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/vault")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

fn is_kg_included(name: &str) -> bool {
    vault::parse(&fixture(name)).is_kg_included()
}

#[test]
fn exp_v01_publish_gate() {
    let meta = vault::parse(&fixture("obsidian-public.md"));
    assert!(meta.public);
    assert!(meta.is_kg_included());
    assert_eq!(meta.format, PageFormat::Obsidian);

    assert!(!is_kg_included("obsidian-private.md"));
    assert!(!vault::parse("# Untitled\n\nJust prose.\n").is_kg_included());
}

#[test]
fn exp_v02_owl_class_bypasses_the_publish_gate() {
    let meta = vault::parse(&fixture("obsidian-owl-class.md"));
    assert!(!meta.public);
    assert!(meta.is_kg_included());
    assert_eq!(meta.owl_class.as_deref(), Some("mv:Foo"));
    assert_eq!(meta.elevated_from.as_deref(), Some("Working Page"));
}

#[test]
fn exp_v03_key_lines_are_body_text() {
    let leading = fixture("legacy-public.md");
    assert!(leading.starts_with("public:: true"));
    let meta = vault::parse(&leading);
    assert!(
        !meta.is_kg_included(),
        "a leading `key::` block is not metadata"
    );
    assert_eq!(meta.format, PageFormat::None);
    assert!(meta.aliases.is_empty());

    let midbody = fixture("legacy-midbody-public.md");
    assert!(midbody.contains("public:: true"));
    assert!(!vault::parse(&midbody).is_kg_included());
}

#[test]
fn namespace_fixture_ingests_and_decodes() {
    assert!(is_kg_included("namespace/A___B Testing.md"));
    assert_eq!(
        vault::page_name_from_path("namespace/A___B Testing.md"),
        "namespace/A/B Testing"
    );
    assert_eq!(
        vault::page_name_from_path("A___B Testing.md"),
        "A/B Testing"
    );
}

#[test]
fn every_fixture_agrees_with_its_expected_verdict() {
    let expected = [
        ("obsidian-public.md", true),
        ("obsidian-private.md", false),
        ("obsidian-owl-class.md", true),
        ("legacy-public.md", false),
        ("legacy-midbody-public.md", false),
        ("namespace/A___B Testing.md", true),
    ];
    for (name, should_ingest) in expected {
        assert_eq!(is_kg_included(name), should_ingest, "verdict for {name}");
    }
}
