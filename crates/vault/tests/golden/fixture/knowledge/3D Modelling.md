---
type: Class
title: 3D Modelling
aliases:
- 3D Modeling
- Manual 3D Modeling
resource: urn:ngm:class:3d-modelling
page_resource: urn:visionflow:page:3d-modelling
public: true
status: stable
domain: spatial-computing
maturity: established
quality: 0.62
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[computer-graphics|Computer Graphics]]'
- '[[3D Development]]'
bridges-to:
- '[[augmented-reality|Augmented Reality]]'
- '[[computer-vision|Computer Vision]]'
- '[[photogrammetry|Photogrammetry]]'
- '[[generative-ai|Generative AI]]'
contrasts-with:
- '[[computer-aided-design|Computer-Aided Design]]'
- '[[digital-art|Digital Art]]'
enables:
- '[[rendering|Rendering]]'
- '[[character-model|Character Model]]'
- '[[texture-mapping|Texture Mapping]]'
- '[[asset-creation|Asset Creation]]'
- '[[character-animation|Character Animation]]'
- '[[virtual-environment-design|Virtual Environment Design]]'
- '[[real-time-graphics|Real-Time Graphics]]'
has-part:
- '[[polygon-mesh|Polygon Mesh]]'
- '[[geometry-processing|Geometry Processing]]'
- '[[topology|Topology]]'
- '[[character-rigging|Character Rigging]]'
- '[[material-definition|Material Definition]]'
part-of:
- '[[asset-pipeline|Asset Pipeline]]'
related-to:
- '[[fbx|FBX]]'
- '[[character-rigging|Character Rigging]]'
- '[[skinning|Skinning]]'
- '[[visual-effects|Visual Effects]]'
requires:
- '[[design-software|Design Software]]'
- '[[polygon-mesh|Polygon Mesh]]'
standardized-by:
- '[[gltf-standard|glTF]]'
- '[[obj-format|OBJ Format]]'
supports:
- '[[visual-effects|Visual Effects]]'
- '[[game-engine|Game Engine]]'
- '[[virtual-production|Virtual Production]]'
- '[[virtual-reality|Virtual Reality]]'
- '[[augmented-reality|Augmented Reality]]'
- '[[game-development|Game Development]]'
uses:
- '[[uv-mapping|UV Mapping]]'
- '[[computer-aided-design|Computer-Aided Design]]'
- '[[3D Model]]'
- '[[level-of-detail|Level of Detail]]'
---

3D modelling is the creation of a mathematical representation of a three-dimensional surface or volume using specialised software. Models are commonly built from polygon meshes, parametric surfaces or volumetric data and define the geometry that is later textured, rigged, animated and rendered. It is the foundational discipline for games, film, visualisation, product design and spatial computing.

- 3D modelling is the creation of a mathematical representation of a three-dimensional surface or volume using [[Design Software]].
- Models are commonly built from a [[Polygon Mesh]], parametric surfaces or volumetric data.
- The resulting geometry is later textured, rigged, animated and rendered.
- It is the foundational discipline of [[Computer Graphics]] for games, film, visualisation and spatial computing.

### Overview

- 3D modelling encompasses several paradigms. Polygonal modelling builds surfaces from vertices, edges and faces and dominates real-time and entertainment work. NURBS and parametric surfaces produce mathematically smooth geometry favoured in industrial design.
- Digital sculpting treats the model like virtual clay, enabling high-detail organic forms that are later retopologised into efficient meshes.
- Procedural and generative approaches construct geometry from rules and parameters, scaling complex environments and assets.
- The discipline is distinct from but adjacent to computer-aided design, which prioritises engineering precision, manufacturability and parametric constraints over artistic surface modelling.

### Key aspects

- Mesh construction from vertices, edges and faces.
- Clean topology supporting deformation and subdivision.
- UV layout to prepare surfaces for texturing.
- Level-of-detail management for performance.
- Interchange through formats such as FBX and glTF.

### Mechanisms

- Box and polygon modelling for hard-surface and stylised assets.
- Digital sculpting and retopology for organic detail.
- Boolean and subdivision-surface operations.
- Procedural generation driven by parameters and graphs.

### Applications

- Game assets and characters for real-time engines.
- Film and visual effects environments and creatures.
- Product visualisation and industrial design.
- Digital humans and props for augmented and virtual reality.

### Workflow stages

- Concept and planning from visual references.
- Rough blocking establishing proportions and silhouette.
- Detail sculpting and refinement.
- Topology optimisation for animation capability.
- Unwrapping for texture application.

### Current Landscape (2026)

- Generative image/text-to-3D crossed from research demo to production tool: Meshy-6 (public preview Oct 2025, GA Jan 2026), Tripo H3.1, Rodin Gen-2.5 (Deemos/Hyper3D, May 2026, 10M+ polygon sculpt-level detail) and Luma Genie now output clean-topology PBR meshes in roughly 10-60 seconds, with automatic retopology and UV unwrapping making outputs drop directly into game engines.
- Open-weights models reached genuine production quality, eroding the closed-model lead: Microsoft's TRELLIS.2 (MIT-licensed, ~3s at 512-cubed) and Tencent's Hunyuan3D-2.1/3.x (two-stage DiT geometry plus Paint texturing, 1024 geometry resolution and 4K PBR in the Pro tier) are now the leading self-hostable families.
- 3D Gaussian splatting matured from a SIGGRAPH 2023 research curiosity into a standardised photoreal-capture format, appearing in film, real-estate and e-commerce; VAST AI open-sourced TripoSplat (MIT, May 2026) for single-image-to-splat output with up to 262,144 Gaussians, alongside Polycam, Luma and Postshot capture flows.
- A new category of "world models" arrived: World Labs made Marble generally available on 12 November 2025, generating navigable 3D scenes (not single objects) from text, image, video or coarse layout, exporting Gaussian splats, meshes or video.
- OpenUSD became the de-facto interchange spine of the 2026 pipeline: Maya 2026 is shifting to a USD-first architecture (USD as source of truth rather than .ma/.mb), and Blender, Houdini 21 Solaris and Unreal Engine 5.6 all handle USD near-natively, with USD 25.02 shipping in Blender 4.4 (18 March 2025) alongside MaterialX 1.39 and OpenColorIO 2.4.
- Incumbent DCC vendors embedded generative 3D directly: Autodesk launched Wonder 3D inside Flow Studio on 4 March 2026 (text/image-to-3D with USD, STL and OBJ export across all subscription tiers), and Blender shipped three major 2025 releases (4.2 LTS through 4.5, December 2025) adding Vulkan support, EEVEE Next, redesigned Geometry Nodes and a first iPad Pro build.
- The AI 3D market was estimated at roughly $3.23B in 2026 (projected ~$9.4B by 2030); open frontier challenges remain around clean quad topology, riggable/editable output versus non-editable splats, and agentic pipelines where MCP lets AI drive Blender and generation tools directly.

### References

- 1. 3D AI Studio (2026). State of AI 3D Generation 2026 - Market, Models. https://www.3daistudio.com/state-of-ai-3d-generation-2026
- 2. Entagl (2026). AI 3D Generation in 2026: What's New. https://www.entagl.com/blog/ai-3d-generation-2026-product-ar-creative?lang=en
- 3. Creative AI News (2026). AI 3D Generation and Spatial Computing: State of the Art in 2026. https://www.creativeainews.com/articles/ai-3d-spatial-computing-2026/
- 4. Autodesk (2026). Autodesk Flow Studio launches Wonder 3D gen-AI model. https://adsknews.autodesk.com/en/news/autodesk-flow-studio-launches-wonder-3d-gen-ai-model-to-help-creators-make-3d-characters-and-objects-faster-and-easier/
- 5. Blender Foundation (2025). Blender 4.4 Release Notes. https://developer.blender.org/docs/release_notes/4.4/
- 6. WaveSpeed AI (2026). Should You Generate Gaussian Splats or a Mesh in 2026? (TripoSplat vs Hunyuan 3D). https://wavespeed.ai/blog/posts/triposplat-vs-hunyuan-3d/

### Provenance
