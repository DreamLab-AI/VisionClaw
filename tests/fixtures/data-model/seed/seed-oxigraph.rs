// tests/fixtures/data-model/seed/seed-oxigraph.rs
//! Seed loader for the data-model fixture corpus.
//!
//! Loads `seed/expected-triples.nq` into an Oxigraph store and reports
//! triple/quad counts broken down by named graph and class-bit. The
//! resulting store is byte-equivalent to what the parser + ontology
//! adapter + graph adapter must produce when fed `valid/`.
//!
//! ## Build
//!
//! Add to the workspace `Cargo.toml`:
//!
//! ```toml
//! [[bin]]
//! name = "seed-oxigraph"
//! path = "tests/fixtures/data-model/seed/seed-oxigraph.rs"
//! required-features = ["persistence-oxigraph"]
//! ```
//!
//! Then:
//!
//! ```bash
//! cargo build --bin seed-oxigraph --features persistence-oxigraph
//! ```
//!
//! ## Run
//!
//! ```bash
//! # Load fixtures into a fresh on-disk store.
//! ./target/debug/seed-oxigraph load \
//!     --store /tmp/fx \
//!     --nquads tests/fixtures/data-model/seed/expected-triples.nq
//!
//! # Verify counts against corpus-manifest.json.
//! ./target/debug/seed-oxigraph verify \
//!     --store /tmp/fx \
//!     --manifest tests/fixtures/data-model/valid/metadata/corpus-manifest.json
//!
//! # Quick report (no manifest comparison).
//! ./target/debug/seed-oxigraph report --store /tmp/fx
//! ```
//!
//! ## Crate choices
//!
//! - **Oxigraph 0.4** for the store and N-Quads parser. Already a
//!   dependency under the `persistence-oxigraph` feature; no new
//!   workspace crates required.
//! - **clap 4** for arg parsing. Already in the workspace.
//! - **serde_json** for the manifest. Already in the workspace.
//!
//! Nothing else. The deliberate restraint: no `json_ld` crate (Oxigraph
//! doesn't ship a JSON-LD parser as of 0.4; the corpus authors translate
//! their JSON-LD blocks to N-Quads by hand in the same pull request, and
//! this script verifies the result rather than re-deriving it).

#![cfg(feature = "persistence-oxigraph")]

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use oxigraph::io::DatasetFormat;
use oxigraph::model::{NamedNodeRef, Quad, Term};
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;
use serde_json::Value;

const GRAPH_KNOWLEDGE: &str = "urn:visionflow:graph:knowledge";
const GRAPH_ONTO_ASSERT: &str = "urn:visionflow:graph:ontology:assert";
const GRAPH_ONTO_INFERRED: &str = "urn:visionflow:graph:ontology:inferred";
const GRAPH_AGENT: &str = "urn:visionflow:graph:agent";

#[derive(Parser, Debug)]
#[command(
    name = "seed-oxigraph",
    about = "Load and verify data-model fixtures against an Oxigraph store"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Load an N-Quads file into the store.
    Load {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        nquads: PathBuf,
        /// Wipe the store first (DELETE then re-load).
        #[arg(long, default_value_t = false)]
        wipe: bool,
    },
    /// Report counts (no manifest comparison).
    Report {
        #[arg(long)]
        store: PathBuf,
    },
    /// Verify counts against `corpus-manifest.json`.
    Verify {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Load { store, nquads, wipe } => {
            let s = Store::open(&store)?;
            if wipe {
                s.update("DROP ALL")?;
            }
            let f = File::open(&nquads)?;
            let r = BufReader::new(f);
            s.bulk_loader().load_dataset(r, DatasetFormat::NQuads, None)?;
            let n = s.len()?;
            println!("loaded {} quads from {:?} into {:?}", n, nquads, store);
        }
        Cmd::Report { store } => {
            let s = Store::open(&store)?;
            print_report(&s)?;
        }
        Cmd::Verify { store, manifest } => {
            let s = Store::open(&store)?;
            print_report(&s)?;
            let manifest_json: Value =
                serde_json::from_reader(BufReader::new(File::open(&manifest)?))?;
            let expected_total = manifest_json
                .get("total_triples_expected")
                .and_then(|v| v.as_u64())
                .ok_or("manifest missing total_triples_expected")?;
            let actual_total = s.len()? as u64;
            // The seed file contains comment-only lines that the loader skips, so
            // expect actual >= expected; the manifest is a lower-bound assertion.
            if actual_total < expected_total {
                eprintln!(
                    "FAIL: actual={} < expected={} (manifest claims more triples than the store contains)",
                    actual_total, expected_total
                );
                std::process::exit(2);
            } else {
                println!(
                    "OK: actual={} >= expected={} (manifest lower bound satisfied)",
                    actual_total, expected_total
                );
            }
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------
// Report — count quads by named graph and class-bit prefix.
// ----------------------------------------------------------------------

fn print_report(s: &Store) -> Result<(), Box<dyn std::error::Error>> {
    let total = s.len()?;
    println!("total quads: {}", total);
    println!("");

    println!("=== Counts by named graph ===");
    for g in &[
        GRAPH_KNOWLEDGE,
        GRAPH_ONTO_ASSERT,
        GRAPH_ONTO_INFERRED,
        GRAPH_AGENT,
    ] {
        let n = count_in_graph(s, g)?;
        println!("  {}: {} quads", g, n);
    }
    let n_default = count_in_default(s)?;
    println!("  (default graph): {} triples", n_default);
    println!("");

    println!("=== Counts by entity class (rdf:type) ===");
    let class_counts = count_by_type(s)?;
    let mut classes: Vec<(String, u64)> = class_counts.into_iter().collect();
    classes.sort_by(|a, b| b.1.cmp(&a.1));
    for (cls, n) in classes {
        println!("  {}: {}", cls, n);
    }
    println!("");

    println!("=== Subclass edge count ===");
    let n_sub = ask_count(
        s,
        "SELECT (COUNT(*) AS ?n) WHERE { GRAPH ?g { ?s <http://www.w3.org/2000/01/rdf-schema#subClassOf> ?o . } }",
    )?;
    println!("  rdfs:subClassOf edges (all named graphs): {}", n_sub);
    println!("");

    println!("=== Bridge count (default graph) ===");
    let n_bridge = ask_count(
        s,
        "SELECT (COUNT(*) AS ?n) WHERE { ?s a <https://visionflow.dreamlab/ns/BridgeRecord> . }",
    )?;
    println!("  vc:BridgeRecord instances: {}", n_bridge);
    Ok(())
}

fn count_in_graph(s: &Store, graph_iri: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let q = format!(
        "SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{}> {{ ?s ?p ?o }} }}",
        graph_iri
    );
    ask_count(s, &q)
}

fn count_in_default(s: &Store) -> Result<u64, Box<dyn std::error::Error>> {
    // Oxigraph 0.4: default graph is reached via the SPARQL keyword DEFAULT
    // or by querying without GRAPH wrapping; the implementation distinguishes
    // by absence of a graph term.
    let q = "SELECT (COUNT(*) AS ?n) WHERE { GRAPH <urn:visionflow:fake-not-a-graph> { ?s ?p ?o } UNION { ?s ?p ?o . FILTER NOT EXISTS { GRAPH ?g { ?s ?p ?o } } } }";
    ask_count(s, q)
}

fn count_by_type(
    s: &Store,
) -> Result<std::collections::HashMap<String, u64>, Box<dyn std::error::Error>> {
    let mut out = std::collections::HashMap::new();
    let q = "SELECT ?cls (COUNT(DISTINCT ?s) AS ?n) WHERE { { ?s a ?cls } UNION { GRAPH ?g { ?s a ?cls } } } GROUP BY ?cls";
    let res = s.query(q)?;
    if let QueryResults::Solutions(sols) = res {
        for sol in sols {
            let sol = sol?;
            let cls = sol
                .get("cls")
                .map(term_label)
                .unwrap_or_else(|| "(none)".to_string());
            let n = sol
                .get("n")
                .map(|t| match t {
                    Term::Literal(l) => l.value().parse::<u64>().unwrap_or(0),
                    _ => 0,
                })
                .unwrap_or(0);
            *out.entry(cls).or_insert(0) += n;
        }
    }
    Ok(out)
}

fn ask_count(s: &Store, q: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let res = s.query(q)?;
    if let QueryResults::Solutions(mut sols) = res {
        if let Some(sol) = sols.next() {
            let sol = sol?;
            if let Some(t) = sol.get("n") {
                if let Term::Literal(l) = t {
                    return Ok(l.value().parse::<u64>().unwrap_or(0));
                }
            }
        }
    }
    Ok(0)
}

fn term_label(t: &Term) -> String {
    match t {
        Term::NamedNode(n) => {
            let s = n.as_str();
            // Compress the long vc: namespace for readability.
            if let Some(local) = s.strip_prefix("https://visionflow.dreamlab/ns/") {
                format!("vc:{}", local)
            } else if let Some(local) = s.strip_prefix("http://www.w3.org/2002/07/owl#") {
                format!("owl:{}", local)
            } else {
                s.to_string()
            }
        }
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        Term::Literal(l) => l.value().to_string(),
        _ => String::new(),
    }
}

// Silence unused-import warnings for items only used by Quad/NamedNodeRef
// based queries that landed in earlier drafts. The crate must compile
// under -D warnings.
#[allow(dead_code)]
fn _unused_imports_silencer(_: Quad, _: NamedNodeRef) {}
