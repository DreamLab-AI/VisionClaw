---
title: Glossary
description: Definitions of technical terms used in VisionClaw documentation
category: reference
difficulty-level: intermediate
tags:
  - glossary
  - definitions
  - terminology
updated-date: 2025-01-29
---

# Glossary

Alphabetically sorted definitions of technical terms used in VisionClaw documentation.

---

## A

### Agent
An autonomous software entity that can perform tasks, make decisions, and coordinate with other agents. In VisionClaw, agents are managed through the [MCP Protocol](./mcp-tools.md).

### API Key
A long-lived authentication credential for programmatic access to VisionClaw APIs. Unlike JWT tokens, API keys do not expire. See [Authentication Reference](./rest-api.md).

### Axiom
In OWL ontologies, a statement that defines relationships between classes, properties, or individuals. Examples include SubClassOf, EquivalentClass, and PropertyAssertion. See [Database Schema](./graph-schema.md).

---

## B

### Barnes-Hut Algorithm
An approximation algorithm used in force-directed graph layouts. It reduces computation complexity from O(n^2) to O(n log n) by grouping distant nodes. Controlled by the `theta` parameter.

### Binary Protocol
VisionClaw's compact wire format for the `/wss` position stream: frozen 52-byte V3 node records, streamed in the sequenced V5 envelope and acknowledged per frame. See [Binary WebSocket Protocol](./binary-protocol.md).

---

## C

### Community Detection
Graph algorithm that identifies clusters of densely connected nodes. VisionClaw supports Louvain, Label Propagation, and Modularity optimization algorithms. See [REST API](./rest-api.md).

### Cypher
A declarative graph query language. VisionClaw originally ran on Neo4j Cypher; since ADR-2004 the graph store is embedded Oxigraph (W3C SPARQL 1.1). Some endpoints still accept a `cypher` field for backwards compatibility, but queries resolve against Oxigraph, not Neo4j. See [Graph Schema](./graph-schema.md).

---

## D

### Delta Encoding
Protocol V4 experimental feature that transmits only changed node positions, reducing bandwidth by 60-80% for stable graphs. See [Protocol Reference](./mcp-tools.md).

### DPoP (Demonstrating Proof-of-Possession)
A security mechanism that binds access tokens to a specific client, preventing token theft. Used in Solid pod authentication.

---

## E

### Edge
A relationship between two nodes in the knowledge graph. Stored in the embedded Oxigraph triple store as an RDF triple (a typed predicate linking subject and object IRIs). See [Graph Schema](./graph-schema.md).

### Embedding
A numerical vector representation of data (text, nodes, etc.) in a high-dimensional space. Used for semantic search and similarity calculations.

---

## F

### Force-Directed Layout
A graph visualization algorithm that simulates physical forces (attraction, repulsion) to position nodes. VisionClaw uses GPU-accelerated force simulation.

### FPS (Frames Per Second)
The update rate for real-time graph visualization. VisionClaw targets 60 FPS with the binary WebSocket protocol.

---

## G

### GDS (Graph Data Science)
A library of graph algorithms (centrality, community detection, pathfinding). Historically VisionClaw used Neo4j GDS; since ADR-2004 these analytics run in-process over the embedded Oxigraph store and GPU pipeline. See [Graph Schema](./graph-schema.md).

### GPU Acceleration
Using graphics processing units for parallel computation. VisionClaw supports CUDA (NVIDIA) for physics simulation. See [Configuration Reference](./configuration.md).

---

## H

### HNSW (Hierarchical Navigable Small World)
An efficient algorithm for approximate nearest neighbor search in high-dimensional spaces. Used for fast similarity search in vector databases.

### Hybrid Protocol
VisionClaw's combination of JSON (control messages, metadata) and binary (position updates) WebSocket communication.

---

## I

### IRI (Internationalized Resource Identifier)
A globally unique identifier for ontology resources, extending URIs to support Unicode characters. Example: `http://example.org/ontology#Person`.

### Inference
The process of deriving new knowledge from existing axioms using reasoning rules. VisionClaw tracks inferred vs. asserted axioms.

---

## J

### JSS (JSON Solid Server)
VisionClaw's sidecar service providing Solid pod functionality for decentralized data storage. See [Configuration Reference](./configuration.md).

### JWT (JSON Web Token)
A compact, URL-safe token format for authentication. Contains claims about the user and is signed with a secret key. See [Authentication Reference](./rest-api.md).

---

## K

### Knowledge Graph
A graph-structured knowledge base where nodes represent entities and edges represent relationships. VisionClaw visualizes knowledge graphs in 3D.

---

## L

### LDP (Linked Data Platform)
A W3C specification for reading and writing Linked Data resources. Solid pods implement LDP for container and resource operations.

### Little-Endian
A byte order where the least significant byte is stored first. Used in VisionClaw's binary protocol for multi-byte values.

### Louvain Algorithm
A community detection algorithm that optimizes modularity through hierarchical clustering. Commonly used for large graphs.

---

## M

### MCP (Model Context Protocol)
VisionClaw's JSON-RPC 2.0 based protocol for agent orchestration over TCP. See [MCP Protocol](./mcp-tools.md).

### Metadata
Descriptive information associated with nodes, edges, or ontology elements. Stored as JSON in the `metadata` field.

### Modularity
A measure of the quality of graph clustering. Higher modularity indicates denser connections within communities and sparser connections between them.

---

## N

### NIP-98
A Nostr Improvement Proposal for HTTP authentication using event signatures. Enables decentralized identity verification. See [Authentication Reference](./rest-api.md).

### Node
A vertex in the knowledge graph representing an entity, concept, or class. Stored with 3D position data for visualization. See [Database Schema](./graph-schema.md).

### Nostr
A decentralized protocol for social networking and identity. VisionClaw uses Nostr for authentication via NIP-98.

---

## O

### OWL (Web Ontology Language)
A W3C standard for representing ontologies with formal semantics. VisionClaw supports OWL 2 including classes, properties, and axioms. See [Ontology Schema](./graph-schema.md).

### Ontology
A formal representation of knowledge that defines concepts, relationships, and rules within a domain.

---

## P

### PageRank
A graph centrality algorithm that measures node importance based on incoming links. Named after Larry Page of Google.

### Pod
In Solid, a personal online data store that users control. VisionClaw integrates with pods for decentralized data storage. See [Solid Pod Schema](./graph-schema.md).

### Protocol Version
The first byte of binary WebSocket messages identifying the format. V2 is current standard, V3 adds analytics, V4 is experimental delta encoding.

---

## Q

### Query
A request for data from the graph store. Since ADR-2004 the store is embedded Oxigraph, queried with W3C SPARQL 1.1 (a `cypher` field is still accepted on some endpoints for backwards compatibility). See [Graph Schema](./graph-schema.md).

---

## R

### Rate Limiting
Restricting the number of API requests within a time window to prevent abuse. VisionClaw enforces limits per IP and per user. See [REST API](./rest-api.md).

### Reasoner
A component that applies logical rules to derive new facts from existing axioms. Used in ontology validation and inference.

### RDF (Resource Description Framework)
A W3C standard for representing information as subject-predicate-object triples. OWL is built on RDF.

---

## S

### Schnorr Signature
A digital signature scheme used in Nostr for event authentication. More efficient than ECDSA signatures.

### Settlement
The state when graph physics simulation reaches equilibrium (low kinetic energy). Indicates stable node positions.

### Solid
A specification for decentralized data storage and identity. See [Solid Pod Schema](./graph-schema.md).

### SSSP (Single-Source Shortest Path)
A graph algorithm finding the shortest paths from one node to all others. Results included in binary protocol messages.

### Swarm
A coordinated group of agents working together on tasks. Managed through MCP protocol with topologies like hierarchical, mesh, ring, or star.

---

## T

### Topology
The structure of agent swarm connections. Options include hierarchical (tree), mesh (fully connected), ring (circular), and star (centralized).

### Turtle
A human-readable RDF serialization format. Used in Solid pods for storing ontology data.

---

## U

### UUID (Universally Unique Identifier)
A 128-bit identifier used for nodes, edges, and other resources. Format: `550e8400-e29b-41d4-a716-446655440000`.

---

## V

### Vector
An array of floating-point numbers representing position, velocity, or embeddings. 3D vectors use x, y, z components.

### Velocity
The rate of change of position over time. Used in physics simulation for smooth animation.

---

## W

### WebSocket
A protocol providing full-duplex communication over a single TCP connection. VisionClaw uses WebSocket for real-time updates. See [WebSocket API](./websocket-protocol.md).

### Wire Format
The binary layout of data transmitted over the network. VisionClaw's V2 format uses 36 bytes per node.

---

## X

### XR (Extended Reality)
An umbrella term covering VR (virtual reality), AR (augmented reality), and MR (mixed reality). VisionClaw supports XR visualization modes.

---

## Related Documentation

- [API Reference](./rest-api.md)
- [Protocol Reference](./mcp-tools.md)
- [Configuration Reference](./configuration.md)
