public:: false
title:: Agent Telemetry — Whelk Inference Run 2026-05-16
agent-telemetry:: true

# Agent Telemetry Snapshot

A single inference run by the Whelk reasoner agent, serialised as
AgentTelemetry events into the `urn:visionflow:graph:agent` named graph.

### Telemetry block

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:agent:run-2026-05-16-001:step-0",
  "@type": "AgentTelemetry",
  "vc:agentDid": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
  "vc:runId": "run-2026-05-16-001",
  "vc:step": 0,
  "vc:eventKind": "InferenceStarted",
  "vc:namedGraph": { "@id": "urn:visionflow:graph:agent" },
  "vc:metrics": {
    "vc:assertedAxiomCount": 11,
    "vc:profileSelected": "EL"
  },
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:32:00Z", "@type": "xsd:dateTime" }
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/context/v1.jsonld",
  "@id": "urn:visionflow:agent:run-2026-05-16-001:step-1",
  "@type": "AgentTelemetry",
  "vc:agentDid": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
  "vc:runId": "run-2026-05-16-001",
  "vc:step": 1,
  "vc:eventKind": "InferenceMaterialised",
  "vc:namedGraph": { "@id": "urn:visionflow:graph:agent" },
  "vc:metrics": {
    "vc:inferredAxiomCount": 4,
    "vc:elapsedMs": 23,
    "vc:reasonerVersion": "whelk-rs/0.2.7"
  },
  "vc:relatesTo": [
    { "@id": "urn:visionflow:owl:axiom:a1f02b9c7e4d" }
  ],
  "prov:wasAttributedTo": { "@id": "did:nostr:npub1whelk00000000000000000000000000000000000000000000000000000" },
  "prov:generatedAtTime": { "@value": "2026-05-16T10:32:00.023Z", "@type": "xsd:dateTime" }
}
```
