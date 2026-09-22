---
public: true
---

# 3D Scanning
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:3-d-scanning",
  "@type": "Page",
  "vc:slug": "3-d-scanning",
  "title": "3D Scanning",
  "vc:public": true,
  "vc:outboundWikilinks": [],
  "vc:schemaVersion": 2
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:3-d-scanning",
  "@type": "Class",
  "label": "3D Scanning",
  "definition": "3D Scanning is the process of capturing the three-dimensional shape, and optionally the colour and texture, of real-world objects, people, or environments using hardware such as structured-light scanners, time-of-flight LiDAR, photogrammetry rigs, or depth cameras, producing digital point clouds or meshes that represent the physical source. The resulting data feeds into digital preservation, reverse engineering, visual-effects production, quality inspection, and spatial computing pipelines. Accuracy, resolution, and scan volume are the primary quality axes that distinguish scanning technologies.",
  "domain": "spatial-computing",
  "maturity": "established",
  "subClassOf": {
    "@id": "urn:ngm:class:3-d-reconstruction",
    "label": "3D Reconstruction"
  },
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:point-cloud",
        "label": "Point Cloud"
      },
      {
        "@id": "urn:ngm:class:mesh-generation",
        "label": "Mesh Generation"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:depth-sensor",
        "label": "Depth Sensor"
      },
      {
        "@id": "urn:ngm:class:calibration",
        "label": "Calibration"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:3-d-li-dar",
        "label": "3D LiDAR"
      },
      {
        "@id": "urn:ngm:class:photogrammetry",
        "label": "Photogrammetry"
      },
      {
        "@id": "urn:ngm:class:structured-light",
        "label": "Structured Light"
      },
      {
        "@id": "urn:ngm:class:time-of-flight-sensor",
        "label": "Time-of-Flight Sensor"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:3-d-scene-reconstruction",
        "label": "3D Scene Reconstruction"
      },
      {
        "@id": "urn:ngm:class:digital-twin-creation",
        "label": "Digital Twin Creation"
      },
      {
        "@id": "urn:ngm:class:point-cloud-processing",
        "label": "Point Cloud Processing"
      },
      {
        "@id": "urn:ngm:class:reverse-engineering",
        "label": "Reverse Engineering"
      },
      {
        "@id": "urn:ngm:class:digital-heritage",
        "label": "Heritage Digitisation"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:building-information-modelling",
        "label": "Building Information Modelling"
      },
      {
        "@id": "urn:ngm:class:quality-inspection",
        "label": "Quality Inspection"
      },
      {
        "@id": "urn:ngm:class:augmented-reality",
        "label": "Augmented Reality"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:structure-from-motion",
        "label": "Structure-from-Motion"
      },
      {
        "@id": "urn:ngm:class:neural-radiance-field",
        "label": "Neural Radiance Field"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:robotics-perception",
        "label": "Robotics Perception"
      },
      {
        "@id": "urn:ngm:class:medical-imaging",
        "label": "Medical Imaging"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:3-d-perception",
        "label": "3D Perception"
      },
      {
        "@id": "urn:ngm:class:gaussian-splatting",
        "label": "Gaussian Splatting"
      },
      {
        "@id": "urn:ngm:class:simultaneous-localisation-and-mapping",
        "label": "Simultaneous Localisation and Mapping"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:3-d-digitisation",
      "label": "3D Digitisation"
    },
    {
      "@id": "urn:ngm:class:3-d-capture",
      "label": "3D Capture"
    }
  ],
  "quality": 0.72,
  "provenance": {
    "attributedTo": "did:nostr:ontology-mesh",
    "generatedAt": "2026-06-14T00:00:00Z",
    "inferenceRule": "RelationEnrichment"
  }
}
```

- ### Definition
  - 3D Scanning is the systematic capture of real-world geometry using active or passive sensing methods to produce digital [[Point Cloud]] or mesh representations. Active methods include structured light (projecting known patterns and reading deformation), time-of-flight [[3D LiDAR]], and laser triangulation scanners, while passive methods rely on [[Photogrammetry]] — computing geometry from multiple overlapping images. The resulting data enables [[3D Scene Reconstruction]] for documentation, reverse engineering, film production, and [[Digital Twin Creation]]. [[Point Cloud Processing]] pipelines refine raw scan data into usable geometry through filtering, registration, and meshing stages.

- ### Relationships
  - 3D Scanning uses [[3D LiDAR]] for large-scale outdoor capture, [[Photogrammetry]] for image-based reconstruction, and [[Depth Sensor]] hardware for close-range scanning. It enables [[3D Scene Reconstruction]] of environments, supports [[Digital Twin Creation]] from physical assets, and produces data for [[Point Cloud Processing]]. The technology is closely related to the [[Point Cloud]] data format, [[Structure-from-Motion]] algorithms, and [[3D Perception]] systems that interpret scanned representations in real time.

- ### Content
  - Industrial 3D scanning began with coordinate measuring machines (CMMs) in the 1960s and early laser triangulation scanners developed for aerospace quality control in the 1970s–1980s. The first commercial structured-light scanners appeared in the 1990s, enabling sub-millimetre accuracy for reverse engineering and medical prosthetics. Ground-penetrating and airborne LiDAR scanning were adopted for large-scale topographic survey and archaeological documentation from the late 1990s onwards.

  - Modern 3D scanning technologies span a resolution and range continuum: handheld structured-light scanners (e.g., Artec Eva) capture objects up to a few metres with sub-millimetre precision; terrestrial LiDAR systems (e.g., Leica BLK, FARO Focus) scan architectural spaces to centimetre accuracy at ranges of hundreds of metres; and aerial/mobile mapping rigs mount LiDAR alongside GNSS/IMU for kilometre-scale terrain capture. Photogrammetry using drone imagery and consumer cameras has become a high-throughput complement, producing textured meshes from photo collections with software such as Agisoft Metashape and RealityCapture.

  - Applications of 3D scanning span heritage preservation (scanning of artefacts and monuments before restoration or replication), film and games production (body and face scanning for digital doubles), construction and BIM (as-built capture for project verification), automotive and aerospace quality control, and medical imaging (orthopaedic implant fitting and surgical planning). The scan-to-BIM workflow, which converts point-cloud data into parametric building information models, has become a standard practice in architecture and facilities management.

  - In 2024–2025, smartphone-class depth sensing (using LiDAR on iPhone and iPad Pro, and structured-light face scanners) has democratised basic 3D scanning. Gaussian splatting and NeRF-based reconstruction tools allow high-quality captures from video sequences without dedicated scanning hardware. AI-driven hole-filling and noise-reduction are incorporated into standard post-processing tools. Integration between scanning platforms and digital-twin management software is maturing, and real-time collaborative scanning workflows are enabling distributed capture projects.

