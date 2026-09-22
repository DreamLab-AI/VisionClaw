// tests/corpus_local_sync.rs
//! ADR-2114 — the sync pipeline over `LocalDirectorySource`.
//!
//! `sync_graphs()` is unchanged by the source extraction, so this exercises it
//! end to end against a twenty-page vault fixture on disk: both base paths,
//! the dual-graph split, wikilink edges, post-sync Whelk reasoning and the
//! change-marker filter on a second run.

use std::sync::Arc;

use visionclaw_server::adapters::{
    OxigraphGraphRepository, OxigraphOntologyRepository, SqliteSettingsRepository,
};
use visionclaw_server::ports::knowledge_graph_repository::KnowledgeGraphRepository;
use visionclaw_server::ports::OntologyRepository;
use visionclaw_server::services::corpus_source::LocalDirectorySource;
use visionclaw_server::services::github_sync_service::{GitHubSyncService, SyncStatistics};

/// Twenty pages: sixteen published working-graph pages wired into a wikilink
/// ring, and four ontology pages whose JSON-LD fences form a subclass chain.
fn write_fixture_vault(root: &std::path::Path) {
    let write = |rel: &str, body: String| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };

    // Four ontology pages in knowledge/pages, carrying the corpus's two
    // `json-ld` fences (page envelope + Class) with Alpha :> Beta :> Gamma :>
    // Delta as the subclass chain the reasoner closes over.
    const ONTOLOGY_PAGE: &str = r#"---
public: true
---

# {NAME}

```json-ld
{ "@context":"https://narrativegoldmine.com/ns/v1", "@id":"urn:visionflow:page:{SLUG}", "@type":"Page", "title":"{NAME}", "vc:slug":"{SLUG}", "vc:public":true, "vc:schemaVersion":2, "vc:outboundWikilinks":[] }
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:{SLUG}",
  "@type": "Class",
  "label": "{NAME}",
  "definition": "Fixture class {NAME}.",
  "domain": "data",
  "maturity": "established",
  "subClassOf": {SUPERCLASSES},
  "quality": 0.5
}
```
"#;

    let chain = ["Alpha", "Beta", "Gamma", "Delta"];
    for (i, name) in chain.iter().enumerate() {
        let slug = name.to_lowercase();
        let superclasses = if i == 0 {
            "[]".to_string()
        } else {
            format!(
                r#"[{{ "@id": "urn:ngm:class:{}", "label": "{}" }}]"#,
                chain[i - 1].to_lowercase(),
                chain[i - 1]
            )
        };
        write(
            &format!("knowledge/pages/{}.md", name),
            ONTOLOGY_PAGE
                .replace("{NAME}", name)
                .replace("{SLUG}", &slug)
                .replace("{SUPERCLASSES}", &superclasses),
        );
    }

    // Twelve published knowledge pages in a wikilink ring.
    for i in 0..12 {
        let next = (i + 1) % 12;
        write(
            &format!("knowledge/pages/Page {:02}.md", i),
            format!(
                "---\npublic: true\n---\n\n# Page {:02}\n\nLinks to [[Page {:02}]] and [[Alpha]].\n",
                i, next
            ),
        );
    }

    // Four working-graph pages, one of them private (it must not ingest).
    for i in 0..4 {
        let public = if i == 3 { "false" } else { "true" };
        write(
            &format!("working/pages/Note {:02}.md", i),
            format!(
                "---\npublic: {}\n---\n\n# Note {:02}\n\nSee [[Page 00]].\n",
                public, i
            ),
        );
    }

    // Excluded by the walk: app config and a non-markdown asset.
    write("knowledge/pages/.obsidian/workspace.json", "{}".to_string());
    write("knowledge/pages/diagram.png", "binary".to_string());
}

struct Harness {
    _dir: tempfile::TempDir,
    vault: tempfile::TempDir,
    service: GitHubSyncService,
    kg_repo: Arc<OxigraphGraphRepository>,
    onto_repo: Arc<OxigraphOntologyRepository>,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let vault = tempfile::tempdir().unwrap();
    write_fixture_vault(vault.path());

    let onto_repo = Arc::new(
        OxigraphOntologyRepository::open(&dir.path().join("oxigraph"))
            .await
            .expect("oxigraph store"),
    );
    let kg_repo = Arc::new(OxigraphGraphRepository::from_store(
        onto_repo.store().clone(),
    ));

    // A fresh Oxigraph store has no named graphs, and the assert-graph rebuild
    // opens with `CLEAR GRAPH <…:assert>`, which errors on a graph that does
    // not exist yet. Production stores are long-lived and already carry it;
    // the fixture creates it with one non-class triple so the rebuild has
    // something to clear.
    onto_repo
        .store()
        .update(
            "INSERT DATA { GRAPH <urn:ngm:graph:ontology:assert> { \
             <urn:ngm:fixture:bootstrap> <http://www.w3.org/2000/01/rdf-schema#label> \"bootstrap\" } }",
        )
        .expect("bootstrap the assert graph");
    let sync_db = Arc::new(
        SqliteSettingsRepository::open(&dir.path().join("settings.sqlite3"))
            .await
            .expect("sqlite settings"),
    );

    let source = Arc::new(LocalDirectorySource::new(
        vault.path(),
        vec![
            std::path::PathBuf::from("knowledge/pages"),
            std::path::PathBuf::from("working/pages"),
        ],
    ));

    let service = GitHubSyncService::new(
        source,
        kg_repo.clone() as Arc<dyn KnowledgeGraphRepository>,
        onto_repo.clone(),
        sync_db,
    );

    Harness {
        _dir: dir,
        vault,
        service,
        kg_repo,
        onto_repo,
    }
}

fn assert_no_stage_failed(stats: &SyncStatistics) {
    assert!(
        stats.errors.is_empty(),
        "no sync stage may fail on the fixture: {:?}",
        stats.errors
    );
}

#[actix_rt::test]
async fn sync_graphs_ingests_the_local_vault() {
    let h = harness().await;

    let stats = h.service.sync_graphs().await.expect("sync should succeed");
    assert_no_stage_failed(&stats);

    // 4 ontology + 12 knowledge + 4 working = 20 listed pages; `.obsidian/`,
    // the PNG and everything outside the two base paths are not listed.
    assert_eq!(stats.total_files, 20, "the source lists exactly the corpus");

    let graph = h.kg_repo.load_graph().await.expect("graph loads");

    // The private working page is gated out; every other page is a node. The
    // fixture carries no domain groups, so no domain roots are materialised.
    assert_eq!(
        graph.nodes.len(),
        19,
        "19 of 20 pages ingest — `public: false` is gated out"
    );
    assert!(
        !graph.edges.is_empty(),
        "the wikilink ring and the subclass chain produce edges"
    );
}

/// The ontology half: a full sync rebuilds the assert graph from the freshly
/// ingested corpus and runs Whelk over it, materialising inferred edges.
#[actix_rt::test]
async fn a_full_sync_rebuilds_the_assert_graph_and_reasons() {
    let h = harness().await;

    let stats = h
        .service
        .sync_graphs_with(true)
        .await
        .expect("full sync should succeed");
    assert_no_stage_failed(&stats);
    assert_eq!(stats.skipped_files, 0, "a full sync bypasses the marker");

    let classes = h.onto_repo.get_classes().await.expect("classes");
    assert!(
        classes.len() >= 4,
        "the four JSON-LD classes reach the assert graph, got {}",
        classes.len()
    );

    // Whelk ran and its closure was persisted. The fixture's chain asserts
    // every immediate parent explicitly, so ADR-2071 suppression correctly
    // emits no *graph* edges — the evidence that reasoning ran is the
    // populated inferred named graph.
    let inferred = h
        .onto_repo
        .store()
        .query("ASK { GRAPH <urn:ngm:graph:ontology:inferred> { ?s ?p ?o } }")
        .expect("inferred-graph query");
    match inferred {
        oxigraph::sparql::QueryResults::Boolean(present) => assert!(
            present,
            "post-sync Whelk reasoning persisted an inferred closure"
        ),
        _ => panic!("ASK must return a boolean"),
    }
}

#[actix_rt::test]
async fn a_second_sync_skips_unchanged_pages() {
    let h = harness().await;
    h.service.sync_graphs().await.expect("first sync");

    let second = h.service.sync_graphs().await.expect("second sync");
    assert_no_stage_failed(&second);
    assert_eq!(
        second.skipped_files, second.total_files,
        "an untouched vault re-syncs nothing — the mtime:size marker holds"
    );

    std::fs::write(
        h.vault.path().join("knowledge/pages/Page 00.md"),
        "---\npublic: true\n---\n\n# Page 00\n\nLinks to [[Page 01]].\n",
    )
    .unwrap();

    let third = h.service.sync_graphs().await.expect("third sync");
    assert_eq!(
        third.total_files - third.skipped_files,
        1,
        "exactly the rewritten page re-processes"
    );
}
