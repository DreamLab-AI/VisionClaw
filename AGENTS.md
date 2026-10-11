# VisionClaw — agent instructions

> Canonical, tool-neutral instructions for every coding agent (Claude Code, Codex, others). Claude-only affordances live in `CLAUDE.md`, which imports this file. `agentbox/` is a separate repository with its own `AGENTS.md`.

## Architecture ground truth

The ADR pack for any domain = its living governing document in `docs/` (BASELINE-architecture, IDENTITY-authority-chain, DATA-authority-erasure, PROTOCOL-registry, IDENTIFIER-taxonomy, SECURITY-profiles, GPU-wire-abi, XR-client; their *Invariants* are the compliance surface) + the `docs/adr/` ledger records amending it. Lookup order: governing doc → its `file:line` citations → `docs/adr/` → `docs/archive/` **for rationale only, never authority** (redirects: `docs/MIGRATION-plan.md`). New decisions: one-page record from `docs/adr/TEMPLATE.md` + update the governing doc in the same change + `node scripts/adr-index-gen.js docs/adr`. Routing: `docs/adr/README.md`.

## Build & test

- **Rust backend**: `cargo test` (unit); workspace crates live in `crates/` (`visionclaw-*`, and `vault`, the AGPL governance adapter).
<!-- lands with ontomark step 10 -->
- **Corpus semantics**: parsing, the vocabulary model, reasoning, validation and the build are the ontomark crates (`ontomark-parse`, `ontomark-model`, `ontomark-reason`, `ontomark-compile`) and the `ontomark` CLI. There is no `vault-core` crate. `crates/vault` keeps only `propose`, `apply` and `panel publish`, which post to the forum. GPU and ontology features are on by default.
- **Client**: `cd client && npm install && npm run dev` (Vite); `npm test` runs Vitest; `npm run build` regenerates types first.
- **Type generation**: `cargo run --bin generate_types` updates `client/src/types/` from Rust structs — run it after changing any API or data struct; never hand-edit generated types.
- **Docker**: `./scripts/launch.sh up dev` (source-only) or `rebuild dev` (Dockerfile/deps) — never plain `docker compose`. To deploy code into a running dev container use `./scripts/launch.sh redeploy dev`, not a raw `supervisorctl restart rust-backend`: only redeploy republishes the root build files (`.dev-inputs/`, ADR-2008). From inside the agentbox container, launch it in the host shell (see the workspace environment notes), never directly.
- **Env**: `.env` is not committed; start from `env.development.template` or `env.example`.

## Code conventions

<!-- lands with ontomark step 10 -->
- **Rust**: `actix-web` for the API, Oxigraph with SQLite for the graph store (a rebuildable query projection of the corpus; `neo4rs` is retired, ADR-2004), and `ontomark-reason` (over `whelk-el`) for ontology reasoning. `OntologyQueryService` and `OntologyMutationService` are the agent-facing ontology API layer.
- **Ontology agent tools** (discover, read, query, traverse, propose, validate, status; `propose` posts a forum 31402 through the `vault` governance adapter): types in `crates/visionclaw-ontology/src/types/ontology_tools.rs`, REST handler in `src/handlers/ontology_agent_handler.rs`; integration tests in `tests/ontology_agent_integration_test.rs`.
- **TypeScript**: `client/src/features/` feature-sliced layout; generated types in `client/src/types/`.

## Memory

Durable agent memory is the shared RuVector store, reached only through the memory MCP tools (never the CLI or raw SQL). Search it (`project-state`, `patterns`) when prior decisions or similar past tasks are likely relevant; store durable lessons afterwards.
