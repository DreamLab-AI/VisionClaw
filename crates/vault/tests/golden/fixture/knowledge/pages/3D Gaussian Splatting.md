---
public: true
aliases:
  - TELE-051-3d-gaussian-splatting
---

# 3D Gaussian Splatting
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:f32f0472a5ee16559fe85eb785fa81ec86cd50b3fe51d1d813e71a88fc87fac7",
  "@type": "Page",
  "vc:slug": "3-d-gaussian-splatting",
  "title": "3D Gaussian Splatting",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:differentiable-rendering",
      "vc:label": "DifferentiableRendering"
    },
    {
      "@id": "urn:visionflow:linked:photorealistic-telepresence",
      "vc:label": "PhotorealisticTelepresence"
    },
    {
      "@id": "urn:visionflow:linked:tele-020-virtual-reality-telepresence",
      "vc:label": "TELE-020-virtual-reality-telepresence"
    },
    {
      "@id": "urn:visionflow:linked:tele-050-neural-rendering-telepresence",
      "vc:label": "TELE-050-neural-rendering-telepresence"
    },
    {
      "@id": "urn:visionflow:linked:tele-052-neural-radiance-fields",
      "vc:label": "TELE-052-neural-radiance-fields"
    },
    {
      "@id": "urn:visionflow:linked:tele-053-volumetric-video-conferencing",
      "vc:label": "TELE-053-volumetric-video-conferencing"
    },
    {
      "@id": "urn:visionflow:linked:tele-060-instant-ngp",
      "vc:label": "TELE-060-instant-ngp"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "TC-0051"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "3D Gaussian Splatting"
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
  "@id": "urn:ngm:class:3-d-gaussian-splatting",
  "@type": "Class",
  "label": "3D Gaussian Splatting",
  "definition": "A neural rendering technique that represents 3D scenes as collections of millions of 3D Gaussian primitives with learnable positions, colours, opacities, and covariances, enabling photorealistic real-time rendering at 100+ frames per second through GPU-accelerated rasterisation, revolutionising telepresence and immersive collaboration with unprecedented visual fidelity.",
  "domain": "spatial-computing",
  "maturity": "emerging",
  "subClassOf": {
    "@id": "urn:ngm:class:neural-rendering",
    "label": "Neural Rendering"
  },
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:gaussian-primitive",
        "label": "Gaussian Primitive"
      },
      {
        "@id": "urn:ngm:class:spherical-harmonics",
        "label": "Spherical Harmonics"
      }
    ],
    "partOf": [
      {
        "@id": "urn:ngm:class:novel-view-synthesis",
        "label": "Novel View Synthesis"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:differentiable-rendering",
        "label": "Differentiable Rendering"
      },
      {
        "@id": "urn:ngm:class:structure-from-motion",
        "label": "Structure-from-Motion"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:photorealistic-telepresence",
        "label": "Photorealistic Telepresence"
      },
      {
        "@id": "urn:ngm:class:tele-053-volumetric-video-conferencing",
        "label": "Volumetric Video Conferencing"
      },
      {
        "@id": "urn:ngm:class:novel-view-synthesis",
        "label": "Real-Time Novel View Synthesis"
      },
      {
        "@id": "urn:ngm:class:avatar",
        "label": "Photorealistic Avatar"
      }
    ],
    "dependsOn": [
      {
        "@id": "urn:ngm:class:multi-view-photography",
        "label": "Multi-View Photography"
      },
      {
        "@id": "urn:ngm:class:point-cloud",
        "label": "Point Cloud"
      },
      {
        "@id": "urn:ngm:class:adam-optimiser",
        "label": "Adam Optimiser"
      }
    ],
    "implements": [
      {
        "@id": "urn:ngm:class:alpha-blending",
        "label": "Alpha Blending"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:stochastic-gradient-descent",
        "label": "Stochastic Gradient Descent"
      },
      {
        "@id": "urn:ngm:class:ssim-loss",
        "label": "SSIM Loss"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:neural-radiance-fields",
        "label": "Neural Radiance Fields"
      },
      {
        "@id": "urn:ngm:class:tele-060-instant-ngp",
        "label": "Instant NGP"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:tele-050-neural-rendering-telepresence",
        "label": "TELE-050-neural-rendering-telepresence"
      },
      {
        "@id": "urn:ngm:class:extended-reality",
        "label": "Extended Reality"
      },
      {
        "@id": "urn:ngm:class:web-xr",
        "label": "WebXR"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:virtual-reality-telepresence",
        "label": "Virtual Reality Telepresence"
      },
      {
        "@id": "urn:ngm:class:3-d-scene-reconstruction",
        "label": "Scene Reconstruction"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:3dgs",
      "label": "3DGS"
    },
    {
      "@id": "urn:ngm:class:gaussian-splatting",
      "label": "Gaussian Splatting"
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
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-gaussian-splatting:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:f32f0472a5ee16559fe85eb785fa81ec86cd50b3fe51d1d813e71a88fc87fac7"
  },
  "vc:resolutions": [
    {
      "raw": "[[DifferentiableRendering]]",
      "resolved": "urn:visionflow:linked:differentiable-rendering",
      "kind": "StubLink"
    },
    {
      "raw": "[[PhotorealisticTelepresence]]",
      "resolved": "urn:visionflow:linked:photorealistic-telepresence",
      "kind": "StubLink"
    },
    {
      "raw": "[[TELE-020-virtual-reality-telepresence]]",
      "resolved": "urn:visionflow:linked:tele-020-virtual-reality-telepresence",
      "kind": "StubLink"
    },
    {
      "raw": "[[TELE-050-neural-rendering-telepresence]]",
      "resolved": "urn:visionflow:linked:tele-050-neural-rendering-telepresence",
      "kind": "StubLink"
    },
    {
      "raw": "[[TELE-052-neural-radiance-fields]]",
      "resolved": "urn:visionflow:linked:tele-052-neural-radiance-fields",
      "kind": "StubLink"
    },
    {
      "raw": "[[TELE-053-volumetric-video-conferencing]]",
      "resolved": "urn:visionflow:linked:tele-053-volumetric-video-conferencing",
      "kind": "StubLink"
    },
    {
      "raw": "[[TELE-060-instant-ngp]]",
      "resolved": "urn:visionflow:linked:tele-060-instant-ngp",
      "kind": "StubLink"
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
  - "A neural rendering technique that represents 3D scenes as collections of millions of 3D Gaussian primitives with learnable positions, colours, opacities, and covariances, enabling photorealistic real-time rendering at 100+ frames per second through GPU-accelerated rasterisation, revolutionising telepresence and immersive collaboration with unprecedented visual fidelity."

- ### Semantic Classification
  - owl-class:: distributed-collaboration:ThreeDGaussianSplatting
  - owl-role:: Process

- ### Relationships
  - is-subclass-of:: [[TELE-050-neural-rendering-telepresence]]
  - requires:: [[DifferentiableRendering]]
  - enables:: [[PhotorealisticTelepresence]]

- ### Content

  ## Definition

  **3D Gaussian Splatting** is a breakthrough neural rendering method published at SIGGRAPH 2023 by Kerbl et al., representing 3D scenes as explicit collections of anisotropic 3D Gaussian distributions rather than implicit neural networks. Each Gaussian primitive encodes a 3D position (mean), colour, opacity, and 3×3 covariance matrix defining its shape and orientation in space. Rendering involves "splatting" these Gaussians onto the image plane through differentiable rasterisation, achieving photorealistic quality at 100-300 frames per second on consumer GPUs—over 100× faster than Neural Radiance Fields ([[TELE-052-neural-radiance-fields]]) whilst matching or exceeding visual fidelity.

  The technique trains by optimising Gaussian parameters (positions, colours, covariances) to match input multi-view photographs through gradient descent, starting with sparse 3D point clouds from Structure-from-Motion (SfM). Gaussians are adaptively split, cloned, or pruned during optimisation to capture fine detail (hair strands, foliage) or remove redundancy. The explicit scene representation enables real-time rendering through GPU rasterisation pipelines, unlocking applications in [[TELE-020-virtual-reality-telepresence]], [[TELE-053-volumetric-video-conferencing]], and immersive telepresence where photorealistic environments must render at 90+ FPS for comfortable VR.

  #### Current Landscape
  3D Gaussian Splatting has rapidly transitioned from academic novelty to production deployment, with major telepresence platforms integrating the technology for photorealistic avatars and environments.

  **Adoption Statistics**:
  - 67% of neural rendering research papers (2024-2025) employ Gaussian splatting variants (arXiv analysis)
  - Meta, Apple, Niantic incorporate Gaussian splatting in AR/VR pipelines
  - 14,000+ GitHub stars on official implementation (most-starred graphics paper 2023)
  - Consumer apps (Luma AI, PolyCam) enable smartphone Gaussian capture

  **Technology Capabilities (2025)**:
  - **Training Time**: 30 minutes for room-scale scene on RTX 4090 (vs. 24 hours for NeRF)
  - **Rendering Speed**: 150-300 FPS at 1080p resolution
  - **Quality**: PSNR 30-35 dB (comparable to NeRF, exceeding mesh-based methods)
  - **Scene Size**: Millions of Gaussians represent entire buildings

  **UK Context**:
  - **Luma AI** (London office): Develops NeRF-to-Gaussian conversion tools
  - **PolyCam** (UK users): Gaussian splatting mode in 3D scanning app
  - **University of Oxford**: Research on dynamic Gaussian splatting for moving objects
  - **Imperial College London**: Compression techniques for streaming Gaussian scenes

  ## Technical Details

  ### Scene Representation

  Each 3D Gaussian primitive 𝒢ᵢ defined by:
  - **Mean μᵢ ∈ ℝ³**: 3D position in world space
  - **Covariance Σᵢ ∈ ℝ³ˣ³**: Defines ellipsoidal shape/orientation
  - **Colour cᵢ ∈ ℝ³** (or spherical harmonics for view-dependent appearance)
  - **Opacity αᵢ ∈ [0,1]**: Transparency

  Gaussian function: G(x) = exp(-½(x-μ)ᵀΣ⁻¹(x-μ))

  ### Rendering Pipeline

  1. **Projection**: Transform 3D Gaussians to 2D image space
   - Project mean μᵢ via camera matrix
   - Approximate 2D covariance via Jacobian of projection

  2. **Sorting**: Order Gaussians by depth (painter's algorithm with α-blending)

  3. **Rasterisation**: For each pixel, accumulate colours of overlapping Gaussians
   - Front-to-back traversal with early stopping when opacity saturates
   - GPU-accelerated parallel processing

  4. **Output**: Photorealistic rendered image from novel viewpoint

  ### Optimisation

  **Input**: 50-200 multi-view photographs with camera poses (from SfM)

  **Initialisation**: Sparse 3D point cloud → one Gaussian per point

  **Loss Function**: L1 + SSIM (Structural Similarity Index) between rendered and ground truth images

  **Optimisation**:
  - Stochastic gradient descent with Adam optimiser
  - 30,000 iterations (~30 minutes on RTX 4090)
  - Adaptive density control: split under-reconstructed regions, prune low-opacity Gaussians

  **Result**: Millions of optimised Gaussians encoding scene

  ### Advantages Over NeRF

  | Aspect | Gaussian Splatting | Neural Radiance Fields ([[TELE-052-neural-radiance-fields]]) |
  |--------|-------------------|--------------------------------------------------------------|
  | **Rendering Speed** | 100-300 FPS | 0.1-1 FPS (real-time variants: 30 FPS) |
  | **Training Time** | 30 minutes | 12-48 hours |
  | **Quality** | Photorealistic (30-35 dB PSNR) | Photorealistic (30-36 dB PSNR) |
  | **Representation** | Explicit (Gaussians) | Implicit (MLP weights) |
  | **Memory** | 100-500 MB per scene | 10-50 MB (more compact) |
  | **Editability** | Easy (move/delete Gaussians) | Difficult (retrain network) |

  ## Applications in Telepresence

  ### Photorealistic Virtual Environments ([[TELE-020-virtual-reality-telepresence]])
  - Scan real office spaces with smartphones (50-100 photos)
  - Train Gaussian scene in 30 minutes
  - Render in VR at 90 FPS for telepresence meetings
  - Example: Meta Horizon Workrooms experimenting with Gaussian environments (2025)

  ### Volumetric Video Conferencing ([[TELE-053-volumetric-video-conferencing]])
  - Capture participant with multi-camera rig (6-12 cameras)
  - Real-time Gaussian optimisation (30 Hz update rate)
  - Stream compressed Gaussians to remote clients
  - Render photorealistic avatar from any angle
  - Example: Microsoft Mesh exploring dynamic Gaussian avatars

  ### Virtual Tourism
  - Museums digitise exhibits with Gaussian scans
  - Remote visitors navigate photorealistic 3D environments
  - Example: Luma AI captures heritage sites for virtual tours

  ### Remote Site Inspection
  - Construction sites scanned with drones
  - Engineers inspect progress remotely in photorealistic 3D
  - Example: UK engineering firms use PolyCam for site documentation

  ## Technical Challenges and Solutions

  ### Challenge: Large File Sizes
  - **Problem**: Millions of Gaussians → 500 MB+ per scene
  - **Solution**: Neural compression, quantisation (reduce to 50-100 MB)
  - **Research**: Compact 3DGS, EAGLES (entropy-aware compression)

  ### Challenge: Dynamic Scenes
  - **Problem**: Original technique assumes static scenes
  - **Solution**: 4D Gaussian splatting (add time dimension), deformable Gaussians
  - **Research**: Dynamic 3DGS, 4DGaussians (moving people, avatars)

  ### Challenge: Training Data Requirements
  - **Problem**: Needs 50-200 high-quality photos with accurate poses
  - **Solution**: Structure-from-Motion automation, smartphone capture apps
  - **Tools**: COLMAP (SfM), Luma AI app, PolyCam

  ### Challenge: Real-Time Streaming
  - **Problem**: 500 MB scenes unsuitable for network streaming
  - **Solution**: Progressive transmission (coarse-to-fine), level-of-detail rendering
  - **Research**: Streamable Gaussians, LoD-GS

  #### Future Directions
  **Near-Term (2025-2027)**:
  - Real-time Gaussian capture from single RGB-D camera (iPhone LiDAR)
  - Compression to <50 MB per scene for mobile deployment
  - Integration into WebXR standard (browser-based Gaussian rendering)

  **Medium-Term (2027-2030)**:
  - Photorealistic full-body Gaussian avatars updating at 60 Hz
  - Gaussian-based telepresence as default in Meta/Apple platforms
  - Semantic Gaussians (each primitive labelled: "table", "wall", etc.)

  **Long-Term (2030+)**:
  - Neural codecs compressing Gaussians 100× further
  - Light-field displays rendering Gaussians holographically (no headset)
  - Real-time global illumination in Gaussian scenes (ray tracing)

  #### Related Concepts
  - [[TELE-050-neural-rendering-telepresence]]
  - [[TELE-052-neural-radiance-fields]]
  - [[TELE-053-volumetric-video-conferencing]]
  - [[TELE-020-virtual-reality-telepresence]]
  - [[TELE-060-instant-ngp]]

  #### References
  1. Kerbl, B., Kopanas, G., Leimkühler, T., & Drettakis, G. (2023). "3D Gaussian Splatting for Real-Time Radiance Field Rendering". *ACM Transactions on Graphics (SIGGRAPH)*, 42(4), 1-14.
  2. Luiten, J., et al. (2023). "Dynamic 3D Gaussians: Tracking by Persistent Dynamic View Synthesis". *arXiv preprint*.
  3. Niedermayr, S., et al. (2024). "Compressed 3D Gaussian Splatting for Accelerated Novel View Synthesis". *CVPR 2024*.

  ## Open-Source Implementations

  - **Official**: https://github.com/graphdeco-inria/gaussian-splatting
  - **Nerfstudio**: Gaussian splatting module in unified NeRF framework
  - **gsplat**: PyTorch library for differentiable Gaussian rasterisation
  - **WebGL Viewer**: Real-time browser-based Gaussian rendering

- ### Provenance
  - sources::
  - migration-date:: 2026-04-26T00:00:00Z
