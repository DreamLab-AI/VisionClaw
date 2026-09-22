---
public: true
---

# 3D Reconstruction
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:45c99e53e3a503b2c170936a5b97d46f4c73c5007d0b6eb74d4a0e7a7513bd86",
  "@type": "Page",
  "vc:slug": "3-d-reconstruction",
  "title": "3D Reconstruction",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:camera-calibration",
      "vc:label": "Camera Calibration"
    },
    {
      "@id": "urn:visionflow:linked:environmental-mapping",
      "vc:label": "Environmental Mapping"
    },
    {
      "@id": "urn:visionflow:linked:feature-matching",
      "vc:label": "Feature Matching"
    },
    {
      "@id": "urn:visionflow:linked:image-processing",
      "vc:label": "Image Processing"
    },
    {
      "@id": "urn:visionflow:linked:point-cloud-generation",
      "vc:label": "Point Cloud Generation"
    },
    {
      "@id": "urn:visionflow:linked:real-world-digitisation",
      "vc:label": "Real-world Digitisation"
    },
    {
      "@id": "urn:visionflow:linked:structure-from-motion",
      "vc:label": "Structure-from-Motion"
    },
    {
      "@id": "urn:visionflow:owl:class:computer-vision",
      "vc:label": "Computer Vision"
    },
    {
      "@id": "urn:visionflow:owl:class:digital-twin",
      "vc:label": "Digital Twin"
    },
    {
      "@id": "urn:visionflow:owl:class:photogrammetry",
      "vc:label": "Photogrammetry"
    },
    {
      "@id": "urn:visionflow:owl:class:point-cloud",
      "vc:label": "Point Cloud"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "MV-9506"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "3D Reconstruction"
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
  "@id": "urn:ngm:class:3-d-reconstruction",
  "@type": "Class",
  "label": "3D Reconstruction",
  "definition": "3D Reconstruction is the computational process of recovering three-dimensional geometric and structural information from multiple 2D images or sensor data (such as LiDAR or depth cameras) using techniques including Computer Vision, photogrammetry, and Structure-from-Motion (SfM), enabling digital capture of real-world objects and environments for Digital Twin creation, immersive environment mapping, and spatial analysis.",
  "domain": "spatial-computing",
  "maturity": "draft",
  "subClassOf": {
    "@id": "urn:ngm:class:sc-content-and-assets",
    "label": "Content and Assets"
  },
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:structure-from-motion",
        "label": "Structure-from-Motion"
      },
      {
        "@id": "urn:ngm:class:multi-view-stereo",
        "label": "Multi-View Stereo"
      },
      {
        "@id": "urn:ngm:class:point-cloud",
        "label": "Point Cloud"
      },
      {
        "@id": "urn:ngm:class:depth-estimation",
        "label": "Depth Estimation"
      }
    ],
    "partOf": [
      {
        "@id": "urn:ngm:class:photogrammetry",
        "label": "Photogrammetry"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:camera-calibration",
        "label": "Camera Calibration"
      },
      {
        "@id": "urn:ngm:class:feature-matching",
        "label": "Feature Matching"
      },
      {
        "@id": "urn:ngm:class:image-processing",
        "label": "Image Processing"
      },
      {
        "@id": "urn:ngm:class:sensor-fusion",
        "label": "Sensor Fusion"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:environmental-mapping",
        "label": "Environmental Mapping"
      },
      {
        "@id": "urn:ngm:class:point-cloud-generation",
        "label": "Point Cloud Generation"
      },
      {
        "@id": "urn:ngm:class:digital-twin",
        "label": "Digital Twin"
      },
      {
        "@id": "urn:ngm:class:augmented-reality",
        "label": "Augmented Reality"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:lidar",
        "label": "Lidar"
      },
      {
        "@id": "urn:ngm:class:neural-radiance-field",
        "label": "Neural Radiance Field"
      },
      {
        "@id": "urn:ngm:class:simultaneous-localisation-and-mapping",
        "label": "Simultaneous Localisation and Mapping"
      }
    ],
    "dependsOn": [
      {
        "@id": "urn:ngm:class:computer-vision",
        "label": "Computer Vision"
      },
      {
        "@id": "urn:ngm:class:deep-learning",
        "label": "Deep Learning"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:spatial-computing",
        "label": "Spatial Computing"
      },
      {
        "@id": "urn:ngm:class:scene-understanding",
        "label": "Scene Understanding"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:computer-vision",
        "label": "Computer Vision"
      },
      {
        "@id": "urn:ngm:class:robotics",
        "label": "Robotics"
      },
      {
        "@id": "urn:ngm:class:machine-learning",
        "label": "Machine Learning"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:3-d-scene-reconstruction",
      "label": "3D Scene Reconstruction"
    },
    {
      "@id": "urn:ngm:class:volumetric-reconstruction",
      "label": "Volumetric Reconstruction"
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

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-reconstruction:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:45c99e53e3a503b2c170936a5b97d46f4c73c5007d0b6eb74d4a0e7a7513bd86"
  },
  "vc:resolutions": [
    {
      "raw": "[[Camera Calibration]]",
      "resolved": "urn:visionflow:linked:camera-calibration",
      "kind": "StubLink"
    },
    {
      "raw": "[[Environmental Mapping]]",
      "resolved": "urn:visionflow:linked:environmental-mapping",
      "kind": "StubLink"
    },
    {
      "raw": "[[Feature Matching]]",
      "resolved": "urn:visionflow:linked:feature-matching",
      "kind": "StubLink"
    },
    {
      "raw": "[[Image Processing]]",
      "resolved": "urn:visionflow:linked:image-processing",
      "kind": "StubLink"
    },
    {
      "raw": "[[Point Cloud Generation]]",
      "resolved": "urn:visionflow:linked:point-cloud-generation",
      "kind": "StubLink"
    },
    {
      "raw": "[[Real-world Digitisation]]",
      "resolved": "urn:visionflow:linked:real-world-digitisation",
      "kind": "StubLink"
    },
    {
      "raw": "[[Structure-from-Motion]]",
      "resolved": "urn:visionflow:linked:structure-from-motion",
      "kind": "StubLink"
    },
    {
      "raw": "[[Computer Vision]]",
      "resolved": "urn:visionflow:owl:class:computer-vision",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Digital Twin]]",
      "resolved": "urn:visionflow:owl:class:digital-twin",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Photogrammetry]]",
      "resolved": "urn:visionflow:owl:class:photogrammetry",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Point Cloud]]",
      "resolved": "urn:visionflow:owl:class:point-cloud",
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
  - 3D Reconstruction is the computational process of recovering three-dimensional geometric and structural information from multiple 2D images or sensor data (such as LiDAR or depth cameras) using techniques including [[Computer Vision]], photogrammetry, and Structure-from-Motion (SfM). This technology enables digital capture of real-world objects and environments for [[Digital Twin]] creation and immersive environment mapping.

- ### Semantic Classification
  - owl-class:: spatial-computing:3DReconstruction
  - owl-role:: concept

- ### Relationships
  - is-subclass-of:: [[Computer Vision]]
  - bridges-to:: [[Computer Vision]], [[Photogrammetry]], [[Digital Twin]]
  - requires:: [[Image Processing]], [[Camera Calibration]], [[Feature Matching]]
  - enables:: [[Real-world Digitisation]], [[Environmental Mapping]], [[Point Cloud Generation]]

- ### Content

  ## Overview

  3D Reconstruction bridges the physical and digital worlds by algorithmically deriving 3D structure from 2D observations. Key methodologies include Structure-from-Motion (recovering both structure and camera motion from video), multi-view stereo (dense depth estimation), and sensor fusion combining multiple data streams.

  ## Primary Techniques
  - **Structure-from-Motion**: Extracting 3D geometry from overlapping photographs and calculating camera trajectories
  - **Multi-View Stereo (MVS)**: Dense depth estimation by analysing matching pixels across multiple images
  - **Photogrammetry**: Professional 3D capture using calibrated imaging workflows
  - **LiDAR Scanning**: Direct depth measurement using laser time-of-flight sensors
  - **Depth Sensors**: Real-time 3D acquisition via structured light or time-of-flight cameras

  ## Applications
  - **Heritage Digitisation**: Preserving cultural artefacts and archaeological sites
  - **Architectural Scanning**: Creating as-built models of buildings
  - **Industrial Inspection**: Quality control through precise dimensional analysis
  - **Real Estate**: Virtual property tours via captured environments

  #### Related Concepts
  - [[Computer Vision]], [[Photogrammetry]], [[Point Cloud]], [[Digital Twin]], [[Structure-from-Motion]]

- ### Provenance
  - sources::
  - migration-date:: 2026-04-26T00:00:00Z
