//! The `vault` binary: contract C2's command surface.
//!
//! Every subcommand accepts `--json`, because the primary caller is an agent
//! in a Bash tool, not a person at a terminal. Exit codes are contractual:
//! `0` success, `1` a failed check or execution error, `2` a refused operation
//! or invalid argument.

#![deny(unsafe_code)]
#![warn(clippy::pedantic)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use vault::{build, conflicts, gate, model::Corpus, nostr, propose, repair, validate};
use vault_core::page::{load_vault, Vault, VaultKind};
use vault_core::proposal::Level;
use vault_core::vocabulary::Vocabulary;

/// The one-line pointer `vault --help` prints for the verbs that moved to the
/// `ontomark` CLI (ADR-OM-005; ontomark `docs/DEPRECATION.md` §2.1, rows
/// VC-29/VC-30).
const MOVED_VERBS_HELP: &str =
    "Moved: `find`, `retrieve`, `tree`, `edit`, `create` and `apply` now live in the `ontomark` CLI.";

/// The single door onto the sovereign corpus.
#[derive(Debug, Parser)]
#[command(name = "vault", version, about, long_about = None, after_help = MOVED_VERBS_HELP)]
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
    /// Build a `PatchProposal` and post it as a forum 31402.
    Propose(ProposeArgs),
    /// The forum governance panel the proposals are filed against.
    #[command(subcommand)]
    Panel(PanelCommand),
    /// The autonomous continuation gate.
    Gate(GateArgs),
    /// Semantic conflict detection.
    Conflicts(ConflictsArgs),
    /// Pages to one generation.
    Build(BuildArgs),
    /// Fix corpus defects in place.
    #[command(subcommand)]
    Repair(RepairCommand),
}

#[derive(Debug, Subcommand)]
enum PanelCommand {
    /// Publish the `ontology-governance` panel definition (kind 31400).
    ///
    /// The forum resolves a proposal's panel at
    /// `31400:<proposal author>:ontology-governance`, so this must be signed
    /// with the same key as `vault propose` — the same `--secret` /
    /// `VAULT_NOSTR_SECRET`, posted to the same relay with NIP-42 AUTH.
    /// Needs no repository.
    Publish(PanelPublishArgs),
}

#[derive(Debug, Args)]
struct PanelPublishArgs {
    /// Print the signed event instead of publishing it.
    #[arg(long)]
    dry_run: bool,
    /// The relay to publish to.
    #[arg(long, default_value = DEFAULT_RELAY, env = "VAULT_RELAY_URL")]
    relay: String,
    /// The 32-byte hex secret key to sign with.
    #[arg(long, env = "VAULT_NOSTR_SECRET")]
    secret: Option<String>,
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
/// `all` means BOTH, everywhere — earlier commands accepted the
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

/// The relay `propose` and `panel publish` post to when neither `--relay` nor
/// `VAULT_RELAY_URL` names one.
const DEFAULT_RELAY: &str = "ws://localhost:7777";

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
    /// `ontomark create`.
    ///
    /// A directory is a manifest: one `*.md` per page, named by page id
    /// (`<root>/<id>.md`, mirroring the vault root), and the result is ONE grouped
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
    #[arg(long, default_value = DEFAULT_RELAY, env = "VAULT_RELAY_URL")]
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

// One arm per subcommand. Splitting it would scatter the CLI's dispatch table
// across several functions and make the surface harder to read, not easier.
#[allow(clippy::too_many_lines)]
fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    // The one subcommand that touches no corpus: it must work from anywhere.
    if let Command::Panel(PanelCommand::Publish(args)) = &cli.command {
        return panel_publish(args, cli.json);
    }
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

            let id = nostr::publish(&signed, &args.relay, Some(&key))?;
            let value = json!({ "proposal": proposal, "event_id": id, "relay": args.relay });
            emit(cli.json, &value, || {
                println!("posted 31402 {id} to {}", args.relay);
            })?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Panel(_) => unreachable!("handled before the repository is resolved"),

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
                    // something it must not. Exit 2 denotes this refusal.
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
                    // A definition the reasoner would not finish on (ADR-2124):
                    // refused before reasoning, same refusal class.
                    if let Some(cap) = e.downcast_ref::<vault::whelk::ReasoningError>() {
                        eprintln!("vault: {cap}; no bundle written");
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
    }
}

/// `vault panel publish`: sign the `ontology-governance` 31400 with the
/// proposing key and post it (or, with `--dry-run`, print it).
fn panel_publish(args: &PanelPublishArgs, json_out: bool) -> anyhow::Result<ExitCode> {
    let secret = args
        .secret
        .clone()
        .context("a signing key is required: --secret or VAULT_NOSTR_SECRET")?;
    let key = nostr::signing_key_from_hex(&secret).map_err(anyhow::Error::msg)?;
    let pubkey = nostr::public_key_hex(&key);
    let created_at =
        u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp()).unwrap_or_default();
    let unsigned = nostr::panel_definition(&pubkey, created_at).map_err(anyhow::Error::msg)?;
    let signed = nostr::sign(unsigned, &key).map_err(anyhow::Error::msg)?;
    let address = nostr::panel_address(&pubkey);

    if args.dry_run {
        let value = json!({ "address": address, "event": signed });
        emit(json_out, &value, || {
            println!(
                "{}",
                serde_json::to_string_pretty(&signed).unwrap_or_default()
            );
        })?;
        return Ok(ExitCode::SUCCESS);
    }

    let id = nostr::publish(&signed, &args.relay, Some(&key))?;
    let value = json!({ "address": address, "event_id": id, "relay": args.relay });
    emit(json_out, &value, || {
        println!("posted 31400 {id} ({address}) to {}", args.relay);
    })?;
    Ok(ExitCode::SUCCESS)
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
            vec!["repair", "fences", "--vault", "bogus"],
            vec!["gate", "--vault", "bogus"],
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
                vec!["repair", "fences", "--vault", vault],
            ] {
                assert!(parse(&args).is_ok(), "{args:?} must be accepted");
            }
        }
    }

    #[test]
    fn retired_migration_command_is_rejected() {
        let error = parse(&["migrate", "--fences-to-properties"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    /// The six verbs ported to `ontomark` are gone from this binary: each is
    /// an unknown subcommand (exit 2), not a silently different behaviour.
    #[test]
    fn the_verbs_moved_to_ontomark_are_rejected() {
        for args in [
            vec!["find", "--query", "x"],
            vec!["retrieve", "Knowledge Graph"],
            vec!["tree", "Spatial Computing"],
            vec!["edit", "Knowledge Graph", "--set", "status=stable"],
            vec!["create", "staged.md", "--expect", "docs=1"],
            vec!["apply", "proposal.json", "--expect", "docs=1"],
        ] {
            let error = parse(&args).expect_err(&format!("{args:?} must be rejected"));
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::InvalidSubcommand,
                "{args:?} -> {error}"
            );
            assert_eq!(error.exit_code(), 2, "{args:?}");
        }
    }

    #[test]
    fn help_points_at_ontomark_for_the_moved_verbs() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains(MOVED_VERBS_HELP), "{help}");
        for verb in ["find", "retrieve", "tree", "edit", "create", "apply"] {
            assert!(help.contains(verb), "the pointer must name `{verb}`");
        }
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
