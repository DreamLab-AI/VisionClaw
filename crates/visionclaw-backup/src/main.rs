//! Command line for `visionclaw-backup`: `backup`, `verify` and `restore`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use visionclaw_backup::{backup, restore, verify, BackupRequest, Manifest, OxigraphSource};

/// Back up, verify and restore the VisionClaw Oxigraph store and SQLite databases.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a dated run (Oxigraph N-Quads, SQLite snapshots, manifest) and prune old runs.
    Backup {
        /// Folder that receives the dated runs (for example the NAS mount).
        #[arg(long, env = "VISIONCLAW_BACKUP_DEST")]
        dest: PathBuf,
        /// The server's data directory.
        #[arg(long, env = "DATA_DIR", default_value = "./data")]
        data_dir: PathBuf,
        /// Oxigraph store to dump directly (default: `<data-dir>/oxigraph`); a store
        /// held by a running server is refused. Use `--checkpoint-dir` for that.
        #[arg(
            long,
            env = "VISIONCLAW_BACKUP_OXIGRAPH_DIR",
            conflicts_with = "checkpoint_dir"
        )]
        oxigraph_dir: Option<PathBuf>,
        /// The server's checkpoint directory (its `ONTOLOGY_CHECKPOINT_DIR`): dump the
        /// latest complete checkpoint instead of the live store.
        #[arg(long, env = "VISIONCLAW_BACKUP_CHECKPOINT_DIR")]
        checkpoint_dir: Option<PathBuf>,
        /// Refuse a checkpoint older than this many seconds (default 24 hours).
        #[arg(
            long,
            env = "VISIONCLAW_BACKUP_MAX_CHECKPOINT_AGE_SECS",
            default_value_t = 86_400
        )]
        max_checkpoint_age_secs: u64,
        /// Complete runs to keep.
        #[arg(long, env = "VISIONCLAW_BACKUP_KEEP", default_value_t = 14)]
        keep: usize,
    },
    /// Check a run against its manifest (sizes and SHA-256).
    Verify {
        /// A run folder, or a backup root (checks the newest run).
        #[arg(long, env = "VISIONCLAW_BACKUP_DEST")]
        run: PathBuf,
    },
    /// Rebuild the stores of a run into an empty directory.
    Restore {
        /// A run folder, or a backup root (restores the newest run).
        #[arg(long, env = "VISIONCLAW_BACKUP_DEST")]
        run: PathBuf,
        /// Empty (or absent) directory to restore into.
        #[arg(long)]
        target: PathBuf,
    },
}

fn summarise(verb: &str, run: &std::path::Path, manifest: &Manifest) {
    println!("{verb} {}", run.display());
    for artefact in &manifest.artefacts {
        println!(
            "  {:<20} {:>12} bytes  sha256 {}",
            artefact.name, artefact.bytes, artefact.sha256
        );
    }
    if let Some(cp) = &manifest.oxigraph_checkpoint {
        println!(
            "  oxigraph taken from server checkpoint {} ({}, {} quads)",
            cp.name, cp.taken_utc, cp.triples
        );
    }
    for name in &manifest.absent_optional {
        println!("  {name:<20} absent (optional)");
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Backup {
            dest,
            data_dir,
            oxigraph_dir,
            checkpoint_dir,
            max_checkpoint_age_secs,
            keep,
        } => {
            let oxigraph = match checkpoint_dir {
                Some(dir) => OxigraphSource::Checkpoint {
                    dir,
                    max_age: Duration::from_secs(max_checkpoint_age_secs),
                },
                None => OxigraphSource::Store(oxigraph_dir),
            };
            let outcome = backup(&BackupRequest {
                data_dir,
                oxigraph,
                dest_root: dest,
                keep,
            })?;
            summarise("backed up to", &outcome.run_dir, &outcome.manifest);
            for old in &outcome.pruned {
                println!("pruned {}", old.display());
            }
        }
        Command::Verify { run } => {
            let (dir, manifest) = verify(&run)?;
            summarise("verified", &dir, &manifest);
        }
        Command::Restore { run, target } => {
            let (dir, manifest) = restore(&run, &target)?;
            summarise("restored from", &dir, &manifest);
            println!("into {}", target.display());
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("visionclaw-backup: {err:#}");
            ExitCode::FAILURE
        }
    }
}
