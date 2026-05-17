# VisionClaw Diagrams

Hero diagrams for the project README and docs. Two-step workflow:

1. **Diagram-as-code** — structural precursor in Mermaid, rendered via `mmdc`. Source of truth for layout, labels, relationships.
2. **Nano Banana Pro upgrade** — precursor PNG passed as `--reference-image` to Gemini 3 Pro Image (`gemini-3-pro-image-preview`) with the VisionClaw aesthetic prompt, producing a publication-quality 2K image in the project's brand style.

## Layout

| File | Purpose |
|------|---------|
| `src/*.mmd` | Mermaid source (edit here) |
| `src/visionclaw-theme.json` | Mermaid theme config — dark navy, violet/cyan/emerald palette |
| `src/aesthetic-prompt.md` | Canonical Nano Banana prompt prefix for brand consistency |
| `src/batch-generate.sh` | Regenerate all five diagrams in one pass |
| `rendered/*.png` | Mermaid precursor renders (reference inputs for nano banana) |
| `upgraded/*.png` | Nano Banana Pro outputs (also copied to repo root for stable links) |
| `*.png` | Published images referenced from README and docs |

## Diagrams

| # | Diagram | Lives In |
|---|---------|----------|
| 01 | Three-Layer Mesh (hero) | README — "The Three Layers of the Dynamic Mesh" |
| 02 | Insight Ingestion Cycle | README — "The Insight Ingestion Loop" |
| 03 | Four-Plane Voice Architecture | README — Layer 1 voice routing details |
| 04 | MCP Tools Radial | README — Layer 2 MCP tools section |
| 05 | System Architecture (hexagonal) | README — "Architecture" |

## Brand Aesthetic

- **Background**: deep midnight navy `#0A1020` with subtle atmospheric haze
- **Governance layer**: violet `#8B5CF6` glow (top / human judgment)
- **Orchestration layer**: cyan `#00D4FF` glow (middle / agents & reasoning)
- **Discovery layer**: emerald `#10B981` glow (bottom / knowledge & ingestion)
- **Trust hubs**: amber `#F59E0B` glow (sparingly, for critical central nodes)
- **Typography**: clean sans-serif, off-white `#E8F4FC`, all labels verbatim from source
- **Style**: cinematic sci-fi UI concept art meets engineering blueprint — no hand-drawn wobble, no cartoon, no watercolour

## Regenerating

Prerequisites: `mmdc` (Mermaid CLI), `bun`, Chrome/Chromium for mmdc headless, `GOOGLE_API_KEY` set.

```bash
# 1. Edit the Mermaid source
vim src/01-three-layer-mesh.mmd

# 2. Render the precursor
cd docs/diagrams
mmdc -i src/01-three-layer-mesh.mmd \
     -o rendered/01-three-layer-mesh.png \
     -t dark -C src/visionclaw-theme.json \
     -w 2400 -H 1600 -b '#0A1020' --scale 2

# 3. Upgrade via Nano Banana Pro (batch all five)
./src/batch-generate.sh

# 4. Promote the chosen render
cp upgraded/01-three-layer-mesh.png ./01-three-layer-mesh.png
```

The `generate-image` tool lives at `~/.claude/skills/art/tools/generate-image.ts` and is invoked via `bun run` with `--reference-image` for image-to-image style transfer, `--model nano-banana-pro` (`gemini-3-pro-image-preview`), `--size 2K`, `--aspect-ratio 16:9`.

## When to Add a New Diagram

Add a new diagram when a README section has high conceptual density but no visual aid — e.g., a new architectural pattern, a new governance flow, a new subsystem with ≥4 components. Do **not** add a diagram for every list or table; they should earn their space by communicating structure that prose cannot.
