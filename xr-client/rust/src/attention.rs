//! File-attention heat — a port of the desktop's
//! `client/src/features/visualisation/attentionHeat.ts` and the brighten
//! factor from `heatColor.ts`.
//!
//! Every `0x23 AGENT_ACTION` touches its target node: +1 raw heat (capped),
//! decaying exponentially with a 20 s half-life. The read side normalises raw
//! heat through `1 - exp(-raw / 1.5)` so a single touch reads ~0.49 and a
//! hammered node approaches but never reaches 1. The map holds at most 512
//! nodes; a new touch over the cap evicts the coldest entry.
//!
//! Heat brightens a node in place (hue and saturation preserved, the brightest
//! channel capped at 1), so a hot community-red node becomes a brighter red,
//! never white. It is applied to the node's per-instance colour in the render
//! store; the edge buffer layout is untouched (XR-client Invariant 3).
//!
//! The clock is injected (`now_ms`, any monotonic millisecond origin) so the
//! maths is unit-tested with the same vectors as `attentionHeat.test.ts`.

use std::collections::HashMap;

use crate::binary_protocol::NODE_ID_MASK;

/// Default decay half-life in ms (20 s).
pub const DEFAULT_HEAT_HALF_LIFE_MS: f64 = 20_000.0;
/// Raw heat added per touch.
pub const HEAT_PER_TOUCH: f64 = 1.0;
/// Upper bound on raw heat so a hammered node still cools promptly.
pub const MAX_RAW_HEAT: f64 = 6.0;
/// Scale of the saturating 0..1 curve.
pub const HEAT_SATURATION: f64 = 1.5;
/// Raw heat at or below this is cold: dropped by sweeps, ignored by `has_heat`.
pub const COLD_RAW_EPSILON: f64 = 0.01;
/// Default cap on tracked nodes.
pub const DEFAULT_MAX_HEAT_ENTRIES: usize = 512;
/// Luminance gain per unit heat (`HEAT_BRIGHTEN_K`).
pub const HEAT_BRIGHTEN_K: f64 = 0.8;

/// Normalise raw heat to a saturating 0..1 value (`normaliseHeat`).
pub fn normalise_heat(raw: f64) -> f64 {
    if raw <= 0.0 {
        0.0
    } else {
        1.0 - (-raw / HEAT_SATURATION).exp()
    }
}

fn decay_raw(raw: f64, from_ms: f64, to_ms: f64, half_life_ms: f64) -> f64 {
    let dt = to_ms - from_ms;
    if dt <= 0.0 || raw <= 0.0 {
        return raw;
    }
    raw * 0.5f64.powf(dt / half_life_ms)
}

/// Uncapped gain for a normalised heat (`heatGain`).
pub fn heat_gain(heat: f64) -> f64 {
    1.0 + heat.max(0.0) * HEAT_BRIGHTEN_K
}

/// Ratio-preserving brighten factor for a base colour at `heat`
/// (`heatBrightenFactor`): `f >= 1`, and `max(r,g,b) * f <= 1` unless the
/// base already clips. Black returns the bare gain.
pub fn heat_brighten_factor(r: f64, g: f64, b: f64, heat: f64) -> f64 {
    let gain = heat_gain(heat);
    let maxc = r.max(g).max(b);
    if maxc <= 0.0 {
        return gain;
    }
    gain.min(1.0 / maxc).max(1.0)
}

#[derive(Debug, Clone, Copy)]
struct HeatEntry {
    raw: f64,
    ts: f64,
}

/// Decaying per-node attention accumulator (`createAttentionHeatAccumulator`).
#[derive(Debug, Clone)]
pub struct AttentionHeat {
    entries: HashMap<u32, HeatEntry>,
    half_life_ms: f64,
    max_entries: usize,
    enabled: bool,
    version: u64,
}

impl Default for AttentionHeat {
    fn default() -> Self {
        Self::new(DEFAULT_HEAT_HALF_LIFE_MS, DEFAULT_MAX_HEAT_ENTRIES)
    }
}

impl AttentionHeat {
    /// New accumulator; half-life and cap are floored at 1 like the TS.
    pub fn new(half_life_ms: f64, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            half_life_ms: half_life_ms.max(1.0),
            max_entries: max_entries.max(1),
            enabled: true,
            version: 0,
        }
    }

    /// Wire ids may carry flag bits 26-31; heat is keyed by the masked id so a
    /// beam targeting `0x4000_0001` and node `1` share one entry.
    fn key(node_id: u32) -> u32 {
        node_id & NODE_ID_MASK
    }

    fn evict_coldest(&mut self, now_ms: f64) {
        let hl = self.half_life_ms;
        let coldest = self
            .entries
            .iter()
            .map(|(&k, e)| (k, decay_raw(e.raw, e.ts, now_ms, hl)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k);
        if let Some(k) = coldest {
            self.entries.remove(&k);
        }
    }

    fn touch_inner(&mut self, node_id: u32, now_ms: f64) {
        let key = Self::key(node_id);
        let hl = self.half_life_ms;
        if let Some(e) = self.entries.get_mut(&key) {
            let decayed = decay_raw(e.raw, e.ts, now_ms, hl);
            e.raw = (decayed + HEAT_PER_TOUCH).min(MAX_RAW_HEAT);
            e.ts = now_ms;
        } else {
            if self.entries.len() >= self.max_entries {
                self.evict_coldest(now_ms);
            }
            self.entries.insert(key, HeatEntry { raw: HEAT_PER_TOUCH.min(MAX_RAW_HEAT), ts: now_ms });
        }
    }

    /// One touch (one version bump). No-op while disabled.
    pub fn touch(&mut self, node_id: u32, now_ms: f64) {
        if !self.enabled {
            return;
        }
        self.touch_inner(node_id, now_ms);
        self.version += 1;
    }

    /// A batch of touches with one version bump; empty batches do nothing.
    pub fn touch_many(&mut self, node_ids: &[u32], now_ms: f64) {
        if !self.enabled || node_ids.is_empty() {
            return;
        }
        for &id in node_ids {
            self.touch_inner(id, now_ms);
        }
        self.version += 1;
    }

    /// Current decayed heat, normalised 0..1. Read-only.
    pub fn get_heat(&self, node_id: u32, now_ms: f64) -> f64 {
        match self.entries.get(&Self::key(node_id)) {
            Some(e) => normalise_heat(decay_raw(e.raw, e.ts, now_ms, self.half_life_ms)),
            None => 0.0,
        }
    }

    /// True while any node still holds meaningful heat.
    pub fn has_heat(&self, now_ms: f64) -> bool {
        self.entries
            .values()
            .any(|e| decay_raw(e.raw, e.ts, now_ms, self.half_life_ms) > COLD_RAW_EPSILON)
    }

    /// Drop cold entries; returns how many went.
    pub fn sweep(&mut self, now_ms: f64) -> usize {
        let hl = self.half_life_ms;
        let before = self.entries.len();
        self.entries.retain(|_, e| decay_raw(e.raw, e.ts, now_ms, hl) > COLD_RAW_EPSILON);
        before - self.entries.len()
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Live tuning; `enabled = false` freezes accumulation without clearing.
    pub fn configure(&mut self, half_life_ms: Option<f64>, enabled: Option<bool>, max_entries: Option<usize>) {
        if let Some(h) = half_life_ms {
            self.half_life_ms = h.max(1.0);
        }
        if let Some(m) = max_entries {
            self.max_entries = m.max(1);
        }
        if let Some(e) = enabled {
            self.enabled = e;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Brighten `rgb` in place by this node's current heat. Returns the factor
    /// applied (1 when cold or disabled reads nothing).
    pub fn brighten(&self, node_id: u32, now_ms: f64, rgb: &mut [f32]) -> f32 {
        if self.entries.is_empty() {
            return 1.0;
        }
        let heat = self.get_heat(node_id, now_ms);
        if heat <= 0.0 {
            return 1.0;
        }
        let f = heat_brighten_factor(rgb[0] as f64, rgb[1] as f64, rgb[2] as f64, heat) as f32;
        for c in rgb.iter_mut().take(3) {
            *c *= f;
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWLEDGE_NODE_FLAG: u32 = 0x4000_0000;
    const ONTOLOGY_CLASS_FLAG: u32 = 0x0400_0000;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 5e-7
    }

    fn fixture() -> serde_json::Value {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/desktop_parity.json");
        serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json")
    }

    // ── one-to-one ports of attentionHeat.test.ts ──

    #[test]
    fn touch_raises_heat_to_the_single_touch_value() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        assert_eq!(acc.get_heat(5, 0.0), 0.0);
        acc.touch(5, 0.0);
        assert!(close(acc.get_heat(5, 0.0), normalise_heat(1.0)));
        assert_eq!(acc.size(), 1);
    }

    #[test]
    fn accumulates_monotonically_and_saturates_below_one() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        acc.touch(1, 0.0);
        let one = acc.get_heat(1, 0.0);
        acc.touch(1, 0.0);
        assert!(acc.get_heat(1, 0.0) > one);
        for _ in 0..100 {
            acc.touch(1, 0.0);
        }
        let hot = acc.get_heat(1, 0.0);
        assert!(hot > 0.9 && hot < 1.0);
        assert!(close(hot, normalise_heat(MAX_RAW_HEAT)));
    }

    #[test]
    fn decays_exponentially_with_the_half_life() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        acc.touch(7, 0.0);
        assert!(close(acc.get_heat(7, 1000.0), normalise_heat(0.5)));
        assert!(close(acc.get_heat(7, 2000.0), normalise_heat(0.25)));
    }

    #[test]
    fn reconciles_wire_flag_bits_with_masked_ids() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        acc.touch(KNOWLEDGE_NODE_FLAG | 42, 0.0);
        assert!(close(acc.get_heat(42, 0.0), normalise_heat(1.0)));
        acc.touch(ONTOLOGY_CLASS_FLAG | 9, 0.0);
        assert!(acc.get_heat(9, 0.0) > 0.0);
        assert_eq!(acc.get_heat(43, 0.0), 0.0);
    }

    #[test]
    fn caps_size_and_evicts_the_coldest() {
        let mut acc = AttentionHeat::new(1000.0, 3);
        acc.touch(1, 0.0);
        acc.touch(2, 0.0);
        acc.touch(2, 0.0);
        for _ in 0..3 {
            acc.touch(3, 0.0);
        }
        assert_eq!(acc.size(), 3);
        acc.touch(4, 0.0);
        assert_eq!(acc.size(), 3);
        assert_eq!(acc.get_heat(1, 0.0), 0.0);
        assert!(acc.get_heat(4, 0.0) > 0.0);
        assert!(acc.get_heat(3, 0.0) > 0.0);
    }

    #[test]
    fn reports_and_sweeps_cold_entries() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        acc.touch(1, 0.0);
        assert!(acc.has_heat(0.0));
        let later = 15_000.0;
        assert!(!acc.has_heat(later));
        assert!(acc.get_heat(1, later) < 0.001);
        assert_eq!(acc.sweep(later), 1);
        assert_eq!(acc.size(), 0);
    }

    #[test]
    fn honours_enabled_and_configure() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        acc.configure(None, Some(false), None);
        acc.touch(1, 0.0);
        acc.touch_many(&[2, 3], 0.0);
        assert_eq!(acc.size(), 0);
        acc.configure(None, Some(true), None);
        acc.touch(1, 0.0);
        assert!(acc.get_heat(1, 0.0) > 0.0);
        acc.configure(Some(500.0), None, None);
        assert!(close(acc.get_heat(1, 500.0), normalise_heat(0.5)));
    }

    #[test]
    fn bumps_version_once_per_batch() {
        let mut acc = AttentionHeat::new(1000.0, 512);
        let v0 = acc.version();
        acc.touch_many(&[1, 2, 3], 0.0);
        assert_eq!(acc.version(), v0 + 1);
        acc.touch(4, 0.0);
        assert_eq!(acc.version(), v0 + 2);
        acc.touch_many(&[], 0.0);
        assert_eq!(acc.version(), v0 + 2);
    }

    #[test]
    fn normalise_heat_is_bounded_and_monotonic() {
        assert_eq!(normalise_heat(0.0), 0.0);
        assert_eq!(normalise_heat(-1.0), 0.0);
        assert!(close(normalise_heat(1.0), 1.0 - (-1.0 / HEAT_SATURATION).exp()));
        assert!(normalise_heat(1000.0) <= 1.0);
        assert!(normalise_heat(MAX_RAW_HEAT) < 1.0);
        assert!(normalise_heat(2.0) > normalise_heat(1.0));
    }

    // ── fixture parity (desktop functions' own outputs) ──

    #[test]
    fn constants_curve_and_brighten_match_the_desktop_fixture() {
        let f = fixture();
        let c = &f["heatConstants"];
        assert_eq!(c["halfLifeMs"].as_f64().unwrap(), DEFAULT_HEAT_HALF_LIFE_MS);
        assert_eq!(c["perTouch"].as_f64().unwrap(), HEAT_PER_TOUCH);
        assert_eq!(c["maxRaw"].as_f64().unwrap(), MAX_RAW_HEAT);
        assert_eq!(c["saturation"].as_f64().unwrap(), HEAT_SATURATION);
        assert_eq!(c["coldEpsilon"].as_f64().unwrap(), COLD_RAW_EPSILON);
        assert_eq!(c["maxEntries"].as_u64().unwrap() as usize, DEFAULT_MAX_HEAT_ENTRIES);
        assert_eq!(c["brightenK"].as_f64().unwrap(), HEAT_BRIGHTEN_K);
        for pair in f["normaliseHeat"].as_array().unwrap() {
            let (raw, want) = (pair[0].as_f64().unwrap(), pair[1].as_f64().unwrap());
            assert!((normalise_heat(raw) - want).abs() < 1e-12, "normalise({raw})");
        }
        for row in f["heatBrighten"].as_array().unwrap() {
            let v: Vec<f64> = row.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            assert!((heat_brighten_factor(v[0], v[1], v[2], v[3]) - v[4]).abs() < 1e-12, "{v:?}");
        }
    }

    #[test]
    fn brighten_preserves_ratio_and_never_clips_to_white() {
        let mut acc = AttentionHeat::default();
        for _ in 0..6 {
            acc.touch(10, 0.0);
        }
        let mut rgb = [0.8f32, 0.2, 0.1, 1.0];
        let f = acc.brighten(10, 0.0, &mut rgb);
        assert!(f > 1.0);
        assert!(rgb[0] <= 1.0 + 1e-6, "brightest channel capped at 1");
        assert!((rgb[1] / rgb[0] - 0.25).abs() < 1e-5, "ratio preserved");
        assert_eq!(rgb[3], 1.0, "alpha untouched");
        let mut cold = [0.5f32, 0.5, 0.5, 1.0];
        assert_eq!(acc.brighten(11, 0.0, &mut cold), 1.0);
        assert_eq!(cold, [0.5, 0.5, 0.5, 1.0]);
    }
}
