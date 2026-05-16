# T5 — Settings Schema Authority (ADR-05 vs ADR-11)

Status      : Resolution proposed
Date        : 2026-05-16
Resolves    : ADR-05 D5 and ADR-11 D5 both specify a SQLite settings schema.
              The shapes are incompatible (document-per-user vs key-per-user).
              On first edit they drift; on first implementation one wins
              implicitly. Need an explicit one-way deferral.
Outcome     : ADR-11 is the sole authority for the SQLite schema and adapter.
              ADR-05 owns the AppSettings domain shape, defaults, validation,
              and UI definition. The contract between them is the unchanged
              `SettingsRepository` trait at `src/ports/settings_repository.rs`.

## Current state of the schema specifications

**ADR-05 D5** (lines 111–137 of `05-settings/ADR-05.md`) specifies a
document-oriented schema: `user_settings(pubkey PK, settings JSON, schema_ver,
updated_at)` plus an index on `updated_at`. One row per user, whole-document
blob. Describes a 3-method actor surface (`load`, `save`, `save_partial`).

**ADR-11 D5** (lines 130–172 of `11-persistence-migration/ADR-11.md`) specifies
a key-value schema: `settings(key, owner_pubkey, value JSON, description,
updated_at, PRIMARY KEY (key, owner_pubkey)) WITHOUT ROWID` plus
`physics_profiles` and `schema_migrations` tables, WAL pragmas, and pubkey
threading via task-local storage. Explicitly preserves the 17-method
`SettingsRepository` trait at `src/ports/settings_repository.rs`.

The shapes are not "drift candidates" of the same design — they are different
access patterns supporting different trait surfaces. The baseline trait
(verified at `src/ports/settings_repository.rs`, 17 `async fn`) is per-key:
`get_setting(key)`, `set_setting(key, value)`, `list_settings(prefix)`, etc.
ADR-05 D5's schema would require shrinking that trait to 3 methods —
contradicting ADR-11 PRD A2 "Port parity. No upstream caller changes by a
single line as a consequence of the persistence swap."

ADR-11 D5 is significantly more complete: it covers physics profiles, schema
migrations, per-user-vs-global resolution order (DDD-11 §"Settings per-user
resolution"), WAL pragmas, `WITHOUT ROWID` tuning, backup procedure (D10), and
pubkey task-local plumbing. ADR-05 D5 covers only the happy-path layout of one
user's blob.

## Trait surface

The current port declares 17 async methods. ADR-05 owns input/output types;
ADR-11 owns row layout and SQL.

| Method                                  | Domain owner (ADR-05) | Storage owner (ADR-11) |
|-----------------------------------------|-----------------------|------------------------|
| `get_setting(key)`                      | key path, return type | row lookup             |
| `set_setting(key, value, desc)`         | validation, value enum| upsert                 |
| `delete_setting(key)`                   | key path              | DELETE                 |
| `has_setting(key)`                      | -                     | EXISTS query           |
| `get_settings_batch(keys)`              | -                     | IN-clause query        |
| `set_settings_batch(updates)`           | validation            | txn-wrapped upsert     |
| `list_settings(prefix)`                 | key shape             | range scan             |
| `load_all_settings()`                   | `AppFullSettings` type| document assembly      |
| `save_all_settings(settings)`           | validation            | txn-wrapped writes     |
| `get_physics_settings(profile)`         | `PhysicsSettings` type| `physics_profiles` row |
| `save_physics_settings(profile, s)`     | validation            | upsert                 |
| `list_physics_profiles()`               | -                     | SELECT DISTINCT        |
| `delete_physics_profile(profile)`       | -                     | DELETE                 |
| `export_settings()`                     | JSON shape            | full dump              |
| `import_settings(json)`                 | validation            | txn-wrapped restore    |
| `clear_cache()`                         | (no-op)               | no-op (trust SQLite)   |
| `health_check()`                        | criteria              | probe                  |

Six capabilities: single-key CRUD, batch CRUD, listing, whole-doc serde,
physics-profile CRUD, operational. All split cleanly on the domain/storage axis.

## Recommended division of responsibility

Hexagonal architecture applies cleanly. Ports in domain, adapters in
infrastructure, schemas with adapters.

- **ADR-05 owns**: `AppFullSettings`, `PhysicsSettings`, `SettingValue` variants,
  schema dotted-path naming, `validation.rs` rules, UI definition, defaults,
  the `settings.ts` → generated-Rust generator (D1), and the per-document
  `schema_version: u32` field embedded in `AppFullSettings`.
- **ADR-11 owns**: SQLite tables, indices, primary keys, `WITHOUT ROWID`,
  JSON-in-TEXT, the adapter at `src/adapters/sqlite_settings_repository.rs`,
  the `schema_migrations` table and runner, per-user-vs-global resolution,
  pubkey task-local plumbing, WAL pragmas, `VACUUM INTO` backup.
- **Contract**: the frozen 17-method `SettingsRepository` trait. Neither ADR
  edits it unilaterally; trait changes require coordinated amendment.

This restores PRD-11 A2's port-stability discipline that ADR-05 D5
inadvertently broke by specifying a schema implying a smaller trait surface.

## Wording change to ADR-05 D5

Replace `05-settings/ADR-05.md` lines 111–137 entirely with:

```markdown
### D5. Persistence: SQLite, schema per ADR-11

Settings persist in SQLite. **The schema, primary-key layout, per-user
resolution order, pragmas, and backup mechanics are owned by ADR-11 §D5
and DDD-11 §"Settings per-user resolution"** — this ADR does not duplicate
them. Section 5 owns only the contents of what gets stored:

- The `AppFullSettings` Rust type (generated from `settings.ts` per D1) is
  the domain-level shape. ADR-11's adapter persists it via the 17-method
  `SettingsRepository` trait at `src/ports/settings_repository.rs`.
- `AppFullSettings` embeds its own `schema_version: u32` field, generated
  from the `settings.ts` AST. This is distinct from ADR-11's
  `schema_migrations` table: the table tracks which SQL migrations the
  database has applied; the embedded version tracks the shape of an
  individual user's stored document. Section 5 owns the document-shape
  migration ladder (`fn(&mut Value, from: u32) -> u32`); ADR-11's adapter
  invokes it on read when the embedded version is lower than current.
- Anonymous sessions read defaults and receive 401 on save (FR-7).
  Authenticated sessions read and write their own settings; the pubkey
  threads through ADR-11's task-local context, not as a method parameter.
- The repository trait is frozen by ADR-11 PRD A2. Changes require
  coordinated edits to both ADRs.
```

Also update ADR-05 R3: delete the "SQLite migration story" text; replace with:

```markdown
- **R3. Per-document `schema_version` drift.** `AppFullSettings::schema_version`
  (Section 5) and `schema_migrations.id` (Section 11) are independent counters
  answering different questions. Mitigation: distinct names in code, plus a
  unit test asserting they cannot be conflated.
```

## Wording check on ADR-11

ADR-11 D5 is already authoritative for the schema and needs two small
clarifications, not an expansion.

**1.** Insert as the first sentence under `### D5. SQLite settings schema`:

```markdown
This ADR is the sole authority for the SQLite settings schema. ADR-05
(Settings & Control Panel) defers to this section for all storage and
operational concerns; Section 5 owns only the domain types
(`AppFullSettings`, `PhysicsSettings`, `SettingValue` variants) persisted
via the unchanged `SettingsRepository` trait surface.
```

**2.** After the `WITHOUT ROWID` bullet (current line ~161), add:

```markdown
- The `AppFullSettings` document persisted via `save_all_settings` embeds its
  own `schema_version: u32` field (generated by Section 5's D1 generator).
  This is distinct from the `schema_migrations` table: the table records
  which SQL migrations the database has applied; the document version
  records which generator emitted the user's blob. Section 5 owns the
  document migration ladder; Section 11's adapter invokes it on read when
  the stored `schema_version` is lower than current.
```

**3.** ADR-11 line 132 claims "A single table covers all 44 SettingsRepository
methods." The trait has 17 methods today; the "44" appears to count the Neo4j
impl's internal helpers (user-management, cache, schema-init). Correct to
"17 methods" for accuracy.

## BDD scenarios

```gherkin
Feature: Settings schema authority cannot drift between ADR-05 and ADR-11

  Background:
    Given the SettingsRepository trait at src/ports/settings_repository.rs
    And the SQLite adapter at src/adapters/sqlite_settings_repository.rs
    And the generated Rust types at src/config/generated_settings.rs

  Scenario: Adding a domain field does not require an ADR-11 schema change
    Given a developer adds `physics.swirl_strength` to settings.ts
    When the schema generator runs (D1)
    Then `AppFullSettings::schema_version` increments in generated Rust
    And the SQLite `settings` table schema is unchanged
    And the new field is stored as JSON inside the existing `value` column
    And no new row is added to `schema_migrations`
    And a unit test asserts that loading a pre-existing v(n-1) blob applies
        Section 5's v(n-1)->v(n) migration function on read

  Scenario: Storage-shape changes require ADR-11 amendment AND trait stability
    Given a developer proposes splitting the `settings` table into per-tab tables
    When the build runs the ADR consistency check
    Then the check fails because ADR-11 D5 specifies a single `settings` table
    And a paired ADR-11 D5 amendment plus a passing trait-surface SHA test
        are required before the change can merge
    And ADR-05 requires no edit if no domain type's shape changes

  Scenario: Cross-cutting type change updates both layers coherently
    Given `PhysicsSettings` gains a `solver_kind: SolverKind` field in Section 5
    When the generator emits the new Rust type
    And the SQLite adapter is rebuilt
    Then a CI test loads a pre-existing profile (no `solver_kind`)
    And Section 5's document migration ladder fills in `SolverKind::default()`
    And the blob round-trips through `save_physics_settings` and
        `get_physics_settings` without data loss
    And the `physics_profiles` table schema is unchanged (the field lives
        inside `settings_json`), confirming ADR-11 needed no SQL change
    And the trait-surface SHA test passes (signature byte-identical)
```

The three scenarios cover the three places drift can occur. Domain-only is the
common case and is cheap (no SQL touched). Storage-only is rare and gated by
the trait-surface SHA test. Cross-cutting is rarest and requires coordinated
amendment of both ADRs. The trait-surface SHA test is the mechanical guard
that makes the discipline hold past the first contributor who reads only
one ADR.
