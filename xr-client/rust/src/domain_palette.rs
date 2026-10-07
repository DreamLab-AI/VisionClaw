//! Corpus-domain node palette (WP1, XR ↔ desktop parity).
//!
//! The desktop client colours knowledge nodes by corpus domain through
//! `client/src/features/graph/utils/domainColors.ts` (`DOMAIN_COLORS`,
//! `getDomainColor`) and lifts hubs in `GemNodes.tsx` (`colorScheme === 'domain'`).
//! This module is the XR copy of both. It is a deliberate copy, not a binding:
//! `tests/domain_palette_parity.rs` parses the TypeScript file and fails if the
//! two tables differ in key, value or order, so neither side can drift silently.
//!
//! Colour maths follows three.js r183 exactly: `Color.set(hex)` decodes sRGB to
//! the linear working space, `getHSL`/`setHSL` operate *in that linear space*,
//! and the renderer encodes back to sRGB. The values this module returns are
//! the sRGB-encoded result — the hex the desktop actually displays — so a
//! desktop node and an XR node with the same domain and degree match. (Doing the
//! HSL step directly in sRGB is visibly wrong: community 1 comes out `#93d22d`
//! instead of the desktop's `#c8ea74`.)

/// Fallback for a node with no domain or an unrecognised one
/// (`DEFAULT_DOMAIN_COLOR` in `domainColors.ts`).
pub const DEFAULT_DOMAIN_COLOR: &str = "#90A4AE";

/// Domain → hex, in the same order as the desktop table. Lookup goes through
/// [`domain_hex`], which reproduces `getDomainColor`'s alias rules.
pub const DOMAIN_COLORS: &[(&str, &str)] = &[
    ("artificial-intelligence", "#4FC3F7"),
    ("blockchain", "#81C784"),
    ("robotics", "#FFB74D"),
    ("spatial-computing", "#CE93D8"),
    ("distributed-collaboration", "#4DB6AC"),
    ("infrastructure", "#FFD54F"),
    ("space-science-and-systems", "#646b9f"),
    ("earth-observation-and-geospatial-sensing", "#438273"),
    ("AI", "#4FC3F7"),
    ("BC", "#81C784"),
    ("RB", "#FFB74D"),
    ("MV", "#CE93D8"),
    ("TC", "#FFD54F"),
    ("NGM", "#4DB6AC"),
    ("DT", "#EF5350"),
    ("SEC", "#FF7043"),
    ("INFRA", "#78909C"),
];

/// How the node MultiMesh picks a node's base colour. Query marks, agent status
/// and the anomaly blend are applied on top in either mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Desktop default (`colorScheme: 'domain'`): corpus-domain palette.
    #[default]
    Domain,
    /// The original XR scheme: golden-ratio hue per Louvain community.
    Community,
}

impl ColorMode {
    /// Wire/GDScript code: 0 = domain, 1 = community. Unknown codes fall back to
    /// the default so a stale HUD build can never select an undefined mode.
    pub fn from_code(code: i64) -> Self {
        match code {
            1 => Self::Community,
            _ => Self::Domain,
        }
    }

    pub fn code(self) -> i64 {
        match self {
            Self::Domain => 0,
            Self::Community => 1,
        }
    }
}

fn table_get(key: &str) -> Option<&'static str> {
    DOMAIN_COLORS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}

/// `getDomainColor`: trim; an upper-cased alias (`AI`, `INFRA`, …) wins, else
/// the lower-cased canonical slug; anything else is [`DEFAULT_DOMAIN_COLOR`].
/// Prototype names (`toString`, `__proto__`) are plain misses here, matching the
/// desktop's `hasOwnProperty` guard.
pub fn domain_hex(domain: Option<&str>) -> &'static str {
    let Some(raw) = domain else {
        return DEFAULT_DOMAIN_COLOR;
    };
    let value = raw.trim();
    if value.is_empty() {
        return DEFAULT_DOMAIN_COLOR;
    }
    table_get(&value.to_uppercase())
        .or_else(|| table_get(&value.to_lowercase()))
        .unwrap_or(DEFAULT_DOMAIN_COLOR)
}

/// `#RRGGBB` → sRGB components in `[0, 1]`. `None` for any other shape.
pub fn hex_to_rgb(hex: &str) -> Option<[f32; 3]> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let mut out = [0.0f32; 3];
    for (i, c) in out.iter_mut().enumerate() {
        let byte = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
        *c = byte as f32 / 255.0;
    }
    Some(out)
}

/// sRGB → HSL with every component in `[0, 1]` (three.js `Color.getHSL`).
pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (min + max) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l <= 0.5 {
        d / (max + min)
    } else {
        d / (2.0 - max - min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn hue2rgb(p: f32, q: f32, mut t: f32) -> f32 {
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

/// HSL → sRGB (three.js `Color.setHSL`: hue wraps, s and l clamp to `[0, 1]`).
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    if s == 0.0 {
        return [l, l, l];
    }
    let p = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let q = 2.0 * l - p;
    [
        hue2rgb(q, p, h + 1.0 / 3.0),
        hue2rgb(q, p, h),
        hue2rgb(q, p, h - 1.0 / 3.0),
    ]
}

/// three.js `SRGBToLinear` (same constants as `ColorManagement.js`).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c < 0.04045 {
        c * 0.077_399_38
    } else {
        (c * 0.947_867_3 + 0.052_132_7).powf(2.4)
    }
}

/// three.js `LinearToSRGB` (note its 0.41666 exponent, not 1/2.4).
pub fn linear_to_srgb(c: f32) -> f32 {
    if c < 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(0.41666) - 0.055
    }
}

/// three.js `color.setHSL(h, s, l)` followed by sRGB output: HSL in the linear
/// working space, returned sRGB-encoded.
pub fn three_hsl_to_srgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let lin = hsl_to_rgb(h, s, l);
    [
        linear_to_srgb(lin[0]),
        linear_to_srgb(lin[1]),
        linear_to_srgb(lin[2]),
    ]
}

/// three.js `color.set(srgb).getHSL()`: the HSL of the linear-space colour.
pub fn three_srgb_to_hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    rgb_to_hsl(
        srgb_to_linear(rgb[0]),
        srgb_to_linear(rgb[1]),
        srgb_to_linear(rgb[2]),
    )
}

/// Node colour under [`ColorMode::Domain`]: the domain swatch, lifted for hubs
/// exactly as `GemNodes.tsx` does — saturation `+ min(cc/30, 0.1)` capped at
/// 0.95, lightness `+ min(cc/40, 0.06)` capped at 0.8, in three.js's linear HSL. The desktop's authority
/// term is absent because the XR wire carries no authority score, so a degree-0
/// node renders the unmodified palette swatch.
pub fn domain_node_color(domain: Option<&str>, degree: u32) -> [f32; 4] {
    hex_node_color(domain_hex(domain), degree)
}

/// [`domain_node_color`] for an already-resolved swatch hex (the render store
/// resolves the domain once at topology time and keeps the hex).
pub fn hex_node_color(hex: &str, degree: u32) -> [f32; 4] {
    let base = hex_to_rgb(hex)
        .or_else(|| hex_to_rgb(DEFAULT_DOMAIN_COLOR))
        .unwrap_or([0.5, 0.5, 0.5]);
    if degree == 0 {
        return [base[0], base[1], base[2], 1.0];
    }
    let cc = degree as f32;
    let (h, s, l) = three_srgb_to_hsl(base);
    let s2 = (s + (cc / 30.0).min(0.1)).min(0.95);
    let l2 = (l + (cc / 40.0).min(0.06)).min(0.8);
    let rgb = three_hsl_to_srgb(h, s2, l2);
    [rgb[0], rgb[1], rgb[2], 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_round_trips_every_swatch() {
        for (_, hex) in DOMAIN_COLORS {
            let [r, g, b] = hex_to_rgb(hex).unwrap();
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let back = hsl_to_rgb(h, s, l);
            for (c, want) in [r, g, b].iter().enumerate() {
                assert!((back[c] - want).abs() < 1e-5, "{hex} ch{c}");
            }
        }
    }

    #[test]
    fn color_mode_codes_round_trip_and_default_to_domain() {
        assert_eq!(ColorMode::default(), ColorMode::Domain);
        for m in [ColorMode::Domain, ColorMode::Community] {
            assert_eq!(ColorMode::from_code(m.code()), m);
        }
        assert_eq!(ColorMode::from_code(99), ColorMode::Domain);
        assert_eq!(ColorMode::from_code(-1), ColorMode::Domain);
    }
}
