public:: false
title:: Agent Comm Edges — Curator ↔ Sync ↔ Whelk
agent-telemetry:: true

# Agent Communication Edges

Three-agent telemetry snapshot showing the curator (Alice) requesting a
re-sync from the sync service, which then triggers the Whelk reasoner.
Each message is an AgentTelemetry event with a `vc:to` field that
generates an `AgentControls` edge in the topology projection.

### Telemetry blocks (uses JSON-LD @graph to put multiple agents in one named graph)

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@graph": [
    {
      "@id": "urn:visionflow:agent:run-2026-05-16-002:step-0",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
      "vc:runId": "run-2026-05-16-002",
      "vc:step": 0,
      "vc:eventKind": "MessageSent",
      "vc:to": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "vc:payload": { "@value": "Please re-sync mainKnowledgeGraph/pages/", "@type": "xsd:string" },
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:00:00Z", "@type": "xsd:dateTime" }
    },
    {
      "@id": "urn:visionflow:agent:run-2026-05-16-002:step-1",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "vc:runId": "run-2026-05-16-002",
      "vc:step": 1,
      "vc:eventKind": "MessageReceived",
      "vc:from": { "@id": "did:nostr:npub1alice000000000000000000000000000000000000000000000000000" },
      "vc:to": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
      "vc:payload": { "@value": "Forwarded re-sync trigger; please re-run inference after sync", "@type": "xsd:string" },
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:00:01Z", "@type": "xsd:dateTime" }
    },
    {
      "@id": "urn:visionflow:agent:run-2026-05-16-002:step-2",
      "@type": "AgentTelemetry",
      "vc:agentDid": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
      "vc:runId": "run-2026-05-16-002",
      "vc:step": 2,
      "vc:eventKind": "MessageReceived",
      "vc:from": { "@id": "did:nostr:npub1sync000000000000000000000000000000000000000000000000000000" },
      "prov:wasAttributedTo": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
      "prov:generatedAtTime": { "@value": "2026-05-16T11:00:02Z", "@type": "xsd:dateTime" }
    }
  ]
}
```
