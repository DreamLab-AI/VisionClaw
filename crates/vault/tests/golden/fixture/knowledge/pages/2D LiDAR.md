---
public: true
---

# 2D LiDAR
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:e3b06892a21a4e08f73d6d8b0e4f4ac671f722e441ab83cc96061dfb8b2d0469",
  "@type": "Page",
  "vc:slug": "2-d-li-dar",
  "title": "2D LiDAR",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:owl:class:lidar",
      "vc:label": "Lidar"
    },
    {
      "@id": "urn:visionflow:owl:class:robotics",
      "vc:label": "Robotics"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "RB-0162"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "2D LiDAR"
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
  "@id": "urn:ngm:class:2-d-li-dar",
  "@type": "Class",
  "label": "2D LiDAR",
  "definition": "2D LiDAR is a laser scanning sensor that emits a rotating beam in a single horizontal or vertical plane, producing a planar point cloud used for obstacle detection, proximity sensing, and 2D mapping. It is widely deployed on mobile robots and autonomous guided vehicles where full 3D sensing is unnecessary or cost-prohibitive.",
  "domain": "robotics",
  "maturity": "established",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:robo-perception",
      "label": "Perception and Sensing"
    }
  ],
  "relations": {
    "relatedTo": [
      {"@id": "urn:ngm:class:3-d-li-dar", "label": "3D LiDAR"},
      {"@id": "urn:ngm:class:slam", "label": "SLAM"},
      {"@id": "urn:ngm:class:point-cloud", "label": "Point Cloud"},
      {"@id": "urn:ngm:class:proximity-sensor", "label": "Proximity Sensor"},
      {"@id": "urn:ngm:class:laser-scanner", "label": "Laser Scanner"},
      {"@id": "urn:ngm:class:ultrasonic-sensor", "label": "Ultrasonic Sensor"}
    ],
    "uses": [
      {"@id": "urn:ngm:class:lidar-scanning", "label": "Lidar Scanning"}
    ],
    "enables": [
      {"@id": "urn:ngm:class:simultaneous-localisation-and-mapping", "label": "Simultaneous Localisation and Mapping"},
      {"@id": "urn:ngm:class:localisation", "label": "Localization"},
      {"@id": "urn:ngm:class:proximity-detection", "label": "Proximity Detection"}
    ],
    "partOf": [
      {"@id": "urn:ngm:class:robo-perception", "label": "Perception and Sensing"}
    ],
    "bridgesTo": [
      {"@id": "urn:ngm:class:lidar", "label": "Lidar"}
    ]
  },
  "quality": 0.8,
  "provenance": {
    "attributedTo": "did:nostr:jjohare",
    "generatedAt": "2026-05-18T07:12:05Z",
    "inferenceRule": "R1Explicit"
  }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:annotation:link-resolutions:2-d-li-dar:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:e3b06892a21a4e08f73d6d8b0e4f4ac671f722e441ab83cc96061dfb8b2d0469"
  },
  "vc:resolutions": [
    {
      "raw": "[[Lidar]]",
      "resolved": "urn:visionflow:owl:class:lidar",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Robotics]]",
      "resolved": "urn:visionflow:owl:class:robotics",
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
  - 2D LiDAR scans single horizontal or vertical plane for obstacle detection.

- ### Semantic Classification
  - owl-class:: robotics:Lidar2d
  - owl-role:: Concept
  - belongs-to-domain:: [[Robotics]]

- ### Relationships
  - is-subclass-of:: [[Lidar]]

- ### Content
  2D LiDAR — content pending enrichment.

- ### Provenance
  - sources:: Chimera Prime Research
  - migration-date:: 2026-04-26T00:00:00Z
