// src/bin/load_ontology.rs
//! Ontology Loader Binary (ADR-2064)
//!
//! Walks the real authored corpus (the vault's `pages/` tree — see
//! `docs/VAULT-corpus-format.md`), parses every page through the ingest seam
//! ([`parse_page`], `vault_core`), projects the pages that declare ontology
//! (`type: Class|Property|Individual` under `knowledge/`) through the corpus
//! vocabulary ([`project_ontology`]), persists the classes and `SubClassOf`
//! axioms to the Oxigraph quad-store via
//! [`OntologyRepository::save_ontology`] (ADR-11) and then runs the
//! [`OwlExtractorService`] over the freshly persisted classes to pull any
//! embedded OWL Functional Syntax blocks out of their `markdown_content` via
//! horned-owl.
//!
//! The corpus is read through the same [`CorpusSource`] port as the live
//! ingest (ADR-2114): a [`LocalDirectorySource`] over the vault root — the
//! optional CLI argument, else `VAULT_ROOT` (the vault path authority per
//! `docs/VAULT-corpus-format.md` Invariant 3) — with the `VAULT_BASE_PATHS`
//! source split. `DATA_DIR` resolves the Oxigraph store location exactly as it
//! does in `sync_corpus`.

use log::{error, info, warn};
use std::path::Path;
use std::sync::Arc;

use visionclaw_server::adapters::OxigraphOntologyRepository;
use visionclaw_server::ports::ontology_repository::{
    OntologyRepository, OwlAxiom, OwlClass, OwlProperty,
};
use visionclaw_server::services::corpus_source::{CorpusSource, LocalDirectorySource};
use visionclaw_server::services::owl_extractor_service::OwlExtractorService;
use visionclaw_server::services::page_parser::{parse_page, project_ontology};

#[derive(Default)]
struct IngestStats {
    files_scanned: usize,
    ontology_files_parsed: usize,
    parse_errors: usize,
    private_skipped: usize,
    persist_errors: usize,
    classes_persisted: usize,
    axioms_persisted: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting ontology loader (Oxigraph backend, ADR-11 / ADR-2064)...");

    // 1. Resolve the corpus source — CLI vault-root override else VAULT_ROOT,
    //    with the VAULT_BASE_PATHS source split either way.
    let source = match std::env::args().nth(1) {
        Some(root) => LocalDirectorySource::new(root, LocalDirectorySource::base_paths_from_env()?),
        None => LocalDirectorySource::from_env()?,
    };
    let corpus_root = source.root.clone();
    info!("Ontology corpus: {}", source.describe());

    // 2. Open the Oxigraph store exactly as `sync_corpus` does.
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let oxigraph_path = Path::new(&data_dir).join("oxigraph");
    info!("Opening Oxigraph store at: {}", oxigraph_path.display());

    let ontology_repo = Arc::new(OxigraphOntologyRepository::open(&oxigraph_path).await?);
    info!("Oxigraph store opened successfully");

    // 3. List the corpus through the port.
    let files = source.list_pages().await?;
    info!("Found {} markdown pages under the corpus root", files.len());

    // 4. Parse every page through the ingest seam, keep the ontology pages,
    //    and project them through the vocabulary.
    let vocabulary = source
        .vocabulary()
        .await?
        .ok_or_else(|| format!("{} has no ontology/vocabulary.yaml", corpus_root.display()))?;
    let mut stats = IngestStats {
        files_scanned: files.len(),
        ..Default::default()
    };
    let mut ontology_pages = Vec::new();
    for page in &files {
        let content = match source.fetch_page(page).await {
            Ok(c) => c,
            Err(e) => {
                warn!("{}", e);
                stats.parse_errors += 1;
                continue;
            }
        };
        match parse_page(&content, &page.path) {
            // The same §V4 gate the sync applies, and the same public boundary
            // `vault build` projects the ontology through.
            Ok(Some(parsed)) if visionclaw_domain::vault::parse(&content).is_kg_included() => {
                ontology_pages.push(parsed)
            }
            Ok(Some(_)) => stats.private_skipped += 1,
            Ok(None) => {}
            Err(e) => {
                warn!("Failed to parse {}: {}", page.path, e);
                stats.parse_errors += 1;
            }
        }
    }
    stats.ontology_files_parsed = ontology_pages.len();
    let projection = project_ontology(&ontology_pages, &vocabulary);
    info!(
        "Projected {} ontology pages: {} classes, {} axioms",
        ontology_pages.len(),
        projection.classes.len(),
        projection.axioms.len()
    );
    match persist(&ontology_repo, &projection.classes, &[], &projection.axioms).await {
        Ok(()) => {
            stats.classes_persisted = projection.classes.len();
            stats.axioms_persisted = projection.axioms.len();
        }
        Err(e) => {
            warn!("Failed to persist the ontology: {}", e);
            stats.persist_errors += 1;
        }
    }

    // 5. Run the real OWL-functional-syntax extraction service over the
    //    classes just persisted, pulling any embedded horned-owl blocks out
    //    of their markdown_content (ADR-2064: previously unwired — dead code,
    //    not even declared as a module — now wired into the real ingest path).
    let extractor = OwlExtractorService::new(ontology_repo.clone());
    let extracted = extractor.extract_all_owl().await.unwrap_or_default();
    let horned_owl_axiom_count: usize = extracted.iter().map(|e| e.axiom_count).sum();

    // 6. Verify data actually landed.
    let all_classes = ontology_repo.get_classes().await?;

    info!("Ontology load complete.");
    info!("  Corpus root:                {}", corpus_root.display());
    info!("  Markdown files scanned:      {}", stats.files_scanned);
    info!(
        "  Ontology pages parsed:       {}",
        stats.ontology_files_parsed
    );
    info!("  Parse errors:                {}", stats.parse_errors);
    info!("  Private ontology pages:      {}", stats.private_skipped);
    info!("  Persist errors:              {}", stats.persist_errors);
    info!("  Classes persisted (this run):{}", stats.classes_persisted);
    info!("  Axioms persisted:            {}", stats.axioms_persisted);
    info!(
        "  Classes with horned-owl blocks: {} ({} axioms)",
        extracted.len(),
        horned_owl_axiom_count
    );
    info!("  Total classes now in store:  {}", all_classes.len());
    info!("Stored in Oxigraph store at: {}", oxigraph_path.display());

    println!("\nOntology load complete!");
    println!("{}", "=".repeat(50));
    println!("  Corpus root:                 {}", corpus_root.display());
    println!("  Markdown files scanned:      {}", stats.files_scanned);
    println!(
        "  Ontology pages parsed:       {}",
        stats.ontology_files_parsed
    );
    println!(
        "  Classes persisted (this run): {}",
        stats.classes_persisted
    );
    println!("  Axioms persisted:            {}", stats.axioms_persisted);
    println!(
        "  Classes with horned-owl blocks: {} ({} axioms)",
        extracted.len(),
        horned_owl_axiom_count
    );
    println!("  Total classes now in store:  {}", all_classes.len());
    println!("  Parse errors:                {}", stats.parse_errors);
    println!("  Private ontology pages:      {}", stats.private_skipped);
    println!("  Persist errors:              {}", stats.persist_errors);
    println!("{}", "=".repeat(50));

    if stats.ontology_files_parsed == 0 || stats.classes_persisted == 0 {
        error!(
            "No ontology data was extracted from {} — check the corpus path and that knowledge/ pages declare `type: Class|Property|Individual`",
            corpus_root.display()
        );
        std::process::exit(1);
    }

    Ok(())
}

async fn persist(
    repo: &Arc<OxigraphOntologyRepository>,
    classes: &[OwlClass],
    properties: &[OwlProperty],
    axioms: &[OwlAxiom],
) -> Result<(), String> {
    repo.save_ontology(classes, properties, axioms)
        .await
        .map_err(|e| e.to_string())
}
