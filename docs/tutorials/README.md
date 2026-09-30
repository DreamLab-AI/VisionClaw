---
title: Tutorials
description: A guided learning path that takes you from understanding VisionClaw to running it, building your first 3D knowledge graph, and promoting a note into the formal ontology.
category: tutorial
difficulty-level: beginner
---

# Tutorials

> [VisionClaw Docs](../README.md) · Tutorials

These four tutorials form a single learning path. Work through them in order: each one assumes what the previous one taught. You start by understanding what VisionClaw is, install and run the stack, build a graph you can walk around, then enrich that graph by promoting a plain note into the formal ontology. No prior knowledge of RDF, CUDA, or actor systems is required — every step is explained as you reach it. There is no separate database to set up; the graph store is an embedded Oxigraph triple store running in-process inside the Rust backend.

By the end you can point VisionClaw at your own notes, navigate the result in 3D, and govern how concepts become first-class ontology classes.

## Learning path

| # | Tutorial | What you learn | Time |
|---|----------|----------------|------|
| 1 | [What is VisionClaw?](what-is-visionclaw.md) | The mental model — what each part does and where your data flows, no commands typed | ~10 min |
| 2 | [Installation](installation.md) | System requirements, Docker Compose setup, and optional NVIDIA GPU acceleration | ~20 min |
| 3 | [Build Your First Graph](first-graph.md) | Start the stack, load notes from GitHub or JSON, navigate the 3D space, and enable GPU physics | 15–45 min |
| 4 | [Promote a Note to the Ontology](promote-note-to-ontology.md) | Turn a Markdown page into a governed OWL class with SHACL validation and PROV-O provenance | ~30 min |

## Before you begin

- **Docker Engine** with the Compose plugin (or Docker Desktop)
- **8 GB RAM** minimum, 16 GB recommended
- **A modern WebGL browser** (Chrome, Firefox, or Safari, current release)
- **Optional:** an NVIDIA GPU with CUDA for GPU-accelerated physics — VisionClaw runs without it on CPU

Tutorial 2 covers each of these in detail, including how to verify your setup.

## Where to go next

Once you have finished the path:

- Pick a task from the [How-To Guides](../how-to/README.md) — deployment, agent orchestration, REST API usage, XR setup.
- Understand the design in [Explanation](../explanation/system-overview.md) — architecture, the physics engine, the ontology pipeline.
- Look things up in the [Reference](../reference/README.md) — REST and WebSocket APIs, the binary protocol, configuration, and the agents catalogue.

## See also

- [How-To Guides](../how-to/README.md) — task recipes once VisionClaw is running
- [System Overview](../explanation/system-overview.md) — how the pieces fit together
- [Reference Hub](../reference/README.md) — exhaustive API and configuration details
- [Troubleshooting](../how-to/operations/troubleshooting.md) — fixes for common setup problems
- Governing ADRs: [ADR-090 Hexagonal crate modularisation](../archive/adr/ADR-090-hexagonal-crate-modularisation.md), [ADR-112 Ontology augmentation retrieval spine](../archive/adr/ADR-112-ontology-augmentation-retrieval-spine.md)
