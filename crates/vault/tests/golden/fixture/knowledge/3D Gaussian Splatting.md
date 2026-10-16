---
type: Class
title: 3D Gaussian Splatting
aliases:
- TELE-051-3d-gaussian-splatting
slug: 3-d-gaussian-splatting
resource: urn:ngm:class:3-d-gaussian-splatting
page_resource: urn:visionflow:page:f32f0472a5ee16559fe85eb785fa81ec86cd50b3fe51d1d813e71a88fc87fac7
legacy-term-id: TC-0051
public: true
status: stable
domain: spatial-computing
maturity: emerging
quality: 0.72
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[differentiable-rendering|DifferentiableRendering]]'
- '[[photorealistic-telepresence|PhotorealisticTelepresence]]'
- '[[tele-020-virtual-reality-telepresence|TELE-020-virtual-reality-telepresence]]'
- '[[tele-050-neural-rendering-telepresence|TELE-050-neural-rendering-telepresence]]'
- '[[tele-052-neural-radiance-fields|TELE-052-neural-radiance-fields]]'
- '[[tele-053-volumetric-video-conferencing|TELE-053-volumetric-video-conferencing]]'
- '[[tele-060-instant-ngp|TELE-060-instant-ngp]]'
is-a:
- '[[neural-rendering|Neural Rendering]]'
bridges-to:
- '[[tele-050-neural-rendering-telepresence|TELE-050-neural-rendering-telepresence]]'
- '[[extended-reality|Extended Reality]]'
- '[[web-xr|WebXR]]'
contrasts-with:
- '[[neural-radiance-fields|Neural Radiance Fields]]'
- '[[tele-060-instant-ngp|Instant NGP]]'
depends-on:
- '[[multi-view-photography|Multi-View Photography]]'
- '[[point-cloud|Point Cloud]]'
- '[[adam-optimiser|Adam Optimiser]]'
enables:
- '[[photorealistic-telepresence|Photorealistic Telepresence]]'
- '[[tele-053-volumetric-video-conferencing|Volumetric Video Conferencing]]'
- '[[novel-view-synthesis|Real-Time Novel View Synthesis]]'
- '[[avatar|Photorealistic Avatar]]'
has-part:
- '[[gaussian-primitive|Gaussian Primitive]]'
- '[[spherical-harmonics|Spherical Harmonics]]'
implements:
- '[[alpha-blending|Alpha Blending]]'
part-of:
- '[[novel-view-synthesis|Novel View Synthesis]]'
related-to:
- '[[virtual-reality-telepresence|Virtual Reality Telepresence]]'
- '[[3D Scene Reconstruction|Scene Reconstruction]]'
requires:
- '[[differentiable-rendering|Differentiable Rendering]]'
- '[[structure-from-motion|Structure-from-Motion]]'
uses:
- '[[stochastic-gradient-descent|Stochastic Gradient Descent]]'
- '[[ssim-loss|SSIM Loss]]'
---

A neural rendering technique that represents 3D scenes as collections of millions of 3D Gaussian primitives with learnable positions, colours, opacities, and covariances, enabling photorealistic real-time rendering at 100+ frames per second through GPU-accelerated rasterisation, revolutionising telepresence and immersive collaboration with unprecedented visual fidelity.

### Semantic Classification

### Content

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

### Provenance
