# VisionFlow Wardley Map

This Wardley map positions VisionFlow's components along the evolution axis (Genesis → Custom-Built → Product → Commodity) and the value chain (Visible to user → Invisible infrastructure). It shows where the ecosystem creates unique value versus where it consumes commodity infrastructure.

## Strategic Map

![VisionFlow Strategic Wardley Map](visionflow-wardley-map.png)

<details>
<summary>Mermaid source (wardleyMap syntax, requires Mermaid 12+)</summary>

```mermaid
---
config:
  theme: base
---
wardleyMap
  title VisionFlow Coordination Platform
  anchor User Need [0.97, 0.59]
  anchor Hard Problems [0.92, 0.20]

  component Governance UI [0.85, 0.35] label [10, -15]
  component Knowledge Graph [0.82, 0.28] label [-70, -15]
  component Agent Skills [0.78, 0.32] label [10, 10]
  component Immersive XR [0.75, 0.25] label [-80, 10]

  component Judgment Broker [0.72, 0.22] label [-100, -15]
  component OWL 2 Reasoning [0.68, 0.18] label [-110, 10]
  component GPU Semantic Physics [0.65, 0.15] label [-130, -15]
  component Agent Control Surface [0.70, 0.30] label [10, -15]

  component VisionClaw [0.62, 0.25] label [-85, 10]
  component Agentbox [0.58, 0.35] label [10, -15]
  component Nostr Relay Mesh [0.55, 0.38] label [10, 10]

  component solid-pod-rs [0.50, 0.30] label [-85, -15]
  component DID Nostr [0.48, 0.28] label [-75, 10]
  component WAC Access Control [0.45, 0.32] label [-120, -15]
  component Web Ledger Payments [0.42, 0.20] label [-140, 10]

  component Nix Flakes [0.35, 0.55] label [10, -15]
  component MCP Protocol [0.40, 0.45] label [10, 10]
  component Nostr Protocol [0.30, 0.55] label [-100, -15]
  component Solid Protocol [0.28, 0.50] label [-100, 10]

  component CUDA [0.22, 0.72] label [10, -15]
  component Cloudflare Workers [0.18, 0.82] label [10, 10]
  component Docker [0.15, 0.85] label [10, -15]
  component PostgreSQL [0.12, 0.90] label [10, 10]
  component secp256k1 [0.08, 0.88] label [-75, -15]

  User Need --> Governance UI
  User Need --> Knowledge Graph
  User Need --> Agent Skills
  Hard Problems --> Judgment Broker
  Hard Problems --> Immersive XR

  Governance UI --> Agent Control Surface
  Knowledge Graph --> OWL 2 Reasoning
  Knowledge Graph --> GPU Semantic Physics
  Agent Skills --> Agentbox
  Agent Skills --> MCP Protocol

  Judgment Broker --> Agent Control Surface
  Agent Control Surface --> Nostr Relay Mesh
  OWL 2 Reasoning --> VisionClaw
  GPU Semantic Physics --> CUDA

  VisionClaw --> solid-pod-rs
  Agentbox --> solid-pod-rs
  Agentbox --> Nix Flakes
  Agentbox --> Docker
  Nostr Relay Mesh --> Nostr Protocol

  solid-pod-rs --> DID Nostr
  solid-pod-rs --> WAC Access Control
  solid-pod-rs --> Web Ledger Payments
  solid-pod-rs --> Solid Protocol
  DID Nostr --> secp256k1

  Nostr Protocol --> secp256k1
  Governance UI --> Cloudflare Workers
  solid-pod-rs --> PostgreSQL

  evolve OWL 2 Reasoning 0.35
  evolve GPU Semantic Physics 0.30
  evolve Agent Control Surface 0.45
  evolve Web Ledger Payments 0.35
  evolve Nostr Relay Mesh 0.52
  evolve DID Nostr 0.42
```

</details>

## Reading the Map

### Where VisionFlow Creates Unique Value (Genesis–Custom)

The top-left quadrant — visible to users, not yet commoditised — is where VisionFlow's strategic advantage lives:

- **Judgment Broker** — the human-in-the-loop governance layer that makes agent meshes trustworthy. No equivalent exists in any competing platform.
- **OWL 2 EL Reasoning** (evolving →) — formal semantic reasoning as the shared vocabulary for human-agent collaboration. Most agent systems use keyword matching; VisionFlow uses ontological subsumption.
- **GPU Semantic Physics** (evolving →) — ontology-driven force simulation where `subClassOf` creates attraction and `disjointWith` creates repulsion. Makes semantic relationships physically visible.
- **Agent Control Surface Protocol** (evolving →) — kinds 31400-31405 for structured agent-human governance decisions over Nostr. Being standardised through use; could become a NIP.

### The Protocol Layer (Custom-Built, evolving toward Product)

The middle band is where VisionFlow's protocol infrastructure sits — custom-built Rust implementations of open standards:

- **solid-pod-rs** — Rust port of JSS at ~98% parity. Compiles to both native and WASM. The foundation for self-sovereign data.
- **DID:Nostr** (evolving →) — bridges Nostr identity (secp256k1) to W3C DID and Solid WebID. Unique in combining these three identity systems.
- **Web Ledger Payments** (evolving →) — HTTP 402 micropayments, DREAM tokens, BIP-341 anchoring. Still genesis-stage; could commoditise agent-to-agent payments.
- **Nostr Relay Mesh** (evolving →) — moving from standalone deployment toward federated mode. The coordination transport layer.

### Commodity Infrastructure (Product–Commodity)

The bottom-right is consumed, not built:

- **CUDA, Docker, PostgreSQL, Cloudflare Workers, secp256k1** — commodity infrastructure. VisionFlow builds on top of these; it doesn't reinvent them.
- **Nostr Protocol, Solid Protocol, MCP Protocol** — open standards consumed as dependencies. VisionFlow implements them; it doesn't own them.
- **Nix Flakes** — product-stage build system consumed by agentbox for reproducibility.

### Strategic Movements

Arrows on `evolve` components show where the ecosystem is actively investing:

1. **OWL 2 Reasoning → Product**: Making formal reasoning accessible to non-ontologists through agent MCP tools and the Insight Ingestion Loop.
2. **Agent Control Surface → Product**: Standardising the governance event protocol for broader adoption beyond the DreamLab deployment.
3. **DID:Nostr → Product**: Pushing the identity bridge toward wider ecosystem adoption.
4. **Nostr Relay Mesh → Product**: Moving from standalone to federated mode (Sprint v12+).
5. **Web Ledger Payments → Custom-Built**: Building the agent micropayment infrastructure that makes token-priced agent services economically viable at scale.

## Inertia Points

- **CF Workers runtime limitation**: Cannot run Tokio, cannot spawn processes. Prevents git-pods, data export, and key provisioning on the edge tier. Resolved operationally by the two-tier pod architecture (ADR-093).
- **Nostr relay mesh federation**: Currently standalone; federated mode is scaffolded but not production-tested. The relay mesh is the critical enabler for enterprise-scale deployment.
- **OWL 2 accessibility**: Ontological reasoning is powerful but intimidating. The Insight Ingestion Loop (Discovery → Codification → Validation → Integration → Amplification) is the UX answer — making formal reasoning invisible to end users.

## Competitive Landscape

| Competitor Shape | What They Build | What They Miss |
|---|---|---|
| Agent frameworks (LangChain, CrewAI) | Orchestration | Governance, shared semantics, self-sovereign data |
| Knowledge management (Notion, Confluence) | Information storage | Formal reasoning, agent integration, provenance |
| Collaboration platforms (Slack, Teams) | Communication | Coordination, semantic grounding, trust hierarchies |
| AI platforms (OpenAI, Anthropic) | Model access | Decentralised identity, data sovereignty, governance |

VisionFlow occupies the intersection: governed coordination of distributed human and AI intelligence at scale, with self-sovereign data and platform-agnostic deployment.
