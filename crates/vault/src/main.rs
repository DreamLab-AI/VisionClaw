//! The `vault` binary: contract C2's command surface.
//!
//! Every subcommand accepts `--json`, because the primary caller is an agent
//! in a Bash tool, not a person at a terminal. Exit codes are contractual:
//! `0` success, `1` a failed check, `2` a migration the vocabulary does not
//! cover.

#![deny(unsafe_code)]
#![warn(clippy::pedantic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use vault::{
    build, conflicts, create, edit, gate, migrate, model::Corpus, nostr, propose, repair, validate,
};
use vault_core::graph::VaultGraph;
use vault_core::page::{load_vault, Vault, VaultKind};
use vault_core::proposal::Level;
use vault_core::vocabulary::Vocabulary;

/// The single door onto the sovereign corpus.
#[derive(Debug, Parser)]
#[command(name = "vault", version, about, long_about = None)]
struct Cli {
    /// The repository root holding `ontology/vocabulary.yaml`, `knowledge/`
    /// and `working/`. Defaults to the nearest ancestor that has one.
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    /// Emit machine-readable JSON instead of prose.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// OKF conformance, vocabulary agreement, link integrity and the public gate.
    Validate(ValidateArgs),
    /// Search titles and aliases.
    Find(FindArgs),
    /// Expand from seed pages along declared edge types.
    Retrieve(RetrieveArgs),
    /// The taxonomy tree beneath a page.
    Tree(TreeArgs),
    /// Guarded mutation; refused without `--expect`.
    Edit(EditArgs),
    /// Write a new knowledge page from a staged file; never overwrites.
    Create(CreateArgs),
    /// Build a `PatchProposal` and post it as a forum 31402.
    Propose(ProposeArgs),
    /// The autonomous continuation gate.
    Gate(GateArgs),
    /// Semantic conflict detection.
    Conflicts(ConflictsArgs),
    /// Pages to one generation.
    Build(BuildArgs),
    /// The one-shot fence-to-properties conversion.
    Migrate(MigrateArgs),
    /// Fix corpus defects in place.
    #[command(subcommand)]
    Repair(RepairCommand),
}

#[derive(Debug, Subcommand)]
enum RepairCommand {
    /// Close or remove unmatched code-fence openers.
    Fences(RepairArgs),
    /// Rewrite Logseq outliner bodies (heading bullets, `{:height}` image
    /// sizing, repeated inlined lines) as Obsidian markdown.
    Bodies(BodiesArgs),
}

#[derive(Debug, Args)]
struct BodiesArgs {
    /// Which vault to convert: `knowledge`, `working` or `all`.
    #[arg(long, default_value = "all", value_parser = VAULT_VALUES)]
    vault: String,
    /// Compute everything, write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Write nothing and exit 1 when any page still needs converting — the
    /// residue gate.
    #[arg(long)]
    check: bool,
    /// Write the converted page list here.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RepairArgs {
    /// Which vault to repair: `knowledge`, `working` or `all`.
    #[arg(long, default_value = "all", value_parser = VAULT_VALUES)]
    vault: String,
    /// Compute everything, write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Write the full change list here.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct VaultSelector {
    /// Which vault to read: `knowledge`, `working` or `all`.
    #[arg(long, default_value = "knowledge", value_parser = VAULT_VALUES)]
    vault: String,
}

impl VaultSelector {
    fn kinds(&self) -> Vec<VaultKind> {
        vault_kinds(&self.vault)
    }
}

/// The three legal `--vault` values. Used as clap's `value_parser` on every
/// subcommand, so an unknown value is rejected by the parser (exit 2) rather
/// than silently falling through to `knowledge`.
pub(crate) const VAULT_VALUES: [&str; 3] = ["knowledge", "working", "all"];

/// The vaults a `--vault` value names.
///
/// `all` means BOTH, everywhere — `migrate` and `build` used to accept the
/// word and quietly run `knowledge/` only, which is the worst of the three
/// possible behaviours: it looks like it did what you asked.
///
/// The `_` arm is `knowledge` because the parser has already rejected anything
/// that is not one of [`VAULT_VALUES`]; it is not a silent default.
fn vault_kinds(value: &str) -> Vec<VaultKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "working" => vec![VaultKind::Working],
        "all" => vec![VaultKind::Knowledge, VaultKind::Working],
        _ => vec![VaultKind::Knowledge],
    }
}

#[derive(Debug, Args)]
struct ValidateArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// Treat warnings as failures.
    #[arg(long)]
    strict: bool,
}

#[derive(Debug, Args)]
struct FindArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// The search text.
    #[arg(long)]
    query: String,
    /// Restrict to one OKF `type`.
    #[arg(long, value_name = "T")]
    r#type: Option<String>,
    /// Maximum hits.
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Also match on token overlap.
    #[arg(long)]
    fuzzy: bool,
}

#[derive(Debug, Args)]
struct RetrieveArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// Seed page ids, titles or aliases.
    #[arg(required = true)]
    ids: Vec<String>,
    /// Per-edge-type depths, e.g. `is-a=2,requires=1`. An edge type absent
    /// from this list is never traversed.
    #[arg(long, default_value = "is-a=1")]
    expand: String,
    /// Cap on expanded documents.
    #[arg(long, default_value_t = 50)]
    max_documents: usize,
}

#[derive(Debug, Args)]
struct TreeArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// The root page.
    id: String,
    /// How many levels to descend.
    #[arg(long, default_value_t = 3)]
    depth: usize,
    /// The edge type to follow, inverted.
    #[arg(long, default_value = "is-a")]
    predicate: String,
}

#[derive(Debug, Args)]
struct EditArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// The page id.
    id: String,
    /// `key=value`, or `key+=value` to append to a list.
    #[arg(long = "set", value_name = "K=V")]
    sets: Vec<String>,
    /// Remove a key.
    #[arg(long = "unset", value_name = "K")]
    unsets: Vec<String>,
    /// The declared blast radius, e.g. `docs=1,blocks=1`. Required.
    #[arg(long)]
    expect: Option<String>,
    /// Show the result without writing it.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct CreateArgs {
    /// The staged page in full. Its `title` names the file:
    /// `knowledge/pages/<title>.md`.
    file: PathBuf,
    /// `key=value`, or `key+=value` to append to a list, applied in the same
    /// write (e.g. `status=stable`, `verified+={by: …, at: …}`).
    #[arg(long = "set", value_name = "K=V")]
    sets: Vec<String>,
    /// Remove a key from the staged page before writing.
    #[arg(long = "unset", value_name = "K")]
    unsets: Vec<String>,
    /// The declared blast radius: `docs=1`, optionally `,blocks=N` for the
    /// keys `--set`/`--unset` change. Required.
    #[arg(long)]
    expect: Option<String>,
    /// Check everything and report, without writing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ProposeArgs {
    /// The subject: a page id, title or `resource` IRI.
    iri: String,
    /// How far-reaching the change is.
    #[arg(long, default_value = "content")]
    level: String,
    /// Why the proposer believes the change is right.
    #[arg(long, default_value = "")]
    hypothesis: String,
    /// The proposed page in full, or a DIRECTORY of them.
    ///
    /// A file whose frontmatter declares a `title` no page has, with a subject
    /// that names no existing page by id or title, is a **creation**
    /// (`kind: "create"`, diffed against `/dev/null`); apply it with
    /// `vault create`.
    ///
    /// A directory is a manifest: one `*.md` per page, named by page id
    /// (`<root>/<id>.md`, mirroring `pages/`), and the result is ONE grouped
    /// proposal whose `diff` is a multi-file unified diff. That is how a
    /// decision spanning 513 pages goes through the gate once instead of 513
    /// times. A manifest entry identical to the corpus is dropped; one naming a
    /// page the vault does not hold refuses the proposal, unless it declares
    /// `title: <that id>`, which makes it a creation.
    #[arg(long)]
    diff: Option<PathBuf>,
    /// A grouped proposal's title. Its subject IRI is
    /// `urn:ngm:proposal:<slug of title>`; without it, the positional subject
    /// is slugged instead. Ignored for a single-page proposal.
    #[arg(long)]
    title: Option<String>,
    /// The proposing actor.
    #[arg(long, default_value = "process:vault/1.0")]
    proposer: String,
    /// Print the signed event instead of publishing it.
    #[arg(long)]
    dry_run: bool,
    /// The relay to publish to.
    #[arg(long, default_value = "ws://localhost:7777", env = "VAULT_RELAY_URL")]
    relay: String,
    /// The 32-byte hex secret key to sign with.
    #[arg(long, env = "VAULT_NOSTR_SECRET")]
    secret: Option<String>,
}

#[derive(Debug, Args)]
struct GateArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// `quick` (validate only) or `full` (+ conflicts + Whelk).
    #[arg(long, default_value = "quick")]
    tier: String,
}

#[derive(Debug, Args)]
struct ConflictsArgs {
    #[command(flatten)]
    selector: VaultSelector,
    /// The severity at or above which a conflict fails the run.
    #[arg(long, default_value = "high")]
    severity: String,
}

#[derive(Debug, Args)]
struct BuildArgs {
    /// The bundle destination.
    #[arg(long)]
    out: PathBuf,
    /// Which vault to build: `knowledge`, `working` or `all`.
    ///
    /// `all` keeps the ontology projections on `knowledge/` — OWL membership is
    /// not a per-page decision — and widens the `publish/` staging to
    /// `working/`'s `public: true` pages.
    #[arg(long, default_value = "knowledge", value_parser = VAULT_VALUES)]
    vault: String,
    /// Print the counts after the build.
    #[arg(long)]
    stats: bool,
    /// Write the `publish/` staging to this directory instead of `<out>/publish`.
    ///
    /// For a publication step that promotes the markdown separately from the
    /// C3 bundle; the credential gate applies either way.
    #[arg(long)]
    publish_out: Option<PathBuf>,
    /// Embed the corpus and emit the portable `ontology-corpus.records.jsonl`. Requires the
    /// embedder; the rest of the build works without it.
    #[arg(long)]
    with_rvdb: bool,
    /// Also emit `api/markdown/` — the title-form markdown mirror. Off by
    /// default: no consumer, 124 MB, and the Pages budget is 1 GB.
    #[arg(long)]
    with_markdown_mirror: bool,
    /// The Xinference endpoint used by `--with-rvdb`.
    #[arg(
        long,
        default_value = build::rvdb::DEFAULT_ENDPOINT,
        env = "VAULT_EMBED_ENDPOINT"
    )]
    embed_endpoint: String,
    /// An expiry to stamp into `.generation.json`.
    #[arg(long)]
    stale_after: Option<String>,
}

#[derive(Debug, Args)]
struct MigrateArgs {
    /// The only supported migration.
    #[arg(long, required = true)]
    fences_to_properties: bool,
    /// Which vault to migrate: `knowledge`, `working` or `all`.
    #[arg(long, default_value = "knowledge", value_parser = VAULT_VALUES)]
    vault: String,
    /// Which tree within each vault: `pages`, `journals` or `all`.
    ///
    /// `--only journals` gives the 1,230 journal pages their first pass without
    /// touching `pages/`, which is the difference between a reviewable diff and
    /// a 10,458-file one.
    #[arg(long, default_value = "all", value_parser = ["pages", "journals", "all"])]
    only: String,
    /// Proceed even when a page infers a type its vault forbids (exit 2 by
    /// default). Read the report before using this.
    #[arg(long)]
    allow_type_fallback: bool,
    /// Compute everything, write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Write the full diff and counts here.
    #[arg(long)]
    report: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("vault: {e:#}");
            ExitCode::from(1)
        }
    }
}

/// Find the repository root: the nearest ancestor with `ontology/vocabulary.yaml`.
fn resolve_repo(explicit: Option<&Path>) -> anyhow::Result<(PathBuf, Vocabulary)> {
    let start = match explicit {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let (path, vocab) = Vocabulary::discover(&start).with_context(|| {
        format!(
            "looking for ontology/vocabulary.yaml from {}",
            start.display()
        )
    })?;
    let root = path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(start);
    Ok((root, vocab))
}

fn load(root: &Path, kinds: &[VaultKind]) -> anyhow::Result<Vec<Vault>> {
    kinds
        .iter()
        .map(|kind| {
            let dir = root.join(kind.dir_name());
            load_vault(&dir).with_context(|| format!("loading {}", dir.display()))
        })
        .collect()
}

fn emit(json: bool, value: &serde_json::Value, prose: impl FnOnce()) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        prose();
    }
    Ok(())
}

#[allow(clippy::too_many_lines)] // One arm per subcommand; splitting hides the surface.
/// The tree a vault's `pages/` or `journals/` lives in.
fn root_of(root: &Path, kind: VaultKind, journals: bool) -> PathBuf {
    root.join(kind.dir_name())
        .join(if journals { "journals" } else { "pages" })
}

// One arm per subcommand. Splitting it would scatter the CLI's dispatch table
// across several functions and make the surface harder to read, not easier.
#[allow(clippy::too_many_lines)]
fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let (root, vocab) = resolve_repo(cli.repo.as_deref())?;

    match &cli.command {
        Command::Validate(args) => {
            let mut failed = false;
            let mut reports = serde_json::Map::new();
            for vault in load(&root, &args.selector.kinds())? {
                let corpus = Corpus::build(&vault, &vocab);
                let report = validate::validate(&vault, &corpus, &vocab);
                failed |=
                    !report.errors().is_empty() || (args.strict && !report.warnings().is_empty());
                if !cli.json {
                    println!(
                        "[{}] {} pages, {} with ontology, {} public — {} errors, {} warnings",
                        vault.kind.dir_name(),
                        report.total_pages,
                        report.pages_with_ontology,
                        report.public_pages,
                        report.errors().len(),
                        report.warnings().len()
                    );
                    for issue in report.errors().iter().take(30) {
                        println!("  {issue}");
                    }
                }
                reports.insert(
                    vault.kind.dir_name().to_owned(),
                    serde_json::to_value(report.detailed())?,
                );
            }
            emit(cli.json, &serde_json::Value::Object(reports), || {})?;
            Ok(ExitCode::from(u8::from(failed)))
        }

        Command::Find(args) => {
            let mut hits = Vec::new();
            for vault in load(&root, &args.selector.kinds())? {
                let graph = VaultGraph::build(&vault, &vocab);
                hits.extend(graph.find(
                    &args.query,
                    args.r#type.as_deref(),
                    args.limit,
                    args.fuzzy,
                ));
            }
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            hits.truncate(args.limit);
            let value = serde_json::to_value(&hits)?;
            emit(cli.json, &value, || {
                for hit in &hits {
                    println!("{:.2}  {}", hit.score, hit.id);
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Retrieve(args) => {
            let expand = parse_expansion(&args.expand)?;
            let vault = load(&root, &args.selector.kinds())?
                .into_iter()
                .next()
                .context("no vault selected")?;
            let graph = VaultGraph::build(&vault, &vocab);
            let result = graph.retrieve(&args.ids, &expand, args.max_documents);
            let value = serde_json::to_value(&result)?;
            emit(cli.json, &value, || {
                for seed in &result.seeds {
                    println!("seed  {}", seed.id);
                }
                for node in &result.expanded {
                    println!(
                        "  +{}  {}  (via {} from {})",
                        node.depth, node.id, node.via, node.from
                    );
                }
                if result.truncated {
                    println!("(truncated at --max-documents)");
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Tree(args) => {
            let vault = load(&root, &args.selector.kinds())?
                .into_iter()
                .next()
                .context("no vault selected")?;
            let graph = VaultGraph::build(&vault, &vocab);
            let tree = graph
                .tree(&args.id, &args.predicate, args.depth)
                .with_context(|| format!("no page matching `{}`", args.id))?;
            let value = serde_json::to_value(&tree)?;
            emit(cli.json, &value, || print_tree(&tree, 0))?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Edit(args) => {
            let Some(expect_raw) = &args.expect else {
                anyhow::bail!(
                    "refused: --expect must declare docs and blocks; \
                     e.g. --expect docs=1,blocks=1"
                );
            };
            let expect = edit::Expectation::parse(expect_raw).map_err(anyhow::Error::msg)?;
            let mut changes: Vec<edit::Change> = args
                .sets
                .iter()
                .map(|s| edit::Change::parse_set(s).map_err(anyhow::Error::msg))
                .collect::<anyhow::Result<_>>()?;
            changes.extend(
                args.unsets
                    .iter()
                    .map(|k| edit::Change::Unset { key: k.clone() }),
            );

            let vault = load(&root, &args.selector.kinds())?
                .into_iter()
                .next()
                .context("no vault selected")?;
            let page = vault
                .get(&args.id)
                .with_context(|| format!("no page with id `{}`", args.id))?;
            let outcome = edit::apply(page, &changes, expect).map_err(anyhow::Error::msg)?;
            if !args.dry_run {
                std::fs::write(&page.path, &outcome.rendered)
                    .with_context(|| format!("writing {}", page.path.display()))?;
            }
            let value = serde_json::to_value(&outcome)?;
            emit(cli.json, &value, || {
                println!(
                    "{}: {} document(s), {} key(s){}",
                    outcome.page,
                    outcome.docs,
                    outcome.blocks,
                    if args.dry_run { " (dry run)" } else { "" }
                );
                for (key, [before, after]) in &outcome.changes {
                    println!(
                        "  {key}: {} -> {}",
                        before.as_deref().unwrap_or("(absent)"),
                        after.as_deref().unwrap_or("(removed)")
                    );
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Create(args) => {
            let expect = edit::Expectation::parse(args.expect.as_deref().unwrap_or(""))
                .map_err(anyhow::Error::msg)?;
            let mut changes: Vec<edit::Change> = args
                .sets
                .iter()
                .map(|s| edit::Change::parse_set(s).map_err(anyhow::Error::msg))
                .collect::<anyhow::Result<_>>()?;
            changes.extend(
                args.unsets
                    .iter()
                    .map(|k| edit::Change::Unset { key: k.clone() }),
            );
            let staged = std::fs::read_to_string(&args.file)
                .with_context(|| format!("reading {}", args.file.display()))?;
            let vault = load(&root, &[VaultKind::Knowledge])?
                .into_iter()
                .next()
                .context("no knowledge vault")?;

            // Every refusal is exit 2 with nothing written, and under `--json`
            // stdout is still one JSON document naming why.
            let refuse = |error: &create::CreateError| -> anyhow::Result<ExitCode> {
                let value = json!({
                    "created": false,
                    "file": args.file.display().to_string(),
                    "code": error.code(),
                    "message": error.to_string(),
                    "blockers": error.blockers(),
                });
                emit(cli.json, &value, || {})?;
                eprintln!("vault: {error}");
                Ok(ExitCode::from(2))
            };
            let outcome = match create::prepare(&vault, &vocab, &staged, &changes, expect) {
                Ok(outcome) => outcome,
                Err(error) => return refuse(&error),
            };
            if !args.dry_run {
                if let Err(e) = create::write(&outcome) {
                    return match e.downcast_ref::<create::CreateError>() {
                        Some(error) => refuse(error),
                        None => Err(e),
                    };
                }
            }
            let mut value = serde_json::to_value(&outcome)?;
            value["dry_run"] = json!(args.dry_run);
            emit(cli.json, &value, || {
                println!(
                    "{} {}{}",
                    if args.dry_run {
                        "would create"
                    } else {
                        "created"
                    },
                    outcome.path,
                    if args.dry_run { " (dry run)" } else { "" }
                );
                for (key, [before, after]) in &outcome.changes {
                    println!(
                        "  {key}: {} -> {}",
                        before.as_deref().unwrap_or("(absent)"),
                        after.as_deref().unwrap_or("(removed)")
                    );
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Propose(args) => {
            let level: Level = args.level.parse().map_err(anyhow::Error::msg)?;
            // The signing key FIRST, before the vault is even loaded.
            //
            // The signed event IS the artefact, so a proposal without a key
            // cannot be produced at any level — `--dry-run` included. Resolving
            // it last meant a whole batch paid for the corpus load, the conflict
            // detector and Whelk and *then* failed on a missing environment
            // variable. The cheapest check that can refuse the command belongs
            // at the front of it.
            let secret = args
                .secret
                .clone()
                .context("a signing key is required: --secret or VAULT_NOSTR_SECRET")?;
            let key = nostr::signing_key_from_hex(&secret).map_err(anyhow::Error::msg)?;
            let vault = load(&root, &[VaultKind::Knowledge])?
                .into_iter()
                .next()
                .context("no knowledge vault")?;
            let corpus = Corpus::build(&vault, &vocab);
            // The base generation the proposal applies to — `+dirty` when the
            // corpus it read has uncommitted changes, exactly as `vault build`
            // marks its bundle.
            let dirty = build::generation::git_dirty(
                &root,
                &[&root.join("knowledge"), &root.join("ontology")],
            );
            let generation =
                build::generation::generation_id(&build::generation::git_sha(&root), dirty);
            let options = propose::Options {
                subject: args.iri.clone(),
                title: args.title.clone(),
                level,
                hypothesis: args.hypothesis.clone(),
                diff_source: args.diff.clone(),
                proposer: args.proposer.clone(),
                now: time::OffsetDateTime::now_utc(),
            };
            let proposal = propose::build(&vault, &corpus, &vocab, &generation, &options)?;

            // A progress note, not output: stderr, so `--json` stdout stays
            // exactly one JSON document.
            if proposal.is_grouped() {
                eprintln!(
                    "grouped proposal: {} pages, subject `{}`",
                    proposal.pages.len(),
                    proposal.iri
                );
            }
            if !proposal.is_postable() {
                let value = serde_json::to_value(&proposal)?;
                emit(cli.json, &value, || {
                    println!("BLOCKED — not posted:");
                    for blocker in &proposal.blockers {
                        println!("  {blocker}");
                    }
                })?;
                return Ok(ExitCode::from(1));
            }

            let pubkey = nostr::public_key_hex(&key);
            let created_at =
                u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp()).unwrap_or_default();
            let unsigned = nostr::action_request(&proposal, &pubkey, created_at)
                .map_err(anyhow::Error::msg)?;
            let signed = nostr::sign(unsigned, &key).map_err(anyhow::Error::msg)?;

            if args.dry_run {
                let value = json!({ "proposal": proposal, "event": signed });
                emit(cli.json, &value, || {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&signed).unwrap_or_default()
                    );
                })?;
                return Ok(ExitCode::SUCCESS);
            }

            let id = nostr::publish(&signed, &args.relay)?;
            let value = json!({ "proposal": proposal, "event_id": id, "relay": args.relay });
            emit(cli.json, &value, || {
                println!("posted 31402 {id} to {}", args.relay);
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Gate(args) => {
            let tier = gate::Tier::parse(&args.tier);
            let vault = load(&root, &args.selector.kinds())?
                .into_iter()
                .next()
                .context("no vault selected")?;
            let corpus = Corpus::build(&vault, &vocab);
            let report = validate::validate(&vault, &corpus, &vocab);
            let graph = (tier == gate::Tier::Full && report.errors().is_empty())
                .then(|| build::turtle::build_graph(&corpus, &vocab, false));
            let verdict = gate::run(&report, &corpus, graph.as_ref(), tier);
            let value = serde_json::to_value(&verdict)?;
            emit(cli.json, &value, || {
                println!(
                    "[gate:{}] {} — {}",
                    verdict.tier,
                    verdict.gate.to_uppercase(),
                    verdict.caveat
                );
                for check in &verdict.checks {
                    println!("  [{}] {}: {}", check.status, check.name, check.detail);
                }
            })?;
            Ok(ExitCode::from(
                u8::try_from(verdict.exit_code()).unwrap_or(1),
            ))
        }

        Command::Conflicts(args) => {
            let vault = load(&root, &args.selector.kinds())?
                .into_iter()
                .next()
                .context("no vault selected")?;
            let corpus = Corpus::build(&vault, &vocab);
            let report = conflicts::analyse(&corpus, &args.severity);
            let value = serde_json::to_value(&report)?;
            emit(cli.json, &value, || {
                println!("Conflicts: {} total", report.summary.total);
                for conflict in report.conflicts.iter().take(40) {
                    println!(
                        "  [{:?}] {}: {}",
                        conflict.severity, conflict.kind, conflict.detail
                    );
                }
            })?;
            Ok(ExitCode::from(
                u8::try_from(report.exit_code()).unwrap_or(1),
            ))
        }

        Command::Build(args) => {
            // The ontology projections are always built from ONE vault: OWL,
            // the indexes and the graph tiers are `knowledge/`'s by definition.
            // `--vault all` widens the `publish/` staging to `working/`'s
            // `public: true` pages, which is the only thing "both vaults" can
            // mean for a build.
            let kinds = vault_kinds(&args.vault);
            let kind = if kinds == [VaultKind::Working] {
                VaultKind::Working
            } else {
                VaultKind::Knowledge
            };
            let options = build::Options {
                with_working_publish: kinds.contains(&VaultKind::Working)
                    && kind == VaultKind::Knowledge,
                publish_out: args.publish_out.clone(),
                vault_root: root.join(kind.dir_name()),
                out: args.out.clone(),
                repo_root: root.clone(),
                with_rvdb: args.with_rvdb,
                with_markdown_mirror: args.with_markdown_mirror,
                embed_endpoint: args.embed_endpoint.clone(),
                stale_after: args.stale_after.clone(),
            };
            let report = match build::run(&options, &vocab) {
                Ok(report) => report,
                Err(e) => {
                    // A credential on a page selected for publication is not a
                    // generic failure: it is the corpus refusing to publish
                    // something it must not. Exit 2, the same code a migration
                    // the vocabulary does not cover uses.
                    if let Some(secrets) = e.downcast_ref::<build::publish::SecretsFound>() {
                        eprintln!("vault: {secrets}");
                        return Ok(ExitCode::from(2));
                    }
                    // Two artefacts claiming one output path: the same refusal
                    // class — the corpus does not permit this build as it stands.
                    if let Some(clash) = e.downcast_ref::<build::OutputCollision>() {
                        eprintln!("vault: {clash}");
                        return Ok(ExitCode::from(2));
                    }
                    return Err(e);
                }
            };
            let value = serde_json::to_value(&report)?;
            emit(cli.json || args.stats, &value, || {
                println!(
                    "{} -> {} ({} artefact(s) written)",
                    report.generation.id,
                    report.out.display(),
                    report.written
                );
                for (vault, counts) in &report.published {
                    println!(
                        "  published {vault}: {} of {} page(s)",
                        counts.published, counts.considered
                    );
                }
            })?;
            // "Nothing built" is a failure, not a success. A build that writes
            // no artefact and exits 0 is indistinguishable from one that worked.
            anyhow::ensure!(
                report.written > 0,
                "the build wrote no artefacts to {}; refusing to report success",
                report.out.display()
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Repair(RepairCommand::Bodies(args)) => {
            let vaults: Vec<&str> = vault_kinds(&args.vault)
                .into_iter()
                .map(VaultKind::dir_name)
                .collect();
            let report = vault::bodies::run(&vault::bodies::Options {
                scopes: repair::scopes_for(&root, &vaults),
                dry_run: args.dry_run || args.check,
            })?;
            if let Some(path) = &args.report {
                let mut text = serde_json::to_string_pretty(&report)?;
                text.push('\n');
                std::fs::write(path, text)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            let value = serde_json::to_value(&report)?;
            emit(cli.json, &value, || {
                let verb = if args.check {
                    "need converting"
                } else if args.dry_run {
                    "would be converted"
                } else {
                    "converted"
                };
                println!(
                    "{} page(s) examined, {} {verb}",
                    report.pages_examined, report.pages_converted
                );
                for change in report.changes.iter().take(10) {
                    println!("  {}/{}", change.vault, change.id);
                }
                if report.changes.len() > 10 {
                    println!(
                        "  … {} more; pass --report for the full list",
                        report.changes.len() - 10
                    );
                }
            })?;
            if args.check && report.pages_converted > 0 {
                return Ok(ExitCode::FAILURE);
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Repair(RepairCommand::Fences(args)) => {
            let vaults: Vec<&str> = vault_kinds(&args.vault)
                .into_iter()
                .map(VaultKind::dir_name)
                .collect();
            let report = repair::run(&repair::Options {
                scopes: repair::scopes_for(&root, &vaults),
                dry_run: args.dry_run,
            })?;
            if let Some(path) = &args.report {
                let mut text = serde_json::to_string_pretty(&report)?;
                text.push('\n');
                std::fs::write(path, text)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            let value = serde_json::to_value(&report)?;
            emit(cli.json, &value, || {
                println!(
                    "{} page(s) examined, {} repaired: {} spurious opener(s) removed, \
                     {} closing fence(s) inserted{}",
                    report.pages_examined,
                    report.pages_repaired,
                    report.openers_removed,
                    report.closers_inserted,
                    if args.dry_run { " (dry run)" } else { "" }
                );
                for change in report.changes.iter().take(10) {
                    println!("  {}/{}: {:?}", change.vault, change.id, change.action);
                }
                if report.changes.len() > 10 {
                    println!(
                        "  … {} more; pass --report for the full list",
                        report.changes.len() - 10
                    );
                }
                if !report.ambiguous.is_empty() {
                    println!(
                        "  {} page(s) NOT repaired — the unmatched marker closes the code \
                         above it, so the missing one is earlier in the file and a human \
                         has to place it. These still fail UNTERMINATED_FENCE, by design.",
                        report.ambiguous.len()
                    );
                    for a in report.ambiguous.iter().take(5) {
                        println!("    {}/{}:{}", a.vault, a.id, a.line);
                    }
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Migrate(args) => {
            // Both trees of every selected vault, `pages/` before `journals/`.
            // `migrate::run` indexes every scope before converting any of
            // them, which is what `migration.block_refs`' ordering note
            // requires of an `--vault all` run.
            let trees: &[bool] = match args.only.as_str() {
                "pages" => &[false],
                "journals" => &[true],
                _ => &[false, true],
            };
            let mut scopes: Vec<migrate::Scope> = Vec::new();
            for kind in vault_kinds(&args.vault) {
                for &journals in trees {
                    scopes.push(migrate::Scope {
                        vault: kind.dir_name().to_owned(),
                        dir: root_of(&root, kind, journals),
                        journals,
                    });
                }
            }
            let options = migrate::Options {
                scopes,
                repo_root: root.clone(),
                allow_type_fallback: args.allow_type_fallback,
                dry_run: args.dry_run,
                now: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)?,
            };
            let report = migrate::run(&options, &vocab, args.report.is_some())?;
            if let Some(path) = &args.report {
                let mut text = serde_json::to_string_pretty(&report)?;
                text.push('\n');
                for page in &report.pages {
                    if let Some(diff) = &page.diff {
                        text.push_str(diff);
                    }
                }
                std::fs::write(path, text)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            let value = serde_json::to_value(&report)?;
            emit(cli.json, &value, || {
                // The verdict FIRST. A refusal used to print below a summary
                // that opened with "8,446 converted", so a run that wrote
                // nothing read as a success and the refusal looked like a
                // footnote.
                let refused = report.exit_code() != 0;
                if refused {
                    println!("REFUSED — nothing was written. Reasons below.");
                } else if report.pages_changed == 0 {
                    println!(
                        "NO-OP — {} page(s) examined and converted, {} already \
                         up to date, 0 written. The corpus is already migrated.",
                        report.pages_examined, report.pages_unchanged
                    );
                }
                println!(
                    "{} examined, {} changed, {} unchanged, {} fences, \
                     {} logseq lines, {} embeds, {} aliases added{}",
                    report.pages_examined,
                    report.pages_changed,
                    report.pages_unchanged,
                    report.fences_removed,
                    report.logseq_lines,
                    report.embeds_rewritten,
                    report.aliases_added,
                    if args.dry_run {
                        " (dry run — nothing written)"
                    } else if refused {
                        " (refused — nothing written)"
                    } else {
                        ""
                    }
                );
                // The extent, per vault. Never implicit: a run over one vault
                // and a run over both used to print the same line.
                for (vault, counts) in &report.per_vault {
                    println!(
                        "  {vault}: {} page(s) + {} journal(s) examined, {} converted",
                        counts.pages_examined, counts.journals_examined, counts.converted
                    );
                }
                if !report.skipped.is_empty() {
                    println!(
                        "  {} file(s) skipped: {}",
                        report.skipped.len(),
                        report
                            .skipped
                            .iter()
                            .take(3)
                            .map(|s| format!("{} ({})", s.path.display(), s.reason))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !report.unconvertible_embeds.is_empty() {
                    println!(
                        "  {} unresolvable block reference(s) removed and reported \
                         on {} page(s)",
                        report.unconvertible_embeds.values().sum::<usize>(),
                        report.unconvertible_embeds.len()
                    );
                }
                if !report.type_not_permitted.is_empty() {
                    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
                    for t in report.type_not_permitted.values() {
                        *kinds.entry(t.as_str()).or_insert(0) += 1;
                    }
                    if args.allow_type_fallback {
                        println!(
                            "  {} page(s) infer a type this vault does not permit \
                             ({kinds:?}); written anyway — --allow-type-fallback",
                            report.type_not_permitted.len()
                        );
                    } else {
                        println!(
                            "  REFUSED: {} page(s) infer a type this vault does not permit \
                             ({kinds:?}); nothing written. A page that falls back has had \
                             its identity erased, not converted. Fix the source, or pass \
                             --allow-type-fallback if the report is understood.",
                            report.type_not_permitted.len()
                        );
                    }
                }
                if !report.unterminated_fences.is_empty() {
                    println!(
                        "  {} page(s) carry an unmatched code-fence opener \
                         (a corpus defect; the region was read as prose)",
                        report.unterminated_fences.len()
                    );
                }
                if !report.normalised_tail_iris.is_empty() {
                    println!(
                        "  {} long-tail reference(s) normalised onto the canonical \
                         namespace (slug preserved)",
                        report.normalised_tail_iris.len()
                    );
                }
                if !report.is_lossless() {
                    println!("REFUSED — the migration map does not cover this corpus:");
                    for (key, count) in &report.uncovered_fence_fields {
                        println!("  fence field `{key}` ({count} occurrences)");
                    }
                    for (key, count) in &report.uncovered_logseq_keys {
                        println!("  logseq key `{key}` ({count} occurrences)");
                    }
                }
            })?;
            Ok(ExitCode::from(
                u8::try_from(report.exit_code()).unwrap_or(2),
            ))
        }
    }
}

/// Parse `is-a=2,requires=1` into per-edge-type depths.
fn parse_expansion(raw: &str) -> anyhow::Result<BTreeMap<String, usize>> {
    let mut out = BTreeMap::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, depth) = part
            .split_once('=')
            .with_context(|| format!("`{part}` is not `edge-type=depth`"))?;
        out.insert(
            key.trim().to_owned(),
            depth
                .trim()
                .parse()
                .with_context(|| format!("`{depth}` is not a depth"))?,
        );
    }
    Ok(out)
}

fn print_tree(node: &vault_core::graph::TreeNode, indent: usize) {
    println!("{:indent$}{}", "", node.id, indent = indent * 2);
    for child in &node.children {
        print_tree(child, indent + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a CLI line the way the binary does, so a rejected value is
    /// observable in a unit test.
    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("vault").chain(args.iter().copied()))
    }

    #[test]
    fn an_unknown_vault_value_is_rejected_on_every_subcommand() {
        // It used to fall through to `knowledge` and do real work under a name
        // the operator did not ask for. Three silent-success defects in this
        // crate were all of that shape: a value the tool did not understand,
        // treated as a default, reported as a success.
        for args in [
            vec!["validate", "--vault", "bogus"],
            vec!["build", "--out", "/tmp/x", "--vault", "bogus"],
            vec!["migrate", "--fences-to-properties", "--vault", "bogus"],
            vec!["repair", "fences", "--vault", "bogus"],
            vec!["find", "--query", "x", "--vault", "bogus"],
        ] {
            let error = parse(&args).expect_err(&format!("{args:?} must be rejected"));
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::InvalidValue,
                "{args:?} -> {error}"
            );
            // clap renders an InvalidValue as exit 2, which is the contract.
            assert_eq!(error.exit_code(), 2, "{args:?}");
        }
    }

    #[test]
    fn the_three_legal_vault_values_are_accepted_everywhere() {
        for vault in VAULT_VALUES {
            for args in [
                vec!["validate", "--vault", vault],
                vec!["build", "--out", "/tmp/x", "--vault", vault],
                vec!["migrate", "--fences-to-properties", "--vault", vault],
                vec!["repair", "fences", "--vault", vault],
            ] {
                assert!(parse(&args).is_ok(), "{args:?} must be accepted");
            }
        }
    }

    #[test]
    fn migrate_only_is_restricted_to_the_two_trees() {
        assert!(parse(&["migrate", "--fences-to-properties", "--only", "pages"]).is_ok());
        assert!(parse(&["migrate", "--fences-to-properties", "--only", "journals"]).is_ok());
        assert!(parse(&["migrate", "--fences-to-properties", "--only", "all"]).is_ok());
        let error = parse(&["migrate", "--fences-to-properties", "--only", "everything"])
            .expect_err("an unknown tree must be rejected");
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn expansion_parsing_accepts_the_contract_form() {
        let e = parse_expansion("is-a=2,requires=1").unwrap();
        assert_eq!(e["is-a"], 2);
        assert_eq!(e["requires"], 1);
    }

    #[test]
    fn expansion_parsing_rejects_nonsense() {
        assert!(parse_expansion("is-a").is_err());
        assert!(parse_expansion("is-a=deep").is_err());
    }

    #[test]
    fn an_empty_expansion_traverses_nothing() {
        assert!(parse_expansion("").unwrap().is_empty());
    }

    #[test]
    fn the_vault_selector_maps_to_kinds() {
        let all = VaultSelector {
            vault: "all".into(),
        };
        assert_eq!(all.kinds().len(), 2);
        let working = VaultSelector {
            vault: "working".into(),
        };
        assert_eq!(working.kinds(), vec![VaultKind::Working]);
        let default = VaultSelector {
            vault: "knowledge".into(),
        };
        assert_eq!(default.kinds(), vec![VaultKind::Knowledge]);
    }

    #[test]
    fn the_cli_parses_every_subcommand() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
