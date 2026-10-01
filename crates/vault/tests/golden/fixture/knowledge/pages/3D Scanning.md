---
type: Class
title: 3D Scanning
slug: 3-d-scanning
resource: urn:ngm:class:3-d-scanning
page_resource: urn:visionflow:page:3-d-scanning
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
- '[[3D Reconstruction]]'
bridges-to:
- '[[robotics-perception|Robotics Perception]]'
- '[[medical-imaging|Medical Imaging]]'
contrasts-with:
- '[[structure-from-motion|Structure-from-Motion]]'
- '[[neural-radiance-field|Neural Radiance Field]]'
enables:
- '[[3D Scene Reconstruction]]'
- '[[digital-twin-creation|Digital Twin Creation]]'
- '[[point-cloud-processing|Point Cloud Processing]]'
- '[[reverse-engineering|Reverse Engineering]]'
- '[[digital-heritage|Heritage Digitisation]]'
has-part:
- '[[point-cloud|Point Cloud]]'
- '[[mesh-generation|Mesh Generation]]'
related-to:
- '[[3D Perception]]'
- '[[gaussian-splatting|Gaussian Splatting]]'
- '[[simultaneous-localisation-and-mapping|Simultaneous Localisation and Mapping]]'
requires:
- '[[depth-sensor|Depth Sensor]]'
- '[[calibration|Calibration]]'
supports:
- '[[building-information-modelling|Building Information Modelling]]'
- '[[quality-inspection|Quality Inspection]]'
- '[[augmented-reality|Augmented Reality]]'
uses:
- '[[3D LiDAR]]'
- '[[photogrammetry|Photogrammetry]]'
- '[[structured-light|Structured Light]]'
- '[[time-of-flight-sensor|Time-of-Flight Sensor]]'
---

3D Scanning is the process of capturing the three-dimensional shape, and optionally the colour and texture, of real-world objects, people, or environments using hardware such as structured-light scanners, time-of-flight LiDAR, photogrammetry rigs, or depth cameras, producing digital point clouds or meshes that represent the physical source. The resulting data feeds into digital preservation, reverse engineering, visual-effects production, quality inspection, and spatial computing pipelines. Accuracy, resolution, and scan volume are the primary quality axes that distinguish scanning technologies.

### Content

- Industrial 3D scanning began with coordinate measuring machines (CMMs) in the 1960s and early laser triangulation scanners developed for aerospace quality control in the 1970s–1980s. The first commercial structured-light scanners appeared in the 1990s, enabling sub-millimetre accuracy for reverse engineering and medical prosthetics. Ground-penetrating and airborne LiDAR scanning were adopted for large-scale topographic survey and archaeological documentation from the late 1990s onwards.
- Modern 3D scanning technologies span a resolution and range continuum: handheld structured-light scanners (e.g., Artec Eva) capture objects up to a few metres with sub-millimetre precision; terrestrial LiDAR systems (e.g., Leica BLK, FARO Focus) scan architectural spaces to centimetre accuracy at ranges of hundreds of metres; and aerial/mobile mapping rigs mount LiDAR alongside GNSS/IMU for kilometre-scale terrain capture. Photogrammetry using drone imagery and consumer cameras has become a high-throughput complement, producing textured meshes from photo collections with software such as Agisoft Metashape and RealityCapture.
- Applications of 3D scanning span heritage preservation (scanning of artefacts and monuments before restoration or replication), film and games production (body and face scanning for digital doubles), construction and BIM (as-built capture for project verification), automotive and aerospace quality control, and medical imaging (orthopaedic implant fitting and surgical planning). The scan-to-BIM workflow, which converts point-cloud data into parametric building information models, has become a standard practice in architecture and facilities management.
- In 2024–2025, smartphone-class depth sensing (using LiDAR on iPhone and iPad Pro, and structured-light face scanners) has democratised basic 3D scanning. Gaussian splatting and NeRF-based reconstruction tools allow high-quality captures from video sequences without dedicated scanning hardware. AI-driven hole-filling and noise-reduction are incorporated into standard post-processing tools. Integration between scanning platforms and digital-twin management software is maturing, and real-time collaborative scanning workflows are enabling distributed capture projects.
