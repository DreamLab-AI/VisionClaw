//! Per-node highlight mask (ADR-2188 D4, ADR-2183 semantics).
//!
//! One byte per render-store slot, indexed like the store's position array, so
//! a pack-plan entry (which records its slot) reads its byte with one index and
//! no hashing. The byte is a set of flags; [`level`] folds it into the ADR-2183
//! tri-state the renderers use: 0 greyed out, 1 normal, 2 emphasised.
//!
//! Interactive filters write [`FILTERED_OUT`] instead of removing a node, so a
//! filtered node keeps its place and its edges and is drawn greyed. What still
//! removes a node is unchanged (ADR-2183 D4): the server visibility filter, the
//! population scope and hierarchy folding (`fold_hidden`).
//!
//! The mask is display-only. It never reaches the layout or the wire.
//!
//! Precedence, pinned by the tests below: an explicit pointer state (selected
//! or hovered) outranks a filter, so the node the user is pointing at is never
//! greyed; a filter outranks the ambient emphases (search hit, focus
//! neighbour), so a search cannot pull a filtered-out node back into view.

/// The node is selected (held or picked).
pub const SELECTED: u8 = 1 << 0;
/// The controller ray is on the node.
pub const HOVERED: u8 = 1 << 1;
/// The node matches the active search or query.
pub const SEARCH_HIT: u8 = 1 << 2;
/// An interactive filter excludes the node: drawn greyed, never removed.
pub const FILTERED_OUT: u8 = 1 << 3;
/// The node is a direct neighbour of the selection.
pub const FOCUS_NEIGHBOUR: u8 = 1 << 4;
/// Every flag the mask defines.
pub const ALL: u8 = SELECTED | HOVERED | SEARCH_HIT | FILTERED_OUT | FOCUS_NEIGHBOUR;

/// ADR-2183 level: greyed out.
pub const LEVEL_GREY: u8 = 0;
/// ADR-2183 level: normal.
pub const LEVEL_NORMAL: u8 = 1;
/// ADR-2183 level: emphasised.
pub const LEVEL_EMPHASIS: u8 = 2;

/// How far a greyed colour is pulled toward the background after desaturation.
pub const GREY_MIX: f32 = 0.7;
/// Near-black blue, close to the dark sky behind the graph (`eye_candy_sky.gdshader`).
pub const GREY_BACKGROUND: [f32; 3] = [0.02, 0.03, 0.05];
/// Size multiplier for an emphasised node: emphasis is size and colour, never glow.
pub const EMPHASIS_SIZE: f32 = 1.35;
/// How far an emphasised colour is lifted toward white.
pub const EMPHASIS_LIFT: f32 = 0.3;

/// Fold a flag byte into its ADR-2183 level.
pub fn level(flags: u8) -> u8 {
    if flags & (SELECTED | HOVERED) != 0 {
        LEVEL_EMPHASIS
    } else if flags & FILTERED_OUT != 0 {
        LEVEL_GREY
    } else if flags & (SEARCH_HIT | FOCUS_NEIGHBOUR) != 0 {
        LEVEL_EMPHASIS
    } else {
        LEVEL_NORMAL
    }
}

/// Desaturate `rgb` to its luminance and pull it toward the background.
pub fn grey_rgb(rgb: &mut [f32]) {
    let l = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    for (c, bg) in rgb.iter_mut().zip(GREY_BACKGROUND) {
        *c = l + (bg - l) * GREY_MIX;
    }
}

/// Lift `rgb` toward white for emphasis.
pub fn emphasise_rgb(rgb: &mut [f32]) {
    for c in rgb.iter_mut() {
        *c += (1.0 - *c) * EMPHASIS_LIFT;
    }
}

/// Apply the mask byte of `slot` to one packed node instance: grey (no halo,
/// no outline) or emphasis (lifted colour, larger, outline flag). Returns the
/// size multiplier. `custom[3]` carries the level to the node shaders
/// (0 grey, 1 normal, 2 emphasised); it was a constant 1 before ADR-2188, and
/// `custom[0]` (the centrality halo tell) is zeroed for a greyed node.
#[inline]
pub fn apply(mask: &HighlightMask, slot: usize, col: &mut [f32; 4], custom: &mut [f32; 4]) -> f32 {
    match level(mask.get(slot)) {
        LEVEL_GREY => {
            grey_rgb(&mut col[..3]);
            custom[0] = 0.0;
            custom[3] = 0.0;
            1.0
        }
        LEVEL_EMPHASIS => {
            emphasise_rgb(&mut col[..3]);
            custom[3] = 2.0;
            EMPHASIS_SIZE
        }
        _ => {
            custom[3] = 1.0;
            1.0
        }
    }
}

/// Edge `custom.r`: 1 when either endpoint slot is greyed out (the edge shaders
/// draw it at `grey_alpha`), else 0.
#[inline]
pub fn edge_grey(mask: &HighlightMask, s_slot: usize, t_slot: usize) -> f32 {
    if mask.level_of(s_slot) == LEVEL_GREY || mask.level_of(t_slot) == LEVEL_GREY {
        1.0
    } else {
        0.0
    }
}

/// The per-slot flag bytes plus a version that changes whenever any byte does.
#[derive(Default, Debug, Clone)]
pub struct HighlightMask {
    flags: Vec<u8>,
    version: u64,
}

impl HighlightMask {
    /// Flags of `slot` (0 for a slot never written).
    #[inline]
    pub fn get(&self, slot: usize) -> u8 {
        self.flags.get(slot).copied().unwrap_or(0)
    }

    /// ADR-2183 level of `slot`.
    #[inline]
    pub fn level_of(&self, slot: usize) -> u8 {
        level(self.get(slot))
    }

    /// Set or clear `bit` on `slot`; returns whether the byte changed.
    pub fn set_bit(&mut self, slot: usize, bit: u8, on: bool) -> bool {
        if !on && slot >= self.flags.len() {
            return false;
        }
        if slot >= self.flags.len() {
            self.flags.resize(slot + 1, 0);
        }
        let old = self.flags[slot];
        let new = if on { old | bit } else { old & !bit };
        if new == old {
            return false;
        }
        self.flags[slot] = new;
        self.version = self.version.wrapping_add(1);
        true
    }

    /// Clear `bit` on every slot; returns how many bytes changed.
    pub fn clear_bit(&mut self, bit: u8) -> usize {
        let mut n = 0;
        for f in self.flags.iter_mut() {
            if *f & bit != 0 {
                *f &= !bit;
                n += 1;
            }
        }
        if n > 0 {
            self.version = self.version.wrapping_add(1);
        }
        n
    }

    /// Number of slots with `bit` set.
    pub fn count(&self, bit: u8) -> usize {
        self.flags.iter().filter(|&&f| f & bit != 0).count()
    }

    /// Drop every byte (a new id space).
    pub fn clear(&mut self) {
        if !self.flags.is_empty() {
            self.flags.clear();
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Changes whenever any byte changes; consumers re-emit only on a new value.
    pub fn version(&self) -> u64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_adr_2183() {
        assert_eq!(level(0), LEVEL_NORMAL);
        assert_eq!(level(FILTERED_OUT), LEVEL_GREY);
        assert_eq!(level(SEARCH_HIT), LEVEL_EMPHASIS);
        assert_eq!(level(FOCUS_NEIGHBOUR), LEVEL_EMPHASIS);
        assert_eq!(level(SELECTED), LEVEL_EMPHASIS);
        assert_eq!(level(HOVERED), LEVEL_EMPHASIS);
    }

    #[test]
    fn pointer_state_outranks_a_filter_and_a_filter_outranks_search() {
        assert_eq!(level(SELECTED | FILTERED_OUT), LEVEL_EMPHASIS);
        assert_eq!(level(HOVERED | FILTERED_OUT), LEVEL_EMPHASIS);
        assert_eq!(level(SEARCH_HIT | FILTERED_OUT), LEVEL_GREY);
        assert_eq!(level(FOCUS_NEIGHBOUR | FILTERED_OUT), LEVEL_GREY);
    }

    #[test]
    fn set_and_clear_bump_the_version_only_on_change() {
        let mut m = HighlightMask::default();
        let v0 = m.version();
        assert!(!m.set_bit(5, SELECTED, false), "clearing an unwritten slot is a no-op");
        assert_eq!(m.version(), v0);
        assert!(m.set_bit(5, SELECTED, true));
        assert!(!m.set_bit(5, SELECTED, true), "same value: no change");
        let v1 = m.version();
        assert!(v1 != v0);
        assert_eq!(m.get(5), SELECTED);
        assert_eq!(m.get(4), 0);
        assert_eq!(m.clear_bit(SELECTED), 1);
        assert!(m.version() != v1);
        assert_eq!(m.clear_bit(SELECTED), 0);
    }

    #[test]
    fn grey_is_desaturated_and_darker_and_emphasis_is_lighter() {
        let mut c = [0.9, 0.2, 0.1];
        grey_rgb(&mut c);
        assert!((c[0] - c[1]).abs() < 0.05 && (c[1] - c[2]).abs() < 0.05, "desaturated: {c:?}");
        assert!(c[0] < 0.2, "pulled toward the background: {c:?}");
        let mut e = [0.5, 0.5, 0.5];
        emphasise_rgb(&mut e);
        assert!(e[0] > 0.5 && e[0] <= 1.0);
    }
}
