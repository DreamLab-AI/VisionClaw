#!/usr/bin/env python3
"""Build fonts/HudSans-SemiBold.ttf: Open Sans SemiBold plus the HUD's symbols.

Why: Godot's default font (Open Sans SemiBold) lacks the arrows, shapes and
check boxes the HUD shows (-> i-circle triangles bullets check boxes warning).
Each missing glyph is drawn from a fallback system font with its own glyph
texture, which breaks the Compatibility renderer's canvas batch at every
symbol (measured: Graph page 18 -> 11 draw calls with the symbols removed).
One font holding every HUD glyph keeps all text on one texture.

Recipe (fontTools >= 4.40):
    python3 build_hud_font.py OUT.ttf SYMBOLS OpenSans-SemiBold.ttf SYMFONT [SYMFONT...]
SYMBOLS is the literal set of extra characters. Open Sans glyphs always win;
each code point Open Sans lacks comes from the first SYMFONT that has it
(as built: DejaVu Sans Bold, weight close to SemiBold, then Noto Sans Symbols
Bold for the circled i, then Noto Sans Symbols 2 for the position indicator).
Hinting is dropped (Godot renders unhinted/light); layout tables are kept for
Open Sans only. Licences: Open Sans and Noto Sans Symbols are OFL-1.1, DejaVu
uses the DejaVu Fonts licence (Bitstream Vera derived); all permit
modification and redistribution with the notices in fonts/LICENSE-HudSans.txt.
The family is renamed, as they require.
"""
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.ttLib.scaleUpem import scale_upem

FAMILY = "VisionClaw HUD Sans"


def subset_font(path, unicodes, keep_layout):
    opts = subset.Options()
    opts.hinting = False
    opts.notdef_outline = True
    opts.name_IDs = ["*"]
    opts.name_languages = ["*"]
    opts.glyph_names = True
    opts.layout_features = ["*"] if keep_layout else []
    opts.drop_tables += ["DSIG", "FFTM"]
    if not keep_layout:
        opts.drop_tables += ["GSUB", "GPOS", "GDEF", "kern"]
    font = subset.load_font(path, opts)
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=unicodes)
    sub.subset(font)
    return font


def _decomposed(font, name):
    from fontTools.pens.recordingPen import DecomposingRecordingPen
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    gs = font.getGlyphSet()
    rec = DecomposingRecordingPen(gs)
    gs[name].draw(rec)
    pen = TTGlyphPen(None)
    rec.replay(pen)
    return pen.glyph()


def main(out_path, symbols, base_path, *sym_paths):
    base_full = TTFont(base_path)
    have = set(base_full.getBestCmap())
    want = {ord(c) for c in symbols} - have
    base = subset_font(base_path, sorted(have), True)
    upem = base["head"].unitsPerEm
    parts = []
    left = set(want)
    for path in sym_paths:
        take = sorted(cp for cp in left if cp in TTFont(path).getBestCmap())
        if not take:
            continue
        f = subset_font(path, take, False)
        if f["head"].unitsPerEm != upem:
            scale_upem(f, upem)
        parts.append(f)
        left -= set(take)
    if left:
        sys.exit("no symbol font has: " + " ".join("U+%04X" % cp for cp in sorted(left)))
    # Copy each symbol glyph (outline + advance) into the base font under a new
    # name and map it in every Unicode cmap subtable. No layout tables travel,
    # so no table merging is needed.
    merged = base
    order = list(merged.getGlyphOrder())
    glyf, hmtx = merged["glyf"], merged["hmtx"]
    for f in parts:
        cmap = f.getBestCmap()
        sglyf = f["glyf"]
        for cp, src in sorted(cmap.items()):
            g = sglyf[src]
            if g.isComposite():
                g = _decomposed(f, src)
            name = "hud_u%04X" % cp
            order.append(name)
            glyf.glyphs[name] = g
            hmtx.metrics[name] = f["hmtx"].metrics[src]
            for table in merged["cmap"].tables:
                if table.isUnicode():
                    table.cmap[cp] = name
    merged.setGlyphOrder(order)
    glyf.glyphOrder = order
    for rec in merged["name"].names:
        if rec.nameID in (1, 16):
            rec.string = FAMILY
        elif rec.nameID == 4:
            rec.string = FAMILY + " SemiBold"
        elif rec.nameID == 6:
            rec.string = "VisionClawHUDSans-SemiBold"
    merged.save(out_path)
    print("wrote %s: %d glyphs, %d symbols added: %s" % (out_path, merged["maxp"].numGlyphs, len(want), "".join(chr(c) for c in sorted(want))))


if __name__ == "__main__":
    if len(sys.argv) < 5:
        sys.exit(__doc__)
    main(*sys.argv[1:])
