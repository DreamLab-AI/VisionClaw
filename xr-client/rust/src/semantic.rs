//! Semantic encoding of agent and memory activity — a port of the desktop's
//! `client/src/features/visualisation/semanticEncoding.ts` (plus the beam
//! palette it re-exports from `services/binaryProtocol/frameTypes.ts`).
//!
//! Two activity streams feed the embodiment, on both clients:
//! - `memory_flash` text frames (RuVector access) → expanding / imploding ring
//!   bursts whose colour, size, lifetime and ring count encode the verb;
//! - `0x23 AGENT_ACTION` beams (KG mutation) → a cylinder coloured by the action
//!   type and tapered by its shape (Create widens into the node, Delete narrows).
//!
//! The tables here are copies of the TypeScript ones by necessity (Godot cannot
//! import TS). Three tests stop them drifting: one parses the TS source text,
//! one compares against `tests/fixtures/desktop_parity.json` (written by the
//! desktop's own functions in `xrParityFixtures.test.ts`), and one parses the
//! beam shader's colour uniforms so the GPU side agrees too.

/// How a burst moves over its lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstMotion {
    /// Grows outward from the point (the default).
    Expand,
    /// Starts large and contracts onto the point (`delete`).
    Implode,
}

/// Burst parameters for one memory verb (`BurstProfile` in TS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BurstProfile {
    /// Base colour `0xRRGGBB` (sRGB) before the namespace hue jitter.
    pub color: u32,
    /// Radius at peak expansion, in desktop cloud units.
    pub max_scale: f32,
    /// Lifetime in seconds.
    pub duration: f32,
    /// Expand or implode.
    pub motion: BurstMotion,
    /// Concentric ring count — the visual weight of the event.
    pub rings: u8,
}

/// Memory verbs in table order (`MemoryAction` in TS). `access` is the fallback.
pub const MEMORY_ACTIONS: [&str; 6] = ["store", "retrieve", "search", "list", "delete", "access"];

const MEMORY_ACTION_PROFILES: [BurstProfile; 6] = [
    BurstProfile { color: 0x39ff14, max_scale: 4.6, duration: 1.6, motion: BurstMotion::Expand, rings: 2 },
    BurstProfile { color: 0x4fc3f7, max_scale: 3.4, duration: 1.8, motion: BurstMotion::Expand, rings: 1 },
    BurstProfile { color: 0x00fff7, max_scale: 5.8, duration: 2.2, motion: BurstMotion::Expand, rings: 3 },
    BurstProfile { color: 0xffd54f, max_scale: 3.0, duration: 1.4, motion: BurstMotion::Expand, rings: 1 },
    BurstProfile { color: 0xff4444, max_scale: 4.0, duration: 1.4, motion: BurstMotion::Implode, rings: 1 },
    BurstProfile { color: 0x9ad6ff, max_scale: 3.2, duration: 1.6, motion: BurstMotion::Expand, rings: 1 },
];

/// Profile for a memory verb. Case-insensitive; unknown or empty verbs fall back
/// to `access`, exactly like `memoryActionProfile`.
pub fn memory_action_profile(action: &str) -> BurstProfile {
    let key = if action.is_empty() { "access".to_owned() } else { action.to_lowercase() };
    MEMORY_ACTIONS
        .iter()
        .position(|a| *a == key)
        .map(|i| MEMORY_ACTION_PROFILES[i])
        .unwrap_or(MEMORY_ACTION_PROFILES[5])
}

/// Deterministic namespace → hue rotation in `[-0.06, +0.06]` turns
/// (`namespaceHueShift`). Hashes UTF-16 code units, as `charCodeAt` does, with
/// the same wrapping `(h * 31 + c) >>> 0`.
pub fn namespace_hue_shift(ns: &str) -> f64 {
    namespace_hue_shift_max(ns, 0.06)
}

/// [`namespace_hue_shift`] with an explicit bound.
pub fn namespace_hue_shift_max(ns: &str, max_shift: f64) -> f64 {
    if ns.is_empty() {
        return 0.0;
    }
    let mut h: u32 = 0;
    for unit in ns.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(unit as u32);
    }
    let norm = (h % 1000) as f64 / 1000.0;
    (norm * 2.0 - 1.0) * max_shift
}

fn srgb_to_linear(c: f64) -> f64 {
    if c < 0.04045 {
        c * 0.0773993808
    } else {
        (c * 0.9478672986 + 0.0521327014).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c < 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(0.41666) - 0.055
    }
}

fn hue2rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * 6.0 * (2.0 / 3.0 - t);
    }
    p
}

/// HSL of a linear-space colour, matching three.js `Color.getHSL`.
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (min + max) / 2.0;
    if min == max {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

/// Linear-space colour of an HSL triple, matching three.js `Color.setHSL`.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 {
        return (l, l, l);
    }
    let p = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let q = 2.0 * l - p;
    (hue2rgb(q, p, h + 1.0 / 3.0), hue2rgb(q, p, h), hue2rgb(q, p, h - 1.0 / 3.0))
}

fn hex_to_linear(hex: u32) -> (f64, f64, f64) {
    let c = |shift: u32| srgb_to_linear(((hex >> shift) & 0xff) as f64 / 255.0);
    (c(16), c(8), c(0))
}

fn linear_to_hex((r, g, b): (f64, f64, f64)) -> u32 {
    let q = |c: f64| (linear_to_srgb(c) * 255.0).clamp(0.0, 255.0).round() as u32;
    q(r) << 16 | q(g) << 8 | q(b)
}

/// Semantic burst colour `0xRRGGBB` (sRGB) for (action, namespace): the verb
/// sets the base hue and the namespace nudges it (`semanticBurstColor`). The
/// rotation happens in three.js's linear working space, as on the desktop, so
/// the headset shows the same hex.
pub fn semantic_burst_color(action: &str, ns: &str) -> u32 {
    let base = memory_action_profile(action).color;
    let shift = namespace_hue_shift(ns);
    if shift == 0.0 {
        return base;
    }
    let (r, g, b) = hex_to_linear(base);
    let (h, s, l) = rgb_to_hsl(r, g, b);
    linear_to_hex(hsl_to_rgb((h + shift + 1.0) % 1.0, s, l))
}

/// sRGB `0xRRGGBB` → `[r, g, b]` in 0..1 (what a Godot `Color` takes).
pub fn hex_to_rgb01(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

// ─── Agent-action beams ─────────────────────────────────────────────────────

/// `AgentActionType` names in wire order (0 Query .. 5 Transform).
pub const AGENT_ACTION_NAMES: [&str; 6] = ["Query", "Update", "Create", "Delete", "Link", "Transform"];

/// Beam colours (`AGENT_ACTION_COLORS`, sRGB) in wire order.
pub const AGENT_ACTION_COLORS: [u32; 6] = [0x3b82f6, 0xeab308, 0x22c55e, 0xef4444, 0xa855f7, 0x06b6d4];

/// Colour for an unknown action type (`agentActionColorHex` fallback).
pub const AGENT_ACTION_FALLBACK_COLOR: u32 = 0xffffff;

/// Beam taper (`BeamShape` in TS). The cylinder runs agent (−Y) → node (+Y),
/// so `radius_top` is the end touching the KG node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeamShape {
    pub radius_top: f32,
    pub radius_bottom: f32,
    /// Radial segment count on the desktop. The XR beam is one shared cylinder
    /// mesh in a MultiMesh, so this is informational only there.
    pub radial_segments: u8,
}

const AGENT_ACTION_SHAPES: [BeamShape; 6] = [
    BeamShape { radius_top: 0.5, radius_bottom: 0.5, radial_segments: 8 },
    BeamShape { radius_top: 1.0, radius_bottom: 1.0, radial_segments: 10 },
    BeamShape { radius_top: 1.8, radius_bottom: 0.4, radial_segments: 12 },
    BeamShape { radius_top: 0.3, radius_bottom: 1.6, radial_segments: 10 },
    BeamShape { radius_top: 1.3, radius_bottom: 1.3, radial_segments: 12 },
    BeamShape { radius_top: 0.9, radius_bottom: 0.9, radial_segments: 16 },
];

/// Beam shape for an action type; unknown types fall back to the Query probe.
pub fn agent_action_shape(action_type: u32) -> BeamShape {
    AGENT_ACTION_SHAPES
        .get(action_type as usize)
        .copied()
        .unwrap_or(AGENT_ACTION_SHAPES[0])
}

/// Beam colour for an action type; unknown types fall back to white.
pub fn agent_action_color(action_type: u32) -> u32 {
    AGENT_ACTION_COLORS
        .get(action_type as usize)
        .copied()
        .unwrap_or(AGENT_ACTION_FALLBACK_COLOR)
}

/// Beam `INSTANCE_CUSTOM.rgb`: `[action code, radius_top, radius_bottom]`.
/// The code is the wire action type, or 6 for unknown (the shader's white).
/// Stride stays 16 (XR-client Invariant 3): `.a` remains the agent status.
pub fn beam_custom_rgb(action_type: u32) -> [f32; 3] {
    let code = if (action_type as usize) < AGENT_ACTION_COLORS.len() { action_type } else { 6 };
    let s = agent_action_shape(action_type);
    [code as f32, s.radius_top, s.radius_bottom]
}

// ─── memory_flash text frames ───────────────────────────────────────────────

/// One decoded `memory_flash` event (`data` of the text frame).
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryFlash {
    pub key: String,
    pub namespace: String,
    pub action: String,
    /// Server epoch ms, 0 when absent.
    pub timestamp: u64,
}

/// Upper bound on events honoured from one frame, so a hostile or runaway
/// batch cannot flood the 64-slot burst pool in a single frame.
pub const MAX_FLASHES_PER_FRAME: usize = 64;

const MAX_FLASH_STRING: usize = 512;

fn short_string(v: Option<&serde_json::Value>) -> String {
    match v.and_then(|x| x.as_str()) {
        Some(s) if s.len() <= MAX_FLASH_STRING => s.to_owned(),
        _ => String::new(),
    }
}

/// Parse a `memory_flash` text frame. `data` may be one event object (the
/// server's per-event broadcast from `/api/memory-flash` and the per-event
/// fan-out of `/api/memory-flash/batch`) or an array of them (the batch form).
/// Mirrors the desktop's guard: an event with neither key nor namespace is
/// dropped. Returns an empty list for anything that is not a `memory_flash`
/// frame or is malformed; never panics.
pub fn parse_memory_flash(json: &str) -> Vec<MemoryFlash> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("memory_flash") {
        return Vec::new();
    }
    let one = |d: &serde_json::Value| -> Option<MemoryFlash> {
        if !d.is_object() {
            return None;
        }
        let key = short_string(d.get("key"));
        let namespace = short_string(d.get("namespace"));
        if key.is_empty() && namespace.is_empty() {
            return None;
        }
        let action = short_string(d.get("action"));
        let timestamp = d.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0);
        Some(MemoryFlash { key, namespace, action, timestamp })
    };
    match v.get("data") {
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(one).take(MAX_FLASHES_PER_FRAME).collect()
        }
        Some(d) => one(d).into_iter().collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_file(rel: &str) -> String {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }

    fn fixture() -> serde_json::Value {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/desktop_parity.json");
        serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("fixture json")
    }

    /// Text between the first `{` after `anchor` and its matching `}`.
    fn block_after<'a>(src: &'a str, anchor: &str) -> &'a str {
        let start = src.find(anchor).unwrap_or_else(|| panic!("anchor {anchor} missing"));
        let open = start + src[start..].find('{').expect("open brace");
        let mut depth = 0;
        for (i, ch) in src[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[open + 1..open + i];
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced block after {anchor}");
    }

    fn field<'a>(row: &'a str, name: &str) -> &'a str {
        let at = row.find(&format!("{name}:")).unwrap_or_else(|| panic!("{name} missing in {row}"));
        let rest = row[at + name.len() + 1..].trim_start();
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        rest[..end].trim().trim_matches('\'')
    }

    #[test]
    fn memory_profiles_match_the_typescript_source_table() {
        let src = repo_file("client/src/features/visualisation/semanticEncoding.ts");
        let table = block_after(&src, "const MEMORY_ACTION_PROFILES");
        let mut seen = 0;
        for line in table.lines() {
            let line = line.trim();
            let Some(colon) = line.find(':') else { continue };
            let verb = line[..colon].trim();
            if !MEMORY_ACTIONS.contains(&verb) {
                continue;
            }
            let row = &line[colon + 1..];
            let p = memory_action_profile(verb);
            let color = u32::from_str_radix(field(row, "color").trim_start_matches("0x"), 16).unwrap();
            assert_eq!(p.color, color, "{verb} colour");
            assert_eq!(p.max_scale, field(row, "maxScale").parse::<f32>().unwrap(), "{verb} maxScale");
            assert_eq!(p.duration, field(row, "duration").parse::<f32>().unwrap(), "{verb} duration");
            assert_eq!(p.rings, field(row, "rings").parse::<u8>().unwrap(), "{verb} rings");
            let motion = if field(row, "motion") == "implode" { BurstMotion::Implode } else { BurstMotion::Expand };
            assert_eq!(p.motion, motion, "{verb} motion");
            seen += 1;
        }
        assert_eq!(seen, MEMORY_ACTIONS.len(), "every TS verb row parsed and none extra");
    }

    #[test]
    fn beam_shapes_and_colours_match_the_typescript_source() {
        let src = repo_file("client/src/features/visualisation/semanticEncoding.ts");
        let shapes = block_after(&src, "const AGENT_ACTION_SHAPES");
        let colours_src = repo_file("client/src/services/binaryProtocol/frameTypes.ts");
        let colours = block_after(&colours_src, "export const AGENT_ACTION_COLORS");
        for (i, name) in AGENT_ACTION_NAMES.iter().enumerate() {
            let tag = format!("[AgentActionType.{name}]:");
            let srow = shapes.lines().find(|l| l.contains(&tag)).unwrap_or_else(|| panic!("shape {name}"));
            let s = agent_action_shape(i as u32);
            assert_eq!(s.radius_top, field(srow, "radiusTop").parse::<f32>().unwrap(), "{name} top");
            assert_eq!(s.radius_bottom, field(srow, "radiusBottom").parse::<f32>().unwrap(), "{name} bottom");
            assert_eq!(s.radial_segments, field(srow, "radialSegments").parse::<u8>().unwrap(), "{name} segs");
            let crow = colours.lines().find(|l| l.contains(&tag)).unwrap_or_else(|| panic!("colour {name}"));
            let hex = crow.split('\'').nth(1).expect("quoted hex").trim_start_matches('#');
            assert_eq!(agent_action_color(i as u32), u32::from_str_radix(hex, 16).unwrap(), "{name} colour");
        }
    }

    #[test]
    fn profiles_shapes_and_burst_colours_match_the_desktop_fixture() {
        let f = fixture();
        for row in f["memoryProfiles"].as_array().unwrap() {
            let p = memory_action_profile(row["action"].as_str().unwrap());
            assert_eq!(p.color as u64, row["color"].as_u64().unwrap());
            assert!((p.max_scale as f64 - row["maxScale"].as_f64().unwrap()).abs() < 1e-6);
            assert_eq!(p.rings as u64, row["rings"].as_u64().unwrap());
        }
        let colours = f["burstColors"].as_array().unwrap();
        assert!(colours.len() >= 40);
        for row in colours {
            let (a, ns) = (row["action"].as_str().unwrap(), row["namespace"].as_str().unwrap());
            assert!((namespace_hue_shift(ns) - row["shift"].as_f64().unwrap()).abs() < 1e-12, "{ns} shift");
            let want = u32::from_str_radix(row["hex"].as_str().unwrap(), 16).unwrap();
            assert_eq!(semantic_burst_color(a, ns), want, "{a}/{ns}");
        }
        for row in f["beam"].as_array().unwrap() {
            let t = row["actionType"].as_u64().unwrap() as u32;
            let hex = row["color"].as_str().unwrap().trim_start_matches('#');
            assert_eq!(agent_action_color(t), u32::from_str_radix(hex, 16).unwrap());
            let s = agent_action_shape(t);
            assert!((s.radius_top as f64 - row["radiusTop"].as_f64().unwrap()).abs() < 1e-6);
            assert!((s.radius_bottom as f64 - row["radiusBottom"].as_f64().unwrap()).abs() < 1e-6);
        }
    }

    #[test]
    fn beam_shader_uniform_defaults_match_the_action_palette() {
        let shader = repo_file("xr-client/materials/agent_beam.gdshader");
        for (i, name) in AGENT_ACTION_NAMES.iter().enumerate() {
            let uni = format!("uniform vec3 action_{}_color", name.to_lowercase());
            let line = shader.lines().find(|l| l.contains(&uni)).unwrap_or_else(|| panic!("{uni} missing"));
            let inner = line.split("vec3(").nth(1).expect("vec3 default").split(')').next().unwrap();
            let got: Vec<f32> = inner.split(',').map(|v| v.trim().parse().unwrap()).collect();
            let want = hex_to_rgb01(AGENT_ACTION_COLORS[i]);
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 0.002, "{name} channel {c}: {} vs {}", got[c], want[c]);
            }
        }
    }

    #[test]
    fn verbs_fall_back_and_ignore_case() {
        assert_eq!(memory_action_profile("STORE"), memory_action_profile("store"));
        assert_eq!(memory_action_profile("bogus"), memory_action_profile("access"));
        assert_eq!(memory_action_profile(""), memory_action_profile("access"));
        assert_eq!(memory_action_profile("delete").motion, BurstMotion::Implode);
        assert_eq!(namespace_hue_shift(""), 0.0);
        assert!(namespace_hue_shift("personal-context").abs() <= 0.06);
    }

    #[test]
    fn beam_custom_carries_code_and_taper() {
        assert_eq!(beam_custom_rgb(2), [2.0, 1.8, 0.4]);
        assert_eq!(beam_custom_rgb(3), [3.0, 0.3, 1.6]);
        assert_eq!(beam_custom_rgb(42), [6.0, 0.5, 0.5], "unknown → white code, Query probe");
    }

    #[test]
    fn memory_flash_parses_single_batch_and_rejects_junk() {
        let one = r#"{"type":"memory_flash","data":{"key":"k1","namespace":"patterns","action":"store","timestamp":12}}"#;
        assert_eq!(
            parse_memory_flash(one),
            vec![MemoryFlash { key: "k1".into(), namespace: "patterns".into(), action: "store".into(), timestamp: 12 }]
        );
        let batch = r#"{"type":"memory_flash","data":[{"key":"a"},{"namespace":"n","action":"search"},{"action":"x"},7]}"#;
        let got = parse_memory_flash(batch);
        assert_eq!(got.len(), 2, "keyless+namespaceless and non-object entries dropped");
        assert_eq!(got[1].action, "search");
        assert!(parse_memory_flash(r#"{"type":"other","data":{"key":"a"}}"#).is_empty());
        assert!(parse_memory_flash("not json").is_empty());
        assert!(parse_memory_flash(r#"{"type":"memory_flash"}"#).is_empty());
        let many: Vec<String> = (0..200).map(|i| format!(r#"{{"key":"k{i}"}}"#)).collect();
        let big = format!(r#"{{"type":"memory_flash","data":[{}]}}"#, many.join(","));
        assert_eq!(parse_memory_flash(&big).len(), MAX_FLASHES_PER_FRAME);
        let long = format!(r#"{{"type":"memory_flash","data":{{"key":"{}"}}}}"#, "x".repeat(600));
        assert!(parse_memory_flash(&long).is_empty(), "over-long key treated as absent");
    }
}
