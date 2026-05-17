# Control Surface Audit

Branch: `feature/unified-control-surface`. Compiled 2026-04-28.

This audit maps every control / display surface in the project, every settings entity behind those surfaces, and every disconnect between the two. It then proposes an aspirational entity-level UI inventory.

## Files

| File | Purpose |
|---|---|
| [`raw/01-auth-surface.md`](raw/01-auth-surface.md) | Sign-in / Onboarding / NIP-98 + passkey / dev login / access-control middleware |
| [`raw/02-control-center.md`](raw/02-control-center.md) | In-app Control Center panel — interactive mode, AI text-entry, advanced toggle, all 180+ leaf controls |
| [`raw/03-enterprise-management.md`](raw/03-enterprise-management.md) | Enterprise Standalone (Broker / Workflows / KPI / Connectors / Policy + WASM drawer_fx), Contributor Studio, Health/Monitoring |
| [`raw/04-server-settings.md`](raw/04-server-settings.md) | `AppFullSettings` schema, every settings HTTP/WS route, validation rules, feature flags, dev_config, back-end-only tunables |
| [`raw/05-client-plumbing.md`](raw/05-client-plumbing.md) | Settings store, settingsApi, autoSaveManager, retry manager, defaults pipeline, WS subscriptions |
| [`raw/06-ancillary-config.md`](raw/06-ancillary-config.md) | Env vars, agentbox manifest knobs, ADR-defined settings, compose orchestration |
| [`comprehensive-settings-table.md`](comprehensive-settings-table.md) | Single inventory of every settings entity across all surfaces, with status flags (LIVE / CLIENT-ONLY / SERVER-ONLY / STUB / DEAD / DUPLICATE / DEPRECATED) |
| [`aspirational-inventory.md`](aspirational-inventory.md) | Same entities, rationalised. Decision codes (KEEP / PROMOTE / DEMOTE / EXPOSE / HIDE / CUT / MERGE / WIRE / VALIDATE / AUTH-GATE) with inline justification |

## How it was produced

Six research agents ran in parallel, each scoped to a non-overlapping concern. Outputs landed in `raw/`. The two synthesis documents draw entirely from those raw files; they do not introduce new findings, only consolidate and rationalise.

## What this audit deliberately did not do

- Did not change any code outside `docs/control-surface-audit/`.
- Did not make grouping, layout, visual-design, or interaction-pattern decisions.
- Did not specify naming above the entity-key level (no copy/labels).
- Did not ignore broken connections — captured them all in the disconnects sections.

## Headline numbers

- **~205** leaf controls in the Control Center today, across **10 tabs**.
- **14** root server settings fields, **40+** nested structs.
- **120+** env vars, **15** agentbox knobs, **15** compose knobs.
- **70+** server-side tunables with no UI today.
- **30** distinct cross-cutting disconnects identified.
- After rationalisation: **~140-150** entities at user tiers 1-2, **~25** at PU tier 3, the rest moved to operator tier 4 or cut.

## Next steps (out of scope for this audit)

1. Review the aspirational inventory's **CUT** and **MERGE** decisions — these are the cheapest wins in tech debt reduction.
2. Decide which **WIRE** entities are real product intents (XR, studio backend, policy persistence, NL command, settings profiles) vs cuts.
3. Once entity set is agreed, start the design-language conversation: grouping, disclosure, naming, theming.
4. Schema work: pick canonical paths for the **MERGE** decisions; deprecate the duplicates with serde aliases for one release.
