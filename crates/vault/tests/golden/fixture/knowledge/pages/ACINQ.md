---
public: true
---

# ACINQ
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:2925ac2f637d4648c084891a1082c4d2fe5739d21ad79206dedcdb4315c37659",
  "@type": "Page",
  "vc:slug": "acinq",
  "title": "ACINQ",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:bitcoin",
      "vc:label": "Bitcoin"
    },
    {
      "@id": "urn:visionflow:linked:payment-channel",
      "vc:label": "Payment Channel"
    },
    {
      "@id": "urn:visionflow:linked:lightning-network",
      "vc:label": "Lightning Network"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "preferred-term",
      "vc:value": "ACINQ"
    }
  ],
  "prov:wasAttributedTo": {
    "@id": "did:nostr:jjohare"
  },
  "prov:generatedAtTime": {
    "@value": "2026-05-29T00:00:00Z",
    "@type": "xsd:dateTime"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:acinq",
  "@type": "Class",
  "label": "ACINQ",
  "definition": "ACINQ is a Paris-based Bitcoin technology company specialising in the development and operation of Lightning Network infrastructure, most notably the Eclair Lightning Network node implementation written in Scala and the Phoenix self-custodial mobile Bitcoin wallet. The company operates one of the largest publicly reachable Lightning Network routing nodes, providing significant liquidity and routing capacity to the network. ACINQ contributes to the BOLT (Basis of Lightning Technology) protocol specification process and has pioneered features such as trampoline routing to reduce the on-device computation required by mobile Lightning clients.",
  "domain": "blockchain",
  "maturity": "established",
  "qualityScore": 0.74,
  "subClassOf": [
    {
      "@id": "urn:ngm:class:lightning-network",
      "label": "Lightning Network"
    }
  ],
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:eclair",
        "label": "Eclair"
      },
      {
        "@id": "urn:ngm:class:phoenix-wallet",
        "label": "Phoenix Wallet"
      }
    ],
    "partOf": [
      {
        "@id": "urn:ngm:class:lightning-network",
        "label": "Lightning Network Ecosystem"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:bitcoin",
        "label": "Bitcoin"
      },
      {
        "@id": "urn:ngm:class:payment-channel",
        "label": "Payment Channel"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:micropayment",
        "label": "Micropayment"
      },
      {
        "@id": "urn:ngm:class:non-custodial-wallet",
        "label": "Non-Custodial Wallet"
      }
    ],
    "implements": [
      {
        "@id": "urn:ngm:class:lightning-network",
        "label": "Basis of Lightning Technology"
      },
      {
        "@id": "urn:ngm:class:onion-routing",
        "label": "Sphinx Routing"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:htlc",
        "label": "HTLC"
      },
      {
        "@id": "urn:ngm:class:onion-routing",
        "label": "Onion Routing"
      },
      {
        "@id": "urn:ngm:class:programming-language",
        "label": "Scala Programming Language"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:splicing",
        "label": "Splicing"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:lnd",
        "label": "LND"
      },
      {
        "@id": "urn:ngm:class:core-lightning",
        "label": "Core Lightning"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:bitcoin-proof-of-work-protocol-layer-2",
        "label": "Bitcoin Layer 2"
      },
      {
        "@id": "urn:ngm:class:channel-liquidity",
        "label": "Channel Liquidity"
      },
      {
        "@id": "urn:ngm:class:bolt-12",
        "label": "BOLT12"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:decentralised-finance",
        "label": "Decentralised Finance"
      },
      {
        "@id": "urn:ngm:class:peer-to-peer-network",
        "label": "Peer-to-Peer Network"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:acinq-sas",
      "label": "ACINQ SAS"
    }
  ],
  "quality": 0.74,
  "provenance": {
    "attributedTo": "did:nostr:ontology-mesh",
    "generatedAt": "2026-06-13T00:00:00Z",
    "inferenceRule": "ManualEnrichment"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:annotation:link-resolutions:acinq:6dcfd0b83e17",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:2925ac2f637d4648c084891a1082c4d2fe5739d21ad79206dedcdb4315c37659"
  },
  "vc:resolutions": [
    {
      "raw": "[[Bitcoin Proof-of-Work Protocol]]",
      "resolved": "urn:visionflow:linked:bitcoin",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Payment Channel]]",
      "resolved": "urn:visionflow:linked:payment-channel",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Lightning Network]]",
      "resolved": "urn:visionflow:linked:lightning-network",
      "kind": "ResolvedLink"
    }
  ],
  "prov:wasAttributedTo": {
    "@id": "did:nostr:lcr-swarm"
  },
  "prov:generatedAtTime": {
    "@value": "2026-05-29T00:00:00Z",
    "@type": "xsd:dateTime"
  }
}
```

- ### Definition
  - ACINQ (pronounced "a-cinq", French for "at five") is a Paris-based Bitcoin technology company that designs, builds, and operates [[Lightning Network]] infrastructure. The company is best known for the [[Eclair]] node implementation — a production-grade [[Lightning Network]] daemon written in [[Scala Programming Language]] — and the [[Phoenix Wallet]], a self-custodial mobile [[Bitcoin]] wallet that communicates exclusively over the Lightning protocol. ACINQ operates one of the best-connected public [[Routing Node]] instances on the network, contributing significant [[Channel Liquidity]] and serving as a trampoline hub for mobile clients that cannot maintain a full routing table. Through its participation in the [[Basis of Lightning Technology]] (BOLT) specification process, ACINQ has helped define protocol features including [[Trampoline Routing]], [[Splicing]], and [[BOLT 12]] offers.

- ### Overview
  - ACINQ was founded in 2014 in Paris, France, originally focused on [[Bitcoin]] tooling. As the [[Lightning Network]] white paper emerged (Poon & Dryja, 2015) the company pivoted to become one of its three flagship node implementation teams, alongside Lightning Labs ([[LND]]) and Blockstream ([[Core Lightning]]).
  - The company's dual mission is (1) to provide a robust open-source [[Lightning Network]] implementation suitable for routing nodes and infrastructure operators, and (2) to make [[Lightning Network]] payments accessible to non-technical end-users through Phoenix, its consumer-facing wallet.
  - ACINQ operates ACINQ.fr, a publicly advertised super-node with a large number of open channels, which acts as a hub to which Phoenix wallet users connect by default, enabling [[Trampoline Routing]] — a technique ACINQ invented and proposed as a BOLT extension to delegate path-finding computation from resource-constrained mobile devices to well-connected routing nodes.

- ### Key Components
  - #### Eclair
    - The flagship open-source Lightning node implementation, written in [[Scala Programming Language]] using the Akka actor framework.
    - Targets server-side routing node operators and institutional Lightning infrastructure.
    - Supports all core BOLT features: [[HTLC]] forwarding, [[Onion Routing]] (layered [[Sphinx Routing]]), [[Payment Channel]] lifecycle, and cooperative/force-close flows.
    - Implements [[Trampoline Routing]], [[Splicing]], [[Anchor Outputs]], and [[BOLT 12]] offers.
    - Exposes a JSON-RPC API and is used as the backend powering the Phoenix wallet's server-side component.
  - #### Phoenix Wallet
    - A [[Non-Custodial Wallet]] iOS and Android application connecting to ACINQ's node as a trampoline hub.
    - Users retain control of their private keys; channels are opened on demand using [[Splicing]] so a single channel can expand or contract without on-chain closure.
    - Introduced the concept of a "zero-configuration" Lightning experience: inbound [[Channel Liquidity]] is provisioned automatically via a "dual-funded channel" or [[Splicing]] mechanism.
    - Operates over [[Tor]] for enhanced privacy on supported platforms.
  - #### Trampoline Routing
    - An ACINQ-pioneered protocol extension allowing a mobile [[Lightning Network]] node to delegate route calculation to one or more intermediate "trampoline" nodes.
    - Reduces the requirement for clients to maintain a full [[Gossip Protocol]] routing table, critical for battery-constrained mobile devices.
    - Preserves payment privacy through nested [[Onion Routing]] packets.
  - #### Splicing
    - A protocol feature enabling the resizing of [[Payment Channel]] capacity — both increasing and decreasing — via a single on-chain transaction, while keeping the channel live for routing during the splice confirmation.
    - ACINQ implemented splicing in Eclair and Phoenix before it was widely supported across implementations.

- ### Applications and Use Cases
  - **Mobile [[Bitcoin]] Payments** — Phoenix provides a user-friendly entry point to the [[Lightning Network]] without requiring users to manage channel balances manually, targeting everyday retail and remittance use cases.
  - **Institutional Routing Infrastructure** — Eclair is deployed by exchanges, payment processors, and [[Bitcoin Layer 2]] service providers that need a reliable, high-throughput [[Routing Node]].
  - **Developer Tooling** — The `eclair-core` library exposes Lightning primitives for integration into third-party applications, making ACINQ a building block in the broader Lightning developer ecosystem.
  - **[[Micropayment]] Streaming** — The sub-satoshi precision of Lightning [[HTLC]] settlements enables value-streaming applications such as pay-per-second media or API metering; ACINQ's infrastructure serves as a routing backbone for these flows.
  - **Cross-border Remittance** — Phoenix's fee-transparent model (swap-in/swap-out via [[Splicing]]) makes it suitable for low-value international transfers that are uneconomical on-chain.
  - **Proof-of-Concept for Protocol Innovation** — ACINQ uses Phoenix as a live testbed for new BOLT features before they are standardised, meaning millions of Phoenix users act as a real-world deployment surface for emerging Lightning protocol capabilities.

- ### Mechanisms
  - **[[HTLC]] (Hashed Time-Locked Contract)** — The atomic swap primitive used to route payments through multi-hop [[Payment Channel]] paths; each hop holds funds conditionally pending a cryptographic pre-image reveal within a time window.
  - **[[Onion Routing]] / [[Sphinx Routing]]** — Payments traverse the network inside layered encrypted packets; each routing node decrypts only the layer addressed to it, preserving sender and recipient privacy.
  - **[[Channel Liquidity]] Management** — ACINQ's node deploys automated liquidity management: closing depleted channels, rebalancing via circular payments, and using [[Splicing]] to top up without channel closure.
  - **[[Gossip Protocol]]** — Lightning nodes broadcast channel announcements and updates; ACINQ's node aggregates this [[Peer-to-Peer Network]] gossip to maintain a network-wide routing graph used by the Eclair path-finder.
  - **[[Anchor Outputs]]** — A channel construction technique (adopted after SegWit) that allows fee-bumping of commitment transactions post-broadcast via Child-Pays-for-Parent (CPFP), improving safety under fee-market volatility.
  - **[[BOLT 12]] Offers** — A reusable, static payment descriptor format that replaces single-use Lightning invoices; ACINQ has been an early implementer and advocate.

- ### Relationships
  - hasPart:: [[Eclair]]
  - hasPart:: [[Phoenix Wallet]]
  - partOf:: [[Lightning Network Ecosystem]]
  - requires:: [[Bitcoin]]
  - requires:: [[Payment Channel]]
  - enables:: [[Micropayment]]
  - enables:: [[Trampoline Routing]]
  - enables:: [[Non-Custodial Wallet]]
  - implements:: [[Basis of Lightning Technology]]
  - implements:: [[Sphinx Routing]]
  - uses:: [[HTLC]]
  - uses:: [[Onion Routing]]
  - uses:: [[Scala Programming Language]]
  - supports:: [[Bitcoin Payment]]
  - supports:: [[Splicing]]
  - contrastsWith:: [[LND]]
  - contrastsWith:: [[Core Lightning]]
  - relatedTo:: [[Bitcoin Layer 2]]
  - relatedTo:: [[Routing Node]]
  - relatedTo:: [[Channel Liquidity]]
  - relatedTo:: [[BOLT 12]]
  - bridges-to:: [[Decentralised Finance]]
  - bridges-to:: [[Peer-to-Peer Network]]

- ### Standards & Context
  - ACINQ participates actively in the **BOLT (Basis of Lightning Technology)** specification process, which is the de-facto standards body for the [[Lightning Network]] protocol.
  - Key BOLTs relevant to ACINQ's work:
    - **BOLT 1** — Base protocol message framing.
    - **BOLT 2** — [[Payment Channel]] establishment and operation.
    - **BOLT 4** — [[Sphinx Routing]] / [[Onion Routing]] packet format.
    - **BOLT 7** — [[Gossip Protocol]] for peer and channel discovery.
    - **BOLT 11** — Invoice encoding (the widely used `lnbc...` format).
    - **BOLT 12** — Offers protocol (reusable static invoices), championed by ACINQ and others.
  - The [[Bitcoin]] base layer provides settlement guarantees via [[Bitcoin Proof-of-Work Protocol]] consensus; ACINQ's stack relies on [[SegWit]] transaction outputs (P2WSH / P2TR) for channel script constructs.
  - As a French company (SAS legal form), ACINQ operates under EU financial regulation; self-custodial wallet design is partly motivated by regulatory positioning to avoid acting as a custodian.
  - The Lightning Network as a whole operates without a central standards organisation; BOLTs are maintained collaboratively by ACINQ, Lightning Labs, Blockstream, and independent contributors via GitHub.

- ### Provenance
  - sources:: ACINQ GitHub (https://github.com/ACINQ), BOLT specification repository (https://github.com/lightning/bolts), ACINQ.fr public node profile
  - updated:: 2026-06-13
