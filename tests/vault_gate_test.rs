// tests/vault_gate_test.rs
//! ADR-2040 inclusion-gate integration test — EXP-V01, EXP-V02, EXP-V03 of
//! `docs/VAULT-corpus-format.md`.
//!
//! This drives the SAME code path the GitHub sync uses. The sync's gate for a
//! plain (non-JSON-LD) page is `github_sync_service::page_is_kg_included`,
//! which is a one-line delegation to `visionclaw_domain::vault::parse(..)
//! .is_kg_included()`; `FileService::page_is_kg_included` delegates to exactly
//! the same call. Both of those are private to their modules, so the gate is
//! exercised here through that shared entry point, and the node-building half
//! is exercised through the real `KnowledgeGraphParser` the sync calls.
//!
//! The fixtures in `tests/fixtures/vault/` are read from disk rather than
//! inlined so that the same corpus can be replayed by `vault-migrate`
//! (ADR-2042) and by the domain-crate twin of this test.

use std::path::PathBuf;

use visionclaw_domain::vault::{self, PageFormat, VaultIndex};
use visionclaw_server::services::parsers::KnowledgeGraphParser;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}

fn fixture(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

/// The gate as the sync applies it to a plain page.
fn is_kg_included(content: &str) -> bool {
    vault::parse(content).is_kg_included()
}

// ---------------------------------------------------------------------------
// EXP-V01 — the publish half of the gate
// ---------------------------------------------------------------------------

#[test]
fn exp_v01_frontmatter_public_true_ingests() {
    let content = fixture("obsidian-public.md");
    let meta = vault::parse(&content);

    assert!(meta.public);
    assert!(is_kg_included(&content));
    assert_eq!(meta.format, PageFormat::Obsidian);
    assert_eq!(meta.source_domain.as_deref(), Some("mv"));
    assert_eq!(meta.aliases, vec!["Public Fixture"]);
}

#[test]
fn exp_v01_frontmatter_public_false_does_not_ingest() {
    let content = fixture("obsidian-private.md");

    assert!(!is_kg_included(&content));
    // The fixture quotes `public:: true` inside a fenced block: the gate reads
    // only the frontmatter carrier, so the quote cannot leak the page.
    assert!(content.contains("public:: true"));
}

#[test]
fn exp_v01_a_page_with_no_frontmatter_does_not_ingest() {
    // Fail-closed (Invariant 2): no carrier at all means private.
    assert!(!is_kg_included("# Untitled\n\nJust prose.\n"));
}

// ---------------------------------------------------------------------------
// EXP-V02 — owl-class bypasses the publish gate
// ---------------------------------------------------------------------------

#[test]
fn exp_v02_owl_class_ingests_and_types_the_node() {
    let content = fixture("obsidian-owl-class.md");
    let meta = vault::parse(&content);

    assert!(!meta.public, "the fixture carries no `public` key");
    assert!(is_kg_included(&content));
    assert_eq!(meta.owl_class.as_deref(), Some("mv:Foo"));

    // The node the sync builds carries the IRI, which is what reclassifies it
    // into the ontology population downstream.
    let parser = KnowledgeGraphParser::new();
    let graph = parser.parse(&content, "obsidian-owl-class.md");
    let node = &graph.nodes[0];

    assert_eq!(node.owl_class_iri.as_deref(), Some("mv:Foo"));
    assert_eq!(node.node_type.as_deref(), Some("ontology_node"));
    assert_eq!(
        node.metadata.get("source_domain").map(String::as_str),
        Some("mv")
    );
}

#[test]
fn exp_v02_elevated_from_resolves_to_the_bare_page_name() {
    // The provenance bridge the sync turns into an `elevated_from` edge.
    let meta = vault::parse(&fixture("obsidian-owl-class.md"));
    assert_eq!(meta.elevated_from.as_deref(), Some("Working Page"));
}

// ---------------------------------------------------------------------------
// EXP-V03 — a `key:: value` line is body text (ADR-2112)
// ---------------------------------------------------------------------------

#[test]
fn exp_v03_a_leading_key_block_is_body_text_and_the_page_is_private() {
    let content = fixture("legacy-public.md");
    assert!(content.starts_with("public:: true"));
    let meta = vault::parse(&content);

    assert!(!meta.public);
    assert!(!is_kg_included(&content));
    assert_eq!(meta.format, PageFormat::None);
    assert!(meta.aliases.is_empty(), "`alias::` is not an alias");
}

#[test]
fn exp_v03_legacy_marker_after_a_heading_or_in_a_fence_does_not_ingest() {
    let content = fixture("legacy-midbody-public.md");

    assert!(
        content.contains("public:: true"),
        "fixture must contain the marker it is not allowed to honour"
    );
    assert!(!is_kg_included(&content));
    assert_eq!(vault::parse(&content).format, PageFormat::None);
}

// ---------------------------------------------------------------------------
// §V1 — namespace identity
// ---------------------------------------------------------------------------

#[test]
fn namespace_pages_ingest_and_keep_a_stable_identity() {
    let content = fixture("namespace/A___B Testing.md");
    assert!(is_kg_included(&content));

    let parser = KnowledgeGraphParser::new();
    let graph = parser.parse(&content, "A___B Testing.md");
    let node = &graph.nodes[0];

    // The page name decodes to the `[[Ns/Title]]` form the corpus links with,
    // and `source_file` is now the vault-relative path.
    assert_eq!(node.metadata_id, "A/B Testing");
    assert_eq!(
        node.metadata.get("source_file").map(String::as_str),
        Some("A/B Testing.md")
    );

    // Invariant 4: the decode does not move the node. Slugification collapses
    // any run of non-alphanumerics to a single `-`, so the encoded and decoded
    // names hash to the same id.
    assert_eq!(
        node.id,
        parser.page_name_to_id("A___B Testing"),
        "node id must be unchanged by the namespace decode"
    );
    assert_eq!(node.id, parser.page_name_to_id("A/B Testing"));
}

#[test]
fn every_fixture_agrees_with_its_expected_verdict() {
    // One table so a new fixture cannot be added without stating its verdict.
    let expected = [
        ("obsidian-public.md", true),
        ("obsidian-private.md", false),
        ("obsidian-owl-class.md", true),
        ("legacy-public.md", false),
        ("legacy-midbody-public.md", false),
        ("namespace/A___B Testing.md", true),
    ];

    for (name, should_ingest) in expected {
        assert_eq!(
            is_kg_included(&fixture(name)),
            should_ingest,
            "gate verdict for fixture {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// The Logseq-format data-model fixtures carry no metadata
// ---------------------------------------------------------------------------

/// `tests/fixtures/data-model/valid/pages/` was authored in the Logseq
/// property-line format for the json-ld ingest tests. Under ADR-2112 none of
/// those pages carries metadata the gate can read: every one is private.
#[test]
fn logseq_format_data_model_fixtures_are_private() {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/data-model/valid/pages");
    for name in [
        "001-minimal-page.md",
        "002-page-with-tags-and-links.md",
        "003-page-with-wikilinks.md",
        "004-stub-page.md",
        "005-page-with-embedded-ontology-block.md",
    ] {
        let content = std::fs::read_to_string(dir.join(name)).expect("fixture readable");
        assert!(content.contains(":: "), "{name} is a `key::` fixture");
        let meta = vault::parse(&content);
        assert!(!meta.is_kg_included(), "{name} must not ingest");
        assert_eq!(meta.format, PageFormat::None, "{name}");
    }
}

// ---------------------------------------------------------------------------
// §V1 — Obsidian wikilink resolution (the shadow-sync defect)
// ---------------------------------------------------------------------------
//
// The corpus links to pages BARE (`[[Title]]`) while page identity is the
// vault-relative path. Without Obsidian's basename rule every such link minted
// a phantom `linked_page` stub beside the real node — 188 podcast-evidence
// pages and 248 subfolder pages' worth of them.

/// The vault the resolution tests run against: two pages in folders, one at the
/// root, and a basename (`Security`) that is deliberately ambiguous.
fn indexed_vault() -> VaultIndex {
    VaultIndex::from_identities([
        "AI Daily Brief",
        "Security",
        "ETSI_Domain_Infrastructure/Security",
        "ETSI_Domain_Governance/Economy",
        "podcast-evidence/black-friday-gpt",
    ])
}

fn page(body: &str) -> String {
    format!("---\npublic: true\n---\n\n{body}\n")
}

#[test]
fn a_bare_link_to_a_subfolder_page_joins_the_real_node() {
    let parser = KnowledgeGraphParser::new();
    let index = indexed_vault();
    let content = page("# AI Daily Brief\n\nCovered in [[black-friday-gpt]].");

    let graph = parser.parse_with_index(&content, "AI Daily Brief.md", Some(&index));

    assert_eq!(graph.edges.len(), 1);
    assert_eq!(
        graph.edges[0].target,
        parser.page_name_to_id("podcast-evidence/black-friday-gpt"),
        "the bare link must land on the page's full vault identity"
    );
    assert_ne!(
        graph.edges[0].target,
        parser.page_name_to_id("black-friday-gpt"),
        "hashing the bare link text is what minted the phantom stub"
    );
}

#[test]
fn a_legacy_underscore_link_resolves_to_the_converted_folder_page() {
    let parser = KnowledgeGraphParser::new();
    let index = indexed_vault();
    let content = page("# Some Page\n\nSee [[ETSI_Domain_Governance___Economy]].");

    let graph = parser.parse_with_index(&content, "Some Page.md", Some(&index));

    assert_eq!(
        graph.edges[0].target,
        parser.page_name_to_id("ETSI_Domain_Governance/Economy")
    );
}

#[test]
fn an_ambiguous_basename_prefers_the_linking_pages_own_folder() {
    let parser = KnowledgeGraphParser::new();
    let index = indexed_vault();
    let content = page("# Interop\n\nSee [[Security]].");

    let graph = parser.parse_with_index(
        &content,
        "ETSI_Domain_Infrastructure/Interop.md",
        Some(&index),
    );

    assert_eq!(
        graph.edges[0].target,
        parser.page_name_to_id("ETSI_Domain_Infrastructure/Security"),
        "a sibling in the linking page's own folder wins the tie-break"
    );
}

#[test]
fn an_unknown_target_still_links_to_a_stub_id() {
    let parser = KnowledgeGraphParser::new();
    let index = indexed_vault();
    let content = page("# Orphan\n\nSee [[No Such Page]].");

    let graph = parser.parse_with_index(&content, "Orphan.md", Some(&index));

    assert_eq!(graph.edges.len(), 1, "the dangling edge is still emitted");
    assert_eq!(
        graph.edges[0].target,
        parser.page_name_to_id("No Such Page")
    );
}

#[test]
fn a_subfolder_pages_identity_label_and_source_file_agree() {
    let parser = KnowledgeGraphParser::new();
    let index = indexed_vault();
    // `vault-migrate` writes the identity path into `title:`; that is not a
    // display title, so it must not produce a label inconsistent with
    // `source_file`.
    let content =
        "---\npublic: true\ntitle: podcast-evidence/black-friday-gpt\n---\n\n# Black Friday GPT\n";

    let graph = parser.parse_with_index(
        content,
        "podcast-evidence/black-friday-gpt.md",
        Some(&index),
    );
    let node = &graph.nodes[0];

    assert_eq!(node.metadata_id, "podcast-evidence/black-friday-gpt");
    assert_eq!(
        node.metadata.get("source_file").map(String::as_str),
        Some("podcast-evidence/black-friday-gpt.md"),
        "source_file must carry the identity, not a bare basename"
    );
    // Obsidian displays a namespaced page by its leaf, so the label is the
    // basename while identity keeps the full path.
    assert_eq!(node.label, "black-friday-gpt");
    assert_eq!(
        node.id,
        parser.page_name_to_id("podcast-evidence/black-friday-gpt")
    );
}

#[test]
fn a_genuine_display_title_still_becomes_the_label() {
    let parser = KnowledgeGraphParser::new();
    let content = "---\npublic: true\ntitle: Black Friday GPT\n---\n\n# Body\n";

    let graph = parser.parse_with_index(content, "podcast-evidence/black-friday-gpt.md", None);

    assert_eq!(graph.nodes[0].label, "Black Friday GPT");
    assert_eq!(
        graph.nodes[0].metadata_id,
        "podcast-evidence/black-friday-gpt"
    );
}

#[test]
fn distinct_pages_sharing_a_basename_stay_distinct_nodes() {
    // The silent merge: 34 basename collisions exist in the converted corpus,
    // e.g. `ETSI_Domain_Infrastructure/Security` and the root `Security`.
    let parser = KnowledgeGraphParser::new();
    assert_ne!(
        parser.page_name_to_id("ETSI_Domain_Infrastructure/Security"),
        parser.page_name_to_id("Security")
    );
}

#[test]
fn the_cross_graph_twin_join_is_preserved() {
    // 254 basename pairs span the two source graphs and MUST share one node.
    let bases = vec![
        "mainKnowledgeGraph/pages".to_string(),
        "workingGraph/pages".to_string(),
    ];
    let parser = KnowledgeGraphParser::new();
    let main = vault::page_name_from_repo_path("mainKnowledgeGraph/pages/Agentic AI.md", &bases);
    let work = vault::page_name_from_repo_path("workingGraph/pages/Agentic AI.md", &bases);
    assert_eq!(main, work);
    assert_eq!(parser.page_name_to_id(&main), parser.page_name_to_id(&work));
}
