---
type: Class
title: 3D Reconstruction
slug: 3-d-reconstruction
resource: urn:ngm:class:3-d-reconstruction
page_resource: urn:visionflow:page:45c99e53e3a503b2c170936a5b97d46f4c73c5007d0b6eb74d4a0e7a7513bd86
legacy-term-id: MV-9506
public: true
status: stable
domain: spatial-computing
maturity: draft
quality: 0.72
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[camera-calibration|Camera Calibration]]'
- '[[environmental-mapping|Environmental Mapping]]'
- '[[feature-matching|Feature Matching]]'
- '[[image-processing|Image Processing]]'
- '[[point-cloud-generation|Point Cloud Generation]]'
- '[[real-world-digitisation|Real-world Digitisation]]'
- '[[structure-from-motion|Structure-from-Motion]]'
- '[[computer-vision|Computer Vision]]'
- '[[digital-twin|Digital Twin]]'
- '[[photogrammetry|Photogrammetry]]'
- '[[point-cloud|Point Cloud]]'
is-a:
- '[[sc-content-and-assets|Content and Assets]]'
bridges-to:
- '[[computer-vision|Computer Vision]]'
- '[[robotics|Robotics]]'
- '[[machine-learning|Machine Learning]]'
depends-on:
- '[[computer-vision|Computer Vision]]'
- '[[deep-learning|Deep Learning]]'
enables:
- '[[environmental-mapping|Environmental Mapping]]'
- '[[point-cloud-generation|Point Cloud Generation]]'
- '[[digital-twin|Digital Twin]]'
- '[[augmented-reality|Augmented Reality]]'
has-part:
- '[[structure-from-motion|Structure-from-Motion]]'
- '[[multi-view-stereo|Multi-View Stereo]]'
- '[[point-cloud|Point Cloud]]'
- '[[depth-estimation|Depth Estimation]]'
part-of:
- '[[photogrammetry|Photogrammetry]]'
related-to:
- '[[spatial-computing|Spatial Computing]]'
- '[[scene-understanding|Scene Understanding]]'
requires:
- '[[camera-calibration|Camera Calibration]]'
- '[[feature-matching|Feature Matching]]'
- '[[image-processing|Image Processing]]'
- '[[sensor-fusion|Sensor Fusion]]'
uses:
- '[[lidar|Lidar]]'
- '[[neural-radiance-field|Neural Radiance Field]]'
- '[[simultaneous-localisation-and-mapping|Simultaneous Localisation and Mapping]]'
---

3D Reconstruction is the computational process of recovering three-dimensional geometric and structural information from multiple 2D images or sensor data (such as LiDAR or depth cameras) using techniques including Computer Vision, photogrammetry, and Structure-from-Motion (SfM), enabling digital capture of real-world objects and environments for Digital Twin creation, immersive environment mapping, and spatial analysis.

### Semantic Classification

### Content

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

### Provenance
