---
public: true
---

# 3GPP
```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:page:3-gpp",
  "@type": "Page",
  "vc:slug": "3-gpp",
  "title": "3GPP",
  "vc:public": true,
  "vc:outboundWikilinks": [],
  "vc:schemaVersion": 2
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:3-gpp",
  "@type": "Class",
  "label": "3GPP",
  "definition": "The 3rd Generation Partnership Project (3GPP) is a global consortium of regional telecommunications standards bodies that develops and maintains versioned technical specifications governing mobile communication systems, from UMTS (3G) through LTE (4G) to NR (5G) and the emerging 6G framework. Operating through working groups (RAN, SA, CT), it produces Releases that introduce new radio access technologies, core network architectures, and service-layer capabilities. 3GPP specifications are the foundational technical substrate for the global cellular infrastructure serving billions of devices.",
  "domain": "standards",
  "maturity": "mature",
  "subClassOf": {
    "@id": "urn:ngm:class:standards-body",
    "label": "Standards Body"
  },
  "relations": {
    "hasPart": [
      {
        "@id": "urn:ngm:class:radio-access-network",
        "label": "Radio Access Network"
      },
      {
        "@id": "urn:ngm:class:5g-core-network",
        "label": "5G Core Network"
      },
      {
        "@id": "urn:ngm:class:5g-new-radio",
        "label": "5G New Radio"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:network-function-virtualization",
        "label": "Network Function Virtualization"
      },
      {
        "@id": "urn:ngm:class:multi-access-edge-computing",
        "label": "MultiAccessEdgeComputing"
      },
      {
        "@id": "urn:ngm:class:internet-of-things",
        "label": "Internet of Things"
      },
      {
        "@id": "urn:ngm:class:vehicle-to-everything",
        "label": "Vehicle-to-Everything"
      },
      {
        "@id": "urn:ngm:class:network-slicing",
        "label": "Network Slicing"
      }
    ],
    "implements": [
      {
        "@id": "urn:ngm:class:5-g-network",
        "label": "5G Network"
      },
      {
        "@id": "urn:ngm:class:5-g-connectivity",
        "label": "5G Connectivity"
      }
    ],
    "requires": [
      {
        "@id": "urn:ngm:class:spectrum-allocation",
        "label": "Spectrum Allocation"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:low-latency",
        "label": "Ultra-Reliable Low-Latency Communications"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:etsi",
        "label": "ETSI"
      },
      {
        "@id": "urn:ngm:class:telecommunications",
        "label": "Telecommunications"
      },
      {
        "@id": "urn:ngm:class:network-architecture",
        "label": "Network Architecture"
      },
      {
        "@id": "urn:ngm:class:itu-r",
        "label": "ITU-R"
      },
      {
        "@id": "urn:ngm:class:ieee",
        "label": "IEEE"
      }
    ],
    "bridgesTo": [
      {
        "@id": "urn:ngm:class:spatial-computing",
        "label": "Spatial Computing"
      },
      {
        "@id": "urn:ngm:class:digital-twin",
        "label": "Digital Twin"
      },
      {
        "@id": "urn:ngm:class:artificial-intelligence",
        "label": "Artificial Intelligence"
      }
    ]
  },
  "sameAs": [
    {
      "@id": "urn:ngm:class:third-generation-partnership-project",
      "label": "Third Generation Partnership Project"
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

- ### Definition
  - The 3rd Generation Partnership Project (3GPP) is an umbrella standards organisation that unites regional telecommunications standards bodies — including ETSI (Europe), ARIB and TTC (Japan), ATIS (USA), TSDSI (India), CCSA (China), and TTAS (Korea) — to produce globally harmonised technical specifications for mobile networks. Each Release cycle (Release 15 onwards for 5G NR) introduces new capabilities, from enhanced mobile broadband and ultra-reliable low-latency communications to massive machine-type communications. 3GPP specifications feed into [[5G Network]] deployment through radio access, core network, and service-layer standards that underpin [[Telecommunications]] infrastructure worldwide.

- ### Relationships
  - 3GPP has standardised [[5G Network]] and [[5G Connectivity]] through its New Radio (NR) and 5G Core specifications. Its work enables [[Network Function Virtualization]] in 5G Core, supports [[MultiAccessEdgeComputing]] via service-layer APIs, and underpins [[Internet of Things]] connectivity through NB-IoT and LTE-M standards. It is closely related to [[ETSI]] as a partner organisation, to [[Telecommunications]] as the overarching domain, and to [[Network Architecture]] as the design framework its specifications instantiate.

- ### Content
  - 3GPP was founded in December 1998 as a collaboration between regional standards development organisations aiming to produce global 3G mobile specifications. Its first major output, UMTS (Universal Mobile Telecommunications System, Release 99), standardised wideband CDMA radio access for 3G networks. Subsequent releases through the 2000s extended UMTS with HSPA (high-speed packet access) enhancements before the pivotal shift to LTE (Long-Term Evolution) beginning with Release 8 in 2008, establishing the Evolved Packet Core architecture and OFDMA radio access.

  - The 5G standardisation cycle began with Release 15 (frozen 2018), specifying 5G NR non-standalone and standalone operation on sub-6 GHz and millimetre-wave spectrum. Release 16 (2020) added ultra-reliable low-latency communications (URLLC) enhancements, vehicle-to-everything (V2X) sidelink, and industrial IoT features. Release 17 (2022) introduced NR-Light (RedCap) for IoT devices, NTN (non-terrestrial networks for satellite connectivity), and positioning enhancements enabling sub-metre accuracy. Release 18, branded 5G-Advanced, adds AI/ML-native radio network functions and extended reality traffic optimisations.

  - 3GPP's working group structure divides responsibility across radio access (RAN groups), service and system aspects (SA groups), and core network and terminals (CT groups). Contributions arrive from hundreds of companies including network equipment vendors (Ericsson, Nokia, Huawei, Samsung), operators (Deutsche Telekom, Verizon, China Mobile), and chipset manufacturers (Qualcomm, MediaTek). The specification process culminates in versioned Technical Specifications (TS) and Technical Reports (TR) that vendors implement in hardware and software.

  - In 2024–2025, 3GPP is advancing Release 18 deployment and beginning Release 19 (5G-Advanced phase 2) work, with AI-native air interface components, network energy efficiency mandates, and XR/holographic traffic profiles becoming central themes. Preliminary 6G research is coordinated through Study Items that will feed into a new generation standard targeted for commercial deployment around 2030. The organisation's influence extends into spatial computing, as 5G network capabilities for ultra-low latency and high-bandwidth underpin cloud-rendered XR experiences and real-time digital-twin synchronisation.

