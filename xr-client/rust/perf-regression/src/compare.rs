//! Metric tables and the comparison rules.
//!
//! Split out of `main.rs` so neither file exceeds 500 lines. This module holds
//! the *policy* — which metrics exist, how a budget is judged, what counts as a
//! regression — while `main.rs` handles arguments, I/O and reporting.

use serde_json::{Map, Value};

/// Godot metric key → baseline key, label, and whether lower is better.
pub const GODOT_METRICS: &[(&str, &str, &str)] = &[
    ("frame_ms_p50", "frame_p50_ms", "Frame p50 (ms)"),
    ("frame_ms_p99", "frame_p99_ms", "Frame p99 (ms)"),
    ("draw_calls_max", "max_draw_calls", "Draw calls (max)"),
    ("tri_count_max", "max_triangles", "Triangles (max)"),
];

/// Criterion bench name → baseline key. Matched exactly, then by suffix.
pub const CRITERION_BENCH_TO_BASELINE: &[(&str, &str)] = &[
    ("encode_pose_frame", "encode_pose_frame"),
    ("decode_pose_frame", "decode_pose_frame"),
    ("validate_pose", "validate_pose"),
    ("delta_compute", "delta_compute"),
    (
        "decode_position_frame_1k/decode_1000_nodes",
        "decode_position_frame_1k_ns",
    ),
    ("presence_0x43_round_trip", "presence_round_trip_ns"),
];

/// Default regression budget for a Criterion bench with none recorded.
pub const DEFAULT_CRITERION_BUDGET_PCT: f64 = 5.0;

/// One row of the comparison table.
pub struct Row {
    pub metric: String,
    pub current: f64,
    /// `None` renders as `n/a` — used by the SKIP row for an unmapped bench.
    pub baseline: Option<f64>,
    pub delta: f64,
    pub delta_pct: f64,
    pub budget: String,
    pub status: &'static str,
}

/// Which input shape `--current` turned out to be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Godot,
    Criterion,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Godot => "godot",
            Kind::Criterion => "criterion",
        }
    }
}

/// Classify `--current` as a Godot scene result or a Criterion estimates file.
pub fn detect_kind(payload: &Value) -> Option<Kind> {
    let object = payload.as_object()?;
    if object.contains_key("mean") && object.get("median").is_some_and(Value::is_object) {
        return Some(Kind::Criterion);
    }
    let godot_marker = ["frame_ms_p99", "fps_p99", "draw_calls_max"]
        .iter()
        .any(|key| object.contains_key(*key));
    godot_marker.then_some(Kind::Godot)
}

/// Read a JSON number as `f64`, whatever its numeric representation.
pub fn number(value: Option<&Value>) -> Option<f64> {
    value?.as_f64()
}

/// Render a budget the way the Python did: `+5%`, `+5`, `+5% or +5`, or `exact`.
pub fn format_budget(pct: Option<&Value>, abs: Option<&Value>) -> String {
    let mut parts = Vec::new();
    if let Some(pct) = pct {
        parts.push(format!("+{pct}%"));
    }
    if let Some(abs) = abs {
        parts.push(format!("+{abs}"));
    }
    if parts.is_empty() {
        "exact".to_string()
    } else {
        parts.join(" or ")
    }
}

/// Compare Godot scene metrics against their absolute PRD-008 targets.
///
/// Note the comparison is against `target_max`, not a moving baseline: once a
/// metric sits at budget there is no headroom left to "regress" into.
pub fn cmp_godot(current: &Map<String, Value>, baseline: &Map<String, Value>) -> (Vec<Row>, bool) {
    let mut rows = Vec::new();
    let mut regressed = false;

    for (current_key, baseline_key, label) in GODOT_METRICS {
        let (Some(current_value), Some(entry)) = (
            number(current.get(*current_key)),
            baseline.get(*baseline_key),
        ) else {
            continue;
        };
        let Some(target) = number(entry.get("target_max")) else {
            continue;
        };

        let budget_pct = entry.get("regression_budget_pct").filter(|v| !v.is_null());
        let budget_abs = entry.get("regression_budget_abs").filter(|v| !v.is_null());

        let mut over_budget = match number(budget_pct) {
            Some(pct) => current_value > target * (1.0 + pct / 100.0),
            None => false,
        };
        if let Some(abs) = number(budget_abs) {
            over_budget = over_budget || current_value > target + abs;
        }
        if budget_pct.is_none() && budget_abs.is_none() {
            over_budget = current_value > target;
        }

        let delta = current_value - target;
        rows.push(Row {
            metric: (*label).to_string(),
            current: current_value,
            baseline: Some(target),
            delta,
            delta_pct: if target != 0.0 {
                delta / target * 100.0
            } else {
                0.0
            },
            budget: format_budget(budget_pct, budget_abs),
            status: if over_budget { "FAIL" } else { "PASS" },
        });
        regressed |= over_budget;
    }

    (rows, regressed)
}

/// Map a Criterion bench name onto a baseline key, exactly then by suffix.
pub fn baseline_key_for(bench_name: &str) -> Option<&'static str> {
    CRITERION_BENCH_TO_BASELINE
        .iter()
        .find(|(name, _)| *name == bench_name)
        .or_else(|| {
            CRITERION_BENCH_TO_BASELINE
                .iter()
                .find(|(name, _)| bench_name.ends_with(name))
        })
        .map(|(_, key)| *key)
}

/// Compare one Criterion median against its recorded baseline.
pub fn cmp_criterion(
    current: &Map<String, Value>,
    baseline: &Map<String, Value>,
    bench_name: Option<&str>,
) -> Result<(Vec<Row>, bool), String> {
    let median = number(current.get("median").and_then(|m| m.get("point_estimate")))
        .ok_or("Criterion input has no median.point_estimate")?;

    let name = bench_name
        .map(str::to_string)
        .or_else(|| current.get("_bench_name")?.as_str().map(str::to_string))
        .unwrap_or_default();

    let entry = baseline_key_for(&name).and_then(|key| baseline.get(key).map(|e| (key, e)));

    let Some((key, entry)) = entry else {
        // Unmapped bench: report it rather than silently passing, but do not
        // fail the run — an unknown bench is not evidence of a regression.
        return Ok((
            vec![Row {
                metric: if name.is_empty() {
                    "(unknown bench)".into()
                } else {
                    name
                },
                current: median,
                baseline: None,
                delta: 0.0,
                delta_pct: 0.0,
                budget: "n/a".into(),
                status: "SKIP",
            }],
            false,
        ));
    };

    let target = number(entry.get("median_ns"))
        .or_else(|| number(entry.get("ns_per_iter")))
        .ok_or_else(|| format!("baseline entry `{key}` has neither median_ns nor ns_per_iter"))?;
    let budget_pct =
        number(entry.get("regression_budget_pct")).unwrap_or(DEFAULT_CRITERION_BUDGET_PCT);

    let mut over_budget = median > target * (1.0 + budget_pct / 100.0);
    if let Some(budget_ns) = number(entry.get("budget_ns")) {
        over_budget = over_budget || median > budget_ns;
    }

    let delta = median - target;
    Ok((
        vec![Row {
            metric: format!("{key} (ns)"),
            current: median,
            baseline: Some(target),
            delta,
            delta_pct: if target != 0.0 {
                delta / target * 100.0
            } else {
                0.0
            },
            budget: format!("+{budget_pct:.0}%"),
            status: if over_budget { "FAIL" } else { "PASS" },
        }],
        over_budget,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> Map<String, Value> {
        value
            .as_object()
            .expect("test fixture is an object")
            .clone()
    }

    fn baseline() -> Map<String, Value> {
        object(json!({
            "encode_pose_frame": {
                "ns_per_iter": 200, "budget_ns": 1000, "median_ns": 200.42,
                "regression_budget_pct": 5
            },
            "frame_p99_ms": { "target_max": 11.1, "regression_budget_pct": 5 },
            "max_draw_calls": { "target_max": 50, "regression_budget_abs": 5 },
            "max_triangles": { "target_max": 100000, "regression_budget_pct": 10 },
        }))
    }

    #[test]
    fn godot_input_is_detected() {
        assert!(matches!(
            detect_kind(&json!({ "frame_ms_p99": 9.0 })),
            Some(Kind::Godot)
        ));
        assert!(matches!(
            detect_kind(&json!({ "draw_calls_max": 20 })),
            Some(Kind::Godot)
        ));
    }

    #[test]
    fn criterion_input_is_detected() {
        let estimates = json!({
            "mean": { "point_estimate": 201.0 },
            "median": { "point_estimate": 200.0 },
        });
        assert!(matches!(detect_kind(&estimates), Some(Kind::Criterion)));
    }

    #[test]
    fn unclassifiable_input_is_rejected() {
        assert!(detect_kind(&json!({ "unrelated": 1 })).is_none());
        assert!(detect_kind(&json!([1, 2, 3])).is_none());
    }

    #[test]
    fn a_metric_inside_budget_passes() {
        let current = object(json!({ "frame_ms_p99": 11.0, "draw_calls_max": 48 }));
        let (rows, regressed) = cmp_godot(&current, &baseline());
        assert_eq!(rows.len(), 2);
        assert!(!regressed);
        assert!(rows.iter().all(|r| r.status == "PASS"));
    }

    #[test]
    fn a_percentage_budget_overrun_fails() {
        // 11.1 * 1.05 = 11.655; 12.0 is beyond it.
        let current = object(json!({ "frame_ms_p99": 12.0 }));
        let (rows, regressed) = cmp_godot(&current, &baseline());
        assert!(regressed);
        assert_eq!(rows[0].status, "FAIL");
    }

    #[test]
    fn an_absolute_budget_overrun_fails() {
        // target 50 + 5 absolute = 55; 56 is beyond it, 55 is not.
        let (_, at_edge) = cmp_godot(&object(json!({ "draw_calls_max": 55 })), &baseline());
        assert!(
            !at_edge,
            "a value exactly at budget must not count as a regression"
        );
        let (_, over) = cmp_godot(&object(json!({ "draw_calls_max": 56 })), &baseline());
        assert!(over);
    }

    #[test]
    fn a_metric_absent_from_the_run_is_skipped_silently() {
        let (rows, regressed) = cmp_godot(&object(json!({ "unrelated": 1 })), &baseline());
        assert!(rows.is_empty());
        assert!(!regressed);
    }

    #[test]
    fn criterion_medians_are_compared_against_the_recorded_baseline() {
        let current = object(json!({
            "mean": { "point_estimate": 205.0 },
            "median": { "point_estimate": 205.0 },
        }));
        let (rows, regressed) =
            cmp_criterion(&current, &baseline(), Some("encode_pose_frame")).unwrap();
        assert!(!regressed, "205ns is inside 200.42 +5%");
        assert_eq!(rows[0].status, "PASS");

        let slow = object(json!({
            "mean": { "point_estimate": 400.0 },
            "median": { "point_estimate": 400.0 },
        }));
        let (rows, regressed) =
            cmp_criterion(&slow, &baseline(), Some("encode_pose_frame")).unwrap();
        assert!(regressed);
        assert_eq!(rows[0].status, "FAIL");
    }

    #[test]
    fn an_absolute_budget_ns_overrun_fails_even_inside_the_percentage() {
        // budget_ns is 1000 and the percentage budget would allow far less,
        // so this checks the ns ceiling is applied independently.
        let mut baseline = baseline();
        if let Some(Value::Object(entry)) = baseline.get_mut("encode_pose_frame") {
            entry.insert("regression_budget_pct".into(), json!(10_000));
        }
        let current = object(json!({
            "mean": { "point_estimate": 1500.0 },
            "median": { "point_estimate": 1500.0 },
        }));
        let (_, regressed) = cmp_criterion(&current, &baseline, Some("encode_pose_frame")).unwrap();
        assert!(regressed, "budget_ns=1000 must fail a 1500ns median");
    }

    #[test]
    fn an_unmapped_bench_is_reported_but_does_not_fail() {
        let current = object(json!({
            "mean": { "point_estimate": 1.0 },
            "median": { "point_estimate": 1.0 },
        }));
        let (rows, regressed) = cmp_criterion(&current, &baseline(), Some("brand_new")).unwrap();
        assert!(!regressed);
        assert_eq!(rows[0].status, "SKIP");
        assert!(rows[0].baseline.is_none());
    }

    #[test]
    fn bench_names_resolve_by_suffix() {
        assert_eq!(
            baseline_key_for("encode_pose_frame"),
            Some("encode_pose_frame")
        );
        assert_eq!(
            baseline_key_for("wire/decode_position_frame_1k/decode_1000_nodes"),
            Some("decode_position_frame_1k_ns")
        );
        assert_eq!(baseline_key_for("no_such_bench"), None);
    }

    #[test]
    fn budgets_render_the_way_the_report_expects() {
        assert_eq!(format_budget(Some(&json!(5)), None), "+5%");
        assert_eq!(format_budget(None, Some(&json!(5))), "+5");
        assert_eq!(format_budget(Some(&json!(5)), Some(&json!(2))), "+5% or +2");
        assert_eq!(format_budget(None, None), "exact");
    }
}
