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

/// Twenty pages: sixteen published pages wired into a wikilink ring, and four
/// ontology pages whose frontmatter `is-a` lists form a subclass chain, plus
/// the vocabulary that makes `is-a` mean `rdfs:subClassOf`.
fn write_fixture_vault(root: &std::path::Path) {
    let write = |rel: &str, body: String| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };

    write(
        "ontology/vocabulary.yaml",
        r#"version: 1
namespace: "urn:ngm:class:"
relations:
  is-a:     { owl: "rdfs:subClassOf", characteristics: [transitive] }
  requires: { owl: "vc:requires", restriction: true }
"#
        .to_string(),
    );

    // Four ontology pages in knowledge/pages: OKF frontmatter with
    // Alpha :> Beta :> Gamma :> Delta as the subclass chain the reasoner
    // closes over.
    let chain = ["Alpha", "Beta", "Gamma", "Delta"];
    for (i, name) in chain.iter().enumerate() {
        let slug = name.to_lowercase();
        let parent = if i == 0 {
            String::new()
        } else {
            format!("is-a:\n- '[[{}]]'\n", chain[i - 1])
        };
        write(
            &format!("knowledge/pages/{}.md", name),
            format!(
                "---\ntype: Class\ntitle: {name}\nresource: urn:ngm:class:{slug}\n\
                 status: stable\npublic: true\ndomain: data\nmaturity: established\n\
                 quality: 0.5\n{parent}---\n\nFixture class {name}.\n"
            ),
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
        "the four frontmatter classes reach the assert graph, got {}",
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

/// Frontmatter relations become typed edges, and a class keeps its node when a
/// public working page shares its identity (the cross-graph twin join).
#[actix_rt::test]
async fn frontmatter_relations_are_typed_edges_and_a_class_wins_its_working_twin() {
    let h = harness().await;
    // A public working note twinned with the Alpha class. `working/` sorts
    // after `knowledge/`, so without the yield rule its batch would upsert
    // over the class node.
    std::fs::write(
        h.vault.path().join("working/pages/Alpha.md"),
        "---\ntype: Note\npublic: true\n---\n\n# Alpha\n\nA working note.\n",
    )
    .unwrap();

    let stats = h.service.sync_graphs_with(true).await.expect("sync");
    assert_no_stage_failed(&stats);
    assert_eq!(stats.total_files, 21);

    let graph = h.kg_repo.load_graph().await.expect("graph loads");
    assert_eq!(graph.nodes.len(), 19, "the twin joins the class node");
    let alpha = graph
        .nodes
        .iter()
        .find(|n| n.metadata_id == "Alpha")
        .expect("the Alpha node");
    assert_eq!(
        alpha.metadata.get("ontology_type").map(String::as_str),
        Some("Class"),
        "the class, not its working twin, owns the node"
    );
    assert_eq!(alpha.owl_class_iri.as_deref(), Some("urn:ngm:class:alpha"));

    let beta = graph
        .nodes
        .iter()
        .find(|n| n.metadata_id == "Beta")
        .expect("the Beta node");
    let subclass = graph
        .edges
        .iter()
        .find(|e| e.source == beta.id && e.target == alpha.id)
        .expect("Beta is-a Alpha is an edge");
    assert_eq!(subclass.edge_type.as_deref(), Some("hierarchical"));
    assert_eq!(
        subclass.owl_property_iri.as_deref(),
        Some("http://www.w3.org/2000/01/rdf-schema#subClassOf"),
        "the predicate survives the store round trip"
    );

    let axioms = h.onto_repo.get_axioms().await.expect("axioms");
    assert!(
        axioms
            .iter()
            .any(|a| a.subject == "urn:ngm:class:beta" && a.object == "urn:ngm:class:alpha"),
        "the assert graph carries Beta SubClassOf Alpha"
    );
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

/// PRD-sovereign-corpus §5 acceptance 3, without a deploy: a full sync of the
/// REAL vault through `LocalDirectorySource`, reporting the node, edge and
/// ontology-class counts to set against the pre-change GitHub-ingest baseline
/// (13,165 nodes / 153,960 edges / 4,167 classes).
///
/// Ignored by default — it reads `VAULT_ROOT` (plus the optional
/// `VAULT_BASE_PATHS`) and walks ~11k pages. Run it with
/// `VAULT_ROOT=/path/to/visionGraph cargo test --test corpus_local_sync \
///  -- --ignored --nocapture real_vault`.
#[actix_rt::test]
#[ignore = "reads the real vault named by VAULT_ROOT"]
async fn real_vault_ingest_counts() {
    let source =
        Arc::new(LocalDirectorySource::from_env().expect("VAULT_ROOT must name the vault"));
    let dir = tempfile::tempdir().unwrap();
    let onto_repo = Arc::new(
        OxigraphOntologyRepository::open(&dir.path().join("oxigraph"))
            .await
            .expect("oxigraph store"),
    );
    let kg_repo = Arc::new(OxigraphGraphRepository::from_store(
        onto_repo.store().clone(),
    ));
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
    let service = GitHubSyncService::new(
        source,
        kg_repo.clone() as Arc<dyn KnowledgeGraphRepository>,
        onto_repo.clone(),
        sync_db,
    );

    let stats = service
        .sync_graphs_with(true)
        .await
        .expect("full sync of the real vault");
    let graph = kg_repo.load_graph().await.expect("graph loads");
    let classes = onto_repo.get_classes().await.expect("classes");

    let mut by_type: std::collections::BTreeMap<String, usize> = Default::default();
    for node in &graph.nodes {
        *by_type
            .entry(node.node_type.clone().unwrap_or_default())
            .or_default() += 1;
    }
    let mut ontology_types: std::collections::BTreeMap<String, usize> = Default::default();
    for node in &graph.nodes {
        if let Some(t) = node.metadata.get("ontology_type") {
            *ontology_types.entry(t.clone()).or_default() += 1;
        }
    }
    let mut edge_types: std::collections::BTreeMap<String, usize> = Default::default();
    for edge in &graph.edges {
        *edge_types
            .entry(edge.edge_type.clone().unwrap_or_default())
            .or_default() += 1;
    }

    println!("REAL-VAULT total_files      = {}", stats.total_files);
    println!("REAL-VAULT sync errors      = {:?}", stats.errors);
    println!("REAL-VAULT nodes            = {}", graph.nodes.len());
    println!("REAL-VAULT edges            = {}", graph.edges.len());
    println!("REAL-VAULT ontology classes = {}", classes.len());
    println!("REAL-VAULT node types       = {:?}", by_type);
    println!("REAL-VAULT ontology_type    = {:?}", ontology_types);
    println!("REAL-VAULT edge types       = {:?}", edge_types);

    assert!(!graph.nodes.is_empty(), "the real vault ingests");
}
