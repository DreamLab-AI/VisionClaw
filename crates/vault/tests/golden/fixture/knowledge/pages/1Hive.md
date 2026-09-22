---
public: true
---

# 1Hive
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:b82f7b7212d41b4decc443e2c3dd1e81f069ea37fd51a1122cc706b17dde51a5",
  "@type": "Page",
  "vc:slug": "1-hive",
  "title": "1Hive",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:decentralized-autonomous-organization",
      "vc:label": "Decentralized Autonomous Organization"
    },
    {
      "@id": "urn:visionflow:linked:governance-token",
      "vc:label": "Governance Token"
    },
    {
      "@id": "urn:visionflow:linked:quadratic-funding",
      "vc:label": "Quadratic Funding"
    },
    {
      "@id": "urn:visionflow:linked:liquid-democracy",
      "vc:label": "Liquid Democracy"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "preferred-term",
      "vc:value": "1Hive"
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
  "@id": "urn:ngm:class:1-hive",
  "@type": "Class",
  "label": "1Hive",
  "definition": "A decentralised community and DAO that issues the Honey community currency and develops governance tooling such as the Gardens framework for conviction voting.",
  "domain": "governance",
  "maturity": "established",
  "qualityScore": 0.7,
  "subClassOf": [
    {
      "@id": "urn:ngm:class:decentralized-autonomous-organization",
      "label": "Decentralized Autonomous Organization"
    }
  ],
  "relations": {
    "requires": [
      {
        "@id": "urn:ngm:class:decentralized-autonomous-organization",
        "label": "Decentralized Autonomous Organization"
      },
      {
        "@id": "urn:ngm:class:governance-token",
        "label": "Governance Token"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:quadratic-funding",
        "label": "Quadratic Funding"
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
  "@id": "urn:visionflow:annotation:link-resolutions:1-hive:5551dc3bd123",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:b82f7b7212d41b4decc443e2c3dd1e81f069ea37fd51a1122cc706b17dde51a5"
  },
  "vc:resolutions": [
    {
      "raw": "[[Decentralized Autonomous Organization]]",
      "resolved": "urn:visionflow:linked:decentralized-autonomous-organization",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Governance Token]]",
      "resolved": "urn:visionflow:linked:governance-token",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Quadratic Funding]]",
      "resolved": "urn:visionflow:linked:quadratic-funding",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Liquid Democracy]]",
      "resolved": "urn:visionflow:linked:liquid-democracy",
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
  - A decentralised community and DAO that issues the Honey community currency and develops governance tooling such as the Gardens framework for conviction voting.

- ### Semantic Classification
  - owl-class:: governance:1Hive
  - owl-role:: Class

- ### Relationships
  - is-subclass-of:: [[Decentralized Autonomous Organization]]
  - bridges-to:: [[Liquid Democracy]]
  - requires:: [[Decentralized Autonomous Organization]], [[Governance Token]]
  - enables:: [[Quadratic Funding]]

- ### Content
  - 1Hive is an open community organised as a decentralised autonomous organisation that distributes a community currency called Honey and funds public goods through its own governance processes. Contributors propose and vote on initiatives using token-weighted mechanisms.
  - The community has produced governance tooling, including the Gardens framework that implements conviction voting, where support for a proposal accrues over time. This emphasises continuous community signalling over discrete ballots.

- ### Provenance
  - sources::
  - migration-date:: 2026-05-29T00:00:00Z
