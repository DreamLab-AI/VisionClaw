# Diagrams as code — moved to the VisionFlow estate canon

The citation-verified Mermaid corpus that lived here (VC-, AB- and ES- topic files, the coverage index and
the generator) moved on 2026-09-07 to the estate repository, where it now covers all nine repositories:

- tree: <https://github.com/DreamLab-AI/VisionFlow/tree/main/docs/diagrams> (local checkout: `../VisionFlow/docs/diagrams`)
- VisionClaw topics: `visionclaw/NN-*.md` (`VC-NN`); agentbox topics: `agentbox/NN-*.md` (`AB-NN`); estate: `estate/`
- tooling: `node scripts/diagram-index-gen.cjs docs/diagrams --check --cite-check` run from the VisionFlow root;
  `sources:` there reach this repo as `../project/…`

History up to the move is in this repository's git log (last in-tree commit 4d1a698e7). The pre-2026-09-05
narrative diagrams remain under `docs/archive/diagrams/2026-09-pre-overhaul/`.
