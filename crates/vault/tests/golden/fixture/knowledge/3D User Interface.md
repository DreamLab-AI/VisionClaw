---
type: Class
title: 3D User Interface
slug: 3-d-user-interface
resource: urn:ngm:class:3-d-user-interface
page_resource: urn:visionflow:page:72b680895805b41a73d849ff6524c93b15e26499e2d4376fb4705af0814f6040
legacy-term-id: MV-9507
public: true
status: stable
domain: spatial-computing
maturity: draft
quality: 0.72
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[accessible-immersion|Accessible Immersion]]'
- '[[gaze-tracking|Gaze Tracking]]'
- '[[gesture-recognition|Gesture Recognition]]'
- '[[interaction-design|Interaction Design]]'
- '[[intuitive-interaction|Intuitive Interaction]]'
- '[[sensor-input|Sensor Input]]'
- '[[user-navigation|User Navigation]]'
- '[[voice-input|Voice Input]]'
- '[[accessibility-standard|Accessibility Standard]]'
- '[[hand-tracking|Hand Tracking]]'
- '[[haptic-feedback|Haptic Feedback]]'
- '[[user-interface|User Interface]]'
is-a:
- '[[sc-interaction|Interaction Technology]]'
bridges-to:
- '[[interaction-design|Interaction Design]]'
- '[[metaverse|Metaverse]]'
- '[[digital-twin|Digital Twin]]'
contrasts-with:
- '[[user-interface|User Interface]]'
- '[[graphical-user-interface|Graphical User Interface]]'
enables:
- '[[user-navigation|User Navigation]]'
- '[[immersive-experience|Immersive Experience]]'
has-part:
- '[[hand-tracking|Hand Tracking]]'
- '[[gaze-tracking|Gaze Tracking]]'
- '[[haptic-feedback|Haptic Feedback]]'
- '[[gesture-recognition|Gesture Recognition]]'
- '[[voice-input|Voice Input]]'
part-of:
- '[[spatial-computing|Spatial Computing]]'
- '[[extended-reality|Extended Reality]]'
related-to:
- '[[augmented-reality|Augmented Reality]]'
- '[[virtual-reality|Virtual Reality]]'
- '[[mixed-reality|Mixed Reality]]'
- '[[embodied-interaction|Embodied Interaction]]'
requires:
- '[[gaze-tracking|Gaze Tracking]]'
- '[[voice-input|Voice Input]]'
- '[[hand-tracking|Hand Tracking]]'
- '[[haptic-feedback|Haptic Feedback]]'
- '[[sensor-input|Sensor Input]]'
supports:
- '[[accessibility-standard|Accessibility Standard]]'
- '[[multimodal-interaction|Multimodal Interaction]]'
uses:
- '[[spatial-audio|Spatial Audio]]'
- '[[depth-sensing|Depth Sensing]]'
- '[[real-time-rendering|Real-Time Rendering]]'
---

A 3D User Interface (3DUI) is an interactive control system within three-dimensional virtual environments enabling users to manipulate objects, navigate spaces, and access functionality through spatial gestures, hand tracking, gaze-based selection, and voice commands. 3DUI design balances accessibility standards with intuitive spatial affordances appropriate for VR, AR, and metaverse interactions.

### Semantic Classification

### Content

## Overview

3D User Interfaces represent a fundamental shift from desktop UI paradigms, leveraging volumetric space, embodied interaction, and natural affordances. Design emphasises ergonomics, comfort (avoiding motion sickness and arm fatigue), and cognitive load management.

## Interaction Modalities

- **Hand Tracking**: Gesture-based selection and manipulation, enabling natural interaction
- **Gaze-Based Selection**: Focusing interaction through eye direction, crucial for accessibility
- **Voice Commands**: Natural language instruction enabling hands-free control
- **Haptic Feedback**: Tactile confirmation enhancing perceived interaction fidelity
- **Controllers**: Traditional handheld input for precise control and navigation

  ## Design Considerations

- Comfort and ergonomic positioning within reach and visibility cones
- Intuitive spatial metaphors avoiding cognitive friction
- Accessibility for users with motor and sensory impairments
- Motion sickness mitigation through stable visual anchors

  #### Related Concepts

- [[User Interface]], [[Gesture Recognition]], [[Accessibility Standard]], [[Haptic Feedback]], [[Sensor Input]]

### Current Landscape (2026)

- Apple's visionOS 26 (previewed WWDC June 2025, GA 15 September 2025) reframed 3D UI around persistent spatial widgets that anchor in a room and survive restarts, 3D-aware SwiftUI modifiers with depth-based layout, generative-AI "spatial scenes" from 2D photos, and Safari inline 3D models — with the "Liquid Glass" design language now propagated across all Apple OSes as a spatialised aesthetic.
- visionOS 27 (WWDC 2026) pushed 3D interaction further with real-time RealityKit rendering of 3D Gaussian Splats, physical-space lighting that casts virtual light onto real surfaces, a Foveated Streaming framework (introduced in 26.4) that integrates with the NVIDIA CloudXR SDK for OpenXR endpoints, and spatial-accessory input such as the Logitech Muse stylus and PlayStation VR2 Sense controllers.
- Android XR (announced 12 December 2024 by Google, Samsung and Qualcomm) shipped in a consumer device: the Samsung Galaxy XR (formerly Project Moohan) launched 21 October 2025 at 1,799 US dollars, running Gemini as a system-level assistant and adopting OpenXR and ARCore, so ordinary 2D Google Play apps spatialise into resizable floating panels.
- A pricing and competitive split hardened through 2026: the Galaxy XR at 1,799 dollars undercuts the M5 Apple Vision Pro, which rose to 3,699 dollars for 256 GB after a 25 June 2026 increase; Meta opened Horizon OS to ASUS, Lenovo and Xbox while XREAL's Aura took reservations at a sub-1,500-dollar ceiling.
- Platform 3D-UI design guidance has converged on concrete, quantified ergonomics: Meta's May 2026 Hands UI guidance sets a 48dp/~22mm minimum hit target, ~2.5–3 degree minimum angular collider and 42–46cm touch distance, while Android XR's spatial-UI guidance fixes panel spawn at 1.75m, a 41-degree comfort field of view and spatial elevation levels for orbiters, popups and dialogs.
- Interaction has settled on a tiered input model — direct touch/poke within arm's reach, stabilised ray casting for distant panels, plus eye tracking, microgestures and voice — with eye-tracked foveated rendering now a mainstream performance lever and a locked high frame rate (72fps+, sub-20ms motion-to-photon) treated as the primary comfort constraint.
- Standards remain anchored on the W3C WebXR Device API and Khronos OpenXR/glTF, but open challenges as of 2026 persist: hand-tracking precision and arm fatigue for sustained fine manipulation, battery life under two hours, absent enterprise MDM on new platforms, and privacy governance around always-on eye/gaze and passthrough-fed AI assistants.

### References

- 1. Apple Newsroom (2025). visionOS 26 introduces powerful new spatial experiences for Apple Vision Pro. https://www.apple.com/newsroom/2025/06/visionos-26-introduces-powerful-new-spatial-experiences-for-apple-vision-pro/
- 2. Apple Developer (2026). What's new in visionOS 27. https://developer.apple.com/visionos/whats-new/
- 3. Wikipedia (2025). Samsung Galaxy XR. https://en.wikipedia.org/wiki/Samsung_Galaxy_XR
- 4. 9to5Google (2025). Samsung's Galaxy XR is the first Android XR headset, now available. https://9to5google.com/2025/10/21/samsung-galaxy-xr-launch/
- 5. Meta Horizon Developers (2026). Hands UI best practices. https://developers.meta.com/horizon/design/hands-ui-best-practices/
- 6. Android Developers (2026). Spatial UI — XR Headsets and wired XR Glasses. https://developer.android.com/design/ui/xr/guides/spatial-ui

### Provenance
