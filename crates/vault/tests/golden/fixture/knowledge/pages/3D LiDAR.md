---
public: true
---

# 3D LiDAR
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:bc54d8d8625320088b1e463caf4a4e51899c9ed028d3984cb89af781213f59dc",
  "@type": "Page",
  "vc:slug": "3-d-li-dar",
  "title": "3D LiDAR",
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
      "vc:value": "RB-0163"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "3D LiDAR"
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
  "@id": "urn:ngm:class:3-d-li-dar",
  "@type": "Class",
  "label": "3D LiDAR",
  "definition": "3D LiDAR is a active ranging sensor that emits pulsed laser light across multiple vertical channels to capture dense three-dimensional point clouds of the surrounding environment. Spinning or solid-state variants measure range and intensity for hundreds of thousands of points per second, enabling robots and autonomous vehicles to perform obstacle detection, SLAM, and high-fidelity scene reconstruction in real time.",
  "domain": "robotics",
  "maturity": "established",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:robo-perception",
      "label": "Perception and Sensing"
    }
  ],
  "relations": {
    "enables": [
      {"@id": "urn:ngm:class:point-cloud", "label": "Point Cloud"},
      {"@id": "urn:ngm:class:slam", "label": "SLAM"},
      {"@id": "urn:ngm:class:simultaneous-localisation-and-mapping", "label": "Simultaneous Localisation and Mapping"},
      {"@id": "urn:ngm:class:autonomous-navigation", "label": "Autonomous Navigation"}
    ],
    "uses": [
      {"@id": "urn:ngm:class:sensor-fusion", "label": "Sensor Fusion"},
      {"@id": "urn:ngm:class:perception-system", "label": "Perception System"}
    ],
    "relatedTo": [
      {"@id": "urn:ngm:class:2-d-li-dar", "label": "2D LiDAR"},
      {"@id": "urn:ngm:class:lidar-scanning", "label": "Lidar Scanning"},
      {"@id": "urn:ngm:class:stereo-camera", "label": "Stereo Camera"},
      {"@id": "urn:ngm:class:multirotor-uav", "label": "Multirotor UAV"}
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
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-li-dar:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:bc54d8d8625320088b1e463caf4a4e51899c9ed028d3984cb89af781213f59dc"
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
  - 3D LiDAR generates three-dimensional point cloud of environment.

- ### Semantic Classification
  - owl-class:: robotics:Lidar3d
  - owl-role:: Concept
  - belongs-to-domain:: [[Robotics]]

- ### Relationships
  - is-subclass-of:: [[Lidar]]

- ### Content
  3D LiDAR — content pending enrichment.

- ### Provenance
  - sources:: Chimera Prime Research
  - migration-date:: 2026-04-26T00:00:00Z
