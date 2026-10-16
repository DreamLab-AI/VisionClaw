---
type: Class
title: 3D Rendering
slug: 3-d-rendering
resource: urn:ngm:class:3-d-rendering
page_resource: urn:visionflow:page:3-d-rendering
public: true
status: stable
domain: spatial-computing
maturity: established
quality: 0.72
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[computer-graphics|Computer Graphics]]'
bridges-to:
- '[[differentiable-rendering|Differentiable Rendering]]'
- '[[neural-radiance-field|Neural Radiance Field]]'
- '[[3D Gaussian Splatting]]'
- '[[3D Content Generation|Generative 3D]]'
depends-on:
- '[[linear-algebra|Linear Algebra]]'
- '[[3D Rendering Engine]]'
enables:
- '[[photorealistic-rendering|Photorealistic Rendering]]'
- '[[physically-based-rendering|Physically Based Rendering]]'
- '[[foveated-rendering|Foveated Rendering]]'
- '[[real-time-graphics|Real-Time Graphics]]'
- '[[augmented-reality|Augmented Reality]]'
- '[[virtual-reality|Virtual Reality]]'
has-part:
- '[[rasterization|Rasterization]]'
- '[[ray-tracing|Ray Tracing]]'
- '[[path-tracing|Path Tracing]]'
- '[[pixel-shader|Pixel Shader]]'
- '[[compute-shader|Compute Shader]]'
- '[[render-pipeline|Render Pipeline]]'
related-to:
- '[[global-illumination|Global Illumination]]'
- '[[anti-aliasing|Anti-Aliasing]]'
- '[[level-of-detail|Level of Detail]]'
- '[[spatial-computing|Spatial Computing]]'
requires:
- '[[graphics-processing-unit|Graphics Processing Unit]]'
- '[[virtual-lighting-model|Lighting Model]]'
- '[[depth-buffer|Depth Buffer]]'
uses:
- '[[gpu-compute|GPU Compute]]'
- '[[3D Model]]'
- '[[texture-mapping|Texture Mapping]]'
- '[[shader|Shader Programming]]'
- '[[scene-graph|Scene Graph]]'
---

3D rendering is the computational process of generating a two-dimensional image or animation from a three-dimensional scene description by simulating the interaction of light with surfaces, materials, and geometry. It encompasses techniques ranging from real-time rasterization used in interactive applications to physically-based ray tracing and path tracing used for photorealistic offline production. The pipeline converts geometric primitives, shader programs, texture maps, and lighting data into final pixel colours via a GPU compute pipeline or software renderer. Contemporary rendering also incorporates neural methods such as Neural Radiance Fields and 3D Gaussian Splatting, as well as AI-driven upscaling and denoising, blurring the boundary between classical computer graphics and machine learning.

### Overview

- 3D rendering is a foundational technology in digital content creation, games, film VFX, architectural visualisation, scientific simulation, and immersive XR experiences. Its core challenge is computationally reproducing the way photons interact with surfaces: how they are emitted by light sources, reflected or absorbed by materials, scattered through volumes, and ultimately focused by a virtual camera onto a pixel grid. Two dominant paradigms address this challenge at different points on the fidelity-versus-speed spectrum:
  - **Rasterization** — projects geometry onto the screen plane and fills pixels with colour computed by shader programs. Extremely fast; powers virtually all real-time games and interactive 3D applications.
  - **Ray/Path Tracing** — simulates individual light rays, enabling correct global illumination, soft shadows, reflections, refractions, and caustics. Computationally expensive; traditionally used for offline film production.
- Hardware-accelerated ray tracing (RT cores, BVH traversal) and neural upscaling are rapidly closing the gap, enabling near-photorealistic quality at real-time frame rates.

### Key Components

- **[[Render Pipeline]]** — the ordered sequence of programmable and fixed-function GPU stages: input assembly, vertex shading, primitive assembly, rasterization, fragment/pixel shading, and output merging.
- **[[Pixel Shader]] / Fragment Shader** — evaluates per-pixel colour, incorporating [[Texture Mapping]], BRDF lighting models, and shadow lookups.
- **[[Compute Shader]]** — general-purpose GPU kernel used for screen-space ambient occlusion, particle simulation, ray marching, and denoising.
- **[[Depth Buffer]] (Z-buffer)** — per-pixel depth storage that resolves visibility without sorting geometry.
- **[[Scene Graph]]** — hierarchical data structure that organises 3D scene objects, transforms, and material bindings for efficient CPU-side culling and draw-call assembly.
- **[[Lighting Model]]** — mathematical description of surface reflectance: Blinn-Phong, Cook-Torrance BRDF, Disney's principled BSDF, or volumetric phase functions for participating media.
- **[[Global Illumination]]** — simulation of indirect light bouncing between surfaces, achieved via path tracing, radiosity, light probes, irradiance caching, or screen-space approximations such as SSAO and SSGI.
- **[[Anti-Aliasing]]** — techniques (MSAA, TAA, DLSS, FSR, XeSS) that remove staircase artefacts from undersampled edges and fine geometric detail.
- **[[Level of Detail]] (LOD)** — substituting lower-resolution geometry meshes or impostor billboards at distance to reduce GPU load.
- **[[Shader Programming]]** — authored in GLSL, HLSL, MSL, or SPIR-V, allowing artists and engineers to define arbitrary per-vertex and per-pixel programs.

### Mechanisms

#### Real-Time Rasterization Pipeline

- Application stage (CPU): frustum culling, batching, draw-call submission via [[Vulkan]], DirectX 12, Metal, or WebGPU.
- Vertex stage (GPU): MVP matrix transform from model space to clip space; skinning for skeletal animation.
- Primitive assembly and clipping: assembles triangles; clips against the view frustum.
- Rasterization: triangle coverage testing; generates screen-space fragments with interpolated attributes (UVs, normals, tangents).
- Fragment stage: executes [[Pixel Shader]] to sample textures, evaluate [[Lighting Model]], and output HDR colour.
- Post-processing: tone mapping, bloom, depth-of-field, temporal anti-aliasing, motion blur, and chromatic aberration.

#### Ray Tracing and Path Tracing

- Primary ray generation: one ray per pixel from the virtual camera.
- BVH (Bounding Volume Hierarchy) traversal: accelerates ray-triangle intersection tests across millions of polygons.
- Material evaluation: BSDF sampling determines reflected or refracted ray direction and weight.
- Russian roulette termination and next-event estimation: statistical techniques to achieve unbiased convergence with bounded ray depth.
- [[Path Tracing]] yields unbiased [[Global Illumination]] but requires many samples per pixel; Monte Carlo denoising (OptiX AI, Intel Open Image Denoise) reduces noise at low sample counts.

#### Neural and Hybrid Methods

- **[[Neural Radiance Field]] (NeRF)**: volumetric neural scene representation trained from multi-view images; differentiable rendering enables view synthesis without explicit geometry.
- **[[3D Gaussian Splatting]]**: point-cloud of oriented Gaussians rasterized with alpha blending; faster training and real-time inference than NeRF.
- **AI upscaling** (DLSS 3, FSR 3, XeSS): convolutional or transformer-based super-resolution reconstructs high-resolution frames from lower-resolution rasterized input, dramatically reducing GPU load.
- **[[Differentiable Rendering]]**: gradients flow back through the render equation, enabling inverse rendering (recovering 3D scene parameters from images) and training generative 3D models.

### Applications / Use Cases

- **Games and Interactive Media** — Real-time rasterization with hybrid ray tracing powers titles on PC and consoles, targeting 60–120 fps at 4K. PBR material workflows (Unreal Engine, Unity) standardise asset authoring.
- **Film and Animation VFX** — Offline [[Path Tracing]] (Arnold, RenderMan, Cycles, Manuka) produces photorealistic imagery for feature films and commercials with multi-hour per-frame budgets.
- **Architectural and Product Visualisation** — Interactive GPU path tracers (NVIDIA Omniverse, Chaos V-Ray RT) provide real-time design feedback for architects and industrial designers.
- **[[Virtual Reality]] and [[Augmented Reality]]** — Stereo rendering at 90–120 fps per eye for HMDs; [[Foveated Rendering]] concentrates compute at the gaze point detected by eye tracking.
- **Scientific and Medical Visualisation** — Volume rendering of CT/MRI data using ray marching and transfer functions; molecular visualisation for structural biology.
- **Autonomous Vehicles** — Synthetic data generation for training perception models; photorealistic simulation of sensor streams (cameras, LiDAR) via differentiable renderers.
- **Digital Twins and [[Spatial Computing]]** — Persistent, physically accurate 3D models of real-world environments rendered in real time for industrial monitoring and urban planning.
- **Generative AI Content Pipelines** — Text-to-3D and image-to-3D systems use [[Differentiable Rendering]] and Score Distillation Sampling to optimise 3D representations guided by 2D diffusion models.

### Standards & Context

- **APIs and Shading Languages**
  - [[OpenGL]] / GLSL — cross-platform rasterization API maintained by Khronos Group; foundational standard since 1992.
  - [[Vulkan]] — low-overhead, explicit GPU API (Khronos, 2016); exposes ray tracing via VK_KHR_ray_tracing_pipeline extension.
  - DirectX 12 / HLSL — Microsoft's low-level API with DirectX Raytracing (DXR) extension.
  - Metal / MSL — Apple's GPU API for macOS and iOS.
  - [[WebGPU]] — W3C standard bringing modern GPU access to web browsers, superseding WebGL.
  - SPIR-V — intermediate bytecode for portable shaders across Vulkan and OpenCL, defined by Khronos.
- **Rendering Standards and Interchange**
  - USD (Universal Scene Description) — Pixar/OpenUSD Alliance open format for scene interchange across DCC tools and renderers.
  - MaterialX — Academy Software Foundation standard for portable material definitions.
  - glTF 2.0 — Khronos runtime 3D asset format with PBR material model; widely used for web and real-time delivery.
  - OpenEXR — Academy Software Foundation HDR image format standard for production rendering output.
- **Governance and Bodies**
  - Khronos Group — manages OpenGL, Vulkan, WebGL, WebGPU, glTF, SPIR-V, OpenXR.
  - Academy Software Foundation (ASWF) — governs OpenVDB, OpenEXR, MaterialX, OpenColorIO for VFX production.
  - W3C WebGPU Working Group — browser-based GPU standard.

### Current Landscape (2026)

- Radiance-field rendering has gone mainstream: 3D Gaussian Splatting (Kerbl et al., SIGGRAPH 2023) now trains a scene in 10-30 minutes on a single RTX 5080 and rasterises at 120+ FPS in 4K, decisively displacing slower ray-marched NeRFs for real-time reconstruction while NeRFs retain edges in memory-efficiency and cross-scene priors.
- Standardisation arrived in 2026: Khronos announced the KHR_gaussian_splatting extension for glTF 2.0 in February 2026 (release-candidate stage, full ratification expected Q2 2026), while the Alliance for OpenUSD advances a Particle Fields schema for splats and OGC 3D Tiles 2.0 adds Gaussian splats as a first-class tile type.
- Real-time path tracing became production-grade: NVIDIA's RTX Kit (v2025.3) and the NvRTX 5.6 branch of Unreal Engine bring RTX Mega Geometry (ray tracing of full-quality Nanite geometry) and ReSTIR PT; the November 2025 "Bonsai" demo hit 60 FPS at 2160p (upscaled from 1080p) but required an RTX 5090.
- Neural rendering reshaped the pipeline: DLSS 4 added Multi Frame Generation (up to three generated frames per rendered frame on GeForce RTX 50-series) plus a transformer-based super-resolution and ray-reconstruction model, and RTX Neural Shaders now train tiny networks inside shaders for texture compression and material evaluation.
- Engine-level splat support shipped natively: Unity 6.2 and Unreal Engine 5.6 both include native Gaussian runtimes, with NVIDIA Omniverse carrying USD-native splats for simulation-grade digital twins.
- Research pushed splats past their early limits with relightable Gaussians (GS-IR, Relightable 3DGS), dynamic/4D temporal splats, animatable Gaussian avatars from Meta and Microsoft, and generative 3DGS from single prompts (Luma Genie, Tencent Hunyuan3D); optimisation work such as Faster-GS (CVPR 2026) cut training to under two minutes with ~5x speed-ups and 30% less VRAM.
- Capital and open challenges: World Labs raised roughly $1bn in February 2026 (backers including NVIDIA, AMD, Autodesk) to pursue spatial intelligence, but format fragmentation (PLY still dominant pre-ratification), relighting and material decomposition, level-of-detail streaming for large scenes on constrained devices, and editability of learned primitives remain unsolved frontiers.

### References

- 1. The Future 3D (2026). The State of Gaussian Splatting in 2026: Standards and Tools. https://www.thefuture3d.com/blog/state-of-gaussian-splatting-2026/
- 2. Internet Pros (2026). 3D Gaussian Splatting 2026 - Neural Rendering Revolution. https://internet-pros.com/blog/gaussian-splatting-neural-rendering-2026/
- 3. NVIDIA Developer (2025). NVIDIA Releases RTX Neural Rendering Tech for Unreal Engine Developers (RTX Kit v2025.3). https://developer.nvidia.com/blog/nvidia-releases-rtx-neural-rendering-tech-for-unreal-engine-developers/
- 4. Digital Foundry (2025). RTX Mega Geometry in our hands: Nvidia's new UE5 path tracing demo. https://www.digitalfoundry.net/news/2025/11/rtx-mega-geometry-in-our-hands-nvidias-new-ue5-path-tracing-demo
- 5. Hahlbohm et al. (2026). Faster-GS: Analyzing and Improving Gaussian Splatting Optimization (CVPR 2026). https://openaccess.thecvf.com/content/CVPR2026/papers/Hahlbohm_Faster-GS_Analyzing_and_Improving_Gaussian_Splatting_Optimization_CVPR_2026_paper.pdf

### Provenance
