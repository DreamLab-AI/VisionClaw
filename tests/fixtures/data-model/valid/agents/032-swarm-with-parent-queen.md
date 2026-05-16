public:: false
title:: Swarm Telemetry — Parent / Queen / Worker Hierarchy
agent-telemetry:: true

# Swarm Telemetry — Parent / Queen / Worker

A four-agent swarm spawned to perform a corpus rebuild. Demonstrates the
`vc:parent` field that builds parent-child relations between agent
telemetry events (used by the topology layer to draw swarm hierarchies).

### Telemetry block

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@graph": [
    {
      "@id": "urn:visionflow:agent:swarm-rebuild-7:step-0",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "vc:runId": "swarm-rebuild-7",
      "vc:step": 0,
      "vc:eventKind": "SwarmSpawned",
      "vc:role": "Queen",
      "vc:childAgents": [
        { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
        { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
        { "@id": "did:nostr:npub1carlo00000000000000000000000000000000000000000000000000000" }
      ],
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:05:00Z", "@type": "xsd:dateTime" }
    },
    {
      "@id": "urn:visionflow:agent:swarm-rebuild-7:step-1",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
      "vc:runId": "swarm-rebuild-7",
      "vc:step": 1,
      "vc:eventKind": "TaskAccepted",
      "vc:role": "Worker",
      "vc:parent": { "@id": "urn:visionflow:agent:swarm-rebuild-7:step-0" },
      "vc:taskDescription": "Re-parse mainKnowledgeGraph/pages/architect-*.md",
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:05:01Z", "@type": "xsd:dateTime" }
    },
    {
      "@id": "urn:visionflow:agent:swarm-rebuild-7:step-2",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
      "vc:runId": "swarm-rebuild-7",
      "vc:step": 2,
      "vc:eventKind": "TaskAccepted",
      "vc:role": "Worker",
      "vc:parent": { "@id": "urn:visionflow:agent:swarm-rebuild-7:step-0" },
      "vc:taskDescription": "Re-parse mainKnowledgeGraph/pages/building-*.md",
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1bob00000000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:05:01Z", "@type": "xsd:dateTime" }
    }
  ]
}
```
