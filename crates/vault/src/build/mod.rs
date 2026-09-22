//! `vault build` — pages in, one generation out (contract C3).
//!
//! The pipeline, in the order it must run:
//!
//! 1. census the inputs (refuses a non-boolean `public` flag);
//! 2. load and project the corpus (redaction precedes the closure, so a
//!    private intermediate class never enters inferred ancestry);
//! 3. validate — **errors block the bundle**;
//! 4. structural closure (`reason.py`'s BFS, whose ordering the scaffold index
//!    encodes) and Whelk's EL++ closure (which `ontology-inferred.ttl` carries);
//! 5. emit every artefact into a staging directory;
//! 6. promote the staging directory atomically.
//!
//! Step 6 matters: a failed build leaves the previous bundle intact, and a
//! successful one never leaves an obsolete page export behind, because the
//! whole tree is replaced rather than written over.

pub mod generation;
pub mod graph_tiers;
pub mod indexes;
pub mod ngg1;
pub mod okf;
pub mod page_api;
pub mod publish;
pub mod rvdb;
pub mod turtle;
pub mod webvowl;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde_json::Value;
use vault_core::json::{to_compact, to_default_ascii, to_default_utf8, to_indented};
use vault_core::page::{load_vault, Vault};
use vault_core::vocabulary::Vocabulary;

use crate::closure;
use crate::model::Corpus;
use crate::projection;
use crate::validate;
use crate::whelk;

/// What to build and where.
#[derive(Debug, Clone)]
pub struct Options {
    /// The vault root, e.g. `<repo>/knowledge`.
    pub vault_root: PathBuf,
    /// The bundle destination.
    pub out: PathBuf,
    /// The repository the generation sha is read from.
    pub repo_root: PathBuf,
    /// Embed the corpus and emit `ontology-corpus.rvdb`.
    pub with_rvdb: bool,
    /// Emit `api/markdown/` — the title-form markdown mirror.
    ///
    /// Off by default (decided 2026-09-22): it has no consumer, weighs 124 MB
    /// and the Pages budget is 1 GB.
    pub with_markdown_mirror: bool,
    /// Xinference endpoint for `--with-rvdb`.
    pub embed_endpoint: String,
    /// Optional bundle expiry, carried into `.generation.json`.
    pub stale_after: Option<String>,
    /// Write the `publish/` staging to this directory instead of
    /// `<out>/publish`, so the markdown can be promoted separately from the C3
    /// bundle. The credential gate applies either way.
    pub publish_out: Option<PathBuf>,
    /// Also stage `working/`'s `public: true` pages under `publish/`.
    ///
    /// The ontology projections stay `knowledge/`-only regardless: publication
    /// is a per-page decision, OWL membership is not.
    pub with_working_publish: bool,
}

/// What the build produced.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Report {
    /// The generation stamp.
    pub generation: generation::Generation,
    /// Public OWL classes.
    pub class_count: usize,
    /// Public pages.
    pub page_count: usize,
    /// Asserted triples.
    pub triples: usize,
    /// Inferred `subClassOf` pairs.
    pub inferred_triples: usize,
    /// Search index entries.
    pub search_entries: usize,
    /// Graph-tier node count.
    pub graph_nodes: usize,
    /// Graph-tier resolvable edge count.
    pub graph_edges: usize,
    /// Classes Whelk found unsatisfiable. Non-empty is a broken corpus.
    pub unsatisfiable: Vec<String>,
    /// Pages staged under `publish/`, per vault. What `--stats` prints, and
    /// the answer to "did `working/` contribute anything?".
    pub published: std::collections::BTreeMap<String, publish::VaultPublished>,
    /// Artefacts written into the bundle. A build that writes none has not
    /// built anything, and the CLI treats that as a failure.
    pub written: usize,
    /// Where the bundle was written.
    pub out: PathBuf,
}

/// Files staged for one bundle, kept in memory so the promotion is atomic.
struct Staged {
    files: Vec<(String, Vec<u8>)>,
}

impl Staged {
    fn new() -> Self {
        Self { files: Vec::new() }
    }

    fn add(&mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) {
        self.files.push((path.into(), content.into()));
    }

    fn artifacts(&self) -> Vec<generation::Artifact> {
        self.files
            .iter()
            .map(|(name, content)| generation::Artifact::of(name.clone(), content))
            .collect()
    }

    /// Write into a fresh staging directory beside `out`, then swap it in.
    fn promote(&self, out: &Path) -> anyhow::Result<()> {
        let parent = out.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        let stage = parent.join(format!(".publication-{}", std::process::id()));
        if stage.exists() {
            std::fs::remove_dir_all(&stage)?;
        }
        std::fs::create_dir_all(&stage)?;

        for (name, content) in &self.files {
            let target = stage.join(name);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&target, content)
                .with_context(|| format!("writing {}", target.display()))?;
        }

        // Replace the destination wholesale so no obsolete export survives.
        let retired = parent.join(format!(".publication-retired-{}", std::process::id()));
        if out.exists() {
            std::fs::rename(out, &retired)?;
        }
        match std::fs::rename(&stage, out) {
            Ok(()) => {
                if retired.exists() {
                    let _ = std::fs::remove_dir_all(&retired);
                }
                Ok(())
            }
            Err(e) => {
                // Put the previous bundle back before surfacing the failure.
                if retired.exists() {
                    let _ = std::fs::rename(&retired, out);
                }
                let _ = std::fs::remove_dir_all(&stage);
                Err(e).context("promoting the staged bundle")
            }
        }
    }
}

/// An ISO-8601 UTC instant in `CPython`'s `datetime.isoformat()` shape.
#[must_use]
pub fn now_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    let format = time::macros::format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]+00:00"
    );
    now.format(&format)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000+00:00".to_owned())
}

/// Today in `YYYY-MM-DD`, matching Python's `date.today().isoformat()`.
#[must_use]
pub fn today() -> String {
    let now = time::OffsetDateTime::now_utc();
    let format = time::macros::format_description!("[year]-[month]-[day]");
    now.format(&format)
        .unwrap_or_else(|_| "1970-01-01".to_owned())
}

/// Every alias declared by each page, keyed by page id — the search index's
/// `labels` source.
fn alias_map(vault: &Vault) -> HashMap<String, Vec<String>> {
    vault
        .pages
        .iter()
        .map(|p| (p.id.clone(), p.frontmatter.strings("aliases")))
        .collect()
}

/// Run the whole build.
///
/// # Errors
/// The census refusing an input, an identity collision, a validation error, or
/// any I/O failure while staging or promoting the bundle.
#[allow(clippy::too_many_lines)] // The pipeline, in the order it must run.
pub fn run(options: &Options, vocab: &Vocabulary) -> anyhow::Result<Report> {
    let pages_dir = options.vault_root.join("pages");
    let census = projection::inspect_inputs(&pages_dir)?;

    let vault = load_vault(&options.vault_root)?;
    let aliases = alias_map(&vault);
    let raw = Corpus::build(&vault, vocab);

    // --- the credential gate, FIRST ------------------------------------------
    //
    // The union of both vaults' `public: true` pages, and the last gate before
    // bytes leave the machine. It runs *before* validation on purpose. A
    // credential on a page selected for publication is a specific, actionable
    // refusal with its own exit code (2), and running it second would let the
    // general "validation rejected N errors" message (exit 1) swallow it — the
    // reader would be told the corpus is invalid rather than that it was about
    // to publish a key. It is also the cheapest check, so failing here skips
    // the reasoner entirely.
    let working = if options.with_working_publish {
        publish::sibling_working(&options.vault_root)?
    } else {
        None
    };
    let mut publish_vaults: Vec<&Vault> = vec![&vault];
    publish_vaults.extend(working.as_ref());
    let staging = publish::stage(&publish_vaults)?;

    let report = validate::validate(&vault, &raw, vocab);
    anyhow::ensure!(
        report.errors().is_empty(),
        "validation rejected {} error(s); no bundle written. First: {}",
        report.errors().len(),
        report.errors()[0]
    );

    let excluded = projection::excluded_identities(&pages_dir)?;
    let corpus = projection::project(&raw, &excluded)?;

    let structural = closure::compute(&corpus);
    let backlinks = indexes::backlink_index(&corpus);
    let graph = turtle::build_graph(&corpus, vocab, true);
    let reasoning = whelk::reason(&graph);
    let generated = now_iso();

    let class_count = corpus.public_classes().count();
    let page_count = corpus.records.len();

    let mut staged = Staged::new();
    // `publish/` either rides inside the bundle or is promoted on its own.
    if let Some(dir) = &options.publish_out {
        publish::write_to(&staging, dir)?;
    } else {
        for file in &staging.files {
            staged.add(file.path.clone(), file.content.clone());
        }
    }
    staged.add("data/ontology.ttl", turtle::serialise(&graph));
    staged.add(
        "data/ontology-inferred.ttl",
        whelk::inferred_turtle(&reasoning, &generated),
    );

    let scaffold = indexes::scaffold_index(&corpus, &structural, &backlinks, &generated);
    staged.add("data/scaffold-index.json", to_compact(&scaffold)?);
    let prose = indexes::prose_index(&corpus, &generated);
    staged.add("data/prose-index.json", to_default_utf8(&prose)?);

    let search = indexes::search_index(&corpus, &aliases);
    let search_entries = search.as_array().map_or(0, Vec::len);
    staged.add("api/search-index.json", to_indented(&search)?);

    let (documents, domain_index) = page_api::build(&corpus, &structural, &backlinks);
    for doc in &documents {
        staged.add(
            format!("api/pages/{}.json", doc.slug),
            to_indented(&doc.value)?,
        );
    }
    staged.add("api/pages/_domain-index.json", to_indented(&domain_index)?);
    staged.add("api/census.json", to_indented(&census)?);
    staged.add(
        "api/validation-report.json",
        to_indented(&report.summary())?,
    );
    // The context is served from /ns/v2.jsonld; the property IRIs inside it
    // still cite narrativegoldmine.com/ns/v1#. Both paths ship, byte-identical,
    // because consumers exist for each (contract C3).
    let context = okf::context(vocab);
    let context_json = to_indented(&context)?;
    staged.add("context/v1.jsonld", context_json.clone());
    staged.add("ns/v2.jsonld", context_json);

    // WebVOWL graph — the explorer's build input.
    staged.add(
        "data/ontology.json",
        to_default_ascii(&webvowl::build(&corpus))?,
    );

    // NGG1 graph tiers — the explorer physics worker reads these from the
    // published site, so the `.bin` layout is byte-frozen.
    let tiers = graph_tiers::emit(&corpus, &today());
    for file in &tiers.files {
        staged.add(format!("data/graph/{}", file.name), file.bytes.clone());
    }

    if options.with_markdown_mirror {
        for record in corpus.public() {
            // Generated from the projected record, never copied from source: a
            // public page can reference a private one.
            let mut text = String::new();
            if !record.definition.is_empty() {
                text.push_str(&record.definition);
                text.push_str("\n\n");
            }
            text.push_str(&record.body);
            text.push('\n');
            for name in [
                record.page_id.replace('/', "___"),
                record.page_id.replace('/', "%2F"),
            ] {
                staged.add(format!("api/markdown/{name}.md"), text.clone());
            }
        }
    }

    let commit = generation::git_sha(&options.repo_root);
    let generation_id = format!("visionGraph@{commit}");
    for file in okf::bundle(&corpus, vocab, &generation_id) {
        staged.add(format!("okf/{}", file.path), file.content);
    }

    if options.with_rvdb {
        let mut records = rvdb::concept_records(&scaffold, &prose, &generation_id);
        let embedder = rvdb::Xinference::new(options.embed_endpoint.clone());
        rvdb::embed_all(&mut records, &embedder, 96)
            .context("embedding the corpus for --with-rvdb")?;
        let jsonl = rvdb::to_jsonl(&records)?;
        let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(jsonl.as_bytes()));
        staged.add("data/ontology-corpus.rvdb", jsonl);
        staged.add(
            "data/ontology-corpus.rvdb.generation.json",
            to_indented(&rvdb::sidecar(&generation_id, records.len(), &digest))?,
        );
    }

    let sources: Vec<(String, Vec<u8>)> = vault
        .pages
        .iter()
        .map(|p| {
            let bytes = std::fs::read(&p.path).unwrap_or_default();
            (p.rel_path.to_string_lossy().into_owned(), bytes)
        })
        .collect();
    let content_digest = generation::content_digest(sources);

    let gen = generation::generation(
        commit,
        content_digest,
        generated,
        class_count,
        page_count,
        vocab.version,
        options.stale_after.clone(),
        staged.artifacts(),
    );
    staged.add(".generation.json", to_indented(&gen)?);
    staged.promote(&options.out)?;

    Ok(Report {
        generation: gen,
        class_count,
        page_count,
        triples: graph.len(),
        inferred_triples: reasoning.inferred.len(),
        search_entries,
        graph_nodes: tiers.nodes,
        graph_edges: tiers.edges,
        unsatisfiable: reasoning.unsatisfiable,
        published: staging.per_vault,
        // Both destinations count: with `--publish-out` the markdown is
        // promoted separately, and it is still something that was written.
        written: staged.files.len()
            + if options.publish_out.is_some() {
                staging.files.len()
            } else {
                0
            },
        out: options.out.clone(),
    })
}

/// Read a JSON artefact back out of a bundle, for `--verify` and the tests.
///
/// # Errors
/// I/O or JSON decoding failure.
pub fn read_artifact(out: &Path, name: &str) -> anyhow::Result<Value> {
    let path = out.join(name);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_vault(dir: &Path) {
        let pages = dir.join("knowledge/pages");
        std::fs::create_dir_all(&pages).unwrap();
        std::fs::write(
            pages.join("Root Concept.md"),
            "---\ntype: Class\npublic: true\nresource: urn:ngm:class:root-concept\nstatus: stable\ndomain: infrastructure\ndefinition: The root.\nmaturity: established\nquality: 0.4\n---\n",
        )
        .unwrap();
        std::fs::write(
            pages.join("Knowledge Graph.md"),
            "---\ntype: Class\npublic: true\nresource: urn:ngm:class:knowledge-graph\nstatus: stable\ndomain: infrastructure\ndefinition: A graph.\nmaturity: established\nquality: 0.35\nis-a: [\"[[Root Concept]]\"]\nlinks: [\"[[Root Concept]]\"]\n---\n- ### Current Landscape (2026)\n  - Widely adopted.\n",
        )
        .unwrap();
        std::fs::write(
            pages.join("Secret.md"),
            "---\ntype: Class\npublic: false\nresource: urn:ngm:class:secret\nstatus: draft\n---\n",
        )
        .unwrap();
    }

    fn vocab() -> Vocabulary {
        Vocabulary::from_yaml_str(
            r#"
version: 1
namespace: "urn:ngm:class:"
types:
  Class: { owl: "owl:Class", required: [resource] }
relations:
  is-a:     { owl: "rdfs:subClassOf", characteristics: [transitive] }
  requires: { owl: "vc:requires" }
scalars:
  domain:     { type: text, enum: [infrastructure] }
  maturity:   { type: text }
  quality:    { type: number, min: 0, max: 1 }
  definition: { type: text }
"#,
        )
        .unwrap()
    }

    fn build_fixture() -> (tempfile::TempDir, Report) {
        let dir = tempfile::tempdir().unwrap();
        fixture_vault(dir.path());
        let options = Options {
            vault_root: dir.path().join("knowledge"),
            out: dir.path().join("www"),
            repo_root: dir.path().to_path_buf(),
            with_rvdb: false,
            with_markdown_mirror: false,
            with_working_publish: false,
            publish_out: None,
            embed_endpoint: rvdb::DEFAULT_ENDPOINT.to_owned(),
            stale_after: None,
        };
        let report = run(&options, &vocab()).unwrap();
        (dir, report)
    }

    #[test]
    fn emits_every_contract_c3_artefact() {
        let (dir, _) = build_fixture();
        let out = dir.path().join("www");
        for name in [
            "data/ontology.ttl",
            "data/ontology-inferred.ttl",
            "data/scaffold-index.json",
            "data/prose-index.json",
            "api/search-index.json",
            "api/census.json",
            "api/validation-report.json",
            "api/pages/_domain-index.json",
            "context/v1.jsonld",
            "okf/index.md",
            ".generation.json",
        ] {
            assert!(out.join(name).is_file(), "missing {name}");
        }
    }

    #[test]
    fn private_pages_reach_no_artefact() {
        let (dir, report) = build_fixture();
        let out = dir.path().join("www");
        assert_eq!(report.class_count, 2);
        assert!(!out.join("api/pages/secret.json").exists());
        let ttl = std::fs::read_to_string(out.join("data/ontology.ttl")).unwrap();
        assert!(!ttl.contains("ngm:secret"));
    }

    #[test]
    fn the_scaffold_index_is_compact_and_ascii_only() {
        let (dir, _) = build_fixture();
        let raw = std::fs::read_to_string(dir.path().join("www/data/scaffold-index.json")).unwrap();
        assert!(raw.is_ascii());
        assert!(!raw.contains(": "), "compact separators expected");
        assert!(raw.starts_with(r#"{"version":1,"generated":"#));
    }

    #[test]
    fn the_generation_stamps_every_artefact() {
        let (dir, report) = build_fixture();
        let gen: Value = read_artifact(&dir.path().join("www"), ".generation.json").unwrap();
        assert!(gen["id"].as_str().unwrap().starts_with("visionGraph@"));
        assert_eq!(gen["class_count"], 2);
        assert_eq!(gen["vocabulary_version"], 1);
        assert_eq!(gen["content_digest"], report.generation.content_digest);
        let names: Vec<&str> = gen["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"data/scaffold-index.json"));
    }

    #[test]
    fn the_closure_and_the_reasoner_agree_on_the_taxonomy() {
        let (dir, report) = build_fixture();
        let scaffold: Value =
            read_artifact(&dir.path().join("www"), "data/scaffold-index.json").unwrap();
        assert_eq!(
            scaffold["classes"]["knowledge-graph"]["sup"][0],
            "root-concept"
        );
        assert!(report.unsatisfiable.is_empty());
    }

    #[test]
    fn the_prose_index_captures_the_current_landscape() {
        let (dir, _) = build_fixture();
        let prose: Value = read_artifact(&dir.path().join("www"), "data/prose-index.json").unwrap();
        assert_eq!(prose["pages"]["knowledge-graph"]["cl"], "Widely adopted.");
    }

    #[test]
    fn promotion_replaces_the_previous_bundle_wholesale() {
        let (dir, _) = build_fixture();
        let out = dir.path().join("www");
        std::fs::write(out.join("api/pages/stale.json"), "{}").unwrap();
        let options = Options {
            vault_root: dir.path().join("knowledge"),
            out: out.clone(),
            repo_root: dir.path().to_path_buf(),
            with_rvdb: false,
            with_markdown_mirror: false,
            with_working_publish: false,
            publish_out: None,
            embed_endpoint: rvdb::DEFAULT_ENDPOINT.to_owned(),
            stale_after: None,
        };
        run(&options, &vocab()).unwrap();
        assert!(!out.join("api/pages/stale.json").exists());
    }

    #[test]
    fn a_validation_error_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fixture_vault(dir.path());
        std::fs::write(
            dir.path().join("knowledge/pages/Duplicate.md"),
            "---\ntype: Class\npublic: true\nresource: urn:ngm:class:root-concept\nstatus: draft\n---\n",
        )
        .unwrap();
        let out = dir.path().join("www");
        let options = Options {
            vault_root: dir.path().join("knowledge"),
            out: out.clone(),
            repo_root: dir.path().to_path_buf(),
            with_rvdb: false,
            with_markdown_mirror: false,
            with_working_publish: false,
            publish_out: None,
            embed_endpoint: rvdb::DEFAULT_ENDPOINT.to_owned(),
            stale_after: None,
        };
        assert!(run(&options, &vocab()).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn the_timestamp_has_python_isoformat_shape() {
        let s = now_iso();
        assert!(s.ends_with("+00:00"), "{s}");
        assert_eq!(s.len(), "2026-09-22T00:00:00.000000+00:00".len(), "{s}");
    }
}
