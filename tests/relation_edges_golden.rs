// tests/relation_edges_golden.rs
//! ADR-2204 S2 (E1) — the byte-identity oracle for value-preserving relation
//! centralisation.
//!
//! A fixture vault exercises every producer of a relation edge in the sync:
//! authored wikilinks, `elevatedFrom`, every typed frontmatter relation the
//! sync maps, a predicate that is deliberately not an edge (`rdf:type`), an
//! unmapped predicate, a domain-root spoke, and dangling-target co-citation at
//! the fan-out boundary (2, 3 and 4 referrers). The test dumps every stored
//! edge as `(source, target, label, weight bits, provenance, predicate)` and
//! compares it with `tests/fixtures/relation_edges.golden`.
//!
//! The golden was recorded on the code BEFORE the schema existed
//! (`RELATION_GOLDEN_WRITE=1`), so a pass after the change proves the produced
//! edges are unchanged, weights compared as raw `f32` bits.

use std::sync::Arc;

use visionclaw_server::adapters::whelk_inference_engine::WhelkInferenceEngine;
use visionclaw_server::adapters::{
    OxigraphGraphRepository, OxigraphOntologyRepository, SqliteSettingsRepository,
};
use visionclaw_server::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use visionclaw_server::services::corpus_source::LocalDirectorySource;
use visionclaw_server::services::github_sync_service::GitHubSyncService;

/// Frontmatter relation key → owl predicate, one per mapped IRI family.
const RELATIONS: &[(&str, &str)] = &[
    ("is-a", "rdfs:subClassOf"),
    ("instance-of", "rdf:type"),
    ("has-part", "vc:hasPart"),
    ("part-of", "vc:isPartOf"),
    ("requires", "vc:requires"),
    ("enables", "vc:enables"),
    ("depends-on", "vc:dependsOn"),
    ("implements", "vc:implements"),
    ("contrasts-with", "vc:contrastsWith"),
    ("bridges-to", "vc:bridgesTo"),
    ("uses", "vc:uses"),
    ("supports", "vc:supports"),
    ("standardized-by", "vc:standardizedBy"),
    ("related-to", "vc:relatedTo"),
    ("same-as", "owl:sameAs"),
    ("applies-to", "vc:appliesTo"),
    ("derived-from", "prov:wasDerivedFrom"),
    ("equivalent", "owl:equivalentClass"),
    ("disjoint", "owl:disjointWith"),
    ("sub-prop", "rdfs:subPropertyOf"),
    ("produces", "vc:produces"),
];

fn write_vault(root: &std::path::Path) {
    let write = |rel: &str, body: String| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };

    let mut vocab = String::from("version: 1\nnamespace: \"urn:ngm:class:\"\nrelations:\n");
    for (key, owl) in RELATIONS {
        vocab.push_str(&format!("  {key}: {{ owl: \"{owl}\" }}\n"));
    }
    write("ontology/vocabulary.yaml", vocab);

    let class_page = |name: &str, extra: &str| {
        format!(
            "---\ntype: Class\ntitle: {name}\nresource: urn:ngm:class:{}\n\
             status: stable\npublic: true\ndomain: data\nmaturity: established\n\
             quality: 0.5\n{extra}---\n\nFixture class {name}.\n",
            name.to_lowercase()
        )
    };

    write("knowledge/Target.md", class_page("Target", ""));
    let mut rels = String::new();
    for (key, _) in RELATIONS {
        rels.push_str(&format!("{key}:\n- '[[Target]]'\n"));
    }
    write("knowledge/Source.md", class_page("Source", &rels));

    // Authored wikilink between two authored pages; one `elevatedFrom`.
    write(
        "knowledge/Linker.md",
        "---\npublic: true\nelevatedFrom: \"[[Origin]]\"\n---\n\n# Linker\n\nSee [[Target]].\n"
            .to_string(),
    );
    write(
        "knowledge/Origin.md",
        "---\npublic: true\n---\n\n# Origin\n\nPlain page.\n".to_string(),
    );

    // Dangling-target co-citation. Referrers per missing target:
    //   Ghost-a, Ghost-b: P1,P2        (2 referrers, pair shares two targets)
    //   Hub-three:        P1,P2,P3     (exactly the default threshold)
    //   Hub-four:         P1,P2,P3,P4  (above it)
    let links = |names: &[&str]| {
        names
            .iter()
            .map(|n| format!("[[{n}]]"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    write(
        "knowledge/P1.md",
        format!(
            "---\npublic: true\n---\n\n# P1\n\n{}\n",
            links(&["Ghost-a", "Ghost-b", "Hub-three", "Hub-four"])
        ),
    );
    write(
        "knowledge/P2.md",
        format!(
            "---\npublic: true\n---\n\n# P2\n\n{}\n",
            links(&["Ghost-a", "Ghost-b", "Hub-three", "Hub-four"])
        ),
    );
    write(
        "knowledge/P3.md",
        format!(
            "---\npublic: true\n---\n\n# P3\n\n{}\n",
            links(&["Hub-three", "Hub-four"])
        ),
    );
    write(
        "knowledge/P4.md",
        format!(
            "---\npublic: true\n---\n\n# P4\n\n{}\n",
            links(&["Hub-four"])
        ),
    );
}

async fn sync_fixture() -> (
    Vec<String>,
    visionclaw_server::services::github_sync_service::SyncStatistics,
) {
    let dir = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    write_vault(vault.path());

    let onto_repo = Arc::new(
        OxigraphOntologyRepository::open(&dir.path().join("oxigraph"))
            .await
            .expect("oxigraph store"),
    );
    let kg_repo = Arc::new(OxigraphGraphRepository::from_store(
        onto_repo.store().clone(),
    ));
    let sync_db = Arc::new(
        SqliteSettingsRepository::open(&dir.path().join("settings.sqlite3"))
            .await
            .expect("sqlite settings"),
    );
    let source = Arc::new(LocalDirectorySource::new(
        vault.path(),
        vec![std::path::PathBuf::from("knowledge")],
    ));
    let engine = Arc::new(tokio::sync::RwLock::new(WhelkInferenceEngine::new()));
    let service = GitHubSyncService::new(
        source,
        kg_repo.clone() as Arc<dyn KnowledgeGraphRepository>,
        onto_repo.clone(),
        sync_db,
        engine,
    );
    let stats = service.sync_graphs_with(true).await.expect("sync");

    let graph = kg_repo.load_graph().await.expect("graph loads");
    let name = |id: u32| {
        graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.metadata_id.clone())
            .unwrap_or_else(|| format!("#missing{id}"))
    };
    let mut lines: Vec<String> = graph
        .edges
        .iter()
        .map(|e| {
            format!(
                "{} -> {} | {} | {:08x} ({}) | {:?} | {} | {}",
                name(e.source),
                name(e.target),
                e.edge_type.as_deref().unwrap_or("-"),
                e.weight.to_bits(),
                e.weight,
                e.provenance,
                e.owl_property_iri.as_deref().unwrap_or("-"),
                e.metadata
                    .as_ref()
                    .and_then(|m| m.get("relation"))
                    .map(String::as_str)
                    .unwrap_or("-"),
            )
        })
        .collect();
    lines.sort();
    (lines, stats)
}

/// One sync serves both assertions: the quarantine ledger is process-wide, so
/// two syncs running concurrently in one test binary would reset each other.
#[actix_rt::test]
async fn sync_edges_match_the_pre_schema_golden_and_unknown_predicates_are_quarantined() {
    let (lines, stats) = sync_fixture().await;
    let actual = lines.join("\n") + "\n";
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/relation_edges.golden");
    if std::env::var("RELATION_GOLDEN_WRITE").is_ok() {
        std::fs::write(&golden_path, &actual).unwrap();
    }
    let golden = std::fs::read_to_string(&golden_path)
        .expect("tests/fixtures/relation_edges.golden is recorded before the change");
    assert_eq!(actual, golden, "produced edges drifted from the golden");
    assert_quarantine(&lines, &stats);
}

/// Unknown predicates are quarantined: counted, no edge, no invented defaults
/// (ADR-2204 D1). `produces` expands to `vc:produces`, which the schema lacks;
/// `instance-of` (`rdf:type`) is known and deliberately makes no edge, so it
/// is NOT counted as quarantined.
fn assert_quarantine(
    lines: &[String],
    stats: &visionclaw_server::services::github_sync_service::SyncStatistics,
) {
    assert_eq!(
        stats.quarantined_relation_targets, 1,
        "one `produces` target"
    );
    assert!(
        lines.iter().all(|l| !l.contains("produces")),
        "a quarantined predicate creates no edge"
    );
    let q = visionclaw_domain::relations::Quarantine::global().snapshot();
    assert_eq!(
        q.get("https://narrativegoldmine.com/ns/v1#produces"),
        Some(&1),
        "the raw IRI is kept: {q:?}"
    );
    assert!(!q.keys().any(|k| k.ends_with("22-rdf-syntax-ns#type")));
}
