---
type: Class
title: A2A Protocol
slug: a2-a-protocol
resource: urn:ngm:class:a2-a-protocol
page_resource: urn:visionflow:page:a2-a-protocol
public: true
status: stable
domain: ai
maturity: emerging
quality: 0.88
generated:
  by: process:vault-migrate/1.0
  at: 2026-10-01T15:32:16.079852276Z
links:
- '[[agent-to-agent-protocol|Agent-to-Agent Protocol]]'
- '[[model-context-protocol|Model Context Protocol]]'
- '[[multi-agent-system|Multi-Agent System]]'
- '[[multi-agent-orchestration|Multi-Agent Orchestration]]'
- '[[agentic-workflow|Agentic Workflow]]'
- '[[inter-agent-communication|Inter-Agent Communication]]'
- '[[agent-frameworks|Agent Frameworks]]'
- '[[agentic-ai|Agentic AI]]'
- '[[function-calling|Function Calling]]'
- '[[agent-communication-protocol|Agent Communication Protocol]]'
- '[[service-discovery|Service Discovery]]'
- '[[json-ld|JSON-LD]]'
- '[[http-protocol|HTTP Protocol]]'
- '[[json-rpc-2-0|JSON-RPC 2.0]]'
- '[[server-sent-events|Server-Sent Events]]'
- '[[oauth-2-0|OAuth 2.0]]'
- '[[mutual-tls|Mutual TLS]]'
- '[[json-schema|JSON Schema]]'
- '[[decentralised-identifier|Decentralised Identifier]]'
- '[[autonomous-agent|Autonomous Agent]]'
- '[[large-language-model|Large Language Model]]'
- '[[task-delegation|Task Delegation]]'
- '[[agent-identity|Agent Identity]]'
- '[[capability-negotiation|Capability Negotiation]]'
- '[[microservices-architecture|Microservices Architecture]]'
- '[[prompt-injection|Prompt Injection]]'
- '[[ai-safety|AI Safety]]'
- '[[distributed-systems|Distributed Systems]]'
- '[[message-passing|Message Passing]]'
- '[[agent-card|Agent Card]]'
- '[[protocol-buffers|Protocol Buffers]]'
- '[[grpc|gRPC]]'
- '[[linux-foundation|Linux Foundation]]'
- '[[agent-development-kit|Agent Development Kit]]'
- '[[langchain-agent-framework|LangChain Agent Framework]]'
- '[[crewai|CrewAI]]'
- '[[fipa-acl|FIPA-ACL]]'
- '[[web-of-things|Web of Things]]'
- '[[api-gateway|API Gateway]]'
- '[[task-lifecycle|Task Lifecycle]]'
- '[[enterprise-ai|Enterprise AI]]'
- '[[agent-marketplace|Agent Marketplace]]'
- '[[supply-chain-automation|Supply Chain Automation]]'
- '[[rate-limiting|Rate Limiting]]'
- '[[zero-trust-security|Zero-Trust Security]]'
- '[[agent2agent-protocol-google-2025|Agent2Agent Protocol (Google 2025)]]'
- '[[open-agent-network|Open Agent Network]]'
- '[[semantic-web|Semantic Web]]'
- '[[workflow-automation|Workflow Automation]]'
- '[[cost-attribution|Cost Attribution]]'
- '[[ai-governance|AI Governance]]'
- '[[protocol-layer|Protocol Layer]]'
- '[[ai-domain|AI Domain]]'
- '[[agent-layer|AgentLayer]]'
- '[[agent-orchestrator|Agent Orchestrator]]'
- '[[credential-management|Credential Management]]'
- '[[streaming-api|Streaming API]]'
- '[[task-planner|Task Planner]]'
- '[[remote-procedure-call|Remote Procedure Call]]'
- '[[contract-net-protocol|Contract Net Protocol]]'
- '[[peer-to-peer-network|Peer-to-Peer Network]]'
- '[[digital-twin|Digital Twin]]'
- '[[human-in-the-loop|Human-in-the-Loop]]'
- '[[open-source|Open Source]]'
- '[[api-integration|API Integration]]'
- '[[vertex-ai|Vertex AI]]'
is-a:
- '[[agent-communication-protocol|Agent Communication Protocol]]'
- '[[agent-to-agent-protocol|Agent-to-Agent Protocol]]'
bridges-to:
- '[[microservices-architecture|Microservices Architecture]]'
- '[[web-of-things|Web of Things]]'
- '[[semantic-web|Semantic Web]]'
- '[[peer-to-peer-network|Peer-to-Peer Network]]'
contrasts-with:
- '[[remote-procedure-call|Remote Procedure Call]]'
- '[[model-context-protocol|Model Context Protocol]]'
depends-on:
- '[[autonomous-agent|Autonomous Agent]]'
- '[[distributed-systems|Distributed Systems]]'
- '[[service-discovery|Service Discovery]]'
enables:
- '[[multi-agent-orchestration|Multi-Agent Orchestration]]'
- '[[agentic-workflow|Agentic Workflow]]'
- '[[inter-agent-communication|Inter-Agent Communication]]'
- '[[task-delegation|Task Delegation]]'
- '[[enterprise-ai|Enterprise AI]]'
- '[[agent-marketplace|Agent Marketplace]]'
has-part:
- '[[agent-card|Agent Card]]'
- '[[task-lifecycle|Task Lifecycle]]'
- '[[capability-negotiation|Capability Negotiation]]'
- '[[streaming-api|Streaming API]]'
related-to:
- '[[model-context-protocol|Model Context Protocol]]'
- '[[multi-agent-system|Multi-Agent System]]'
- '[[fipa-acl|FIPA-ACL]]'
- '[[contract-net-protocol|Contract Net Protocol]]'
- '[[prompt-injection|Prompt Injection]]'
requires:
- '[[http-protocol|HTTP Protocol]]'
- '[[json-data-interchange-format-rpc-2-0|JSON-RPC 2.0]]'
- '[[server-sent-events|Server-Sent Events]]'
- '[[oauth-2-0|OAuth 2.0]]'
- '[[json-data-interchange-format-schema|JSON Schema]]'
uses:
- '[[agent-frameworks|Agent Frameworks]]'
- '[[agentic-ai|Agentic AI]]'
- '[[function-calling|Function Calling]]'
- '[[json-ld|JSON-LD]]'
- '[[protocol-buffer|Protocol Buffers]]'
---

The Agent-to-Agent (A2A) Protocol is an open communication specification, initially proposed by Google in April 2025 and transferred to the Linux Foundation in June 2025, that defines how autonomous AI agents discover one another, negotiate capabilities, delegate tasks, and exchange results across heterogeneous agent frameworks and cloud environments. It uses HTTP/HTTPS transport with JSON-RPC 2.0 structured messages and Server-Sent Events for streaming, an Agent Card system at a well-known URI for capability advertisement, and enterprise-grade authentication through OAuth 2.0, mTLS, and JWT, enabling cross-vendor agent interoperability without requiring shared infrastructure. A2A complements tool-access protocols such as the Model Context Protocol by standardising the agent-to-agent interaction layer, and reached v1.0 production readiness in 2026 with adoption by over 150 organisations including Microsoft, AWS, Salesforce, SAP, and ServiceNow.

### Semantic Classification

### Content

## Compositional Relationships (Components)
```manchester-owl
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:AgentCard))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:TaskLifecycle))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:CapabilityNegotiation))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:StreamingAPI))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:AgentSkillManifest))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:TaskMessagePart))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:hasPart ai:ArtifactObject))
```

## Dependency Relationships
```manchester-owl
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:HTTPProtocol))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:JSONRpc20))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:ServerSentEvents))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:OAuth20))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:JSONSchema))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:requires ai:MutualTLS))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:dependsOn ai:AutonomousAgent))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:dependsOn ai:DistributedSystems))
```

## Capability Relationships
```manchester-owl
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:MultiAgentOrchestration))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:AgenticWorkflow))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:InterAgentCommunication))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:TaskDelegation))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:EnterpriseAI))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:enables ai:AgentMarketplace))
```

## Implementation Relationships
```manchester-owl
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:implements ai:AgentToAgentProtocol))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:implements ai:Agent2AgentProtocolGoogle2025))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:uses ai:JSONLD))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:uses ai:ProtocolBuffers))
```

## Reduction Relationships
```manchester-owl
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:reducesTo ai:AgentCommunicationProtocol))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:reducesTo ai:HTTPBasedProtocol))
SubClassOf(ai:A2AProtocol
  ObjectSomeValuesFrom(ai:reducesTo ai:ServiceDiscoveryMechanism))
```

## About

The Agent-to-Agent (A2A) Protocol addresses a fundamental gap that emerged as AI agent deployments scaled beyond single-framework implementations in enterprise settings. Organisations increasingly deploy heterogeneous agent ecosystems: a customer-service [[Large Language Model]] agent built on [[LangChain Agent Framework]] may need to delegate background research to a specialist search agent on Google's [[Vertex AI]], route financial analysis to a risk-assessment agent on AWS Bedrock, and escalate compliance questions to a governance agent built with a proprietary in-house framework. Prior to A2A, each of these integrations required bespoke API contracts, shared message brokers, or manual data piping — creating fragile, high-maintenance integration webs that broke whenever individual agents were updated. A2A formalises the agent-to-agent interaction layer as a thin, framework-agnostic protocol analogous in role to HTTP for web services: a universal substrate that allows any conforming agent to delegate work to any other without knowledge of the receiving agent's internal implementation.

Google announced A2A on 9 April 2025 at Google Cloud Next, backed by over 50 technology partners. The protocol was published as an open-source [[Open Source]] specification under the Apache 2.0 licence and accepted into the [[Linux Foundation]]'s Agentic AI Foundation in June 2025, establishing vendor-neutral governance. The specification progressed through versions 0.2.5 and 0.3.0 during 2025 before reaching v1.0 production status. By April 2026 — the one-year mark — over 150 organisations supported A2A, including every major cloud provider; Microsoft (Azure AI Foundry), AWS (Bedrock AgentCore), and Google Cloud (ADK) all integrated A2A natively into their platform agent orchestration layers. The Agentic AI Foundation also oversees the [[Model Context Protocol]] and IBM's Agent Communication Protocol (ACP), with all three maintained under compatible governance, reflecting recognition that the agentic standards ecosystem functions as a complementary stack rather than a set of competing alternatives.

The A2A protocol's design philosophy deliberately treats participating agents as black boxes with well-defined interfaces. An orchestrating agent need not share code, memory, security contexts, or runtime infrastructure with agents it delegates to — only the A2A wire format is assumed. This property is critical in enterprise environments where different teams, business units, or external vendors build and operate agents on incompatible technology stacks. The [[Zero-Trust Security]] model embedded in A2A requires explicit authentication on every agent interaction, rejecting the implicit trust assumptions that frequently underlie monolithic microservice architectures.

## Components / Architecture

A2A defines its core data model in [[Protocol Buffers]] (with auto-generated [[JSON Schema]] 2020-12 equivalents), providing language-agnostic definitions for the following primary types:

- **[[Agent Card]]** — A public JSON document served at `/.well-known/agent-card.json` (per RFC 8615). It is the agent's technical self-description, including its canonical name, human-readable description, a list of `AgentSkill` objects (each with an `id`, `name`, `description`, `inputModes`, `outputModes`, and optionally a [[JSON Schema]] for structured inputs), the endpoint URL and binding type (JSON-RPC, gRPC, REST), and a `securitySchemes` block listing accepted authentication methods. From v1.2, Agent Cards support cryptographic signatures using the domain's private key, enabling verifiable provenance. [[Service Discovery]] for A2A is currently push-based (registering with agent directories) or out-of-band (sharing Agent Card URLs directly), with DNS-based discovery under discussion for v1.3.
- **Task** — The central [[Task Lifecycle]] unit. A Task has a UUID `id`, an optional `sessionId` for grouping correlated tasks, a `status` enumeration (submitted → working → input-required / completed / failed / cancelled), a `history` list of `Message` objects recording the conversation, and an `artifacts` list of `Artifact` objects containing the agent's outputs. Tasks support synchronous completion (for fast operations) and asynchronous streaming (for operations lasting seconds to days) via [[Server-Sent Events]].
- **Message and Part** — A `Message` is either user-supplied input or agent output. It contains a `role` (user or agent), a `metadata` block, and a list of `Part` objects. A `Part` is a typed content unit: `TextPart` (plain text or markdown), `FilePart` (inline bytes or a URI reference), or `DataPart` (structured JSON for [[Function Calling]] results and structured data exchange).
- **Artifact** — An output produced by the executing agent, referenced in the `Task.artifacts` list. Artifacts may contain multiple named parts, enabling an agent to return both a textual summary and a structured data object in a single task result.
- **Extension** — A versioned extension namespace mechanism allowing vendors to add proprietary metadata to Agent Cards, Tasks, and Messages without breaking protocol compatibility. Extensions are type-safe and declared in the Agent Card, enabling progressive capability negotiation.

  The three concrete wire bindings in v1.0 are: (1) JSON-RPC 2.0 over HTTPS — the canonical and most widely deployed binding, using standard request–response for synchronous methods and [[Server-Sent Events]] for streaming; (2) gRPC — a [[gRPC]] binding offering lower latency and native streaming for high-throughput deployments; (3) HTTP+JSON/REST — a simplified REST binding for environments where JSON-RPC tooling is unavailable.

  Authentication in A2A mirrors OpenAPI security schemes: API key in HTTP header or query parameter, HTTP Basic or Bearer (JWT), [[OAuth 2.0]] authorisation code and client credentials flows, and [[Mutual TLS]] for mTLS-first environments. The `securitySchemes` block in the [[Agent Card]] declares what the server accepts; clients select a compatible scheme from their own credentials, enabling [[Capability Negotiation]] of authentication methods alongside capability negotiation of skills.

  ## Use Cases / Major Families

  **Enterprise Cross-Vendor Agent Orchestration** is A2A's primary design target. An [[Agent Orchestrator]] built on one vendor's platform submits tasks to specialist agents built on different platforms. Concrete deployments by April 2026 include: Salesforce Agentforce agents delegating to third-party data analysis agents via A2A; SAP supply chain [[Supply Chain Automation]] agents coordinating procurement decisions across supplier agent networks; ServiceNow IT operations agents composing resolution workflows from specialist diagnostic agents. The critical property is that no single vendor's infrastructure must host all participating agents.

  **Agent Marketplace Discovery** — Agent directories (analogous to npm registries or API catalogues) allow [[Agent Orchestrator]] systems to query for agents matching a skill type and retrieve their [[Agent Card]] for dynamic task routing. Google Cloud and Azure AI have both announced agent registry features building on A2A discovery semantics.

  **Long-Running Autonomous Tasks** — A2A's asynchronous [[Task Lifecycle]] with [[Streaming API]] support via [[Server-Sent Events]] enables agents to execute multi-hour or multi-day tasks (scientific simulations, large-scale data processing, [[Digital Twin]] scenario runs) while emitting progress updates. Orchestrators subscribe to status streams rather than polling, and may checkpoint results to external state stores.

  **Cross-Organisational Delegation** — In procurement and financial services, agents at different companies negotiate orders and settlements directly, with A2A providing the interoperability substrate. [[Cost Attribution]] between delegating and executing agents is tracked in task metadata, enabling billing reconciliation across organisational boundaries.

  **Agentic Metaverse and Simulation Coordination** — Early research deployments coordinate simulation agents across [[Digital Twin]] environments, with A2A enabling [[Message Passing]] between physics simulation agents, AI planning agents, and human-interface agents operating at different update rates.

  **Human-in-the-Loop Escalation** — The `input-required` task status provides a standardised mechanism for agents to pause execution and request human input, supporting [[Human-in-the-Loop]] governance patterns in high-stakes deployments where autonomous action is bounded by organisational policy.

  ## Academic Context

  The A2A Protocol is the most prominent contemporary instantiation of a research tradition spanning four decades. The theoretical antecedents are: the [[Contract Net Protocol]] (Reid Smith, 1980), which established task-delegation by announcement-bid-award; [[FIPA-ACL]] (Foundation for Intelligent Physical Agents, 1996–2002), which produced the first comprehensive agent communication language specification with performatives, content languages, and ontology support; and the JADE platform (Bellifemine, Caire, Poggi, Rimassa, 2003), which implemented FIPA protocols as deployable Java middleware. The contemporary framing of agent interoperability as an HTTP-native, JSON-based protocol layer rather than a dedicated agent middleware layer reflects lessons from the success of REST APIs and the Language Server Protocol in creating open, polyglot ecosystems.

  Academic attention to A2A is nascent but growing. Relevant work includes: Xiao et al. (2025) "Beyond Message Passing: A Semantic View of Agent Communication Protocols" (arXiv:2604.02369), which contextualises A2A within a semantic agent communication taxonomy; Ameri et al. (2025) "Security Threat Modeling for Emerging AI-Agent Protocols" (arXiv:2602.11327), which analyses A2A, MCP, Agora, and ANP threat models; and "The Provenance Paradox in Multi-Agent LLM Routing" (arXiv:2603.18043), which examines [[Agent Identity]] and attested delegation chains in protocols including A2A.

  The protocol's relationship to the [[Semantic Web]] community's [[Web of Things]] (WoT) Thing Description standard is noted in both communities: WoT Thing Descriptions serve a functionally analogous discovery role for IoT devices, and the A2A [[Agent Card]] can be viewed as a specialisation of the Thing Description concept for LLM-backed agents. Convergence between these standards is being discussed in W3C working groups.

  ## Current Landscape (2026)

  As of June 2026, A2A sits at version 1.2 under [[Linux Foundation]] governance via the Agentic AI Foundation, which also governs [[Model Context Protocol]] and ACP under a unified open-agent-standards umbrella. The three protocols have been explicitly positioned as complementary rather than competitive: MCP handles agent-to-tool access, A2A handles agent-to-agent delegation, and ACP addresses asynchronous agent communication bus patterns. The combined stack is increasingly described as the "agentic internet" infrastructure — the agent-layer equivalents of HTTP, DNS, and OAuth for the web.

  Enterprise adoption metrics published by the Linux Foundation at the April 2026 one-year milestone: 150+ supporting organisations, production deployments at scale in financial services (Citigroup, several European banks), supply chain (SAP ecosystem partners), IT operations (ServiceNow), and insurance underwriting. All three major cloud hyperscalers ship native A2A support in their managed agent platforms.

  Framework support as of mid-2026: Google ADK (native), [[CrewAI]] v1.10+ (native MCP + A2A), [[LangChain Agent Framework]] (via community adapters, native support announced), AutoGen (adapters available). The pattern emerging in enterprise deployments is an [[API Gateway]]-style A2A proxy layer that handles authentication, [[Rate Limiting]], [[Cost Attribution]], and routing, abstracting individual agent endpoints behind a managed agent mesh.

  Open challenges for 2026: cryptographic [[Agent Identity]] verification (signed Agent Cards at v1.2 address this partially); cross-protocol semantic interoperability between A2A skills and MCP tool schemas; [[Prompt Injection]] via A2A task payloads carrying adversarial instructions from external agent networks; and standardisation of [[Cost Attribution]] across multi-hop delegation chains.

  ## UK Context

  The UK's AI research community engages with A2A through several channels. The Alan Turing Institute's programme on trustworthy agentic AI is studying A2A's security model and its alignment with the UK's AI Safety Institute's evaluation frameworks. The University of Edinburgh's School of Informatics — the UK's leading NLP and multi-agent research centre — has published on [[Agent Communication Protocol]] design, and Edinburgh researchers contributed to the semantic agent communication survey that contextualises A2A academically. Imperial College London's AI research group is exploring A2A for [[Digital Twin]] coordination in smart manufacturing. BT Group's research labs are piloting A2A for autonomous network operation agents that coordinate across different vendors' network management systems. DeepMind's London-based infrastructure team has explored A2A-compatible interfaces for research agent coordination.

  The UK AI Opportunities Action Plan (January 2026) explicitly identifies agent interoperability infrastructure as a key enabler for the UK's national AI compute and deployment strategy, and the DSIT AI regulation team is monitoring A2A adoption as part of its agentic AI safety review.

  ## Future Directions (2026-2030)

  Several directions are shaping A2A's evolution over the next four years:

- **Federated Agent Identity** — integrating [[Decentralised Identifier]] (DID) infrastructure with A2A [[Agent Card]] authentication, enabling agents to have self-sovereign, cryptographically verifiable identities that persist across cloud platforms and organisational boundaries. W3C DID Working Group members are contributing to A2A extensions addressing this.
- **Semantic Skill Matching** — moving beyond keyword-based skill description to ontology-grounded skill taxonomies that enable automated, semantically-aware skill matching. This connects A2A to the [[Semantic Web]] tradition and proposals for an [[Open Agent Network]] with shared ontologies for agent capability description.
- **Multi-hop Delegation Accountability** — developing provenance chains that record the full delegation tree for any given output, enabling [[AI Governance]] and auditability of complex multi-agent workflows. This is analogous to W3C PROV-DM but applied to agent task chains.
- **Privacy-Preserving Agent Interaction** — federated and confidential computing extensions that allow agents to collaborate on sensitive data without exposing it across trust boundaries, relevant for healthcare, legal, and financial agent deployments.
- **Real-Time Coordination** — A2A v1.3 is anticipated to formalise WebSocket transport for sub-second agent coordination scenarios, extending beyond the SSE streaming model suited to longer-horizon tasks.
- **Cross-Protocol Gateways** — lightweight adapter specifications enabling MCP tool endpoints to be exposed as A2A skills and vice versa, reducing friction for the large existing ecosystem of MCP servers.

  ## Research & Literature

  1. Google. (2025). *Agent2Agent Protocol: Open Standard for AI Agent Interoperability*. Google Cloud Blog, April 2025. https://cloud.google.com/blog/products/ai-machine-learning/a2a-a-new-era-of-agent-interoperability
  2. Linux Foundation. (2025). *Linux Foundation Launches the Agent2Agent Protocol Project*. Press release, June 2025. https://www.linuxfoundation.org/press/linux-foundation-launches-the-agent2agent-protocol-project
  3. Linux Foundation. (2026). *A2A Protocol Surpasses 150 Organizations, Lands in Major Cloud Platforms*. Press release, April 2026. https://www.linuxfoundation.org/press/a2a-protocol-surpasses-150-organizations-lands-in-major-cloud-platforms
  4. A2A Protocol Specification. (2026). *Agent2Agent Protocol Official Specification v1.0*. a2a-protocol.org. https://a2a-protocol.org/v1.0/specification/
  5. Xiao, Y. et al. (2025). "Beyond Message Passing: A Semantic View of Agent Communication Protocols." arXiv:2604.02369.
  6. Ameri, M. et al. (2025). "Security Threat Modeling for Emerging AI-Agent Protocols: A Comparative Analysis of MCP, A2A, Agora, and ANP." arXiv:2602.11327.
  7. Anonymous. (2025). "The Provenance Paradox in Multi-Agent LLM Routing: Delegation Contracts and Attested Identity in LDP." arXiv:2603.18043.
  8. Anonymous. (2025). "Agentic Web: Weaving the Next Web with AI Agents." arXiv:2507.21206.
  9. Anonymous. (2025). "Aiming for AI Interoperability: Challenges and Opportunities." arXiv:2601.14512.
  10. Anonymous. (2025). "AgentRFC: Security Design Principles and Conformance Testing for Agent Protocols." arXiv:2603.23801.
  11. Smith, R. G. (1980). "The Contract Net Protocol: High-Level Communication and Control in a Distributed Problem Solver." *IEEE Transactions on Computers*, 29(12), 1104–1113.
  12. FIPA. (2002). *FIPA ACL Message Structure Specification*. Foundation for Intelligent Physical Agents. http://www.fipa.org/specs/fipa00061/
  13. Bellifemine, F., Caire, G., Poggi, A., & Rimassa, G. (2003). "JADE: A White Paper." *exp: In Search of Innovation*, 3(3), 6–19.
  14. Wooldridge, M. (2009). *An Introduction to MultiAgent Systems* (2nd ed.). Wiley.
  15. Anthropic. (2024). *Model Context Protocol Specification*. https://modelcontextprotocol.io/specification
  16. W3C. (2023). *Decentralized Identifiers (DIDs) v1.0*. W3C Recommendation. https://www.w3.org/TR/did-core/
  17. W3C. (2023). *Web of Things (WoT) Thing Description 1.1*. W3C Recommendation. https://www.w3.org/TR/wot-thing-description11/
  18. Atlassian/Atlan. (2025). "Google A2A Protocol: How Agent-to-Agent Coordination Works." https://atlan.com/know/google-a2a-protocol/
  19. Galileo. (2025). "Google's Agent2Agent Protocol Explained." https://galileo.ai/blog/google-agent2agent-a2a-protocol-guide
  20. n8n. (2025). "Agent-to-Agent (A2A) Protocol: Implementation and Trade-offs." https://blog.n8n.io/agent-to-agent-protocol/
  21. Intuz. (2026). "MCP vs A2A: AI Agent Protocol Comparison (2026)." https://www.intuz.com/blog/mcp-vs-a2a
  22. Zylos Research. (2026). "Agent Interoperability Protocols 2026: MCP, A2A, ACP and the Path to Convergence." https://zylos.ai/research/2026-03-26-agent-interoperability-protocols-mcp-a2a-acp-convergence/
  23. OpenAgents. (2026). "CrewAI vs LangGraph vs AutoGen vs OpenAgents — Best AI Agent Framework (2026)." https://openagents.org/blog/posts/2026-02-23-open-source-ai-agent-frameworks-compared
  24. Tyk. (2025). "A2A Protocol: Architecture and Technical Specification." https://tyk.io/learning-center/a2a-protocol-architecture-and-technical-specification/
  25. Red Hat Developer. (2025). "How to Enhance Agent2Agent (A2A) Security." https://developers.redhat.com/articles/2025/08/19/how-enhance-agent2agent-security
  26. Wikipedia. (2026). "Agent2Agent." https://en.wikipedia.org/wiki/Agent2Agent
  27. FreeCodeCamp. (2025). "How to Build a Multi-Agent AI System with LangGraph, MCP, and A2A." https://www.freecodecamp.org/news/how-to-build-a-multi-agent-ai-system-with-langgraph-mcp-and-a2a-full-book/
  28. Stellagent. (2026). "A2A Protocol Explained: How Google's Agent-to-Agent Standard Grew to 150+ Organizations in One Year." https://stellagent.ai/insights/a2a-protocol-google-agent-to-agent

### Provenance
