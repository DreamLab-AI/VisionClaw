# Dormant Innovation Services

These files contain scaffolded-but-never-wired innovation features.
They were moved here from `features/graph/services/` and `features/graph/innovations/`
on 2026-05-09 after a full audit confirmed zero UI consumers.

## Status: DORMANT — not compiled into production builds

To re-activate any service:
1. Move the file back to `features/graph/services/`
2. Wire it to the consuming UI component
3. Remove the `@deprecated DORMANT` tag
4. Add tests

## Inventory

| File | Lines | Feature |
|------|-------|---------|
| aiInsights.ts | 1,109 | Layout optimization, cluster detection, pattern recognition |
| advancedInteractionModes.ts | 862 | Time-travel, collaborative editing, VR spatial UI |
| graphComparison.ts | 677 | Graph diff, node matching, similarity analysis |
| graphAnimations.ts | 661 | Transition animations, camera flight paths |
| gnnPhysics.ts | 335 | GNN-enhanced physics weights |
| graphSynchronization.ts | 276 | Camera sync, multi-graph reconciliation |
| gnnPhysicsConnector.ts | 52 | Settings toggle → gnnPhysics bridge |
| innovationManager.ts | 411 | Singleton orchestrator (was innovations/index.ts) |

Total: ~4,383 lines preserved for future use.

## Audit evidence (2026-05-09)

- `aiInsights`: imported only by innovationManager. GraphOptimisationTab has a local state variable but does NOT import the module.
- `advancedInteractionModes`: imported only by innovationManager. Zero external consumers.
- `graphComparison`: imported only by innovationManager. Zero external consumers.
- `graphSynchronization`: imported only by innovationManager. Zero external consumers.
- `graphAnimations`: InnovationManager calls start()/stop()/dispose() but no UI component triggers animations.
- `gnnPhysics`: imported only by gnnPhysicsConnector. checkAndApplyGNNPhysics() has zero callers.
- `gnnPhysicsConnector`: sole export never called from any file.
- `innovationManager`: imported only by AppInitializer.tsx. Adds names to Set, logs messages, returns status that nothing reads.
