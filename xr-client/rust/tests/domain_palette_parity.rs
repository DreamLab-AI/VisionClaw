//! WP1 parity gate: the XR domain palette must equal the desktop
//! `client/src/features/graph/utils/domainColors.ts` table exactly.
//!
//! The TS file is parsed at test time (not copied), so a palette edit on either
//! side that is not mirrored on the other fails this test instead of drifting.

use std::path::PathBuf;

use visionclaw_xr_gdext::domain_palette::{
    domain_hex, domain_node_color, hex_to_rgb, DEFAULT_DOMAIN_COLOR, DOMAIN_COLORS,
};

fn ts_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../client/src/features/graph/utils/domainColors.ts");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Pull `(key, '#hex')` pairs out of the `DOMAIN_COLORS` object literal and the
/// `DEFAULT_DOMAIN_COLOR` constant. Keys may be quoted (`'robotics'`) or bare
/// identifiers (`AI`); comment lines are skipped.
fn parse_ts(src: &str) -> (String, Vec<(String, String)>) {
    let mut default = String::new();
    let mut table = Vec::new();
    let mut in_table = false;
    for raw in src.lines() {
        let line = raw.trim();
        if line.starts_with("export const DEFAULT_DOMAIN_COLOR") {
            default = line
                .split('\'')
                .nth(1)
                .expect("DEFAULT_DOMAIN_COLOR has a quoted value")
                .to_string();
            continue;
        }
        if line.starts_with("export const DOMAIN_COLORS") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if line.starts_with("})") {
            break;
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (k, v) = line.split_once(':').expect("table row is key: value");
        let key = k.trim().trim_matches('\'').trim_matches('"').to_string();
        let val = v
            .trim()
            .trim_end_matches(',')
            .trim()
            .trim_matches('\'')
            .trim_matches('"')
            .to_string();
        table.push((key, val));
    }
    (default, table)
}

#[test]
fn rust_table_equals_the_desktop_table_exactly() {
    let (ts_default, ts_table) = parse_ts(&ts_source());
    assert!(ts_table.len() >= 17, "parsed only {} TS rows", ts_table.len());
    assert_eq!(DEFAULT_DOMAIN_COLOR, ts_default, "fallback colour drifted");
    let rust: Vec<(String, String)> = DOMAIN_COLORS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(rust, ts_table, "DOMAIN_COLORS drifted from domainColors.ts (order and values)");
}

#[test]
fn every_ts_key_resolves_to_its_hex_through_the_lookup() {
    let (_, ts_table) = parse_ts(&ts_source());
    for (k, hex) in &ts_table {
        assert_eq!(domain_hex(Some(k)), hex.as_str(), "key {k}");
    }
}

#[test]
fn lookup_semantics_match_get_domain_color() {
    // Vectors from client/src/features/graph/utils/__tests__/domainColors.test.ts.
    let legacy = [
        ("artificial-intelligence", "AI", "#4FC3F7"),
        ("blockchain", "BC", "#81C784"),
        ("robotics", "RB", "#FFB74D"),
        ("spatial-computing", "MV", "#CE93D8"),
        ("distributed-collaboration", "NGM", "#4DB6AC"),
        ("infrastructure", "TC", "#FFD54F"),
    ];
    for (canonical, alias, colour) in legacy {
        for d in [
            canonical.to_string(),
            canonical.to_uppercase(),
            alias.to_string(),
            alias.to_lowercase(),
        ] {
            assert_eq!(domain_hex(Some(&d)), colour, "{d}");
        }
    }
    assert_eq!(domain_hex(Some("space-science-and-systems")), "#646b9f");
    assert_eq!(domain_hex(Some("earth-observation-and-geospatial-sensing")), "#438273");
    for (d, c) in [("DT", "#EF5350"), ("SEC", "#FF7043"), ("INFRA", "#78909C")] {
        assert_eq!(domain_hex(Some(d)), c);
    }
    for d in [None, Some(""), Some(" "), Some("unrecognised"), Some("toString"), Some("__proto__"), Some("constructor")] {
        assert_eq!(domain_hex(d), DEFAULT_DOMAIN_COLOR, "{d:?}");
    }
    assert_eq!(domain_hex(Some(" ai ")), "#4FC3F7");
    // `infra` upper-cases to the legacy INFRA alias, not `infrastructure`.
    assert_eq!(domain_hex(Some("infra")), "#78909C");
}

#[test]
fn hex_decodes_to_unit_rgb() {
    let [r, g, b] = hex_to_rgb("#4FC3F7").unwrap();
    assert!((r - 0x4F as f32 / 255.0).abs() < 1e-6);
    assert!((g - 0xC3 as f32 / 255.0).abs() < 1e-6);
    assert!((b - 0xF7 as f32 / 255.0).abs() < 1e-6);
    assert_eq!(hex_to_rgb("#646b9f").unwrap(), hex_to_rgb("#646B9F").unwrap());
    assert!(hex_to_rgb("4FC3F7").is_none());
    assert!(hex_to_rgb("#4FC3F").is_none());
    assert!(hex_to_rgb("#GGGGGG").is_none());
}

#[test]
fn degree_zero_node_takes_the_exact_palette_colour() {
    // GemNodes domain scheme: HSL(base) + min(cc/30,0.1) sat, + min(cc/40,0.06)
    // light, no authority on the XR wire → degree 0 is the unmodified swatch.
    for (k, hex) in DOMAIN_COLORS {
        let want = hex_to_rgb(hex).unwrap();
        let got = domain_node_color(Some(k), 0);
        for c in 0..3 {
            assert!((got[c] - want[c]).abs() < 1e-4, "{k} channel {c}: {} vs {}", got[c], want[c]);
        }
        assert_eq!(got[3], 1.0);
    }
}

/// Ground truth generated with the worktree's own three.js 0.183.0
/// (`client/node_modules/three`), replaying `GemNodes.tsx`'s domain branch with
/// authority 0: `c.set(hex).getHSL(hsl); c.setHSL(h, min(s + min(cc/30, .1), .95),
/// min(l + min(cc/40, .06), .8)); c.getHexString()`.
#[test]
fn hubs_are_lifted_exactly_like_three_js() {
    let cases = [
        (None, 0u32, "#90a4ae"),
        (None, 5, "#92b2c2"),
        (None, 60, "#92b2c2"),
        (Some("infrastructure"), 10_000, "#fdda81"),
        (Some("AI"), 3, "#6cccfd"),
    ];
    for (domain, cc, want_hex) in cases {
        let got = domain_node_color(domain, cc);
        let want = hex_to_rgb(want_hex).unwrap();
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= 0.5 / 255.0 + 1e-4,
                "{domain:?} cc={cc} ch{c}: {} vs {} ({want_hex})",
                got[c] * 255.0,
                want[c] * 255.0
            );
        }
    }
}

fn hud_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/hud.gd");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `Color("#…")` inside the named GDScript const, in order, plus (for
/// array-of-rows consts) the first quoted string on each row.
fn hud_const(src: &str, name: &str) -> Vec<(Option<String>, String)> {
    let start = src
        .find(&format!("const {name}"))
        .unwrap_or_else(|| panic!("hud.gd has no const {name}"));
    let body = &src[start..];
    // A one-line constant ends at its own newline; only a multi-line array runs
    // to its closing "\n]" (otherwise a one-line array would swallow the next
    // multi-line constant's colours).
    let first_nl = body.find('\n').unwrap_or(body.len());
    let end = if body[..first_nl].trim_end().ends_with(']') || !body[..first_nl].contains('[') {
        first_nl
    } else {
        body.find("\n]").map(|i| i + 2).unwrap_or(first_nl)
    };
    let mut out = Vec::new();
    for line in body[..end].lines() {
        let mut rest = line;
        while let Some(i) = rest.find("Color(\"#") {
            let hex = &rest[i + 7..i + 14];
            let key = line.trim_start().starts_with("[\"").then(|| line.split('"').nth(1).unwrap().to_string());
            out.push((key, hex.to_string()));
            rest = &rest[i + 14..];
        }
    }
    out
}

#[test]
fn hud_key_domain_swatches_match_the_palette() {
    let src = hud_source();
    let rows = hud_const(&src, "KEY_DOMAIN_SWATCHES");
    assert_eq!(rows.len(), 8, "one swatch per canonical domain");
    for (key, hex) in rows {
        let key = key.expect("row carries its domain key");
        assert!(
            domain_hex(Some(&key)).eq_ignore_ascii_case(&hex),
            "hud.gd swatch for {key} is {hex}, palette says {}",
            domain_hex(Some(&key))
        );
    }
    let fallback = hud_const(&src, "KEY_DOMAIN_FALLBACK");
    assert!(fallback[0].1.eq_ignore_ascii_case(DEFAULT_DOMAIN_COLOR));
}

#[test]
fn hud_key_hull_swatches_match_the_cluster_palette() {
    let rows = hud_const(&hud_source(), "KEY_HULL_SWATCHES");
    let want = &visionclaw_xr_gdext::hulls::GPU_CLUSTER_COLORS[..4];
    let got: Vec<String> = rows.into_iter().map(|(_, h)| h).collect();
    assert_eq!(got.len(), 4);
    for (g, w) in got.iter().zip(want) {
        assert!(g.eq_ignore_ascii_case(w), "{g} vs {w}");
    }
}
