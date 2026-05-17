---
title: Promoting a Note to the Ontology
description: Step-by-step tutorial — write a Logseq note, watch it surface in the Broker Inbox, review and approve it, and see it become a live OWL class in the 3D graph
category: tutorial
tags: [tutorial, insight-migration, ontology, broker, logseq, promotion]
updated-date: 2026-04-18
difficulty-level: intermediate
---

# Promoting a Note to the Ontology

This tutorial walks you through the complete migration journey: from writing a Logseq note to watching a live OWL 2 Web Ontology Language (OWL 2) class appear in VisionClaw's 3D physics graph. The whole process takes under ten minutes once the system is running.

If you want to understand why the loop works the way it does before trying it, read the [Insight Migration Loop explanation](../explanation/insight-migration-loop.md) first. If you want to get started immediately, begin here.

---

## Prerequisites

Before you start, confirm all three of these hold:

1. **VisionClaw is running.** The stack must be up and healthy:

   ```bash
   curl http://localhost:3030/api/health
   # Expected: {"status":"ok"}
   ```

   If it is not running, follow the [Installation Guide](installation.md) and [Build Your First Graph](first-graph.md).

2. **A Logseq graph is connected.** VisionClaw must be synced to a GitHub repository containing your Logseq pages. Confirm in the left sidebar that GitHub sync shows a green connected status and a non-zero page count.

3. **You have the Broker role.** Only users with the `Broker` role can see the migration candidate inbox and approve promotions. Check with your VisionClaw admin, or check your own role at `http://localhost:3030/api/me`.

   If you are running a personal instance, your user is already set to `Broker` by default.

---

## Step 1 — Write a Candidate Note in Logseq

Open Logseq and create a new page. For this tutorial, we will use a concept from the blockchain domain. The specific domain does not matter; what matters is the structure of the note.

Create a file named `Digital Signature.md` with the following content:

```markdown
public:: true
domain:: bc
maturity:: ready

# Digital Signature

A digital signature is a cryptographic mechanism that binds a signer's identity to a message or document. It provides authentication, non-repudiation, and integrity verification.

- Is a type of [[Cryptographic Primitive]]
- Used by [[Smart Contract]] to authorise execution
- Relies on [[Public Key Infrastructure]]
- Produces a [[Hash]] of the original message

owl:class:: CryptographicMechanism
```

Key properties explained:

| Property | Purpose |
|----------|---------|
| `public:: true` | Marks the note for inclusion in the public knowledge graph |
| `domain:: bc` | Assigns the note to the blockchain domain |
| `maturity:: ready` | Signals to the scoring engine that this concept is ready for review |
| Wikilinks (`[[...]]`) | Each wikilink to an existing ontology class is a scoring signal |
| `owl:class::` | An explicit OWL declaration — one of the strongest scoring signals |

Save the file. Logseq will sync it to GitHub on the next sync cycle. VisionClaw will pick it up during its next ingestion run (every few minutes by default, or you can trigger a manual sync from the Settings panel).

<!-- screenshot: logseq page showing the Digital Signature note with public:: true and wikilinks to ontology classes -->

---

## Step 2 — See It Appear in the Graph

After VisionClaw ingests the note, open the 3D graph at `http://localhost:3001`.

The note will appear as a knowledge node (a blue gem shape). Because the bridging attraction force (Phase B physics) is active, the node will already be pulled gently toward the `CryptographicMechanism` class — you can see this as a slight lean in the direction of the ontology cluster.

If the orphan repulsion force is active and `CryptographicMechanism` does not yet have a `BRIDGE_TO` edge connecting to it, the note will initially appear in the amber orphan zone to one side of the main graph. This is correct behaviour — it is the physics view telling you this note has no ontology anchor yet.

To locate the node quickly, press `Ctrl+K` to open the command palette and type `Digital Signature`. The camera will fly to the node.

<!-- screenshot: 3D graph with the Digital Signature node visible, either in the orphan zone or showing a faint bridging aura toward the ontology cluster -->

---

## Step 3 — Watch Candidate Detection Surface It in the Broker Inbox

Within a few minutes of ingestion, the scoring engine (BC13 — the Insight Discovery bounded context) will evaluate the note. With `maturity:: ready`, three wikilinks to existing ontology classes, and an explicit `owl:class::` declaration, the confidence score will cross the 0.60 surfacing threshold comfortably.

When this happens, two things occur simultaneously:

- A `BRIDGE_TO` edge with `kind: "candidate"` appears in Neo4j connecting the `KGNode` to the target `OntologyClass`.
- A pulsing aura appears around the node in the 3D graph at approximately 1.2 Hz.

The pulsing aura means: this node is a migration candidate and is waiting for broker review.

To confirm the candidate has reached the inbox, navigate to the Broker Workbench at `http://localhost:3001/broker`. You will see a **Migration Candidates** lane on the left side of the inbox, separate from the Escalations lane. Your `Digital Signature` card will appear there, showing:

- The note label
- The target ontology IRI (e.g. `vc:bc/digital-signature`)
- A green confidence badge (the score at surfacing, e.g. `0.84`)
- Wikilink count
- Source: "System" (auto-detected) plus any agent proposals if any fired

<!-- screenshot: broker inbox with migration lane showing the Digital Signature candidate card with confidence badge -->

---

## Step 4 — Review the Decision Canvas

Click the `Digital Signature` card. The Decision Canvas opens in split-pane mode:

**Left pane** — The note's full Markdown content, rendered exactly as you wrote it in Logseq. This is the raw evidence: what the author actually said.

**Right pane** — The proposed `OntologyClass` preview: label, IRI, proposed parent class (`CryptographicMechanism`), and the properties the system has extracted (`authentication`, `non-repudiation`, `integrity-verification`).

**Diff strip** (below both panes) — The OWL delta shown as a structured diff: the axioms that will be added to the ontology if you approve.

```diff
+ SubClassOf(vc:bc/digital-signature vc:bc/cryptographic-mechanism)
+ rdfs:label "Digital Signature" @en
+ ObjectProperty(vc:bc/authorises)
  domain: vc:bc/digital-signature
  range:  vc:bc/smart-contract
```

This is the moment for human judgment. Ask yourself:

- Does `CryptographicMechanism` feel like the right parent? If you want a different parent, you can edit the proposed class in the right pane before approving.
- Are the properties correct? The system proposes based on signals; you may want to add or remove some.
- Does `Digital Signature` mean the same thing here as it means in the rest of the ontology? If there is an existing class that already covers this concept, reject with reason "duplicate of vc:bc/asymmetric-signature" and the system will suppress future proposals for this note.

<!-- screenshot: Decision Canvas in split-pane mode showing the Markdown note on the left and the proposed OWL class on the right, with diff strip below -->

---

## Step 5 — Approve and Watch the PR Created and Merged

When you are satisfied, click **Approve**.

What happens next is fast:

1. The `DecisionOrchestrator` emits `BrokerApprovedMigration`.
2. Within 30 seconds, a GitHub pull request appears in your ontology repository. The PR URL appears in the BrokerTimeline panel at the bottom of the canvas.
3. The PR title will read something like: `chore: promote vc:bc/digital-signature from KG candidate`.
4. The PR body includes the OWL delta, the Whelk consistency hash (confirming no existing classes become unsatisfiable), and a link back to the broker decision record.

If your instance is configured for self-merge (the default for single-broker setups), the PR merges automatically after a short delay. If a second reviewer is required (common in regulated deployments), the PR waits in GitHub until approved there.

On merge:

- BC2 applies the SPARQL PATCH to the OWL store.
- Whelk re-checks global consistency.
- BC13 emits `MigrationPromoted`.
- BC3 flips the `BRIDGE_TO` edge from `kind: "candidate"` to `kind: "promoted"`.

The broker timeline entry updates to show the PR URL, the merge commit SHA, and the re-ingest timestamp.

To confirm the PR was created:

```bash
# If you have the GitHub CLI installed
gh pr list --repo your-org/your-ontology-repo --state open
```

<!-- screenshot: broker timeline showing the PR link chip and the sequence of state transitions from Approved through PRAssigned to Promoted -->

---

## Step 6 — See the Bridge Edge Form in the 3D Graph

Return to the 3D graph at `http://localhost:3001`.

After re-ingest completes (30–90 seconds on a typical corpus), watch the `Digital Signature` node. Over approximately two seconds:

- The pulsing aura changes from the candidate rhythm to a steady glow.
- The node accelerates toward the `CryptographicMechanism` class.
- The `bridge_strength` of the `BRIDGE_TO` edge has jumped from its candidate value (around 0.24 at the default settings) to the promoted value (0.75), and the physics simulation responds to this change by pulling the node more strongly.
- A toast notification appears in the top-right corner: "Promoted: vc:bc/digital-signature — centrality +0.03."

The node is no longer in the orphan zone. It has settled near its parent class, z-position shifting toward the stable core as the maturity force recognises its new `status: stable`. If you switch to the ontology-only view (use the population toggle in the left sidebar), `Digital Signature` is now visible in the class hierarchy alongside `CryptographicMechanism`.

Press `Ctrl+K` and type `Digital Signature` again to fly to the node and confirm its new position in the graph.

<!-- screenshot: 3D graph with Digital Signature node settled near its parent class, bridge edge visible, no longer in amber orphan zone -->

---

## What Can Go Wrong

### Whelk rejects an inconsistent axiom

**Symptom.** The PR is created but fails CI with a message such as: `Whelk consistency check failed — vc:bc/digital-signature introduces unsatisfiable class vc:bc/cryptographic-mechanism`.

**Cause.** The proposed axioms conflict with an existing class definition. This is the most common issue when the proposed parent class already has a disjointness axiom that excludes your new class.

**How to recover:**

1. Read the Whelk report attached to the PR as a comment. It will identify the specific conflict.
2. In the broker workbench, use the **Reopen** action on the case (visible in the BrokerTimeline under the PR link) to bring the candidate back to `UnderReview`.
3. Edit the proposed parent class or the axioms in the right pane of the Decision Canvas to resolve the conflict.
4. Approve again. A new PR is opened.

If you are unsure how to resolve the conflict, read the [Ontology Pipeline explanation](../explanation/ontology-pipeline.md) for background on how Whelk checks consistency, and consult the class hierarchy in the Neo4j Browser (`http://localhost:7474`) to understand the existing disjointness structure.

### Broker rejects with reason

**Symptom.** You click **Reject**, enter a reason, and the card disappears from the inbox. Later, the same note comes back with slightly different signals.

**Cause.** Rejection with the reason "not a concept" suppresses the candidate permanently for this note. Other rejection reasons apply a cooldown and allow re-surfacing if new signals accumulate (for example, if a domain agent later proposes the same note with high confidence).

**How to recover:**

- If you rejected with the wrong reason and want to re-surface the note manually, an admin can issue a force-surface via `POST /api/broker/migration-candidates/force-surface` with the note's `kg_note_id` and an `override_reason`.
- If the note came back because of new signals and you still do not want it promoted, reject again with reason "not a concept" to suppress permanently.

---

## Next Steps

You have completed the full migration loop from note to live ontology class. From here:

| Topic | Where to go |
|-------|-------------|
| Understand why the loop is designed this way | [Insight Migration Loop explanation](../explanation/insight-migration-loop.md) |
| Learn about the two-tier identity model | [ADR-048](../adr/ADR-048-dual-tier-identity-model.md) |
| Understand the broker workflow in detail | [ADR-049](../adr/ADR-049-insight-migration-broker-workflow.md) |
| See how Whelk checks consistency | [Ontology Pipeline](../explanation/ontology-pipeline.md) |
| Understand the DDD model behind migration | [DDD: Insight Migration Context](../explanation/ddd-insight-migration-context.md) |
| Configure candidate scoring thresholds | [PRD §6 and §8](../prd-insight-migration-loop.md) |
