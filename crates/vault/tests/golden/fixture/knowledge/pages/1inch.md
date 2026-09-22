---
public: true
---

# 1inch
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:b4284e8dbb926c35f84ee43a5cb9b1c4316f7864638716d2a7bc56d54a269e4c",
  "@type": "Page",
  "vc:slug": "1-inch",
  "title": "1inch",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:automated-market-maker",
      "vc:label": "Automated Market Maker"
    },
    {
      "@id": "urn:visionflow:linked:smart-contract",
      "vc:label": "Smart Contract"
    },
    {
      "@id": "urn:visionflow:linked:ethereum",
      "vc:label": "Ethereum"
    },
    {
      "@id": "urn:visionflow:linked:liquidity-pool",
      "vc:label": "Liquidity Pool"
    },
    {
      "@id": "urn:visionflow:linked:uniswap",
      "vc:label": "Uniswap"
    },
    {
      "@id": "urn:visionflow:linked:decentralized-exchange",
      "vc:label": "Decentralized Exchange"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "preferred-term",
      "vc:value": "1inch"
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
  "@id": "urn:ngm:class:1-inch",
  "@type": "Class",
  "label": "1inch",
  "definition": "A decentralised exchange aggregator that routes trades across multiple liquidity sources to find favourable execution prices on Ethereum and other chains.",
  "domain": "blockchain",
  "maturity": "established",
  "qualityScore": 0.7,
  "subClassOf": [
    {
      "@id": "urn:ngm:class:decentralized-exchange",
      "label": "Decentralized Exchange"
    }
  ],
  "relations": {
    "requires": [
      {
        "@id": "urn:ngm:class:automated-market-maker",
        "label": "Automated Market Maker"
      },
      {
        "@id": "urn:ngm:class:smart-contract",
        "label": "Smart Contract"
      },
      {
        "@id": "urn:ngm:class:ethereum-smart-contract-platform",
        "label": "Ethereum Smart Contract Platform"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:liquidity-pool",
        "label": "Liquidity Pool"
      }
    ]
  },
  "quality": 0.6,
  "provenance": {
    "attributedTo": "did:nostr:lcr-swarm",
    "generatedAt": "2026-05-29T00:00:00Z",
    "inferenceRule": "GapFillTier5"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:annotation:link-resolutions:1-inch:08c8ad9ad7cc",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:b4284e8dbb926c35f84ee43a5cb9b1c4316f7864638716d2a7bc56d54a269e4c"
  },
  "vc:resolutions": [
    {
      "raw": "[[Automated Market Maker]]",
      "resolved": "urn:visionflow:linked:automated-market-maker",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Smart Contract]]",
      "resolved": "urn:visionflow:linked:smart-contract",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Ethereum Smart Contract Platform]]",
      "resolved": "urn:visionflow:linked:ethereum",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Liquidity Pool]]",
      "resolved": "urn:visionflow:linked:liquidity-pool",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Uniswap]]",
      "resolved": "urn:visionflow:linked:uniswap",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Decentralized Exchange]]",
      "resolved": "urn:visionflow:linked:decentralized-exchange",
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
  - A decentralised exchange aggregator that routes trades across multiple liquidity sources to find favourable execution prices on Ethereum and other chains.

- ### Semantic Classification
  - owl-class:: blockchain:1inch
  - owl-role:: Class

- ### Relationships
  - is-subclass-of:: [[Decentralized Exchange]]
  - bridges-to:: [[Uniswap]]
  - requires:: [[Automated Market Maker]], [[Smart Contract]], [[Ethereum Smart Contract Platform]]
  - enables:: [[Liquidity Pool]]

- ### Content
  - 1inch is a DeFi protocol that aggregates liquidity from many decentralised exchanges and automated market makers, splitting orders across venues to reduce price impact and improve effective execution. Its routing algorithm searches available pools to construct a path for each trade.
  - Beyond aggregation, 1inch offers its own liquidity protocol and limit order functionality, and it is governed by a token-based decentralised organisation. It operates across several EVM-compatible networks.

- ### Provenance
  - sources::
  - migration-date:: 2026-05-29T00:00:00Z
