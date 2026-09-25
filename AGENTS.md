# VisionClaw — agent instructions

> Canonical, tool-neutral instructions for every coding agent (Claude Code, Codex, others). Claude-only affordances live in `CLAUDE.md`, which imports this file. `agentbox/` is a separate repository with its own `AGENTS.md`.

## Architecture ground truth

The ADR pack for any domain = its living governing document in `docs/` (BASELINE-architecture, IDENTITY-authority-chain, DATA-authority-erasure, PROTOCOL-registry, IDENTIFIER-taxonomy, SECURITY-profiles, GPU-wire-abi, XR-client; their *Invariants* are the compliance surface) + the `docs/adr/` ledger records amending it. Lookup order: governing doc → its `file:line` citations → `docs/adr/` → `docs/archive/` **for rationale only, never authority** (redirects: `docs/MIGRATION-plan.md`). New decisions: one-page record from `docs/adr/TEMPLATE.md` + update the governing doc in the same change + `node scripts/adr-index-gen.js docs/adr`. Routing: `docs/adr/README.md`.

## Build & test

- **Rust backend**: `cargo test` (unit); workspace crates live in `crates/` (`visionclaw-*`, `vault`). GPU and ontology features are on by default.
- **Client**: `cd client && npm install && npm run dev` (Vite); `npm test` runs Vitest; `npm run build` regenerates types first.
- **Type generation**: `cargo run --bin generate_types` updates `client/src/types/` from Rust structs — run it after changing any API or data struct; never hand-edit generated types.
- **Docker**: `./scripts/launch.sh up dev` (source-only) or `rebuild dev` (Dockerfile/deps) — never plain `docker compose`. From inside the agentbox container, launch it in the host shell (see the workspace environment notes), never directly.
- **Env**: `.env` is not committed; start from `env.development.template` or `env.example`.

## Code conventions

- **Rust**: `actix-web` for the API, `neo4rs` for the graph DB, `whelk-rs` for ontology reasoning. `OntologyQueryService` and `OntologyMutationService` are the agent-facing ontology API layer.
- **Ontology agent tools** (discover, read, query, traverse, propose, validate, status): types in `crates/visionclaw-ontology/src/types/ontology_tools.rs`, REST handler in `src/handlers/ontology_agent_handler.rs`; integration tests in `tests/ontology_agent_integration_test.rs`.
- **TypeScript**: `client/src/features/` feature-sliced layout; generated types in `client/src/types/`.

## Memory

Durable agent memory is the shared RuVector store, reached only through the memory MCP tools (never the CLI or raw SQL). Search it (`project-state`, `patterns`) when prior decisions or similar past tasks are likely relevant; store durable lessons afterwards.
