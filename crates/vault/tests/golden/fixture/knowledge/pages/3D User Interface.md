---
public: true
---

# 3D User Interface
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:72b680895805b41a73d849ff6524c93b15e26499e2d4376fb4705af0814f6040",
  "@type": "Page",
  "vc:slug": "3-d-user-interface",
  "title": "3D User Interface",
  "vc:public": true,
  "vc:outboundWikilinks": [
    {
      "@id": "urn:visionflow:linked:accessible-immersion",
      "vc:label": "Accessible Immersion"
    },
    {
      "@id": "urn:visionflow:linked:gaze-tracking",
      "vc:label": "Gaze Tracking"
    },
    {
      "@id": "urn:visionflow:linked:gesture-recognition",
      "vc:label": "Gesture Recognition"
    },
    {
      "@id": "urn:visionflow:linked:interaction-design",
      "vc:label": "Interaction Design"
    },
    {
      "@id": "urn:visionflow:linked:intuitive-interaction",
      "vc:label": "Intuitive Interaction"
    },
    {
      "@id": "urn:visionflow:linked:sensor-input",
      "vc:label": "Sensor Input"
    },
    {
      "@id": "urn:visionflow:linked:user-navigation",
      "vc:label": "User Navigation"
    },
    {
      "@id": "urn:visionflow:linked:voice-input",
      "vc:label": "Voice Input"
    },
    {
      "@id": "urn:visionflow:owl:class:accessibility-standard",
      "vc:label": "Accessibility Standard"
    },
    {
      "@id": "urn:visionflow:owl:class:hand-tracking",
      "vc:label": "Hand Tracking"
    },
    {
      "@id": "urn:visionflow:owl:class:haptic-feedback",
      "vc:label": "Haptic Feedback"
    },
    {
      "@id": "urn:visionflow:owl:class:user-interface",
      "vc:label": "User Interface"
    }
  ],
  "vc:schemaVersion": 2,
  "vc:legacyProperties": [
    {
      "vc:key": "legacy-term-id",
      "vc:value": "MV-9507"
    },
    {
      "vc:key": "preferred-term",
      "vc:value": "3D User Interface"
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
  "@id": "urn:ngm:class:3-d-user-interface",
  "@type": "Class",
  "label": "3D User Interface",
  "definition": "A 3D User Interface (3DUI) is an interactive control system within three-dimensional virtual environments enabling users to manipulate objects, navigate spaces, and access functionality through spatial gestures, hand tracking, gaze-based selection, and voice commands. 3DUI design balances accessibility standards with intuitive spatial affordances appropriate for VR, AR, and metaverse interactions.",
  "domain": "spatial-computing",
  "maturity": "draft",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:sc-interaction",
      "label": "Interaction Technology"
    }
  ],
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:hand-tracking",
        "label": "Hand Tracking"
      },
      {
        "@id": "urn:ngm:class:gaze-tracking",
        "label": "Gaze Tracking"
      },
      {
        "@id": "urn:ngm:class:haptic-feedback",
        "label": "Haptic Feedback"
      },
      {
        "@id": "urn:ngm:class:gesture-recognition",
        "label": "Gesture Recognition"
      },
      {
        "@id": "urn:ngm:class:voice-input",
        "label": "Voice Input"
      }
    ],
    "partOf": [
      {
        "@id": "urn:ngm:class:spatial-computing",
        "label": "Spatial Computing"
      },
      {
        "@id": "urn:ngm:class:extended-reality",
        "label": "Extended Reality"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:gaze-tracking",
        "label": "Gaze Tracking"
      },
      {
        "@id": "urn:ngm:class:voice-input",
        "label": "Voice Input"
      },
      {
        "@id": "urn:ngm:class:hand-tracking",
        "label": "Hand Tracking"
      },
      {
        "@id": "urn:ngm:class:haptic-feedback",
        "label": "Haptic Feedback"
      },
      {
        "@id": "urn:ngm:class:sensor-input",
        "label": "Sensor Input"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:user-navigation",
        "label": "User Navigation"
      },
      {
        "@id": "urn:ngm:class:immersive-experience",
        "label": "Immersive Experience"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:spatial-audio",
        "label": "Spatial Audio"
      },
      {
        "@id": "urn:ngm:class:depth-sensing",
        "label": "Depth Sensing"
      },
      {
        "@id": "urn:ngm:class:real-time-rendering",
        "label": "Real-Time Rendering"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:accessibility-standard",
        "label": "Accessibility Standard"
      },
      {
        "@id": "urn:ngm:class:multimodal-interaction",
        "label": "Multimodal Interaction"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:user-interface",
        "label": "User Interface"
      },
      {
        "@id": "urn:ngm:class:graphical-user-interface",
        "label": "Graphical User Interface"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:interaction-design",
        "label": "Interaction Design"
      },
      {
        "@id": "urn:ngm:class:metaverse",
        "label": "Metaverse"
      },
      {
        "@id": "urn:ngm:class:digital-twin",
        "label": "Digital Twin"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:augmented-reality",
        "label": "Augmented Reality"
      },
      {
        "@id": "urn:ngm:class:virtual-reality",
        "label": "Virtual Reality"
      },
      {
        "@id": "urn:ngm:class:mixed-reality",
        "label": "Mixed Reality"
      },
      {
        "@id": "urn:ngm:class:embodied-interaction",
        "label": "Embodied Interaction"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:3dui",
      "label": "3DUI"
    },
    {
      "@id": "urn:ngm:class:spatial-user-interface",
      "label": "Spatial User Interface"
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
  "@id": "urn:visionflow:annotation:link-resolutions:3-d-user-interface:776c802a9fc9",
  "@type": "vc:LinkResolutionsAnnotation",
  "vc:appliesTo": {
    "@id": "urn:visionflow:page:72b680895805b41a73d849ff6524c93b15e26499e2d4376fb4705af0814f6040"
  },
  "vc:resolutions": [
    {
      "raw": "[[Accessible Immersion]]",
      "resolved": "urn:visionflow:linked:accessible-immersion",
      "kind": "StubLink"
    },
    {
      "raw": "[[Gaze Tracking]]",
      "resolved": "urn:visionflow:linked:gaze-tracking",
      "kind": "StubLink"
    },
    {
      "raw": "[[Gesture Recognition]]",
      "resolved": "urn:visionflow:linked:gesture-recognition",
      "kind": "StubLink"
    },
    {
      "raw": "[[Interaction Design]]",
      "resolved": "urn:visionflow:linked:interaction-design",
      "kind": "StubLink"
    },
    {
      "raw": "[[Intuitive Interaction]]",
      "resolved": "urn:visionflow:linked:intuitive-interaction",
      "kind": "StubLink"
    },
    {
      "raw": "[[Sensor Input]]",
      "resolved": "urn:visionflow:linked:sensor-input",
      "kind": "StubLink"
    },
    {
      "raw": "[[User Navigation]]",
      "resolved": "urn:visionflow:linked:user-navigation",
      "kind": "StubLink"
    },
    {
      "raw": "[[Voice Input]]",
      "resolved": "urn:visionflow:linked:voice-input",
      "kind": "StubLink"
    },
    {
      "raw": "[[Accessibility Standard]]",
      "resolved": "urn:visionflow:owl:class:accessibility-standard",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Hand Tracking]]",
      "resolved": "urn:visionflow:owl:class:hand-tracking",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[Haptic Feedback]]",
      "resolved": "urn:visionflow:owl:class:haptic-feedback",
      "kind": "ResolvedLink"
    },
    {
      "raw": "[[User Interface]]",
      "resolved": "urn:visionflow:owl:class:user-interface",
      "kind": "ResolvedLink"
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
  - A 3D User Interface (3DUI) is an interactive control system within three-dimensional virtual environments enabling users to manipulate objects, navigate spaces, and access functionality through spatial gestures, hand tracking, gaze-based selection, and voice commands. 3DUI design balances [[Accessibility Standard]]s with intuitive spatial affordances appropriate for VR, AR, and metaverse interactions.

- ### Semantic Classification
  - owl-class:: spatial-computing:3DUserInterface
  - owl-role:: concept

- ### Relationships
  - is-subclass-of:: [[User Interface]], [[Interaction Design]]
  - bridges-to:: [[Sensor Input]], [[Gesture Recognition]], [[Accessibility Standard]]
  - requires:: [[Hand Tracking]], [[Gaze Tracking]], [[Voice Input]], [[Haptic Feedback]]
  - enables:: [[Intuitive Interaction]], [[Accessible Immersion]], [[User Navigation]]

- ### Content

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

- ### Current Landscape (2026)
  - Apple's visionOS 26 (previewed WWDC June 2025, GA 15 September 2025) reframed 3D UI around persistent spatial widgets that anchor in a room and survive restarts, 3D-aware SwiftUI modifiers with depth-based layout, generative-AI "spatial scenes" from 2D photos, and Safari inline 3D models — with the "Liquid Glass" design language now propagated across all Apple OSes as a spatialised aesthetic.
  - visionOS 27 (WWDC 2026) pushed 3D interaction further with real-time RealityKit rendering of 3D Gaussian Splats, physical-space lighting that casts virtual light onto real surfaces, a Foveated Streaming framework (introduced in 26.4) that integrates with the NVIDIA CloudXR SDK for OpenXR endpoints, and spatial-accessory input such as the Logitech Muse stylus and PlayStation VR2 Sense controllers.
  - Android XR (announced 12 December 2024 by Google, Samsung and Qualcomm) shipped in a consumer device: the Samsung Galaxy XR (formerly Project Moohan) launched 21 October 2025 at 1,799 US dollars, running Gemini as a system-level assistant and adopting OpenXR and ARCore, so ordinary 2D Google Play apps spatialise into resizable floating panels.
  - A pricing and competitive split hardened through 2026: the Galaxy XR at 1,799 dollars undercuts the M5 Apple Vision Pro, which rose to 3,699 dollars for 256 GB after a 25 June 2026 increase; Meta opened Horizon OS to ASUS, Lenovo and Xbox while XREAL's Aura took reservations at a sub-1,500-dollar ceiling.
  - Platform 3D-UI design guidance has converged on concrete, quantified ergonomics: Meta's May 2026 Hands UI guidance sets a 48dp/~22mm minimum hit target, ~2.5–3 degree minimum angular collider and 42–46cm touch distance, while Android XR's spatial-UI guidance fixes panel spawn at 1.75m, a 41-degree comfort field of view and spatial elevation levels for orbiters, popups and dialogs.
  - Interaction has settled on a tiered input model — direct touch/poke within arm's reach, stabilised ray casting for distant panels, plus eye tracking, microgestures and voice — with eye-tracked foveated rendering now a mainstream performance lever and a locked high frame rate (72fps+, sub-20ms motion-to-photon) treated as the primary comfort constraint.
  - Standards remain anchored on the W3C WebXR Device API and Khronos OpenXR/glTF, but open challenges as of 2026 persist: hand-tracking precision and arm fatigue for sustained fine manipulation, battery life under two hours, absent enterprise MDM on new platforms, and privacy governance around always-on eye/gaze and passthrough-fed AI assistants.

- ### References
  - 1. Apple Newsroom (2025). visionOS 26 introduces powerful new spatial experiences for Apple Vision Pro. https://www.apple.com/newsroom/2025/06/visionos-26-introduces-powerful-new-spatial-experiences-for-apple-vision-pro/
  - 2. Apple Developer (2026). What's new in visionOS 27. https://developer.apple.com/visionos/whats-new/
  - 3. Wikipedia (2025). Samsung Galaxy XR. https://en.wikipedia.org/wiki/Samsung_Galaxy_XR
  - 4. 9to5Google (2025). Samsung's Galaxy XR is the first Android XR headset, now available. https://9to5google.com/2025/10/21/samsung-galaxy-xr-launch/
  - 5. Meta Horizon Developers (2026). Hands UI best practices. https://developers.meta.com/horizon/design/hands-ui-best-practices/
  - 6. Android Developers (2026). Spatial UI — XR Headsets and wired XR Glasses. https://developer.android.com/design/ui/xr/guides/spatial-ui

- ### Provenance
  - sources::
  - migration-date:: 2026-04-26T00:00:00Z
