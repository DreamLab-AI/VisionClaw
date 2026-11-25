//! `temporal-refit`: one-off offline refit of a VisionClaw provenance store to
//! the `estate-temporal-v2` form (ADR-2219). Dry run unless `--apply`.
//!
//! ```text
//! temporal-refit <STORE_PATH> [--apply]
//!                [--expect-versions N] [--expect-closes N] [--expect-other-activities N]
//! ```
//!
//! Run it on a checkpoint copy first, then (application stopped) on the live
//! volume. It never touches a running store: RocksDB's lock refuses a second
//! opener. Exit status: 0 done or nothing to do, 1 refused or verification
//! failed, 2 usage or store error.

use std::process::ExitCode;

use oxigraph::store::Store;
use visionclaw_adapters::temporal_refit::{run, Options, RefitError};

const USAGE: &str = "usage: temporal-refit <STORE_PATH> [--apply] \
[--expect-versions N] [--expect-closes N] [--expect-other-activities N]";

fn count(flag: &str, value: Option<String>) -> Result<usize, String> {
    value
        .ok_or_else(|| format!("{flag} needs a number"))?
        .parse()
        .map_err(|_| format!("{flag} needs a number"))
}

fn parse(args: impl Iterator<Item = String>) -> Result<(String, Options), String> {
    let mut args = args;
    let mut path = None;
    let mut opts = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--apply" => opts.apply = true,
            "--expect-versions" => opts.expect_versions = Some(count(&arg, args.next())?),
            "--expect-closes" => opts.expect_closes = Some(count(&arg, args.next())?),
            "--expect-other-activities" => {
                opts.expect_other_activities = Some(count(&arg, args.next())?)
            }
            "-h" | "--help" => return Err(String::new()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ if path.is_none() => path = Some(arg),
            _ => return Err("only one store path".to_string()),
        }
    }
    Ok((path.ok_or("a store path is required")?, opts))
}

fn main() -> ExitCode {
    let (path, opts) = match parse(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // A dry run opens read-only, so it cannot write even by mistake.
    let store = if opts.apply {
        Store::open(&path)
    } else {
        Store::open_read_only(&path)
    };
    let store = match store {
        Ok(store) => store,
        Err(e) => {
            eprintln!("cannot open {path}: {e} (is the application stopped?)");
            return ExitCode::from(2);
        }
    };
    println!(
        "temporal-refit {} {path}",
        if opts.apply { "APPLY" } else { "DRY RUN" }
    );
    match run(&store, &opts) {
        Ok(report) => {
            report.lines.iter().for_each(|l| println!("{l}"));
            ExitCode::SUCCESS
        }
        Err(e @ RefitError::Store(_)) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
