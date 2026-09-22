---
public: true
aliases:
  - 3D Modeling
---

# 3D Modelling

```json-ld
{ "@context":"https://narrativegoldmine.com/ns/v1", "@id":"urn:visionflow:page:3d-modelling", "@type":"Page", "title":"3D Modelling", "vc:slug":"3d-modelling", "vc:public":true, "vc:schemaVersion":2, "vc:outboundWikilinks":[] }
```

```json-ld
{
  "@context":"https://narrativegoldmine.com/ns/v2.jsonld",
  "@id":"urn:ngm:class:3d-modelling",
  "@type":"Class",
  "label":"3D Modelling",
  "definition":"3D modelling is the creation of a mathematical representation of a three-dimensional surface or volume using specialised software. Models are commonly built from polygon meshes, parametric surfaces or volumetric data and define the geometry that is later textured, rigged, animated and rendered. It is the foundational discipline for games, film, visualisation, product design and spatial computing.",
  "domain":"spatial-computing",
  "maturity":"established",
  "subClassOf":[{"@id":"urn:ngm:class:computer-graphics","label":"Computer Graphics"},{"@id":"urn:ngm:class:3-d-development","label":"3D Development"}],
  "relations":{
    "hasPart":[{"@id":"urn:ngm:class:polygon-mesh","label":"Polygon Mesh"},{"@id":"urn:ngm:class:geometry-processing","label":"Geometry Processing"},{"@id":"urn:ngm:class:topology","label":"Topology"},{"@id":"urn:ngm:class:character-rigging","label":"Character Rigging"},{"@id":"urn:ngm:class:material-definition","label":"Material Definition"}],
    "partOf":[{"@id":"urn:ngm:class:asset-pipeline","label":"Asset Pipeline"}],
    "requires":[{"@id":"urn:ngm:class:design-software","label":"Design Software"},{"@id":"urn:ngm:class:polygon-mesh","label":"Polygon Mesh"}],
    "enables":[{"@id":"urn:ngm:class:rendering","label":"Rendering"},{"@id":"urn:ngm:class:character-model","label":"Character Model"},{"@id":"urn:ngm:class:texture-mapping","label":"Texture Mapping"},{"@id":"urn:ngm:class:asset-creation","label":"Asset Creation"},{"@id":"urn:ngm:class:character-animation","label":"Character Animation"},{"@id":"urn:ngm:class:virtual-environment-design","label":"Virtual Environment Design"},{"@id":"urn:ngm:class:real-time-graphics","label":"Real-Time Graphics"}],
    "uses":[{"@id":"urn:ngm:class:uv-mapping","label":"UV Mapping"},{"@id":"urn:ngm:class:computer-aided-design","label":"Computer-Aided Design"},{"@id":"urn:ngm:class:3-d-model","label":"3D Model"},{"@id":"urn:ngm:class:level-of-detail","label":"Level of Detail"}],
    "supports":[{"@id":"urn:ngm:class:visual-effects","label":"Visual Effects"},{"@id":"urn:ngm:class:game-engine","label":"Game Engine"},{"@id":"urn:ngm:class:virtual-production","label":"Virtual Production"},{"@id":"urn:ngm:class:virtual-reality","label":"Virtual Reality"},{"@id":"urn:ngm:class:augmented-reality","label":"Augmented Reality"},{"@id":"urn:ngm:class:game-development","label":"Game Development"}],
    "standardizedBy":[{"@id":"urn:ngm:class:gltf-standard","label":"glTF"},{"@id":"urn:ngm:class:obj-format","label":"OBJ Format"}],
    "contrastsWith":[{"@id":"urn:ngm:class:computer-aided-design","label":"Computer-Aided Design"},{"@id":"urn:ngm:class:digital-art","label":"Digital Art"}],
    "relatedTo":[{"@id":"urn:ngm:class:fbx","label":"FBX"},{"@id":"urn:ngm:class:character-rigging","label":"Character Rigging"},{"@id":"urn:ngm:class:skinning","label":"Skinning"},{"@id":"urn:ngm:class:visual-effects","label":"Visual Effects"}],
    "bridgesTo":[{"@id":"urn:ngm:class:augmented-reality","label":"Augmented Reality"},{"@id":"urn:ngm:class:computer-vision","label":"Computer Vision"},{"@id":"urn:ngm:class:photogrammetry","label":"Photogrammetry"},{"@id":"urn:ngm:class:generative-ai","label":"Generative AI"}]
  },
  "sameAs":[{"@id":"urn:ngm:class:3d-modelling","label":"3D Modeling"},{"@id":"urn:ngm:class:3-d-model-authoring","label":"3D Model Authoring"},{"@id":"urn:ngm:class:three-dimensional-modeling","label":"Three-Dimensional Modeling"}],
  "quality":0.62,
  "provenance":{"attributedTo":"did:nostr:ontology-mesh","generatedAt":"2026-06-15T00:00:00Z","inferenceRule":"GapMaterialisation"}
}
```

- ### Definition
- 3D modelling is the creation of a mathematical representation of a three-dimensional surface or volume using [[Design Software]].
- Models are commonly built from a [[Polygon Mesh]], parametric surfaces or volumetric data.
- The resulting geometry is later textured, rigged, animated and rendered.
- It is the foundational discipline of [[Computer Graphics]] for games, film, visualisation and spatial computing.
- ### Overview
- 3D modelling encompasses several paradigms. Polygonal modelling builds surfaces from vertices, edges and faces and dominates real-time and entertainment work. NURBS and parametric surfaces produce mathematically smooth geometry favoured in industrial design.
- Digital sculpting treats the model like virtual clay, enabling high-detail organic forms that are later retopologised into efficient meshes.
- Procedural and generative approaches construct geometry from rules and parameters, scaling complex environments and assets.
- The discipline is distinct from but adjacent to computer-aided design, which prioritises engineering precision, manufacturability and parametric constraints over artistic surface modelling.
- ### Key aspects
- Mesh construction from vertices, edges and faces.
- Clean topology supporting deformation and subdivision.
- UV layout to prepare surfaces for texturing.
- Level-of-detail management for performance.
- Interchange through formats such as FBX and glTF.
- ### Mechanisms
- Box and polygon modelling for hard-surface and stylised assets.
- Digital sculpting and retopology for organic detail.
- Boolean and subdivision-surface operations.
- Procedural generation driven by parameters and graphs.
- ### Applications
- Game assets and characters for real-time engines.
- Film and visual effects environments and creatures.
- Product visualisation and industrial design.
- Digital humans and props for augmented and virtual reality.
- ### Workflow stages
- Concept and planning from visual references.
- Rough blocking establishing proportions and silhouette.
- Detail sculpting and refinement.
- Topology optimisation for animation capability.
- Unwrapping for texture application.
- ### Relationships
- hasPart:: [[Polygon Mesh]]
- hasPart:: [[Geometry Processing]]
- hasPart:: [[Topology]]
- hasPart:: [[Character Rigging]]
- hasPart:: [[Material Definition]]
- partOf:: [[Asset Pipeline]]
- requires:: [[Design Software]]
- requires:: [[Polygon Mesh]]
- enables:: [[Rendering]]
- enables:: [[Character Model]]
- enables:: [[Texture Mapping]]
- enables:: [[Asset Creation]]
- enables:: [[Character Animation]]
- enables:: [[Virtual Environment Design]]
- enables:: [[Real-Time Graphics]]
- uses:: [[UV Mapping]]
- uses:: [[Computer-Aided Design]]
- uses:: [[3D Model]]
- uses:: [[Level of Detail]]
- supports:: [[Visual Effects]]
- supports:: [[Game Engine]]
- supports:: [[Virtual Production]]
- supports:: [[Virtual Reality]]
- supports:: [[Augmented Reality]]
- supports:: [[Game Development]]
- standardizedBy:: [[glTF]]
- standardizedBy:: [[OBJ Format]]
- contrastsWith:: [[Computer-Aided Design]]
- contrastsWith:: [[Digital Art]]
- relatedTo:: [[FBX]]
- relatedTo:: [[Character Rigging]]
- relatedTo:: [[Skinning]]
- relatedTo:: [[Visual Effects]]
- bridgesTo:: [[Augmented Reality]]
- bridgesTo:: [[Computer Vision]]
- bridgesTo:: [[Photogrammetry]]
- bridgesTo:: [[Generative AI]]
- sameAs:: [[3D Modelling]]
- ### Current Landscape (2026)
  - Generative image/text-to-3D crossed from research demo to production tool: Meshy-6 (public preview Oct 2025, GA Jan 2026), Tripo H3.1, Rodin Gen-2.5 (Deemos/Hyper3D, May 2026, 10M+ polygon sculpt-level detail) and Luma Genie now output clean-topology PBR meshes in roughly 10-60 seconds, with automatic retopology and UV unwrapping making outputs drop directly into game engines.
  - Open-weights models reached genuine production quality, eroding the closed-model lead: Microsoft's TRELLIS.2 (MIT-licensed, ~3s at 512-cubed) and Tencent's Hunyuan3D-2.1/3.x (two-stage DiT geometry plus Paint texturing, 1024 geometry resolution and 4K PBR in the Pro tier) are now the leading self-hostable families.
  - 3D Gaussian splatting matured from a SIGGRAPH 2023 research curiosity into a standardised photoreal-capture format, appearing in film, real-estate and e-commerce; VAST AI open-sourced TripoSplat (MIT, May 2026) for single-image-to-splat output with up to 262,144 Gaussians, alongside Polycam, Luma and Postshot capture flows.
  - A new category of "world models" arrived: World Labs made Marble generally available on 12 November 2025, generating navigable 3D scenes (not single objects) from text, image, video or coarse layout, exporting Gaussian splats, meshes or video.
  - OpenUSD became the de-facto interchange spine of the 2026 pipeline: Maya 2026 is shifting to a USD-first architecture (USD as source of truth rather than .ma/.mb), and Blender, Houdini 21 Solaris and Unreal Engine 5.6 all handle USD near-natively, with USD 25.02 shipping in Blender 4.4 (18 March 2025) alongside MaterialX 1.39 and OpenColorIO 2.4.
  - Incumbent DCC vendors embedded generative 3D directly: Autodesk launched Wonder 3D inside Flow Studio on 4 March 2026 (text/image-to-3D with USD, STL and OBJ export across all subscription tiers), and Blender shipped three major 2025 releases (4.2 LTS through 4.5, December 2025) adding Vulkan support, EEVEE Next, redesigned Geometry Nodes and a first iPad Pro build.
  - The AI 3D market was estimated at roughly $3.23B in 2026 (projected ~$9.4B by 2030); open frontier challenges remain around clean quad topology, riggable/editable output versus non-editable splats, and agentic pipelines where MCP lets AI drive Blender and generation tools directly.

- ### References
  - 1. 3D AI Studio (2026). State of AI 3D Generation 2026 - Market, Models. https://www.3daistudio.com/state-of-ai-3d-generation-2026
  - 2. Entagl (2026). AI 3D Generation in 2026: What's New. https://www.entagl.com/blog/ai-3d-generation-2026-product-ar-creative?lang=en
  - 3. Creative AI News (2026). AI 3D Generation and Spatial Computing: State of the Art in 2026. https://www.creativeainews.com/articles/ai-3d-spatial-computing-2026/
  - 4. Autodesk (2026). Autodesk Flow Studio launches Wonder 3D gen-AI model. https://adsknews.autodesk.com/en/news/autodesk-flow-studio-launches-wonder-3d-gen-ai-model-to-help-creators-make-3d-characters-and-objects-faster-and-easier/
  - 5. Blender Foundation (2025). Blender 4.4 Release Notes. https://developer.blender.org/docs/release_notes/4.4/
  - 6. WaveSpeed AI (2026). Should You Generate Gaussian Splats or a Mesh in 2026? (TripoSplat vs Hunyuan 3D). https://wavespeed.ai/blog/posts/triposplat-vs-hunyuan-3d/

- ### Provenance
- updated:: 2026-06-15
- attributedTo:: did:nostr:ontology-mesh
- inferenceRule:: GapMaterialisation
