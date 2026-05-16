# ADR-032: Embed solid-pod-rs as Rust library (replace JSS sidecar)

## Status

Proposed

## Date

2026-05-16

## Context

VisionClaw is being refactored. The current Solid integration uses
JavaScriptSolidServer (JSS, Node.js) as a sidecar container
([`solid-sidecar-architecture.md`](../explanation/solid-sidecar-architecture.md))
reached over HTTP via `src/handlers/solid_proxy_handler.rs` against
`JSS_URL=http://visionflow-jss:3030`. Every pod read/write crosses a
process boundary, a TCP socket, and a JSON-LD parse round-trip per call.

Three forces converge on this refactor:

1. **DreamLab-AI's [`solid-pod-rs`](https://github.com/DreamLab-AI/solid-pod-rs)
   is now feature-mapped to JSS at ~98% strict parity** (132 rows tracked
   in [`PARITY-CHECKLIST.md`](https://github.com/DreamLab-AI/solid-pod-rs/blob/main/crates/solid-pod-rs/PARITY-CHECKLIST.md)).
   It is a Rust-native port of JSS, AGPL-3.0, co-maintained with Melvin
   Carvalho's JSS upstream. New JSS features land in JSS first and are
   ported to solid-pod-rs as Cargo features. Library surface is
   framework-agnostic and consumable in-process.

2. **JSS Phase 1 (v0.0.190, May 2026) shipped pod-native Nostr
   identity** (`--provision-keys`, `/private/privkey.jsonld`, NIP-05
   endpoint, JSON-LD data export). These features are being ported into
   `solid-pod-rs` as Cargo features in the current sprint cycle.
   VisionClaw needs them to consolidate three keygen paths
   (`VISIONCLAW_NOSTR_PRIVKEY` env, JSS sidecar bootstrap, broker actor
   signing) into a single pod-resident identity.

3. **Agentbox proved the binary-aggregation pattern** in
   [agentbox ADR-010](https://github.com/DreamLab-AI/agentbox/blob/main/docs/reference/adr/ADR-010-rust-solid-pod-adoption.md).
   That ADR rejected library-linking for AGPL reasons because agentbox's
   first-party Rust code is not AGPL. **VisionClaw's situation is
   different**: AGPL is acceptable here because DreamLab-AI
   co-maintains both `solid-pod-rs` and `JavaScriptSolidServer` (the
   upstream from which solid-pod-rs inherits AGPL). The licence-as-blocker
   argument that constrains agentbox does not constrain VisionClaw.

### Current integration shape

| Component | Current state |
|---|---|
| `src/handlers/solid_proxy_handler.rs` | NIP-98-signed HTTP proxy to JSS sidecar |
| `src/config.js` | Reads 18 `JSS_*` env vars; orchestrates sidecar lifecycle |
| `docker-compose.yml` | `solidproject/community-server:latest` container exposed at `:3030` |
| `src/services/nostr_bridge.rs` | Loads signing key from `VISIONCLAW_NOSTR_PRIVKEY` env var |
| `src/services/nostr_bead_publisher.rs` | Same env-key dependency |
| `src/actors/broker_actor.rs` | Same env-key dependency (via `ServerNostrActor`) |

Failure modes today:
- Sidecar restart loses in-flight WebSocket subscriptions (Solid
  Notifications 0.2 reconnect handled by client, but bead-publisher
  drops events).
- Three keygen surfaces: env var, sidecar `--provision-keys` (not yet
  wired), broker actor signer. No single source of truth.
- Pod data is in a Docker volume opaque to the Rust process. Backups
  require a separate path.
- HTTP+JSON-LD parse on every WAC check (~3-5 ms p50, 15 ms p99 against
  loopback) accumulates against the bead-publishing hot path.

## Decision

**Replace the JSS sidecar with an in-process embedding of `solid-pod-rs`
Rust crates linked directly into the `webxr` workspace.** Pod storage,
WAC enforcement, NIP-98 verification, LDP container semantics, and Solid
Notifications become library calls, not HTTP round-trips.

### Dependency surface

Six crates from the `solid-pod-rs` workspace, all from a single pinned
git rev (initially `v0.4.0-alpha.7+sprint10` once the JSS Phase 1 ports
land):

| Crate | Role in VisionClaw |
|---|---|
| `solid-pod-rs` (core) | LDP resources/containers, WAC, NIP-98 verification, atomic-rename `fs-backend`, Solid Notifications 0.2 |
| `solid-pod-rs-nostr` | `did:nostr` resolver, NIP-05 endpoint, Schnorr verifier |
| `solid-pod-rs-idp` | Pod provisioning, account scaffolding, **key provisioning (Phase 1)** |
| `solid-pod-rs-server` | **Not depended on directly** — reference only for HTTP route templates |
| `solid-pod-rs-activitypub` | Deferred to follow-up ADR (only needed if VisionClaw federates) |
| `solid-pod-rs-git` | Deferred (graph-as-git is a separate workstream) |

Cargo features enabled by default in the embedding:

```toml
[dependencies.solid-pod-rs]
git = "https://github.com/DreamLab-AI/solid-pod-rs"
rev = "<pinned>"
default-features = false
features = [
  "fs-backend",
  "nip98-schnorr",
  "did-nostr",
  "security-primitives",
  "rate-limit",
  "quota",
  "webhook-signing",
  "notifications",
  "provision-keys",   # JSS Phase 1
  "nip05-endpoint",   # JSS Phase 1
  "export-jsonld",    # JSS Phase 1
]
```

### Architectural transition

```
BEFORE:                                    AFTER:
+----------------+   HTTP    +-------+    +----------------------------+
| VisionClaw     |---------->|  JSS  |    | VisionClaw                 |
| (Rust, MIT)    |  :3030    | (Node)|    | (Rust, AGPL-3.0-only)      |
|                |           |       |    |   ├── solid-pod-rs (lib)   |
|                |           +-------+    |   ├── ActixWeb routes      |
|                |               |        |   │   mounted at /solid/*  |
+----------------+               v        |   └── Pod storage: fs      |
                            Docker vol    |       (mounted as Solid    |
                                          |        directory tree)     |
                                          +----------------------------+
```

Pod storage path under `/var/lib/visionclaw/pods/` (POSIX, atomic-rename
guarantees from `fs-backend`). `solid-pod-rs` is invoked via its
framework-agnostic library API; Actix-web handlers in VisionClaw wrap
the library's request/response primitives. No internal HTTP loopback.

### Licence consequence (accepted)

Linking AGPL-3.0 `solid-pod-rs` as a library makes the combined
VisionClaw binary AGPL-3.0-only. **VisionClaw is relicensed from MIT
to AGPL-3.0-only** as part of this refactor:

- `Cargo.toml` `license = "MIT"` → `license = "AGPL-3.0-only"`
- `LICENSE` replaced with AGPL-3.0 text; `LICENSE.MIT` retained for the
  pre-cut history.
- `NOTICE` file added preserving solid-pod-rs / JSS attribution per
  AGPL §5(a-c).
- AGPL §13 (network corresponding source) satisfied by the public
  GitHub repo URL exposed in `/.well-known/solid` and the server-info
  HTTP header `X-Source-URL`.

Acceptable because DreamLab-AI co-maintains both `solid-pod-rs` and
upstream JSS. The AGPL boundary at the library link is consistent with
the rest of the sovereign data stack (agentbox aggregates the same
upstream as a binary; nostr-rust-forum and dreamlab-ai-website
federate via HTTP). No external dependency forces a non-AGPL link.

### Migration sequence

Five PRs, each independently reviewable, totalling ~1500 LoC delta:

**M1 — Relicense + crate add (small).** `Cargo.toml` license switch,
LICENSE swap, NOTICE file, `solid-pod-rs` git dep added behind feature
flag `embedded-pod` (off by default). Builds both ways. No behavioural
change.

**M2 — Replace `solid_proxy_handler.rs`.** New `src/handlers/pod/`
module with sub-handlers per LDP verb, each calling into `solid-pod-rs`
library entry points. The proxy handler retained behind the
`embedded-pod = false` flag as a fallback for one minor release.

**M3 — Key consolidation.** `src/services/nostr_bridge.rs`,
`nostr_bead_publisher.rs`, `actors/broker_actor.rs` switch from
`VISIONCLAW_NOSTR_PRIVKEY` to a `PodResidentSigner` that loads
`pods/<webid>/private/privkey.jsonld` via `solid-pod-rs-idp`. Env var
demoted to test override (`SERVER_NOSTR_PRIVKEY_OVERRIDE`,
test/CI only).

**M4 — Pod NIP-05 wiring.** VisionClaw's WebID profile (`profile/card`)
gains a `nostr:pubkey` triple seeded at pod creation; the embedded
`solid-pod-rs-nostr` serves `/.well-known/nostr.json?name=<local>` from
the same pod tree. Forum auth-worker NIP-05 federation (separate ADR in
`nostr-rust-forum`) consumes this endpoint.

**M5 — Sidecar removal.** `docker-compose.yml` drops the
`solidproject/community-server` service. `JSS_URL` removed from `.env`.
The `embedded-pod` feature flag becomes default-on; the proxy fallback
code is deleted. `src/config.js` (entire JS stub) deleted.

Default flag flip happens at M5; pre-M5 the embedded path is opt-in via
env so dev/staging can validate behavioural parity before cut.

### What stays an HTTP boundary

The forum (`nostr-rust-forum` / `dreamlab-ai-website`) **continues to
federate to VisionClaw's pod over HTTP** — the embed is an
in-process optimisation, not a topology collapse. Federated reads via
`GET /solid/<path>` against VisionClaw's Actix routes resolve through
the in-process `solid-pod-rs` library. External clients see no
difference except lower latency and consistent WAC enforcement.

The agentbox container (`solid-pod-rs` aggregated as a supervisord
binary per agentbox ADR-010) is unchanged. Agentbox and VisionClaw both
host `solid-pod-rs`, but at different boundaries (binary vs library) for
different licence reasons (agentbox AGPL-aggregation, VisionClaw
AGPL-derivative). The shared code path means feature parity is free.

## Consequences

### Positive

- **One keygen surface.** `provision-keys` writes the canonical
  `privkey.jsonld` at pod creation; every signing path reads from
  there. Three env-var-driven paths collapse to one.
- **WAC actually enforces in-process.** Today, JSS WAC checks happen
  inside the sidecar but VisionClaw cannot consult them at the Rust
  layer; access control logic is duplicated. Post-embed, the WAC engine
  is a function call from any handler.
- **Latency drops materially on the bead-publishing hot path.**
  Eliminates the loopback HTTP + JSON-LD parse on every pod write. The
  sidecar p50 of 3-5 ms / p99 of 15 ms becomes sub-millisecond library
  calls.
- **Atomic-rename durability is real.** `fs-backend` guarantees apply
  directly to VisionClaw's pod tree. No more "filesystem says yes,
  sidecar says no" inconsistencies during crashes.
- **Single binary deployable.** `docker-compose` shrinks by one
  service. Operational surface area drops.
- **Future feature work lands once.** When JSS Phase 2 ships (BIP-39
  seed wrapping, key rotation, passkey-wrap), the port lands in
  `solid-pod-rs`; VisionClaw and agentbox absorb via `cargo update`.
- **NIP-05 federation becomes free.** Forum federates to
  VisionClaw's pod endpoint; pod owns its own identity record.

### Negative

- **Cargo build cost.** `solid-pod-rs` workspace adds ~120 transitive
  dependencies; cold build adds ~3-4 minutes to `cargo build --release`
  (warm/incremental is unchanged). Mitigation: pin via `Cargo.lock` and
  use `sccache` in CI.
- **Tighter coupling to `solid-pod-rs` rev cadence.** A breaking change
  upstream forces a VisionClaw fix in the same sprint. Mitigation:
  co-maintenance gives early signal; pin by git rev (not semver range)
  until upstream cuts v1.0.
- **AGPL imposes operator obligations.** Anyone running a VisionClaw
  instance on a public network must offer source. The public GitHub
  repo satisfies §13 today; future closed-source forks are not viable
  without a separate licence.
- **`src/config.js` deletion is a real cut.** Operators with custom
  JSS_* env tuning must migrate to the new `[integrations.solid_pod_rs]`
  TOML section (or the matching env namespace, which solid-pod-rs's
  `config-loader` feature preserves drop-in via the same JSS_* names).
- **The Phase 1 features must land in solid-pod-rs before M4.** If the
  upstream port slips, VisionClaw's pod-resident NIP-05 wiring slips
  with it. Mitigation: the port plan is six sprints (A-F in the
  internal port plan) and provision-keys is the first port.

### Neutral

- **Forum federation unchanged in shape.** Still HTTP, still NIP-98
  signed. Only the server-side implementation changes.
- **WebSocket Solid Notifications surface unchanged.** Library exposes
  the same WebSocket channel; routes mount at the same paths.
- **Neo4j is untouched.** This ADR is about the Solid layer. Graph
  data continues to live in Neo4j; pod data is the LDP-shaped surface
  on top.
- **Docker volume layout unchanged.** Pod data still under
  `/var/lib/visionclaw/pods/` (renamed from `/data/pods/`). One-line
  volume rename in `docker-compose.yml`.

## Alternatives considered

**Keep JSS sidecar; upgrade to v0.0.190.** Rejected: every additional
sprint extends the duplicate-keygen problem and the HTTP loopback cost.
The refactor window is now; deferring loses leverage. Also: JSS is
Node.js — embedding a Node runtime into a Rust binary is not viable, so
the choice is sidecar-forever vs library-once.

**Aggregate solid-pod-rs as a binary (agentbox pattern).** Rejected for
VisionClaw because the licence-blocker argument that drove agentbox to
binary-aggregation does not apply: VisionClaw is fine going AGPL.
Library linking gives in-process latency and a unified WAC surface that
aggregation cannot.

**Fork solid-pod-rs into VisionClaw's tree.** Rejected: forks decay,
co-maintenance breaks, parity tracking against JSS becomes a
two-repository problem. Pin via git rev is sufficient.

**Hand-roll a minimal Solid impl in VisionClaw's Rust.** Rejected:
reinvents LDP semantics, WAC evaluation, atomic-rename storage, NIP-98
verification, Notifications. solid-pod-rs already exists and is
co-maintained.

**Wait for `solid-pod-rs` v0.5.0 stable.** Rejected: v0.4.x is already
running in agentbox production. v0.5.0 is the umbrella version for the
JSS Phase 1 absorption; the embed work starts now and tracks the rev
forward.

## Related decisions

- agentbox ADR-010: solid-pod-rs as first-class pod server (binary
  aggregation pattern). This ADR is the library-link counterpart for
  VisionClaw.
- ADR-027: pod-backed graph views (consumer of LDP semantics)
- ADR-028: SPARQL-patch ontology (consumer of PATCH semantics)
- ADR-029: type-index discovery (consumer of WebID + type-index)
- ADR-030: agent memory pods (consumer of WAC + LDP containers)
- ADR-041: Judgment Broker Workbench (consumer of `PodResidentSigner`
  per M3; broker decisions sign with the pod-resident key, replacing
  the env-var path that exists today)

## Open questions for the refactor team

1. **Pod root for VisionClaw's server identity?** Proposal:
   `/var/lib/visionclaw/pods/<server-webid-npub>/`. Confirm with
   ops before M5.
2. **Migration of existing pod data from `solid-data` volume.**
   solid-pod-rs `fs-backend` reads existing JSS pod trees, but `.meta`
   sidecars may need a one-shot migration script. Out of scope for
   M1-M5; tracked separately.
3. **Should the `solid-pod-rs-activitypub` crate land in a follow-up
   ADR?** ActivityPub federation is out of scope here; flagged as
   follow-up.
4. **AGPL relicense — any contributors who never agreed to AGPL?**
   Audit `git shortlog -s` against contributor records before the cut;
   contact any external contributors for re-licence consent.

## References

- agentbox ADR-010 — solid-pod-rs as first-class pod server
- JSS upstream: https://github.com/JavaScriptSolidServer/JavaScriptSolidServer
- solid-pod-rs upstream: https://github.com/DreamLab-AI/solid-pod-rs
- JSS Phase 1 feature roadmap: JSS issue #437
- solid-pod-rs parity tracking:
  [`PARITY-CHECKLIST.md`](https://github.com/DreamLab-AI/solid-pod-rs/blob/main/crates/solid-pod-rs/PARITY-CHECKLIST.md)
- AGPL-3.0 text:
  [`LICENSE`](https://www.gnu.org/licenses/agpl-3.0.txt)
- `docs/explanation/solid-sidecar-architecture.md` — current sidecar
  shape (will be superseded post-M5)
