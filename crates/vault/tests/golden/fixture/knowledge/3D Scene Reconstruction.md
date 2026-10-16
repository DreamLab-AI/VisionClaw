---
type: Class
title: 3D Scene Reconstruction
aliases:
- Scene Reconstruction
slug: 3-d-scene-reconstruction
resource: urn:ngm:class:3-d-scene-reconstruction
page_resource: urn:visionflow:page:3-d-scene-reconstruction
public: true
status: stable
domain: spatial-computing
maturity: established
quality: 0.8
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[3D Reconstruction]]'
enables:
- '[[digital-twin-creation|Digital Twin Creation]]'
- '[[3D Perception]]'
- '[[ar-registration|AR Registration]]'
related-to:
- '[[3D Scanning]]'
- '[[ne-rf|NeRF]]'
- '[[gaussian-splatting|Gaussian Splatting]]'
uses:
- '[[simultaneous-localisation-and-mapping|Simultaneous Localisation and Mapping]]'
- '[[structure-from-motion|Structure-from-Motion]]'
- '[[multi-view-stereo|Multi-View Stereo]]'
---

3D Scene Reconstruction is the computational process of recovering a complete, coherent three-dimensional model of an environment or scene from sensor observations such as images, depth maps, or LiDAR returns, integrating multiple viewpoints and modalities into a unified representation. Methods include volumetric fusion, truncated signed distance function (TSDF) integration, neural implicit representations, and Gaussian splatting, each offering different trade-offs between completeness, accuracy, and real-time capability. The output is used in robotics, autonomous driving, spatial computing, and cultural heritage documentation.

### Content

- Systematic scene reconstruction research emerged alongside early computer vision in the 1970s–1980s, with the development of stereo vision and structure-from-motion algorithms. The KinectFusion system (Microsoft Research, 2011) was a landmark achievement demonstrating dense real-time TSDF volumetric fusion on a consumer GPU, enabling live reconstruction from a moving depth camera. This inspired subsequent work on large-scale reconstruction (ElasticFusion, TSDF++, InfiniTAM) that addressed memory and drift limitations.
- Modern pipelines are divided into offline batch reconstruction and online real-time methods. Offline systems such as COLMAP and OpenMVS process image collections through structure-from-motion followed by multi-view stereo, producing high-quality meshes suitable for film and digital heritage. Online systems operate on live sensor streams, maintaining probabilistic models of scene geometry that are updated as new observations arrive. Loop closure detection corrects accumulated drift by recognising previously visited locations and applying graph optimisation.
- Neural reconstruction methods have transformed the state of the art since 2020. NeRF learns a continuous volumetric radiance field from posed images, yielding photo-realistic novel-view synthesis from arbitrary camera positions. Gaussian splatting (2023) represents scenes as collections of 3D Gaussians with learnable colour and opacity, enabling real-time rendering while preserving reconstruction quality. These methods are displacing classical mesh-based pipelines for visual-effects and spatial computing workflows that prioritise appearance fidelity over explicit geometric access.
- Through 2024–2025, scene reconstruction is integrating with semantic understanding: systems now simultaneously recover geometry and assign semantic labels to regions, enabling navigation-ready scene graphs. Foundation-model-guided priors improve reconstruction quality in occluded and low-texture regions. Reconstruction from moving video on mobile devices (using LiDAR-equipped smartphones) enables widespread capture without specialised equipment. Industrial digital-twin platforms are standardising on neural reconstruction pipelines for rapid site capture and ongoing change detection.
