# Panel 1 — "THE ECOSYSTEM" (Portrait, High Resolution)

## Image Generation Prompt

Create a high-resolution portrait-orientation (2:3 aspect ratio, minimum 2400×3600px) technical infographic panel with the following precise visual specifications and content layout. This is panel 1 of a triptych — it covers the ecosystem landscape, identity layer, and value proposition. No content from panels 2 or 3 appears here.

---

### GLOBAL VISUAL STYLE

**Background:** Deep navy-black (#0a0e1a) with subtle star field particles (tiny white/blue dots at 5-8% opacity), faint blue-purple nebula clouds in upper-right and lower-left corners, barely-visible orthographic grid lines (#1a2040 at 3% opacity) creating a technical blueprint feel. The overall impression is deep space meets engineering schematic.

**Color Palette (strict):**
- Primary: Cyan/teal (#00e5ff, #00bcd4) — used for borders, primary text headers, infrastructure elements
- Accent 1: Magenta/hot pink (#ff4081, #e040fb) — used for identity/security concepts, DID elements
- Accent 2: Lime/green (#76ff03, #69f0ae) — used for data flow arrows, active states, success indicators
- Accent 3: Amber/gold (#ffd740, #ffab40) — used for metrics, numbers, warning states
- Accent 4: Deep purple (#b388ff, #7c4dff) — used for agent/AI elements, secondary containers
- Text: White (#ffffff) at 90% opacity for body, 60% opacity for secondary labels
- Borders: 1.5px strokes with outer glow (4px blur, 30% opacity of stroke color)

**Typography:**
- Title: Bold condensed sans-serif (like Rajdhani, Orbitron, or Exo 2), uppercase, with subtle outer glow matching text color
- Section headers: ALL CAPS, 18-22pt equivalent, with a thin colored underline extending 60% of container width
- Body text: Clean technical sans-serif (like Inter, Source Sans Pro), 9-11pt equivalent, white at 85% opacity
- Metric numbers: Extra-bold, 48-72pt equivalent, amber/gold with subtle glow
- Labels: Monospace (like JetBrains Mono), 7-8pt, white at 55% opacity

**Containers:** Rounded rectangles (8px radius) with thin neon borders (1.5px). Interior fill is background color at 60% opacity with very subtle gradient (slightly lighter at top). Some containers have a faint inner glow along the top edge.

**Connectors:** Thin dashed or dotted lines (1px) in muted versions of the palette colors, with small triangular arrowheads (4px). Lines route orthogonally (horizontal/vertical with 90-degree bends), never diagonal.

**Icons:** Outlined geometric style, single-color, 24-32px. Minimalist technical icons — not emoji, not filled, not photorealistic. Think Lucide, Phosphor, or Tabler icon style.

---

### LAYOUT (top to bottom, full width)

---

#### ZONE A — TITLE BANNER (top 8% of canvas)

**Left-aligned title block:**
- "DreamLab" in white bold condensed, 64pt, with very subtle teal outer glow
- "AI" immediately after in magenta, same size, same font
- Below the title: tagline in white at 60% opacity, 14pt: *"Sovereign knowledge federation across five substrates — where contributors, agents, and strategy compound"*

**Right side of title bar:**
- Five small pill-shaped badges in a horizontal row, each with a distinct icon and label:
  - Pill 1: Cyan border, gear icon, "5 SUBSTRATES"
  - Pill 2: Magenta border, fingerprint icon, "DID:NOSTR IDENTITY"
  - Pill 3: Green border, git-branch icon, "GIT-NATIVE INGEST"
  - Pill 4: Amber border, coin icon, "HTTP 402 PAYMENTS"
  - Pill 5: Purple border, bot icon, "AGENT JOB PRICING"

**Thin horizontal cyan line** separating title from content below, with small diamond ornament at center.

---

#### ZONE B — THE FIVE SUBSTRATES (next 28% of canvas)

**Section header:** "THE FIVE-SUBSTRATE LANDSCAPE" in cyan, ALL CAPS, with thin cyan underline.

**Layout:** Five rounded-rectangle cards arranged in a cross/diamond formation — one large card in the center (VisionClaw), four smaller cards at compass points connected by glowing circuit-line connectors.

**Center card (largest, ~40% of section width):**
- Double-border in cyan (outer thin, inner thinner with 4px gap)
- Header: "VISIONCLAW" in cyan bold with small hexagonal badge showing "INTEGRATION SUBSTRATE"
- Icon: Stylized claw mark / eye symbol in cyan glow
- Three stat pills inside: "236K Rust" | "112K TypeScript" | "7K CUDA"
- Brief: "Knowledge graph + XR + actor mesh + GPU physics + HTTP 402 pay handler. Per-endpoint GPU cost table (inference 10×, image-gen 100×, analytics 5×). Master fixture host. The integration spine connecting all substrates."
- Small GitHub icon with "DreamLab-AI/VisionClaw"

**Top card (connected to center by upward cyan line):**
- Purple border
- Header: "AGENTBOX" with container/box icon
- Badge: "SOVEREIGN AGENT CONTAINER"
- Stats: "104K lines" | "Python + JS + Nix" | "101 skills"
- Brief: "Nix-based v2 agent container. Pod bridge, nostr-rs-relay, skill provider (cost-estimation skill, 101 total). Payment routes (/v1/pay/*), HTTP 402 cost gate, kind 38200/38201 job events. Mesh peer with pluggable adapter architecture."

**Right card (connected by rightward magenta line):**
- Magenta border
- Header: "SOLID-POD-RS" with shield/lock icon
- Badge: "FOUNDATION LIBRARY"
- Stats: "42K lines" | "Rust + JS"
- Brief: "LDP / WAC / WebID / NIP-98 / DID Tier-3 / MRC20 tokens / HTTP 402 Web Ledgers. Data sovereignty + payment primitives consumed by all substrates."

**Bottom card (connected by downward green line):**
- Green border
- Header: "NOSTR-RUST-FORUM" with chat/forum icon
- Badge: "CONFIGURABLE KIT"
- Stats: "54K lines" | "Rust"
- Brief: "Generic forum kit extracted from production. NIP-98 auth, D1 atomic payments, MRC20 token buy/withdraw, agent job CRUD (create/start/settle/cancel), per-endpoint GPU cost tiers, username reservations, mesh scaffolding."

**Left card (connected by leftward amber line):**
- Amber border
- Header: "DREAMLAB WEBSITE" with globe icon
- Badge: "KIT CONSUMER"
- Stats: "28K lines" | "TypeScript"
- Brief: "DreamLab's branded forum deployment. DREAM token (10/sat), PaymentDashboard, CI pipeline, 24-table D1 migration (incl. agent_jobs), agent job invoicing via nostr-bridge."

**Between cards:** Small label on each connector line showing the relationship:
- Center→Top: "BC20 actor mesh"
- Center→Right: "DID Tier-3 + payments"
- Center→Bottom: "kit + payment routes"
- Center→Left: "consumer + invoicing"

---

#### ZONE C — IDENTITY FOUNDATION (next 22% of canvas)

**Section header:** "IDENTITY + PAYMENTS — did:nostr AS UNIVERSAL PRIMITIVE" in magenta, ALL CAPS, with thin magenta underline.

**Left half — Identity Architecture diagram:**

A vertical stack of four rounded boxes connected by downward arrows:

**Box 1 (top, magenta border):**
- "DID DOCUMENT (CANONICAL)" header
- Content showing (in monospace):
  ```
  did:nostr:<64-hex-pubkey>
  verificationMethod:
    SchnorrSecp256k1VerificationKey2019
    z-form base58btc multibase
  ```
- Small icon: key symbol
- Small note: "W3C DID Core v1 context — solid-pod-rs canonical format"

**Box 2 (purple border):**
- "NIP-26 DELEGATION" header
- Content: "Cross-system trust pivot — agents delegate to substrates via Schnorr signatures. One identity, five systems."
- Small icon: chain-link symbol

**Box 3 (cyan border):**
- "THREE CUSTODY TIERS" header
- Three horizontal pills inside:
  - "TIER 1 — Browser (NIP-07)" in green
  - "TIER 2 — CF Workers Secrets" in amber
  - "TIER 3 — Solid Pod + WAC" in magenta
- Small note: "per ADR-081"

**Box 4 (bottom, amber border):**
- "HTTP 402 WEB LEDGERS" header
- Content: "did:nostr identities carry sat balances. Agents and humans are indistinguishable — same DID, same debit/credit."
- Three horizontal pills: "MRC20 Tokens" (amber), "BIP-341 Taproot" (cyan), "JCS State Chains" (green)
- Small icon: coin/payment symbol

**Right half — IS-Envelope Message Contract:**

A stylized envelope/packet diagram showing the message structure:

**Header bar:** "IS-ENVELOPE v1" in cyan with "ADR-075" badge

**Envelope visualization:** A rounded rectangle styled as a translucent message packet with layers:
- Outer layer label: "NIP-59 GIFT WRAP" (magenta dashed border)
- Middle layer label: "JCS CANONICALISED" (cyan border)
- Inner content showing 7 envelope kinds as small colored pills arranged in two rows:
  - Row 1: "chat" (green), "tool_invoke" (purple), "tool_result" (purple), "knowledge_link" (cyan)
  - Row 2: "moderation" (amber), "mesh_ping" (teal), "job_estimate" (gold), "payment" (gold)

**Below envelope:** Small flow arrow showing: "Nostr Wire → JCS Verify → AS2 LDN → Solid Pod Inbox"

---

#### ZONE D — THE COMPOUNDING LOOP (next 20% of canvas)

**Section header:** "THE COMPOUNDING LOOP — KNOWLEDGE ACCRUES, TRUST PROPAGATES" in green, ALL CAPS, with thin green underline.

**Central visualization:** A horizontal flow diagram showing the share-state ladder with three large circular nodes connected by thick glowing arrows:

**Circle 1 (left, smaller, green border):**
- "PRIVATE" in bold
- Icon: lock/closed
- Label below: "Agent workspace. Local git. Enrichments accumulate."

**Arrow 1→2:** Thick green gradient arrow, label above: "CONTRIBUTOR PROMOTES"
- Small annotation below arrow: "Broker reviews quality, provenance, trust score"

**Circle 2 (center, medium, amber border):**
- "TEAM" in bold
- Icon: people/group
- Label below: "Shared within bounded context. Peer review. Skill evaluation."

**Arrow 2→3:** Thick amber-to-cyan gradient arrow, label above: "MESH FEDERATION"
- Small annotation below arrow: "DID-gated, Nostr-signed, precedent auto-approved"

**Circle 3 (right, largest, cyan border with double glow):**
- "MESH" in bold
- Icon: network/globe
- Label below: "Cross-substrate knowledge. Federated graph. Public provenance."

**Below the flow:** A thin dashed feedback arrow curves from Circle 3 back to Circle 1, labeled "PRECEDENT LEARNING — 40% of routine enrichments auto-approved after N=3 approvals"

**Second feedback arrow:** A thin amber dashed arrow curves from Circle 3 below the main arrow back to Circle 1, labeled "PAYMENT SETTLEMENT — agent jobs estimated, held, executed, settled via HTTP 402"

**Bottom of zone:** Four small stat boxes in a row:
- "7 GOALS DELIVERED" (green) — "G1-G7: Git ingest, DID registry, provenance, write-back, pod bridge, broker, Nostr control"
- "5 HARDENING ITEMS" (amber) — "R1-R5: Broadcast wiring, persistence, precedent, conflict handling, NIP-17 DMs"
- "13+9 QE FIXES" (magenta) — "13 Rust security items, 9 JS hardening items across agentbox bridge"
- "PAYMENT SYSTEM" (gold) — "MRC20 tokens, per-endpoint GPU pricing (10×/100×/5×), agent job CRUD lifecycle, D1 atomic ledger (24 tables), kind 38200/38201 settlement events, cost-estimation skill"

---

#### ZONE E — CROSS-SUBSTRATE INFRASTRUCTURE (next 15% of canvas)

**Section header:** "FEDERATION INFRASTRUCTURE" in purple, ALL CAPS, with thin purple underline.

**Three equal-width columns:**

**Column 1 — "MESH TOPOLOGY" (cyan border):**
- Small network diagram showing 5 nodes (one per substrate) in a mesh with bidirectional lines
- Label: "Private Nostr relay mesh (ADR-073)"
- Bullet points:
  - "NIP-42 AUTH gate per relay"
  - "Gossip protocol for event propagation"
  - "Hierarchical-mesh hybrid topology"

**Column 2 — "TEST FIXTURES" (green border):**
- Icon: beaker/flask
- Label: "13 cross-substrate reference vectors (ADR-082)"
- Small list:
  - "paulmillr/nip44 encryption"
  - "BIP-340 Schnorr signatures"
  - "RFC 8785 JCS canonicalisation"
  - "HKDF key derivation"
- Footer: "Single source of truth — VisionClaw hosts, others consume"

**Column 3 — "LIBRARY CONVERGENCE" (amber border):**
- Icon: merge/consolidate arrows
- Label: "ADR-078 convergence registry"
- Small table:
  - "nostr-core → rust-nostr upstream"
  - "solid-pod-rs → all 5 substrates (incl. agentbox)"
  - "did:nostr → canonical 64-hex-lowercase"
  - "payments → solid-pod-rs::payments upstream"
  - "mrc20 → solid-pod-rs::mrc20 (JCS + BIP-341)"
  - "cost-estimation → agentbox skill (GPU tiers + job lifecycle)"
  - "kind 38200/38201 → nostr relay job events"
- Footer: "Eliminates 10K+ duplicate lines, single payment model + cost model across ecosystem"

---

#### ZONE F — FOOTER (bottom 7% of canvas)

**Left:** "PANEL 1 OF 3 — THE ECOSYSTEM" in white at 40% opacity, small text
**Center:** DreamLab AI logo mark (stylized, outlined, in cyan)
**Right:** "dreamlab.ai" in white at 40% opacity, small text

**Very bottom edge:** A thin gradient line spanning full width, cycling through cyan → magenta → green → amber → purple, representing the five substrates.

---

### RENDERING NOTES

- Every text element must be crisp and legible — no blur, no decorative illegibility
- Maintain 12-16px minimum padding inside all containers
- Leave 8-12px gutters between adjacent containers
- All connector lines must be clean and orthogonal — no spaghetti
- The overall density should feel like a technical reference poster, not a marketing brochure
- No photographs, no 3D renders, no gradients on text — flat neon on dark
- The cosmic background should be subtle enough that all foreground elements read clearly
- Portrait orientation: width roughly 2400px, height roughly 3600px (2:3 ratio)
