---
title: Contributor Studio — Canonical Diagrams Index
description: Index of the 9 sprint-sourced mermaid diagrams plus the Nano-Banana-upcycled hero illustration. Points to rendered PNGs under `docs/diagrams/` and to the in-source mermaid blocks. All artefacts use the VisionClaw dark theme (primary #2471A3, background #0B2545, text #FFFFFF).
category: design
tags: [contributor-studio, diagrams, mermaid, 2026-04-20]
updated-date: 2026-04-21
---

# Contributor Studio — Canonical Diagrams Index

## Purpose

All diagrams supporting the 2026-04-20 Contributor AI Support Stratum sprint,
with direct links to both the rendered PNGs (embedded in docs and the
README) and the source mermaid blocks (inside the sprint artifacts). The
rendered PNGs are the canonical visual assets; the mermaid blocks are
the canonical source-of-truth that the renders are derived from.

## Theme

All mermaid renders use `~/.claude/skills/mermaid-diagrams/resources/templates/theme-dark.json`:

```json
{
  "theme": "dark",
  "themeVariables": {
    "primaryColor": "#2471A3",
    "primaryTextColor": "#FFFFFF",
    "primaryBorderColor": "#5DADE2",
    "lineColor": "#ABB2B9",
    "secondaryColor": "#1B4F72",
    "tertiaryColor": "#0B2545",
    "background": "#0B2545",
    "mainBkg": "#13315C",
    "nodeBorder": "#5DADE2",
    "clusterBkg": "#1B4F72",
    "clusterBorder": "#2471A3",
    "titleColor": "#FFFFFF",
    "edgeLabelBackground": "#13315C",
    "textColor": "#FFFFFF",
    "fontSize": "14px"
  }
}
```

Render command (mmdc 11.12.0):

```bash
mmdc -i source.mmd \
  -o docs/diagrams/<name>.png \
  -t dark -b "#0B2545" \
  --width 1800 --height 1200 \
  -c ~/.claude/skills/mermaid-diagrams/resources/templates/theme-dark.json \
  -p /tmp/puppeteer.json
```

## Index

| # | Diagram | Rendered | Mermaid source | Type |
|---|---------|----------|----------------|------|
| 12 | Stratum layering (substrate / stratum / mesh) — **hero, upcycled via Nano Banana 2** | [`docs/diagrams/12-contributor-stratum-layering.png`](../../diagrams/12-contributor-stratum-layering.png) | `docs/explanation/contributor-support-stratum.md` (mermaid block 1) | Block / TB |
| 13 | Share-state transition matrix (Private → Team → Mesh) | [`docs/diagrams/13-adr057-share-state-transitions.png`](../../diagrams/13-adr057-share-state-transitions.png) | `docs/adr/ADR-057-contributor-enablement-platform.md` (mermaid block 1) | Block / TB |
| 14 | Skill lifecycle state machine (Draft → Retired) | [`docs/diagrams/14-adr057-skill-lifecycle.png`](../../diagrams/14-adr057-skill-lifecycle.png) | `docs/adr/ADR-057-contributor-enablement-platform.md` (mermaid block 2) | stateDiagram-v2 |
| 15 | BC18 / BC19 context map (19 bounded contexts) | [`docs/diagrams/15-ddd-context-map.png`](../../diagrams/15-ddd-context-map.png) | `docs/explanation/ddd-contributor-enablement-context.md` (mermaid block 1) | Block / TB |
| 16 | ACL flow across contexts (6 anti-corruption layers) | [`docs/diagrams/16-ddd-acl-flow.png`](../../diagrams/16-ddd-acl-flow.png) | `docs/explanation/ddd-contributor-enablement-context.md` (mermaid block 2) | sequenceDiagram |
| 17 | Mesh-promotion sequence (ShareIntent → BrokerCase) | [`docs/diagrams/17-ddd-mesh-promotion-sequence.png`](../../diagrams/17-ddd-mesh-promotion-sequence.png) | `docs/explanation/ddd-contributor-enablement-context.md` (mermaid block 3) | sequenceDiagram |
| 18 | Skill retirement sequence (RetirementAdvisor → archive) | [`docs/diagrams/18-skill-retirement-sequence.png`](../../diagrams/18-skill-retirement-sequence.png) | `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md` (mermaid block 1) | sequenceDiagram |
| 19 | Skill Dojo topology (DojoDiscoveryActor + pod index) | [`docs/diagrams/19-skill-dojo-topology.png`](../../diagrams/19-skill-dojo-topology.png) | `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md` (mermaid block 2) | stateDiagram-v2 |
| 20 | Skill evaluation lifecycle (`SkillEvaluationActor`) | [`docs/diagrams/20-skill-eval-lifecycle.png`](../../diagrams/20-skill-eval-lifecycle.png) | `docs/design/2026-04-20-contributor-studio/02-skill-dojo-and-evals.md` (mermaid block 3) | sequenceDiagram |

## Hero-diagram upcycle

Diagram 12 is the only one rendered twice. Its pipeline:

1. **mmdc → baseline PNG** (65 KB, 1800×1200) — the deterministic mermaid
   layout that the rest of the diagrams also use.
2. **Nano Banana 2 (Gemini 3.1 Flash Image) → publication-grade illustration**
   (4.5 MB, 2528×1696) — passes the baseline PNG as `--reference-image` and
   restyles with the cosmic/crystalline/bioluminescent aesthetic while
   preserving every node label and arrow relationship. Prompt strategy
   documented in the commit message at `6ae44edb0`.
3. **Converted JPEG → PNG** for file-extension consistency; pngquant was
   unavailable so size stayed at ~4.5 MB.

The other 8 diagrams stay on the mmdc render because they encode state
machines and sequence flows where mermaid's deterministic layout is
preferable to stylistic upcycling. Revisit if Nano Banana Pro ever ships
a mode that preserves symbolic-layout fidelity end to end.

## Rendering a diagram locally

```bash
# 1. Extract the mermaid block from its source file to a .mmd:
awk '/^```mermaid$/{flag=1; next} /^```$/{flag=0} flag' \
  docs/adr/ADR-057-contributor-enablement-platform.md \
  > /tmp/adr057.mmd

# 2. Render:
mmdc -i /tmp/adr057.mmd \
  -o docs/diagrams/13-adr057-share-state-transitions.png \
  -t dark -b "#0B2545" --width 1800 --height 1200 \
  -c ~/.claude/skills/mermaid-diagrams/resources/templates/theme-dark.json \
  -p /tmp/puppeteer.json

# 3. (Hero only) Upcycle via Nano Banana:
~/.bun/bin/bun run ~/.claude/skills/art/tools/generate-image.ts \
  --model nano-banana-2 \
  --reference-image docs/diagrams/12-contributor-stratum-layering.png \
  --prompt "…" \
  --size 2K --aspect-ratio 3:2 --thinking high \
  --output docs/diagrams/12-contributor-stratum-layering.png
```

## References

- [ADR-057 Contributor Enablement Platform](../../adr/ADR-057-contributor-enablement-platform.md)
- [PRD-003 Contributor AI Support Stratum](../../PRD-003-contributor-ai-support-stratum.md)
- [BC18 / BC19 DDD](../../explanation/ddd-contributor-enablement-context.md)
- [Sprint master](00-master.md)
- [Integration validation report](98-integration-report.md)
- [Sprint close retrospective](99-sprint-close.md)
