//! Command line for `visionclaw-backup`: `backup`, `verify` and `restore`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use visionclaw_backup::{backup, restore, verify, BackupRequest, Manifest};

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
        /// Oxigraph store or checkpoint directory (default: `<data-dir>/oxigraph`).
        #[arg(long, env = "VISIONCLAW_BACKUP_OXIGRAPH_DIR")]
        oxigraph_dir: Option<PathBuf>,
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
            keep,
        } => {
            let outcome = backup(&BackupRequest {
                data_dir,
                oxigraph_dir,
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
