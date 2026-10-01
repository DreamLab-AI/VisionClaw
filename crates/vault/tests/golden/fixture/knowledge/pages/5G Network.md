---
type: Class
title: 5G Network
aliases:
- 5G Networks
slug: 5-g-network
resource: urn:ngm:class:5-g-network
page_resource: urn:visionflow:page:5-g-network
public: true
status: stable
domain: infrastructure
maturity: established
quality: 0.74
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links: []
is-a:
- '[[telecommunications-infrastructure|Telecommunications Infrastructure]]'
bridges-to:
- '[[6G Network Slice]]'
- '[[artificial-intelligence-operations|AI-Driven Network Operations]]'
- '[[digital-twin|Digital Twin]]'
contrasts-with:
- '[[wi-fi|Wi-Fi]]'
enables:
- '[[5G Connectivity]]'
- '[[io-t-infrastructure|IoT Infrastructure]]'
- '[[autonomous-vehicle|Autonomous Vehicles]]'
- '[[extended-reality|Extended Reality]]'
has-part:
- '[[5G|5G New Radio]]'
- '[[multi-access-edge-computing|MultiAccessEdgeComputing]]'
- '[[network-slicing|Network Slicing]]'
related-to:
- '[[network-infrastructure|Network Infrastructure]]'
- '[[open-ran|Open RAN]]'
- '[[mobile-edge-computing|Mobile Edge Computing]]'
requires:
- '[[spectrum-allocation|Spectrum Allocation]]'
- '[[cloud-native-architecture|Cloud-Native Architecture]]'
- '[[transport-network|Transport Network]]'
standardized-by:
- '[[3GPP]]'
- '[[o-ran-alliance|O-RAN Alliance]]'
- '[[etsi|ETSI]]'
uses:
- '[[network-function-virtualization|Network Function Virtualization]]'
- '[[software-defined-networking|Software-Defined Networking]]'
- '[[edge-computing|Edge Computing]]'
- '[[massive-mimo|Massive MIMO]]'
---

A 5G Network is the end-to-end fifth-generation mobile telecommunications system comprising a 5G New Radio (NR) radio access network built around gNB base stations, a cloud-native 5G Core implementing a service-based architecture with independently deployable network functions (AMF, SMF, UPF, PCF, NRF, AUSF), and the transport infrastructure interconnecting them. The architecture fundamentally separates the control plane from the user plane, enables network slicing to provision multiple virtualised logical networks on shared physical resources, and integrates multi-access edge computing nodes for deterministic low-latency application hosting. Defined across 3GPP Release 15 (frozen 2018) through Release 18 (5G-Advanced), 5G networks support three canonical service classes: enhanced Mobile Broadband (eMBB), Ultra-Reliable Low-Latency Communications (URLLC), and massive Machine-Type Communications (mMTC), making them the foundational connectivity layer for industrial IoT, autonomous systems, extended reality, and private campus networks.

### Overview

- 5G is the successor to [[4G LTE]], standardised by [[3GPP]] with Release 15 frozen in 2018. Unlike prior generations, the architecture was co-designed with virtualisation in mind from the outset:
  - The radio access network uses [[5G New Radio]], a new air interface operating across sub-6 GHz (FR1) and [[Millimetre Wave]] (FR2) spectrum bands.
  - The [[5G Core]] is a fully disaggregated, microservices-based system where each network function (AMF, SMF, UPF, PCF, NRF, AUSF, UDM) is a container-deployable software unit.
  - [[Software-Defined Networking]] principles govern traffic forwarding, enabling dynamic policy enforcement and user-plane function placement at the network edge.
  - [[Multi-Access Edge Computing]] (MEC) nodes co-located at or near base stations reduce application round-trip latency to single-digit milliseconds.
- Why it matters:
  - Enables [[Extended Reality]] (XR), cloud gaming, and holographic communications at scale.
  - Provides the connectivity substrate for [[Autonomous Vehicles]], smart infrastructure, and [[Digital Twin]] deployments requiring real-time sensing and control.
  - Private 5G deployments on licensed, shared, or unlicensed spectrum are replacing Wi-Fi in demanding industrial environments.
  - The [[O-RAN Alliance]] disaggregation initiative is restructuring the vendor ecosystem, enabling software-defined programmable RAN controllers (RIC) to optimise radio resources using machine-learning models.

### Key Components

#### Radio Access Network (RAN)

- **gNB base stations** implement the [[5G New Radio]] interface; they subdivide into CU (centralised unit), DU (distributed unit), and RU (radio unit) in O-RAN architectures.
- **[[Massive MIMO]]** — arrays of 32–256+ antenna elements enable spatial multiplexing and beamforming, dramatically increasing spectral efficiency versus 4G.
- **[[Millimetre Wave]]** bands (24–100 GHz) deliver multi-gigabit peak throughputs over short distances; sub-6 GHz bands provide wide-area coverage.
- **Small cells** — pico- and femtocells densify urban coverage and offload capacity from macro sites.
- **[[Open RAN]]** — the O-RAN Alliance defines open interfaces (fronthaul eCPRI, E2, O1, A1) enabling multi-vendor RAN decomposition and AI-driven RAN Intelligent Controllers (RICs).

#### 5G Core (5GC)

- **AMF** (Access and Mobility Management Function) — handles registration, authentication, and mobility.
- **SMF** (Session Management Function) — controls session establishment, UPF selection, and policy enforcement.
- **UPF** (User Plane Function) — the data-plane forwarder; can be instantiated at the edge for local breakout, reducing latency.
- **PCF** (Policy Control Function) — enforces QoS and charging policies.
- **NRF** (Network Repository Function) — service discovery registry for all core NFs.
- **AUSF/UDM** — authentication and subscriber data management.
- Functions communicate via a Service-Based Interface (SBI) over HTTP/2 + REST, containerised with [[Cloud-Native Architecture]] on Kubernetes.

#### [[Network Slicing]]

- Logically isolated end-to-end slices span RAN, transport, and core, each with independent QoS, security, and resource guarantees.
- Slice types: eMBB (broadband), URLLC (< 1 ms, 99.9999% reliability), mMTC (low-power, high-density devices).
- Network Slice Selection Assistance Information (NSSAI) signals which slice a device should use.

#### [[Multi-Access Edge Computing]]

- Compute resources physically distributed at the RAN edge (base station sites, aggregation nodes).
- Enables ultra-low-latency applications: real-time video analytics, AR overlays, v2x coordination.
- Standardised through ETSI MEC ISG; integrated with 5GC via LADN and Local Area Data Network configurations.

#### Transport Network

- Fronthaul (RU↔DU), midhaul (DU↔CU), and backhaul (CU↔5GC) segments.
- Uses fibre (preferred), microwave, and mmWave fixed-wireless links.
- Time-sensitive networking (TSN) and IEEE 1588 precision timing synchronise the distributed RAN.

### Applications and Use Cases

- **Enhanced Mobile Broadband (eMBB)**
  - Consumer streaming, fixed wireless access (FWA) replacing home broadband in areas with limited fibre.
  - Ultra-HD video, immersive [[Extended Reality]] (AR/VR/MR) over mobile devices.
- **Ultra-Reliable Low-Latency Communications (URLLC)**
  - Industrial automation: robotic arms, CNC machine control, collaborative mobile robots on factory floors.
  - [[Autonomous Vehicles]] and vehicle-to-everything (V2X) coordination over PC5 and Uu interfaces.
  - Remote surgery and tele-operation of heavy machinery.
  - Power grid protection with sub-millisecond fault detection and isolation.
- **Massive Machine-Type Communications (mMTC)**
  - [[IoT Infrastructure]] at scale: smart meters, environmental sensors, agricultural IoT.
  - Building management systems (BMS), smart city infrastructure.
  - Asset tracking across logistics and supply-chain networks.
- **Private 5G Networks**
  - Campus networks for manufacturing (e.g. automotive production lines), ports, airports, mining sites.
  - Dedicated spectrum slices provide guaranteed QoS, physical security isolation, and on-premise data residency.
  - Replaces Wi-Fi and legacy wired fieldbus systems in high-mobility, high-reliability environments.
- **Network-as-a-Service and B2B slicing**
  - Operators offer slice-as-a-service APIs to enterprises via network exposure functions (NEF).
  - Enables SLA-bound connectivity products programmable by enterprise customers.

### Standards and Context

- **3GPP Release 15 (2018)** — Non-Standalone (NSA) 5G NR; first deployable 5G specification anchored to 4G LTE core.
- **3GPP Release 16 (2020)** — Standalone (SA) 5G Core; URLLC enhancements; industrial IoT; V2X over NR-PC5.
- **3GPP Release 17 (2022)** — NR-Light (RedCap) for mid-tier IoT; NTN (satellite integration); positioning enhancements.
- **3GPP Release 18 (5G-Advanced, 2024)** — AI/ML integration in RAN; XR optimisation; sidelink relay; energy efficiency.
- **[[O-RAN Alliance]]** — Open fronthaul interface (eCPRI), O-RAN Software Community (OSC) reference implementation, AI-powered RAN Intelligent Controllers (Near-RT RIC, Non-RT RIC).
- **ETSI MEC ISG** — Multi-access Edge Computing specifications for edge application lifecycle management and APIs.
- **ETSI NFV ISG** — Network Function Virtualisation framework underpinning the 5GC software-container model.
- **ITU-R IMT-2020** — International Telecommunication Union requirements document defining the technical targets that 5G NR fulfils (20 Gbps peak downlink, 1 ms latency, 10^6 devices/km²).
- **GSMA** — Industry body coordinating 5G roaming, spectrum policy advocacy, and operator interoperability frameworks.
- **Spectrum regulation** — managed nationally (Ofcom in the UK, FCC in the US, BNetzA in Germany); coordinated internationally through ITU World Radiocommunication Conferences (WRC).
- **Security** — 3GPP TS 33.501 defines the 5G security architecture: 5G-AKA and EAP-AKA' authentication, SUPI/SUCI concealment, and slice-level security policies. [[Security]] hardening of open RAN interfaces is an active standardisation area.

### Evolution Trajectory

- **5G-Advanced (Release 18–19)** introduces native AI/ML model inference inside the RAN and core, sidelink relaying, reduced-capability NR (RedCap/NR-Light), and advanced positioning.
- **Non-Terrestrial Networks (NTN)** — 3GPP Release 17+ integrates LEO satellite access with the 5G NR air interface, extending coverage to maritime, aviation, and rural areas.
- **[[6G Network Slice]]** research and pre-standardisation (ITU IMT-2030) is active; 6G is expected to incorporate terahertz bands, integrated sensing and communication (ISAC), and native AI as a first-class network capability.
- **[[Digital Twin]]** — network digital twins for real-time simulation and closed-loop optimisation of 5G deployments are entering commercial solutions.
- **Energy efficiency** — 5G network energy consumption is a major operator concern; AI-driven sleep modes, RAN sharing, and green energy procurement are active operational priorities.

### Provenance
