//! Compare a fresh perf result against the committed baseline.
//!
//! A port of `xr-client/perf/regression_check.py`. Two input shapes are
//! accepted and auto-detected:
//!
//! * the Godot benchmark object emitted as `[XR_PERF_RESULT]={...}` in logcat
//!   by `xr-client/perf/benchmark.gd` (just the `{...}` payload), and
//! * a Criterion `estimates.json` from `target/criterion/<bench>/new/`.
//!
//! ```text
//! xr-perf-regression --current run.json \
//!                    --baseline crates/visionclaw-xr-presence/benches/baseline.json
//! ```
//!
//! Exit codes: `0` no regression beyond budget, `1` at least one metric
//! regressed, `2` invalid input or schema mismatch — matching the Python it
//! replaces, so `xr-godot-ci.yml` needs no change.
//!
//! Output is a markdown comparison table on stdout, suitable for a PR comment.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod compare;

use compare::{
    baseline_key_for, cmp_criterion, cmp_godot, detect_kind, number, Kind, Row, GODOT_METRICS,
};
use serde_json::{Map, Value};

struct Args {
    current: PathBuf,
    baseline: PathBuf,
    bench_name: Option<String>,
    update_baseline: bool,
}

const USAGE: &str = "\
Compare a fresh XR perf result against the committed baseline.

Usage:
  xr-perf-regression --current <PATH> --baseline <PATH> [options]

Options:
  --current <PATH>     Fresh perf result JSON (Godot scene output or Criterion
                       estimates.json). Required.
  --baseline <PATH>    Committed baseline.json. Required.
  --bench-name <NAME>  For Criterion input: the bench name, e.g. encode_pose_frame.
  --update-baseline    Overwrite the baseline with this run's numbers.
                       Author-then-reviewer workflow only; CI must not pass it.
  -h, --help           Show this message.

Exit codes: 0 no regression, 1 regression beyond budget, 2 invalid input.";

fn parse_args() -> Result<Args, String> {
    let mut current = None;
    let mut baseline = None;
    let mut bench_name = None;
    let mut update_baseline = false;

    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut take = |flag: &str| -> Result<String, String> {
            argv.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match arg.as_str() {
            "--current" => current = Some(PathBuf::from(take("--current")?)),
            "--baseline" => baseline = Some(PathBuf::from(take("--baseline")?)),
            "--bench-name" => bench_name = Some(take("--bench-name")?),
            "--update-baseline" => update_baseline = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unrecognised argument: {other}")),
        }
    }

    Ok(Args {
        current: current.ok_or("--current is required")?,
        baseline: baseline.ok_or("--baseline is required")?,
        bench_name,
        update_baseline,
    })
}

fn load_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
}

fn render_table(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "_(no comparable metrics)_\n".to_string();
    }
    let mut out = String::from(
        "| Metric | Current | Baseline | Delta | Δ% | Budget | Status |\n\
         |---|---:|---:|---:|---:|---:|:---:|\n",
    );
    for row in rows {
        let baseline = match row.baseline {
            Some(value) => format!("{value:.3}"),
            None => "n/a".to_string(),
        };
        out.push_str(&format!(
            "| {} | {:.3} | {} | {:+.3} | {:+.2}% | {} | {} |\n",
            row.metric, row.current, baseline, row.delta, row.delta_pct, row.budget, row.status
        ));
    }
    out
}

/// Fold this run's numbers back into the baseline, preserving key order.
fn update_baseline(
    path: &Path,
    baseline: &mut Map<String, Value>,
    current: &Map<String, Value>,
    kind: Kind,
    bench_name: Option<&str>,
) -> Result<(), String> {
    match kind {
        Kind::Godot => {
            for (current_key, baseline_key, _) in GODOT_METRICS {
                let Some(observed) = current.get(*current_key) else {
                    continue;
                };
                if let Some(Value::Object(entry)) = baseline.get_mut(*baseline_key) {
                    entry.insert("last_observed".into(), observed.clone());
                }
            }
        }
        Kind::Criterion => {
            let name = bench_name
                .map(str::to_string)
                .or_else(|| current.get("_bench_name")?.as_str().map(str::to_string))
                .unwrap_or_default();
            let median = current.get("median");
            let point = number(median.and_then(|m| m.get("point_estimate")));

            if let (Some(key), Some(ns)) = (baseline_key_for(&name), point) {
                if let Some(Value::Object(entry)) = baseline.get_mut(key) {
                    entry.insert("median_ns".into(), json_number(ns));
                    entry.insert("ns_per_iter".into(), Value::from(ns.round() as i64));
                    let interval = median.and_then(|m| m.get("confidence_interval"));
                    if let Some(lower) = number(interval.and_then(|i| i.get("lower_bound"))) {
                        entry.insert("lower_ns".into(), json_number(lower));
                    }
                    if let Some(upper) = number(interval.and_then(|i| i.get("upper_bound"))) {
                        entry.insert("upper_ns".into(), json_number(upper));
                    }
                }
            }
        }
    }

    let mut text = serde_json::to_string_pretty(&Value::Object(baseline.clone()))
        .map_err(|e| format!("could not serialise the baseline: {e}"))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Wrap an `f64` as a JSON number, falling back to null for NaN/infinity.
fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn run() -> Result<bool, String> {
    let args = parse_args()?;

    let current = load_json(&args.current)?;
    let mut baseline = load_json(&args.baseline)?;

    let kind =
        detect_kind(&current).ok_or("could not classify --current as Godot or Criterion JSON")?;
    let current_object = current
        .as_object()
        .ok_or("--current is not a JSON object")?;
    let baseline_object = baseline
        .as_object_mut()
        .ok_or("--baseline is not a JSON object")?;

    let (rows, regressed) = match kind {
        Kind::Godot => cmp_godot(current_object, baseline_object),
        Kind::Criterion => {
            cmp_criterion(current_object, baseline_object, args.bench_name.as_deref())?
        }
    };

    println!(
        "### XR perf regression report — `{}` input\n",
        kind.as_str()
    );
    println!("{}", render_table(&rows));
    if regressed {
        println!("\n**FAIL** — at least one metric regressed beyond budget.");
    } else {
        println!("\n**PASS** — no regressions beyond budget.");
    }

    if args.update_baseline {
        update_baseline(
            &args.baseline,
            baseline_object,
            current_object,
            kind,
            args.bench_name.as_deref(),
        )?;
        println!("\nbaseline updated at {}", args.baseline.display());
    }

    Ok(regressed)
}

fn main() -> ExitCode {
    match run() {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(1),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_renders_a_skip_baseline_as_not_applicable() {
        let rows = vec![Row {
            metric: "x".into(),
            current: 1.0,
            baseline: None,
            delta: 0.0,
            delta_pct: 0.0,
            budget: "n/a".into(),
            status: "SKIP",
        }];
        let table = render_table(&rows);
        assert!(table.contains("| n/a |"), "{table}");
        assert!(!table.contains("NaN"), "{table}");
    }

    #[test]
    fn the_table_renders_an_empty_run_without_panicking() {
        assert_eq!(render_table(&[]), "_(no comparable metrics)_\n");
    }
}
