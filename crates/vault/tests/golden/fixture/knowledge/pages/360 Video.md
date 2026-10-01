---
type: Class
title: 360 Video
resource: urn:ngm:class:360-video
page_resource: urn:visionflow:page:b0b671b647f173818c9b1ed7fd3026f3c7e5d1420f8502d76c23c5ce59bae03c
public: true
status: stable
domain: spatial-computing
maturity: mature
quality: 0.8
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[immersive-media|Immersive Media]]'
- '[[vr-experiences|VR Experiences]]'
- '[[volumetric-video|Volumetric Video]]'
- '[[virtual-reality|Virtual Reality]]'
is-a:
- '[[immersive-media|Immersive Media]]'
contrasts-with:
- '[[volumetric-video|Volumetric Video]]'
- '[[volumetric-capture|Volumetric Capture]]'
enables:
- '[[vr-experiences|VR Experiences]]'
related-to:
- '[[virtual-reality|Virtual Reality]]'
uses:
- '[[video-streaming|Video Streaming]]'
---

A spherical video format captured with omnidirectional camera rigs that records the full 360-degree field of view around a single vantage point, allowing viewers to look in any direction during playback on headsets, browsers, or mobile devices. Because the footage is fixed to the capture position, it offers rotational (three-degrees-of-freedom) immersion but no positional movement, distinguishing it from volumetric formats that reconstruct scene geometry.

### Semantic Classification

### Content

## Definition

**360 Video** (also called spherical or omnidirectional video) is recorded with multi-lens camera rigs whose overlapping views are stitched into a single equirectangular or cubemap projection covering the entire sphere around the camera. During playback the viewer controls the viewing direction — by dragging in a browser, moving a phone, or turning their head in a VR headset — while the playback engine reprojects the relevant portion of the sphere into a conventional rectilinear viewport.

The format's defining constraint is its three-degrees-of-freedom (3DoF) nature: viewers can rotate (yaw, pitch, roll) but cannot translate, because the imagery exists only for the original camera position. This makes 360 video far cheaper to produce and distribute than six-degrees-of-freedom volumetric media, at the cost of weaker presence and no motion parallax. Within this graph it therefore serves as the canonical contrast case for [[Volumetric Video]] and volumetric capture pipelines, and as a widely deployed building block of [[VR Experiences]] such as virtual tours, immersive journalism, training scenarios, and live event coverage.

Monoscopic 360 video presents the same sphere to both eyes; stereoscopic variants capture separate per-eye spheres for depth perception. Spatial (ambisonic) audio is commonly paired with the format so that sound sources remain anchored to scene directions as the viewer looks around.

## Technical Details

- **Projection formats**: equirectangular (dominant), cubemap, and equi-angular cubemap (EAC, used by YouTube) trade pixel-density uniformity against encoder compatibility.
- **Capture**: dual-fisheye consumer cameras (Insta360, Ricoh Theta) through professional multi-camera rigs; stitching performed on-device or in post-production.
- **Resolution**: because pixels are spread across the full sphere, 8K spherical resolution yields only roughly HD quality in the visible viewport, driving demand for 8K+ capture.
- **Delivery**: standard H.264/H.265/AV1 codecs over conventional streaming stacks; viewport-adaptive tiled streaming reduces bandwidth by prioritising the region being watched.
- **Standards**: MPEG Omnidirectional Media Format (OMAF, ISO/IEC 23090-2) specifies projection, packing, and signalling for spherical media; WebXR and platform players (YouTube VR, Meta Quest TV) provide playback.

  ## Current Landscape

- **Apple Projected Media Profile (June 2025)**: at WWDC 2025 Apple introduced APMP, an extension to the QuickTime/MP4 format describing lens projection parameters, so that 360 and wide-field-of-view footage from Insta360, GoPro, and Canon cameras plays back natively on Apple Vision Pro under visionOS 26 — a significant platform endorsement of the format.
- **8K became the consumer baseline in 2025**: the Insta360 X4/X5, DJI Osmo 360 (8K/50fps), and GoPro Max 2 all shipped 8K spherical capture, and Insta360's budget X4 Air retained 8K as a competitive essential.
- **Market scale**: industry estimates placed the global 360-degree camera market at roughly USD 1.5–2.4 billion in 2025, with forecast compound annual growth rates between about 12% and 28% depending on the analyst.
- **Resolution economics remain the constraint**: an 8K equirectangular sphere yields only ~4K in a 170° reframed view and ~1440p at a 110° linear field of view, which is why professional VR production is moving towards 12K+ capture as headset pixel density rises.

  **Sources**:

- https://tech.yahoo.com/cameras/articles/gopro-insta360-cameras-getting-massive-164146262.html
- https://www.digitalcameraworld.com/cameras/360-cameras/from-niche-to-necessary-this-is-why-360-cameras-took-off-in-2025
- https://xtendedview.com/360-camera-statistics/
- https://www.insta360.com/blog/tips/understanding-8K-360-video.html

### Provenance
