# ADR-080 — Forum Kit Deployment Topology Patterns

| Field | Value |
|-------|-------|
| Status | Proposed (2026-05-07) |
| Drives | PRD-011 G1, G3, G4, G8; PRD-010 G3, G7 |
| Companion ADRs | ADR-073, ADR-074, ADR-075, ADR-076, ADR-077, ADR-078, ADR-079 |
| Companion PRDs | PRD-010, PRD-011 |
| Companion DDD | `docs/ddd-mesh-federation-context.md` |
| Affected repos | `nostr-rust-forum` (canonical kit; product `nostr-bbs-rs`), `dreamlab-ai-website` (downstream exemplar), `agentbox` (mesh peer + skill provider), `solid-pod-rs` (foundation library), `VisionClaw` (mesh substrate) |

## Context

PRD-011 extracts the VisionFlow forum kit (`nostr-bbs-rs`, hosted at `DreamLab-AI/nostr-rust-forum`) from the `dreamlab-ai-website` monorepo into a standalone Rust workspace of crates and Cloudflare Workers. Once extracted, the kit is consumed by N operators in M operating environments — DreamLab is one consumer (G3), but the kit must serve coalition deployments, single-team forums, managed-hosting providers, regulated air-gapped operators, and CI evaluators.

ADR-073 specified **mesh topology** at the wire-protocol level: standalone vs federated vs client mode, fan-out semantics, NIP-42 AUTH, federation key custody. That ADR answered "how do relays talk to each other when they decide to talk." It did **not** answer "in what configurations should an operator deploy the kit, and which configuration fits which operator."

This ADR specifies those configurations as **canonical topology patterns**. Each pattern is named, scoped, and tied to a TOML profile shape; operators select a pattern at deployment time, and the kit's TOML schema (PRD-011 §5.2) carries enough config to realise it. Patterns are not rigid — they are composable defaults that operators tune — but naming them lets the kit's documentation, the AI configurator (ADR-079), and the federation smoke test fixture (Q5 §T7 `docs/integration-research/qe-fleet/Q5-test-fixture-design.md`:753) all converge on a shared vocabulary.

### Forces

- **F-substrate-pluggability**: the kit must be deployable without VisionClaw, without agentbox, and without the rest of the DreamLab mesh — a single-team operator should not have to provision an `agentbox` or run a `MeshBridge` to use it.
- **F-federation-optionality**: federation is an opt-in (ADR-073 D6); the default must be standalone or the kit becomes hostile to small operators.
- **F-key-custody-spectrum**: operators have wildly different key-custody risk tolerances. Filesystem custody (the agentbox sovereign-bootstrap default per `docs/integration-research/03-agentbox-surfaces.md`:11) is fine for a single-team forum; a regulated operator wants HSM custody; a CI runner wants ephemeral KMS-issued keys.
- **F-availability-tier**: operators range from "best-effort, single-region" to "production-grade with multi-region failover and DO geo-pinning per ADR-073 D11".
- **F-air-gapped-operability**: regulated environments (defence contractors, classified-network operators) need the kit to run with zero external dependencies. ADR-079's `agentbox-nostr` skill provider is unavailable in such environments; the kit's topology must accommodate this.
- **F-migration-continuity**: the existing `dreamlab-ai-website/community-forum-rs/` deployment must migrate to a kit-based deployment without downtime (PRD-011 R5, Phase X5).
- **F-multi-tenant-economy**: managed-forum hosting providers want to run N operator forums on one infrastructure footprint; the kit must enable this without compromising tenant isolation.

### Non-forces

- **Public-Nostr ecosystem federation** is out of scope per ADR-073 (private mesh only).
- **Custom wire protocols** are out of scope; all topology patterns reuse NIP-01/42/65 + IS-Envelope (ADR-075).
- **Kit-internal branding/zoning** is explicitly *not* a topology concern — branding lives in the consumer package per D7. The kit ships zone-shape primitives, not a "DreamLab" theme.

## Decision

### D1 — Standalone deployment (default)

```toml
[mesh]
mode = "standalone"
```

Single-forum deployment. Kit runs its own relay (CF Worker DO at the operator's `wrangler.toml`-configured route) and its own pod-worker / forum-client / kv / d1 surfaces. No `peer_relays`, no fan-out worker, no inbound federation. The relay accepts only locally-AUTHed sessions (clients holding keys admitted via the kit's invite/trust-progression flow per PRD-011 G3).

**Use cases**:
- Single-team forum (a co-op, a small open-source project, an enterprise team).
- Tight-perimeter community with no intent to federate.
- Evaluation / development deployment by an operator considering the kit.
- CI smoke harness for the kit itself.

**Rationale**: per ADR-073 D6, standalone is the explicit default. New operators should not need to understand federation, peer keys, mesh probes, or DID-resolution semantics to run a forum. The kit's `forum-setup` skill (ADR-079) defaults to this mode unless the operator declares `mesh = "federated"` in the wizard.

**Footprint**: 5 CF Workers (relay, pod-worker, forum-web, kv, d1) per PRD-011 §5.1; no agentbox, no VisionClaw, no peer relays. Deployable on Cloudflare's free tier for low-traffic deployments.

### D2 — Federated mesh deployment

```toml
[mesh]
mode               = "federated"
peer_relays        = ["wss://agentbox.example/", "wss://visionclaw.example:7777/"]
federated_kinds    = [14, 1059, 30033, 30910, 30911, 30912, 30913, 30914, 30915, 30916]
allowed_remote_dids = ["did:nostr:<agentbox-fed-key-hex>", "did:nostr:<visionclaw-fed-key-hex>"]
delegation_required = true
```

Kit deploys with its own relay AND federates with peer relays per ADR-073 D2 fan-out semantics. Inbound federation is gated by NIP-42 AUTH using the peer's federation key (ADR-073 D4); outbound fan-out is gated by `federated_kinds` + `federated_pubkeys` and protected by per-peer LRU + tag-injection loop avoidance (ADR-073 D9).

**Use cases**:
- DreamLab production: forum + agentbox + VisionClaw form an integrated mesh; cross-substrate DMs (kind-1059), bead-stamp fan-out (kind-30200, see Q5 §T7-c at `docs/integration-research/qe-fleet/Q5-test-fixture-design.md`:830), and moderation events propagate.
- Coalition of operators each running their own kit, federating selectively (peer pubkey allowlist + kind allowlist).
- Federated working group (e.g. multiple research teams sharing a discussion zone via cross-relay fan-out of kind-1).

**Rationale**: this is the canonical "PRD-010 mesh" pattern. The kit's relay-worker module (`crates/relay-worker`) implements both inbound and outbound federation per ADR-073 D10; an operator who flips `mode = "federated"` and enumerates peers gets the full mesh semantics, including the `/health/mesh` probe surface (ADR-073 D11).

**Footprint**: standalone footprint + per-peer persistent WSS sessions + `mesh_peer_*` Prometheus counters + a federation key (D6 below). With 2 peers this is ~6 sockets steady-state. CF Worker DO outbound charges apply per fan-out.

**Compatibility note**: federation key MUST appear in each peer's `mesh.allowed_remote_dids`. Bilateral admission is mandatory; there is no "one-sided" federation.

### D3 — Client-mode deployment

```toml
[mesh]
mode        = "client"
peer_relays = ["wss://agentbox.example/"]
subscribed_kinds = [1, 14, 1059, 30033]
```

Kit deploys **without** a relay process; subscribes to a peer's relay as a client. The kit's pod-worker, forum-web, kv, d1 still deploy, but the relay-worker CF Worker is not provisioned. All ingress (subscriptions) and egress (publishes) flow through the peer's relay.

**Use cases**:
- Low-resource operator who wants forum participation but does not want to run a NIP-01 endpoint.
- Hobbyist operator running on Cloudflare's free tier with constrained DO usage.
- Cohort-of-one deployment where the operator's identity is the only forum participant and a peer relay is sufficient.
- Disaster-recovery fallback: an operator's primary federated kit is down; client-mode lets users continue posting through a peer relay until the primary recovers.

**Rationale**: ADR-073 D6's `client` mode is fully defined at the protocol level but underspecified at the deployment level. This pattern names it: client-mode deployments still get the kit's UX surfaces (forum-web, pod-worker, trust progression) but rely on a peer for relay infrastructure. The operator's federation key (if any) is used solely to AUTH client sessions; no inbound federation exists.

**Footprint**: 4 CF Workers (no relay-worker) + persistent client-WS to each peer in `peer_relays`. The kit's `relay.rs:23` is configured to connect to the first peer relay rather than `wss://localhost`.

**Limitation**: the operator MUST trust the peer relay operationally (uptime, admission policy, censorship resistance). Mitigations: subscribe to multiple peer relays for read redundancy (NIP-65-style client-side outbox routing per ADR-073 D5; out-of-scope for P1 but enabled by client-mode by default).

### D4 — Multi-tenant deployment (kit-internal)

```toml
[deployment]
mode = "multi-tenant"

[[tenant]]
slug      = "alpha-forum"
hostname  = "alpha.forum-host.example"
admin_pubkey = "<hex>"
relay_subdomain = "alpha-relay.forum-host.example"

[[tenant]]
slug      = "beta-forum"
hostname  = "beta.forum-host.example"
admin_pubkey = "<hex>"
relay_subdomain = "beta-relay.forum-host.example"

[mesh]
mode = "standalone"   # per-tenant; or override per-tenant
```

One kit deployment serving N forum instances via TOML profile selection. The kit's CF Workers route based on `Host:` header to per-tenant D1 / KV namespaces; the relay-worker DO is partitioned by tenant slug (one DO per tenant, hosted under the same Worker route).

**Use cases**:
- Managed-forum hosting provider running many forums on one Cloudflare account + one D1 organisation.
- An organisation running multiple departmental forums (engineering, product, ops) under one operations team.

**Tradeoffs (explicit)**:
- **Shared rate-limit budgets**: the kit's per-IP rate limit (`relay_do/broadcast.rs:75`, ADR-073 D8) applies *across tenants* unless the kit is patched to make rate limiting tenant-scoped. Workaround: add `tenant_slug` to the rate-limit key.
- **Single-tenant blast radius**: a misbehaving tenant's traffic affects the shared CF Worker; a misconfigured tenant's `wrangler.toml` typo can break others if config is monolithic. Mitigation: per-tenant TOML files validated independently, never merged at build time.
- **Federation cardinality**: each tenant may have its own federation key and peer set. Fan-out per ADR-073 D2 happens per-tenant; the kit's federation worker MUST scope `seen_ids` LRU per `(tenant, peer)` pair, not just per peer (otherwise tenant A's events can suppress tenant B's fan-out to the same peer).

**Rationale**: managed hosting is a likely commercial extension of the kit. Naming it as a topology lets the QE policy (ADR-077) ship contract tests for tenant isolation; without naming it, the kit accumulates ad-hoc multi-tenant patches that don't compose.

**Footprint**: shared CF Workers; per-tenant D1, KV, R2 namespaces; per-tenant relay DO instance.

### D5 — High-availability deployment

```toml
[deployment.ha]
enabled = true
regions = ["wnam", "enam", "weur", "eeur", "apac"]
relay_pinning = "geo"           # DO regional pinning per ADR-073 D11
d1_replication = "multi-region" # Cloudflare D1 read replicas
kv_replication = "multi-region"
r2_replication = "geo"          # R2 bucket geo-replication

[mesh]
mode = "federated"
peer_relays = [...]
auto_failover_threshold_seconds = 60
```

Production-grade availability. CF Workers globally distributed (Cloudflare default). D1 with multi-region read replicas. KV with multi-region replication. R2 with geo-replication. Relay DO regionally pinned per `regions` list; ADR-073 D11 mesh health probes drive auto-failover when a peer is unreachable for `auto_failover_threshold_seconds`.

**Use cases**:
- Production deployment with SLA (target 99.9%+ availability).
- Global community with users distributed across continents (latency-sensitive read paths).
- Mission-critical operator (regulated, contractually-obligated availability).

**Rationale**: Cloudflare's primitives already support most of this; the kit's contribution is making it declarative in TOML + ensuring the relay-worker's federation logic handles peer unreachability gracefully (ADR-073 D11 already specifies the probe + counter; D5 here pulls failover behaviour to the deployment-config layer).

**Failover semantics**:
1. Health probe (ADR-073 D11) detects peer unreachable for `auto_failover_threshold_seconds`.
2. Mesh service-list (kind-30033, ADR-074) is consulted for alternative peer endpoints advertised by the same DID.
3. If alternatives exist, federation socket is reconnected to the alternative; `mesh_peer_failover_total{from, to}` Prometheus counter increments.
4. If no alternatives, the peer is marked unreachable; outbound fan-out for that peer is buffered up to `mesh.fanout_buffer_size` events (default 1024); on reconnect, buffered events are replayed in order.

**Footprint**: ~Cloudflare standard production CF Worker tier + D1 enterprise + KV multi-region + R2 geo-replication + Prometheus / Grafana for mesh dashboards. Order-of-magnitude $200-500/mo for low-to-mid traffic.

### D6 — Operator-key custody patterns

The kit's relay-worker holds two key categories: the **operator** key (pubkey published in `mesh.service-list` kind-30033, signs admin moderation) and the **federation** key (per ADR-073 D4, used for relay-relay AUTH; one per private relay per Q3 §I12 `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md`:581). Both have three custody options:

#### (a) Filesystem custody (default)

```toml
[custody]
mode             = "filesystem"
operator_key_file = "/var/lib/nostr-bbs/identities/operator.json"
federation_key_file = "/var/lib/nostr-bbs/identities/federation.json"
```

Plaintext JSON, file mode `0600`, owner-readable only. Matches the agentbox `sovereign-bootstrap.py` pattern (`docs/integration-research/03-agentbox-surfaces.md`:11 §1) — keys generated on first boot if files do not exist; persisted in a named volume; survives container restart, lost on volume nuke.

**Use cases**: standalone deployments, single-operator forums, evaluation environments.

**Risk**: filesystem compromise = key compromise. Mitigation: `chmod 0600`, named-volume isolation, periodic rotation per ADR-074 D9.

#### (b) AWS KMS / Cloudflare Workers Secrets

```toml
[custody]
mode = "cloud-secret"

[custody.cloud_secret]
provider          = "cloudflare-workers-secret"     # or "aws-kms"
operator_key_ref  = "OPERATOR_NSEC"                 # CF Worker secret name
federation_key_ref = "FEDERATION_NSEC"
```

Keys live in the cloud provider's secret store. The relay-worker reads them via the platform's secret-binding API at boot; never written to disk. CI/CD pipelines inject keys via `wrangler secret put` or AWS KMS data-key envelopes.

**Use cases**: federated deployments where secrets-management hygiene is a compliance requirement; CI deployments where ephemeral keys are issued per release.

**Risk**: cloud provider becomes part of the trust boundary. Mitigation: KMS audit logs, key rotation on a schedule.

#### (c) Hardware HSM (YubiHSM, Ledger, AWS CloudHSM)

```toml
[custody]
mode = "hsm"

[custody.hsm]
provider           = "yubihsm-2"
operator_key_slot  = 0x0001
federation_key_slot = 0x0002
pkcs11_module      = "/usr/lib/yubihsm_pkcs11.so"
```

Keys never leave the HSM; signing happens inside the device via PKCS#11. The relay-worker's signer interface (`crates/nostr-bbs-config/src/signer.rs`, new in PRD-011) abstracts over `FilesystemSigner` / `KmsSigner` / `Pkcs11Signer`.

**Use cases**: highest-security-tier operators (defence contractors, regulated finance, classified networks). Manual rotation; no automated key generation.

**Risk**: operational complexity. HSM unavailable = forum signing unavailable. Mitigation: dual-HSM redundancy or fallback to KMS-issued temporary key with operator approval.

#### Cardinality summary (per Q3 §I12 `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md`:581)

| Key | Cardinality | Custody recommendation |
|-----|-------------|------------------------|
| Operator key | 1 per deployment | (a) for standalone, (b)/(c) for federated/HA |
| Federation key | 1 per private relay (per ADR-073 D4) | (a) for standalone, (b)/(c) for federated/HA |
| Welcome-bot key | 1 per forum (optional, kit-default zone bot) | (a) acceptable; key has limited authority (kind-1 in welcome cohort only) |

### D7 — Downstream-consumer pattern (`dreamlab-ai-website` as exemplar)

Per PRD-011 §5.4 (`docs/PRD-011-visionflow-forum-kit-extraction.md`:273), the kit publishes `nostr-bbs-*` crates to crates.io; downstream packages depend on them via `Cargo.toml` and supply a `<deployment>.toml`.

```toml
# dreamlab-ai-website/forum-config/Cargo.toml
[dependencies]
nostr-bbs-relay-worker = "3.0"
nostr-bbs-pod-worker   = "3.0"
nostr-bbs-forum-web    = "3.0"
nostr-bbs-config       = "3.0"
```

```rust
// dreamlab-ai-website/forum-config/src/main.rs
use nostr_bbs_relay_worker::RelayWorker;
use nostr_bbs_config::Config;

fn main() {
    let config = Config::load("dreamlab.toml").unwrap();
    let branding = include_bytes!("../assets/dreamlab-branding.png");
    RelayWorker::new(config).with_branding(branding).run();
}
```

**Branding/zoning lives in the consumer's package, never in the kit.** PRD-011 G7 (de-branding completeness) is the load-bearing constraint: the kit must not contain the strings "DreamLab", `dreamlab-ai.com`, or any DreamLab-specific copy. `dreamlab.toml` (PRD-011 §5.4 lines 290-296) reproduces the existing forum's exact configuration; `dreamlab_branding.rs` carries the logos and copy.

**Pattern generalisation**: any operator extracts a `<operator>-forum-config` package, depends on the kit, supplies a `<operator>.toml`, and bakes in their own branding. The kit ships zone-shape primitives (lobby / members / trusted) and trust-progression mechanics; consumers rename zones, recolor, and inject branding.

**Use cases**:
- DreamLab production (the anchor consumer; PRD-011 Phase X5 cutover).
- Future kit consumers each running their own thin wrapper.
- Test fixtures that mock a downstream consumer.

**Anti-pattern**: forking the kit to add branding. The kit is *consumed*, never forked; G3 ("DreamLab as one consumer among many") is intentionally egalitarian.

### D8 — Air-gapped deployment

```toml
[mesh]
mode = "standalone"
peer_relays = []   # empty by definition

[deployment.network]
external_egress = "deny"
loopback_only   = true

[setup_provider]
default = "anthropic"   # NOT "agentbox-nostr" — that requires external relay
# OR
default = "local-llm"   # if a sixth provider is added (out of scope for ADR-079)
```

Zero external network egress. All Nostr traffic is loopback-only. Used in regulated environments (defence, classified networks, airgapped research clusters) where a kit deployment must not reach the public internet.

**Use cases**:
- Defence / intelligence operators.
- Regulated finance with airgapped review networks.
- Research clusters with classified-data discussion forums.

**Implications**:
- Federation is impossible by definition; air-gapped is necessarily standalone.
- ADR-079's `agentbox-nostr` skill provider is unusable (it requires reaching an agentbox relay over Nostr). Operators must use a direct-API skill provider (Anthropic / OpenAI direct, with a one-time air-gap-to-internet exception during setup) or wait for a future local-LLM provider.
- Cloudflare-hosted CF Workers are non-starters; operators must deploy via Workers' on-prem alternatives (Cloudflare's planned air-gapped Workers offering, or an OSS CF Workers-compatible runtime like `workerd` self-hosted).
- Pod-worker, forum-web, kv, d1 must all run on-prem; D1 substituted with PostgreSQL or SQLite per the kit's `[storage]` adapter abstraction (PRD-011 §5.2 storage adapters).

**Rationale**: air-gapping is a real operator class; calling it out lets the kit's `forum-setup` skill (ADR-079 D2) detect `external_egress = "deny"` and refuse to offer the `agentbox-nostr` provider. Without this naming, operators discover the incompatibility at provisioning time.

**Footprint**: on-prem `workerd` or equivalent; on-prem PostgreSQL/SQLite; on-prem R2-equivalent (MinIO, S3-on-prem). No Cloudflare account required.

### D9 — Migration topology

The existing `dreamlab-ai-website/community-forum-rs/` deployment migrates to a kit-based deployment without downtime per PRD-011 R5 (`docs/PRD-011-visionflow-forum-kit-extraction.md`:493) + Phase X5 (`docs/PRD-011-visionflow-forum-kit-extraction.md`:460).

#### Migration topology (transient)

```
       ┌──────────────────────────┐
       │ dreamlab-ai-website      │
       │ community-forum-rs       │   (existing; "old path")
       │ legacy CF Workers        │
       └──────────────────────────┘
                  │
                  │  Cloudflare Worker route (feature-flag split)
                  ▼
       ┌──────────────────────────┐
       │ Cloudflare router        │
       │ (e.g. CF Workers route)  │
       │ flag: USE_KIT_PATH       │
       └──────────────────────────┘
                  │
                  ▼
       ┌──────────────────────────┐
       │ dreamlab-ai-website/     │
       │ forum-config (D7)        │   (new; "kit path")
       │ nostr-bbs-* crates       │
       └──────────────────────────┘
```

#### Phase X5 sequence

1. Phase X5.1 — Provision new path. `dreamlab-ai-website/forum-config/` package created (PRD-011 §5.4); `dreamlab.toml` reproduces current forum config (admin pubkey, hostname, relay URL, pod URL). New path deployed to a parallel CF Worker route under a sub-domain (e.g. `forum-kit.dreamlab-ai.com`). Both old and new paths read/write the same D1 + KV + R2 (data-plane unchanged).
2. Phase X5.2 — Mirror traffic. Both old and new paths run live; a CF Worker route does percentage-based traffic split (start at 1% → 10% → 50% → 100% over ~2 weeks). Delta reconciliation: any divergence in D1 reads between the two paths is logged; per PRD-011 R5, mismatches block the rollout.
3. Phase X5.3 — Cut over. At 100% kit-path traffic for 1 week with zero divergence, delete `community-forum-rs/`. Old CF Worker decommissioned.
4. Phase X5.4 — Rollback path. For 30 days post-cutover, the old path remains in git but not deployed; `git revert` + redeploy is the rollback. After 30 days, deletion (PRD-011 G8 follow-up sprint).

#### Data-plane invariants (must hold throughout)

- Kind-1059 (gift-wrapped DM) reads return identical results from both paths for the same `#p` filter.
- Trust-progression state (zone membership) is identical: a user in `trusted` cohort on the old path is in `trusted` on the new path.
- Federation: during migration, the existing forum's federation key continues to AUTH against agentbox + VisionClaw; the kit-path uses the same key (no key rotation during migration).

#### Use cases

- DreamLab cutover (the canonical instance; PRD-011 R5 + Phase X5).
- Any future operator migrating from a forked kit-precursor to the kit proper.
- Operators upgrading between major kit versions (3.x → 4.x) with breaking schema changes can use this topology.

### D10 — Topology-selection decision tree

Operators select a topology based on five constraints: federation needs, availability tier, key-custody preference, network egress posture, multi-tenancy needs.

```
Q1: Is the deployment air-gapped (no external egress)?
    YES → D8 (Air-gapped).        STOP.
    NO  → continue.

Q2: Does the operator host N>1 forum tenants on shared infra?
    YES → D4 (Multi-tenant). Combine with Q3-Q5 per-tenant.
    NO  → continue.

Q3: Does the operator want to federate with peer relays?
    NO  → continue Q4.
    YES → continue Q3a.

  Q3a: Does the operator run their own relay process?
       YES → D2 (Federated mesh).
       NO  → D3 (Client mode).

Q4: Has the operator chosen "no federation, run own relay"?
    YES → D1 (Standalone).
    NO  → revisit Q3.

Q5: What availability tier?
    Best-effort   → footprint of D1/D2/D3 as selected.
    SLA / global  → layer D5 (HA) on top of selected pattern.

Q6: What key-custody tier (orthogonal)?
    Single-team / eval     → D6(a) Filesystem.
    CI/CD or compliance    → D6(b) Cloud secret store.
    Highest security tier  → D6(c) Hardware HSM.

Q7: Existing pre-kit deployment?
    YES → D9 (Migration topology) for the cutover; settle to one of D1-D5 post-migration.
    NO  → no migration step needed.

Q8: Branding/zoning customisation?
    Always → D7 (Downstream-consumer pattern). The kit is consumed,
             never forked.
```

**Worked examples**:

- Single-team co-op forum, evaluating the kit, no federation, basic availability, filesystem keys → D1 + D6(a) + D7.
- DreamLab production → D2 (federated, peers = agentbox + VisionClaw) + D5 (HA) + D6(b) (Cloudflare Worker secrets) + D7 (`dreamlab.toml` consumer package).
- Hosting provider running 50 forums → D4 + per-tenant D1/D2/D3 selection + D6(b) + D7 per tenant.
- Defence contractor → D8 + D6(c) + D7 + on-prem `workerd`.
- Coalition of three research labs → D2 (peers = each other) + D6(b) + D7 per lab.
- Hobbyist operator on free CF tier → D3 (peer to a friend's federated kit) + D6(a) + D7.

## Consequences

### Positive

- **Named topology vocabulary**: operator docs, the AI configurator (ADR-079), QE smoke tests (Q5 §T7 `docs/integration-research/qe-fleet/Q5-test-fixture-design.md`:753), and federation health probes all converge on the same six pattern names. Conversation between an operator, the configurator, and the kit's docs is shared-language by construction.
- **Default path is the simple path**: standalone (D1) is the default; new operators do not need to understand mesh, federation, or multi-tenancy to get a working deployment. Federation is opt-in (preserving ADR-073 D6).
- **Operator-tier symmetry**: D6's three custody options + D5's HA layer let every topology scale from "evaluation laptop" to "regulated production". Operators don't have to switch patterns to upgrade tier — they just layer in D5 and D6(b)/(c).
- **Air-gapped is a first-class topology**: D8 names it. The configurator (ADR-079) detects `external_egress = "deny"` and steers operators away from the `agentbox-nostr` provider; without this naming, operators hit the incompatibility at provision time and waste a wizard run.
- **Migration topology is bounded**: D9 specifies a transient pattern, with feature-flag split + parallel paths + reconciliation. Migrations are no longer ad-hoc.
- **Downstream-consumer pattern (D7) preserves G3**: the kit is consumed, never forked. Branding lives in the consumer; the kit is brand-neutral.

### Negative

- **Six topology patterns is non-trivial documentation surface**: each pattern needs a TOML example, a use-case section, a footprint summary, a tradeoff list. Estimated ~400 lines of operator-facing documentation per topology in the kit's `docs/topologies/` (delivered in Phase X3 or X6).
- **Multi-tenant (D4) requires kit-internal scoping work**: the rate-limit key, the federation `seen_ids` LRU, the per-tenant TOML loader all need careful implementation. Estimated ~1.5 sprints of additional work beyond Phase X1.
- **HA (D5) introduces failover semantics not yet in ADR-073**: D5's auto-failover via mesh service-list (kind-30033, ADR-074) is new behaviour. ADR-073 D11 specified probes + counters but not failover decisions; D5 here adds that. The failover algorithm is specified inline in D5's "Failover semantics" subsection (steps 1-4: probe-detect → service-list-consult → reconnect-or-buffer → replay-on-recover); implementation lands in PRD-011 Phase X3 alongside the kit's `nostr-bbs-mesh` crate. No separate follow-up ADR required.
- **Air-gapped (D8) implies an on-prem deploy story not yet validated**: `workerd` self-hosted, on-prem D1 substitution, on-prem R2-equivalent — these are operator-side concerns, but the kit must support `[storage]` adapter abstraction (PRD-011 §5.2) before D8 is realisable. A follow-up sprint adds storage-adapter tests for SQLite + MinIO.
- **Topology-selection pressure on the configurator**: ADR-079's `forum-setup` skill must implement Q1-Q8 as conversation steps. Approx +5 questions to the existing ~15-question flow.

### Neutral

- **Topology choice is reversible**: an operator who deploys D1 can migrate to D2 by adding `peer_relays` + `mesh.mode = "federated"` and re-deploying. Most patterns are layered on top of D1's primitives, so reversibility is the default.
- **Patterns compose**: D5 (HA) and D6 (custody) layer on top of D1/D2/D3/D4 without conflict. D7 (consumer pattern) is orthogonal to all others. D9 (migration) is transient and time-bounded. Only D8 (air-gapped) is mutually exclusive with D2/D3/D4-with-federation.

## Alternatives Considered

### Alt-A — No named topologies; let operators tune `[mesh]` directly

The kit ships ADR-073's `[mesh]` block with three modes (`standalone`, `federated`, `client`) and operator chooses fields à la carte; no overarching topology vocabulary.

*Rejected*: forces every operator to learn the mesh wire-protocol semantics. Hostile to small operators (a co-op evaluating the kit should not need to read ADR-073). Also blocks the configurator from detecting common patterns and auto-deriving fields. Without naming, ad-hoc patterns proliferate and the kit's documentation has no anchor.

### Alt-B — Two topologies only (standalone + federated)

Strip D3 (client mode), D4 (multi-tenant), D5 (HA), D8 (air-gapped) — the kit ships only D1 + D2.

*Rejected*: client mode (D3) is already specified in ADR-073 D6; refusing to name it as a topology pattern means ADR-073 says one thing and ADR-080 says another. Multi-tenant (D4) is a likely commercial extension; not naming it now means ad-hoc patches accumulate. HA (D5) and air-gapped (D8) are real operator classes (per F-availability-tier and F-air-gapped-operability); ignoring them means PRD-011 doesn't address its target audience.

### Alt-C — Per-topology feature flags with mutually exclusive modes

Each topology becomes a compile-time feature in the kit (`--features federated,multi-tenant`). Operators choose a feature set at build time.

*Rejected*: operators would compile their own kit binaries; loses the "consume the kit as a library" pattern (D7); breaks the `cargo install nostr-bbs-*` story. Also forces decisions at build time that should be config-time (e.g. switching from standalone to federated should not require a rebuild). Per ADR-073 D6, mode selection is runtime via `[mesh]`; that choice already settled this.

### Alt-D — One mega-topology with optional sub-features

Define a single topology "VisionFlow Forum Kit" with all features available; operators flip TOML flags to enable/disable. No named patterns.

*Rejected*: same problem as Alt-A (no shared vocabulary). Also encourages operators to enable features they don't need (every flag is "available"); patterns codify "the right set of flags for this use case", which is the whole point.

### Alt-E — Inherit topologies from agentbox

agentbox already defines `mesh.mode = "standalone" | "federated" | "client"` (per agentbox `ADR-005-pluggable-adapter-architecture.md`). The kit just inherits agentbox's topology vocabulary verbatim.

*Rejected*: agentbox's modes describe agentbox's own deployment, not a forum kit. The kit has additional concerns (multi-tenant per D4, downstream-consumer per D7, migration topology per D9, air-gapped per D8) that agentbox does not. Sharing the underlying mode names (`standalone`/`federated`/`client`) where they overlap is fine and intentional (D1/D2/D3 use the same names) — but the topology vocabulary on top is kit-specific.

### Alt-F — Kit ships an opinionated "DreamLab production" topology only

The kit assumes DreamLab's exact topology (federated with agentbox + VisionClaw, HA, Cloudflare Workers secrets, KMS custody) and other operators adapt by editing the kit.

*Rejected*: violates PRD-011 G3 (DreamLab as one consumer among many). Forces every operator to fork the kit. Defeats the purpose of extracting it as a reusable library.

## Implementation notes

### Topology declaration in TOML

The kit's `[deployment]` block (PRD-011 §5.2) gains a `topology` field:

```toml
[deployment]
topology = "standalone"   # one of: standalone | federated | client | multi-tenant | ha | air-gapped
# (D7 + D9 are not topology values — D7 is structural, D9 is transient)
```

The `forum-setup` skill (ADR-079) infers `topology` from the conversation flow and writes it explicitly. The kit's TOML validator (PRD-011 F3.3) enforces that TOML field combinations match the declared topology (e.g. `topology = "client"` ⇒ `[mesh].mode = "client"` ⇒ `peer_relays` non-empty).

### Per-topology test fixtures

Per ADR-077 P3 (federation contract testing) + Q5 §T7 (`docs/integration-research/qe-fleet/Q5-test-fixture-design.md`:753), the kit ships per-topology test fixtures:

```
nostr-rust-forum/tests/topologies/
├── standalone/
│   ├── docker-compose.yml
│   └── topology-smoke.test.ts
├── federated/
│   ├── docker-compose.yml         # forum + agentbox-relay + visionclaw-bridge per Q5 §T7
│   └── topology-smoke.test.ts
├── client/
│   ├── docker-compose.yml
│   └── topology-smoke.test.ts
├── multi-tenant/
│   ├── docker-compose.yml         # 3 tenants on one Worker
│   └── topology-smoke.test.ts
├── ha/
│   ├── docker-compose.yml         # 2 regions, peer failover scripted
│   └── topology-smoke.test.ts
├── air-gapped/
│   ├── docker-compose.yml         # internal-network-only, external egress blocked
│   └── topology-smoke.test.ts
└── migration/
    ├── docker-compose.yml         # old + new path, traffic split, reconciliation
    └── topology-migration.test.ts
```

Each topology's smoke test boots the deployment, runs a canonical event flow (post → DM → moderate, scaled to topology), and asserts wire-level invariants. The federated topology's fixture is exactly Q5 §T7 with no additions.

### Per-topology Prometheus metrics

The relay-worker exports `mesh_topology_info{topology, deployment_id}` as a constant gauge (value=1) labelled with the operator's declared topology. Aggregating across operators (with consent) gives the kit's maintainers visibility into which topologies are in production use.

### Migration tooling

For D9, the kit ships `nostr-bbs-migrate` CLI (under `crates/nostr-bbs-migrate/`):

```bash
nostr-bbs-migrate plan \
  --from /path/to/community-forum-rs \
  --to ./forum-config

nostr-bbs-migrate split \
  --route-config ./cf-route.toml \
  --start-percentage 1

nostr-bbs-migrate reconcile \
  --window 7d \
  --threshold 0.0001     # max-tolerable divergence rate

nostr-bbs-migrate cutover \
  --route-config ./cf-route.toml
```

Each subcommand maps to a Phase X5 step. The reconciliation tool consumes D1 logs from both paths and asserts read-result equality; on divergence beyond the threshold, it blocks `cutover`.

### Documentation footprint

The kit's `docs/topologies/` directory ships one Markdown per topology:

- `docs/topologies/D1-standalone.md`
- `docs/topologies/D2-federated.md`
- `docs/topologies/D3-client.md`
- `docs/topologies/D4-multi-tenant.md`
- `docs/topologies/D5-ha.md`
- `docs/topologies/D6-key-custody.md`
- `docs/topologies/D7-downstream-consumer.md`
- `docs/topologies/D8-air-gapped.md`
- `docs/topologies/D9-migration.md`
- `docs/topologies/decision-tree.md` (D10)

Each operator-facing doc has: full TOML example, footprint summary (CF Workers / DO / D1 / KV / R2), tradeoff list, common operator pitfalls, smoke-test invocation. Estimated 400-600 lines per document; total ~4000-5000 lines of operator docs, delivered in Phase X3 (parallel to QE policy work) and finalised in Phase X6.

### Configurator integration (ADR-079)

The `forum-setup` skill's conversation flow gains questions for D10's Q1-Q8. Topology classification happens as the user answers, and downstream questions branch on it (e.g. if `topology = "air-gapped"`, the skill skips the `agentbox-nostr` provider question and the cloud-custody options in D6). Estimated +5 questions to the existing ~15.

### QE invariants

Per ADR-077 P1-P5, the kit's topology test surface includes:

- Topology classifier round-trip: write TOML for topology X, parse, classify, assert X matches.
- Topology composition tests: `topology = "ha" + "federated"` produces a valid composed config; `topology = "air-gapped" + "federated"` is rejected by the TOML validator.
- Migration reconciliation property test: random valid pre-migration state + random user actions → post-migration state is deterministically equal between old and new paths (modulo declared-divergence allowlist).
- Multi-tenant isolation contract: tenant A cannot read tenant B's events even with the same `#p` filter (rate-limit and federation `seen_ids` are tenant-scoped).
- HA failover smoke test: kill a peer, assert `mesh_peer_failover_total` increments within `auto_failover_threshold_seconds + 5s`.

## References

- PRD-010 — DID:Nostr Mesh Federation (G3, G7)
- PRD-011 — VisionFlow Forum Kit Extraction (G1, G3, G4, G8; §5.2 TOML schema; §5.4 downstream-consumer pattern; R5 + Phase X5 migration)
- ADR-073 — Private Nostr Relay Mesh Topology & NIP-42 AUTH (D2 fan-out, D4 federation key, D6 mode selection, D9 loop avoidance, D11 health probes)
- ADR-074 — Cross-System DID:Nostr Canonicalisation (D9 key rotation, kind-30033 mesh service-list, D10 delegation patterns)
- ADR-075 — IS-Envelope v1 contract (used in agentbox-nostr skill provider via `tool_invoke` / `tool_result`)
- ADR-076 — `nostr-core` absorption into upstream `nostr` crate (kit consumes upstream)
- ADR-077 — Ecosystem QE Policy (per-topology test fixtures, contract tests, mutation-testing targets)
- ADR-078 — Cross-substrate library convergence (kit consumes upstream Rust libraries)
- ADR-079 — Forum-Setup Skill Provider Abstraction (configurator integrates D10 decision tree)
- `docs/ddd-mesh-federation-context.md` — bounded-context model for mesh federation
- `docs/integration-research/qe-fleet/Q3-identity-custody-audit.md` §I12 — federation key cardinality
- `docs/integration-research/qe-fleet/Q5-test-fixture-design.md` §T7 — federation smoke topology
- `docs/integration-research/03-agentbox-surfaces.md` §1 — `sovereign-bootstrap.py` filesystem custody pattern
- `agentbox/docs/reference/adr/ADR-005-pluggable-adapter-architecture.md` — agentbox's federation/standalone adapter precedent
- GitHub repos:
  - https://github.com/DreamLab-AI/nostr-rust-forum (canonical kit; product `nostr-bbs-rs`)
  - https://github.com/DreamLab-AI/dreamlab-ai-website (downstream consumer exemplar; D7)
  - https://github.com/DreamLab-AI/agentbox (mesh peer + skill provider; D2 + ADR-079)
  - https://github.com/DreamLab-AI/solid-pod-rs (foundation library; pod adapter)
  - https://github.com/DreamLab-AI/VisionClaw (this monorepo; mesh integration substrate)

## Cross-reference notes (post-ADR-079)

ADR-079's configurator (`forum-setup` skill) is the runtime that applies this ADR's D10 decision tree. The two ADRs are tightly coupled: ADR-080 specifies the topology space; ADR-079 specifies the conversation flow that selects within it. Per PRD-011 §5.5, the skill's conversation flow gains a topology-classification stage early in the wizard so subsequent questions branch correctly (e.g. air-gapped operators are not asked about Cloudflare Workers Secrets). The smoke test fixture in `nostr-rust-forum/tests/topologies/federated/` shares the docker-compose.yml shape from Q5 §T7 — these files are intentionally cross-referenceable so the QE policy (ADR-077) tests the topology and the federation contract simultaneously.
