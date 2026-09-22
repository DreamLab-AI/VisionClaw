---
public: true
---

# 3D Model
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:0bd264303fc3ce01603d67dd2fcc53b898655b91b7a02a7b82d083e2a526b9f6",
  "@type": "Page",
  "vc:slug": "3-d-model",
  "title": "3D Model",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:3-d-rendering",
      "vc:label": "3D Rendering"
    },
    {
      "@id": "urn:visionflow:linked:interactive-visualization",
      "vc:label": "Interactive Visualization"
    },
    {
      "@id": "urn:visionflow:linked:material-definition",
      "vc:label": "Material Definition"
    },
    {
      "@id": "urn:visionflow:linked:mesh-data",
      "vc:label": "Mesh Data"
    },
    {
      "@id": "urn:visionflow:linked:virtual-environment-design",
      "vc:label": "Virtual Environment Design"
    },
    {
      "@id": "urn:visionflow:owl:class:3-d-development",
      "vc:label": "3D Development"
    },
    {
      "@id": "urn:visionflow:owl:class:3-d-file-format",
      "vc:label": "3D File Format"
    },
    {
      "@id": "urn:visionflow:owl:class:3-d-modeling",
      "vc:label": "3D Modeling"
    },
    {
      "@id": "urn:visionflow:owl:class:animation-retargeting",
      "vc:label": "Animation Retargeting"
    },
    {
      "@id": "urn:visionflow:owl:class:computer-vision",
      "vc:label": "Computer Vision"
    },
    {
      "@id": "urn:visionflow:owl:class:digital-asset",
      "vc:label": "Digital Asset"
    },
    {
      "@id": "urn:visionflow:owl:class:metaverse",
      "vc:label": "Metaverse"
    },
    {
      "@id": "urn:visionflow:owl:class:texture-mapping",
      "vc:label": "Texture Mapping"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "MV-9504"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "3D Model"
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
  "@id": "urn:ngm:class:3-d-model",
  "@type": "Class",
  "label": "3D Model",
  "definition": "A 3D Model is a digital representation of a three-dimensional object or environment constructed from vertices, edges, faces, and materials, encoded in standard formats such as glTF, OBJ, FBX, or USD, and used as a foundational asset in spatial computing, simulation, gaming, and metaverse applications.",
  "domain": "spatial-computing",
  "maturity": "draft",
  "subClassOf": {
    "@id": "urn:ngm:class:sc-content-and-assets",
    "label": "Content and Assets"
  },
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:mesh-data",
        "label": "Mesh Data"
      },
      {
        "@id": "urn:ngm:class:material-definition",
        "label": "Material Definition"
      },
      {
        "@id": "urn:ngm:class:texture-mapping",
        "label": "Texture Mapping"
      },
      {
        "@id": "urn:ngm:class:skeletal-rig",
        "label": "Skeletal Rig"
      }
    ],
    "partOf": [
      {
        "@id": "urn:ngm:class:virtual-environment-design",
        "label": "Virtual Environment Design"
      },
      {
        "@id": "urn:ngm:class:digital-asset",
        "label": "Digital Asset"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:3-d-file-format",
        "label": "3D File Format"
      },
      {
        "@id": "urn:ngm:class:3d-modelling",
        "label": "3D Modeling"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:3-d-rendering",
        "label": "3D Rendering"
      },
      {
        "@id": "urn:ngm:class:interactive-visualization",
        "label": "Interactive Visualization"
      },
      {
        "@id": "urn:ngm:class:animation-retargeting",
        "label": "Animation Retargeting"
      },
      {
        "@id": "urn:ngm:class:augmented-reality",
        "label": "Augmented Reality"
      },
      {
        "@id": "urn:ngm:class:virtual-reality",
        "label": "Virtual Reality"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:level-of-detail",
        "label": "Level of Detail"
      },
      {
        "@id": "urn:ngm:class:uv-mapping",
        "label": "UV Mapping"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:metaverse",
        "label": "Metaverse"
      },
      {
        "@id": "urn:ngm:class:digital-twin",
        "label": "Digital Twin"
      }
    ],
    "standardizedBy": [
      {
        "@id": "urn:ngm:class:gltf-standard",
        "label": "Gltf Standard"
      },
      {
        "@id": "urn:ngm:class:universal-scene-description",
        "label": "Universal Scene Description"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:computer-vision",
        "label": "Computer Vision"
      },
      {
        "@id": "urn:ngm:class:3-d-development",
        "label": "3D Development"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:photogrammetry",
        "label": "Photogrammetry"
      },
      {
        "@id": "urn:ngm:class:procedural-generation",
        "label": "Procedural Generation"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:three-dimensional-model",
      "label": "Three-Dimensional Model"
    },
    {
      "@id": "urn:ngm:class:3-d-object",
      "label": "3D Object"
    }
  ],
  "quality": 0.70,
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
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-model:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:0bd264303fc3ce01603d67dd2fcc53b898655b91b7a02a7b82d083e2a526b9f6"
  },
  "vc:resolutions": [
    {
      "raw": "[[3D Rendering]]",
      "resolved": "urn:visionflow:linked:3-d-rendering",
      "kind": "StubLink"
    },
    {
      "raw": "[[Interactive Visualization]]",
      "resolved": "urn:visionflow:linked:interactive-visualization",
      "kind": "StubLink"
    },
    {
      "raw": "[[Material Definition]]",
      "resolved": "urn:visionflow:linked:material-definition",
      "kind": "StubLink"
    },
    {
      "raw": "[[Mesh Data]]",
      "resolved": "urn:visionflow:linked:mesh-data",
      "kind": "StubLink"
    },
    {
      "raw": "[[Virtual Environment Design]]",
      "resolved": "urn:visionflow:linked:virtual-environment-design",
      "kind": "StubLink"
    },
    {
      "raw": "[[3D Development]]",
      "resolved": "urn:visionflow:owl:class:3-d-development",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[3D File Format]]",
      "resolved": "urn:visionflow:owl:class:3-d-file-format",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[3D Modelling]]",
      "resolved": "urn:visionflow:owl:class:3-d-modeling",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Animation Retargeting]]",
      "resolved": "urn:visionflow:owl:class:animation-retargeting",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Computer Vision]]",
      "resolved": "urn:visionflow:owl:class:computer-vision",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Digital Asset]]",
      "resolved": "urn:visionflow:owl:class:digital-asset",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Metaverse]]",
      "resolved": "urn:visionflow:owl:class:metaverse",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Texture Mapping]]",
      "resolved": "urn:visionflow:owl:class:texture-mapping",
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
  - A 3D Model is a digital representation of a three-dimensional object or environment constructed from vertices, edges, faces, and materials, encoded in formats such as [[3D File Format]]. Models serve as foundational assets in [[3D Development]], enabling visualisation, simulation, and interaction within virtual spaces, games, and [[Metaverse]] applications.

- ### Semantic Classification
  - owl-class:: spatial-computing:3DModel
  - owl-role:: concept

- ### Relationships
  - is-subclass-of:: [[Digital Asset]]
  - bridges-to:: [[Computer Vision]], [[3D Modelling]], [[Animation Retargeting]]
  - requires:: [[Mesh Data]], [[Material Definition]], [[Texture Mapping]]
  - enables:: [[3D Rendering]], [[Interactive Visualization]], [[Virtual Environment Design]]

- ### Content

  ## Overview

  3D Models form the visual foundation of immersive experiences, ranging from simple geometric primitives to complex character rigs with hundreds of thousands of polygons. Each model is composed of geometric data (mesh), material properties, and optional skeletal rigging for animation.

  ## Components
  - **Geometry**: Vertices and faces forming the visible shape
  - **Materials**: Surface properties including colour, roughness, and metallic values
  - **Textures**: 2D images applied to surfaces for visual detail
  - **Rigging**: Skeletal structure enabling animation and deformation
  - **LOD Variants**: Multiple detail levels for performance optimisation

  #### Related Concepts
  - [[3D Modelling]], [[Mesh Data]], [[Material Definition]], [[Texture Mapping]], [[Animation Retargeting]]

- ### Provenance
  - sources::
  - migration-date:: 2026-04-26T00:00:00Z
