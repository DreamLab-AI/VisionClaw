---
public: true
---

# 3D Engine
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:7020754142b489fd593975b4b32172b02ab5d31db07ff3c44a027ba779a1eff2",
  "@type": "Page",
  "vc:slug": "3-d-engine",
  "title": "3D Engine",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:computational-geometry",
      "vc:label": "Computational Geometry"
    },
    {
      "@id": "urn:visionflow:linked:3-d-engine",
      "vc:label": "3D Engine"
    },
    {
      "@id": "urn:visionflow:linked:physics-simulation-engine",
      "vc:label": "Physics Simulation Engine"
    },
    {
      "@id": "urn:visionflow:linked:game-engine",
      "vc:label": "Game Engine"
    },
    {
      "@id": "urn:visionflow:linked:https-en-wikipedia-org-wiki-game-engine",
      "vc:label": "https://en.wikipedia.org/wiki/Game_engine"
    },
    {
      "@id": "urn:visionflow:linked:https-docs-unity-3-d-com-manual-index-html",
      "vc:label": "https://docs.unity3d.com/Manual/index.html"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "preferred-term",
      "vc:value": "3D Engine"
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
  "@id": "urn:ngm:class:3-d-engine",
  "@type": "Class",
  "label": "3D Engine",
  "definition": "A 3D engine is software that handles the representation, rendering, physics simulation, and animation of three-dimensional scenes, providing an abstraction layer over GPU hardware and platform-specific graphics APIs so developers can build interactive applications and simulations without writing low-level graphics code.",
  "domain": "spatial-computing",
  "maturity": "established",
  "qualityScore": 0.7,
  "subClassOf": [
    {
      "@id": "urn:ngm:class:game-engine",
      "label": "Game Engine"
    }
  ],
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:rendering-pipeline",
        "label": "Rendering Pipeline"
      },
      {
        "@id": "urn:ngm:class:physics-simulation-engine",
        "label": "Physics Simulation Engine"
      },
      {
        "@id": "urn:ngm:class:scene-graph",
        "label": "Scene Graph"
      },
      {
        "@id": "urn:ngm:class:animation-system",
        "label": "Animation System"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:computational-geometry",
        "label": "Computational Geometry"
      },
      {
        "@id": "urn:ngm:class:graphics-processing-unit",
        "label": "Graphics Processing Unit"
      },
      {
        "@id": "urn:ngm:class:shader",
        "label": "Shader"
      },
      {
        "@id": "urn:ngm:class:linear-algebra",
        "label": "Linear Algebra"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:virtual-reality",
        "label": "Virtual Reality"
      },
      {
        "@id": "urn:ngm:class:augmented-reality",
        "label": "Augmented Reality"
      },
      {
        "@id": "urn:ngm:class:digital-twin",
        "label": "Digital Twin"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:vulkan",
        "label": "Vulkan"
      },
      {
        "@id": "urn:ngm:class:open-gl",
        "label": "OpenGL"
      },
      {
        "@id": "urn:ngm:class:directx",
        "label": "DirectX"
      },
      {
        "@id": "urn:ngm:class:webgpu",
        "label": "WebGPU"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:spatial-computing",
        "label": "Spatial Computing"
      },
      {
        "@id": "urn:ngm:class:metaverse",
        "label": "Metaverse"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:machine-learning",
        "label": "Machine Learning"
      },
      {
        "@id": "urn:ngm:class:robotics",
        "label": "Robotics"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:extended-reality",
        "label": "Extended Reality"
      },
      {
        "@id": "urn:ngm:class:level-of-detail",
        "label": "Level of Detail"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:graphics-engine",
      "label": "Graphics Engine"
    },
    {
      "@id": "urn:ngm:class:real-time-rendering-engine",
      "label": "Real-Time Rendering Engine"
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
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-engine:1492a742b231",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:7020754142b489fd593975b4b32172b02ab5d31db07ff3c44a027ba779a1eff2"
  },
  "vc:resolutions": [
    {
      "raw": "[[Computational Geometry]]",
      "resolved": "urn:visionflow:linked:computational-geometry",
      "kind": "StubLink"
    },
    {
      "raw": "[[3D Engine]]",
      "resolved": "urn:visionflow:linked:3-d-engine",
      "kind": "StubLink"
    },
    {
      "raw": "[[Physics Simulation Engine]]",
      "resolved": "urn:visionflow:linked:physics-simulation-engine",
      "kind": "StubLink"
    },
    {
      "raw": "[[Game Engine]]",
      "resolved": "urn:visionflow:linked:game-engine",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[https://en.wikipedia.org/wiki/Game_engine]]",
      "resolved": "urn:visionflow:linked:https-en-wikipedia-org-wiki-game-engine",
      "kind": "StubLink"
    },
    {
      "raw": "[[https://docs.unity3d.com/Manual/index.html]]",
      "resolved": "urn:visionflow:linked:https-docs-unity-3-d-com-manual-index-html",
      "kind": "StubLink"
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
  - A 3D engine is software that handles the representation, rendering, and often the physics and animation of three-dimensional scenes. It forms the core of many games and simulations.

- ### Semantic Classification
  - owl-class:: general:3DEngine
  - owl-role:: Class

- ### Relationships
  - is-subclass-of:: [[Game Engine]]
  - bridges-to:: [[Physics Simulation Engine]]
  - requires:: [[Computational Geometry]]
  - enables:: [[3D Engine]]

- ### Content
  - A 3D engine manages geometry, materials, lighting, and cameras to produce images of a virtual scene. It typically drives the graphics processor through a rendering pipeline and may include subsystems for physics, animation, and audio.
  - These engines provide tools and abstractions so that developers can build interactive applications without writing low-level graphics code for each platform. They are used in games, simulations, and visualisation.

- ### Provenance
  - sources:: [[https://en.wikipedia.org/wiki/Game_engine]], [[https://docs.unity3d.com/Manual/index.html]]
  - migration-date:: 2026-05-29T00:00:00Z
