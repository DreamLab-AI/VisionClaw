---
public: true
---

# 6G Network Slice
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:69d3ae05f1d4395dcb4c424b9f74acb97db4b62a15f5c8f76854c1bba942c934",
  "@type": "Page",
  "vc:slug": "6-g-network-slice",
  "title": "6G Network Slice",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:3-gpp-release-21",
      "vc:label": "3GPP Release 21"
    },
    {
      "@id": "urn:visionflow:linked:6-g-network-infrastructure",
      "vc:label": "6G Network Infrastructure"
    },
    {
      "@id": "urn:visionflow:linked:dynamic-resource-allocation",
      "vc:label": "Dynamic Resource Allocation"
    },
    {
      "@id": "urn:visionflow:linked:etsi-eni-008",
      "vc:label": "ETSI ENI 008"
    },
    {
      "@id": "urn:visionflow:linked:low-latency-service",
      "vc:label": "Low Latency Service"
    },
    {
      "@id": "urn:visionflow:linked:network-slicing-orchestrator",
      "vc:label": "Network Slicing Orchestrator"
    },
    {
      "@id": "urn:visionflow:linked:qo-s-policy",
      "vc:label": "QoS Policy"
    },
    {
      "@id": "urn:visionflow:linked:resource-allocation-unit",
      "vc:label": "Resource Allocation Unit"
    },
    {
      "@id": "urn:visionflow:linked:sdn-controller",
      "vc:label": "SDN Controller"
    },
    {
      "@id": "urn:visionflow:linked:service-level-agreement",
      "vc:label": "Service Level Agreement"
    },
    {
      "@id": "urn:visionflow:linked:traffic-classifier",
      "vc:label": "Traffic Classifier"
    },
    {
      "@id": "urn:visionflow:linked:workload-isolation",
      "vc:label": "Workload Isolation"
    },
    {
      "@id": "urn:visionflow:owl:class:guaranteed-bandwidth",
      "vc:label": "Guaranteed Bandwidth"
    },
    {
      "@id": "urn:visionflow:owl:class:infrastructure",
      "vc:label": "InfrastructureDomain"
    },
    {
      "@id": "urn:visionflow:owl:class:network-layer",
      "vc:label": "NetworkLayer"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "20140"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "6G Network Slice"
    }
  ],
  "prov:wasAttributedTo": {
    "@id": "did:nostr:jjohare"
  },
  "prov:generatedAtTime": {
    "@value": "2026-05-18T07:12:05Z",
    "@type": "xsd:dateTime"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:6-g-network-slice",
  "@type": "Class",
  "label": "6G Network Slice",
  "definition": "Virtual partition of 6G infrastructure guaranteeing specified quality-of-service levels for immersive workloads through isolated resource allocation.",
  "domain": "infrastructure",
  "maturity": "draft",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:infra-network-and-comms",
      "label": "Network and Communication"
    }
  ],
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:resource-allocation",
        "label": "Resource Allocation Unit"
      },
      {
        "@id": "urn:ngm:class:service-level-agreement",
        "label": "Service Level Agreement"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:network-infrastructure",
        "label": "6G Network Infrastructure"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:resource-allocation",
        "label": "Dynamic Resource Allocation"
      },
      {
        "@id": "urn:ngm:class:guaranteed-bandwidth",
        "label": "Guaranteed Bandwidth"
      }
    ]
  },
  "quality": 0.35,
  "provenance": {
    "attributedTo": "did:nostr:lcr-swarm",
    "generatedAt": "2026-05-18T07:12:05Z",
    "inferenceRule": "R5DomainRootFallback"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:annotation:link-resolutions:6-g-network-slice:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:69d3ae05f1d4395dcb4c424b9f74acb97db4b62a15f5c8f76854c1bba942c934"
  },
  "vc:resolutions": [
    {
      "raw": "[[3GPP Release 21]]",
      "resolved": "urn:visionflow:linked:3-gpp-release-21",
      "kind": "StubLink"
    },
    {
      "raw": "[[6G Network Infrastructure]]",
      "resolved": "urn:visionflow:linked:6-g-network-infrastructure",
      "kind": "StubLink"
    },
    {
      "raw": "[[Dynamic Resource Allocation]]",
      "resolved": "urn:visionflow:linked:dynamic-resource-allocation",
      "kind": "StubLink"
    },
    {
      "raw": "[[ETSI ENI 008]]",
      "resolved": "urn:visionflow:linked:etsi-eni-008",
      "kind": "StubLink"
    },
    {
      "raw": "[[Low Latency Service]]",
      "resolved": "urn:visionflow:linked:low-latency-service",
      "kind": "StubLink"
    },
    {
      "raw": "[[Network Slicing Orchestrator]]",
      "resolved": "urn:visionflow:linked:network-slicing-orchestrator",
      "kind": "StubLink"
    },
    {
      "raw": "[[QoS Policy]]",
      "resolved": "urn:visionflow:linked:qo-s-policy",
      "kind": "StubLink"
    },
    {
      "raw": "[[Resource Allocation Unit]]",
      "resolved": "urn:visionflow:linked:resource-allocation-unit",
      "kind": "StubLink"
    },
    {
      "raw": "[[SDN Controller]]",
      "resolved": "urn:visionflow:linked:sdn-controller",
      "kind": "StubLink"
    },
    {
      "raw": "[[Service Level Agreement]]",
      "resolved": "urn:visionflow:linked:service-level-agreement",
      "kind": "StubLink"
    },
    {
      "raw": "[[Traffic Classifier]]",
      "resolved": "urn:visionflow:linked:traffic-classifier",
      "kind": "StubLink"
    },
    {
      "raw": "[[Workload Isolation]]",
      "resolved": "urn:visionflow:linked:workload-isolation",
      "kind": "StubLink"
    },
    {
      "raw": "[[Guaranteed Bandwidth]]",
      "resolved": "urn:visionflow:owl:class:guaranteed-bandwidth",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[InfrastructureDomain]]",
      "resolved": "urn:visionflow:owl:class:infrastructure",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[NetworkLayer]]",
      "resolved": "urn:visionflow:owl:class:network-layer",
      "kind": "ResolvedLink"
    }
  ],
  "prov:wasAttributedTo": {
    "@id": "did:nostr:lcr-swarm"
  },
  "prov:generatedAtTime": {
    "@value": "2026-05-18T07:12:05Z",
    "@type": "xsd:dateTime"
  }
}
```


- ### Definition
  - Virtual partition of 6G infrastructure guaranteeing specified quality-of-service levels for immersive workloads through isolated resource allocation.

- ### Semantic Classification
  - owl-class:: infrastructure:SixGNetworkSlice
  - owl-role:: Object
  - owl-inferred:: infrastructure:VirtualObject
  - belongs-to-domain:: [[InfrastructureDomain]]
  - implemented-in-layer:: [[NetworkLayer]]

- ### Relationships
  - has-part:: [[QoS Policy]], [[Resource Allocation Unit]], [[Service Level Agreement]], [[Traffic Classifier]]
  - requires:: [[6G Network Infrastructure]], [[Network Slicing Orchestrator]], [[SDN Controller]]
  - enables:: [[Low Latency Service]], [[Guaranteed Bandwidth]], [[Workload Isolation]], [[Dynamic Resource Allocation]]

- ### Content
  6G Network Slice — content pending enrichment.

- ### Provenance
  - sources:: [[3GPP Release 21]], [[ETSI ENI 008]]
  - migration-date:: 2026-04-26T00:00:00Z
