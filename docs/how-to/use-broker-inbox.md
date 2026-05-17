# How to use the Judgment Broker Inbox

> Sprint 3 surface introduced by ADR-048 (migration events), ADR-049 (node
> visibility) and ADR-051 (broker inbox + decision canvas).

The Judgment Broker Inbox is the control surface for promoting Knowledge Graph
(KG) nodes into the ontology (OWL) layer. It lives as a dockable panel in the
top-right of the main graph view and appears as soon as the
`BRIDGE_EDGE_ENABLED` feature flag is on.

![Broker inbox panel](./screenshots/broker-inbox.png)
*Screenshot placeholder &mdash; capture once the flag lands on staging.*

## Prerequisites

1. You are authenticated via NIP-07 / NIP-98 (standard client login).
2. The backend reports `BRIDGE_EDGE_ENABLED=true` in `GET /api/features`.
3. The Judgment Broker has surfaced at least one candidate at
   `GET /api/bridge/candidates?status=surfaced`.

## Panel layout

The panel refreshes every 30 seconds and shows:

- A list of **surfaced candidates**, newest first, with:
  - KG node label and id.
  - Proposed ontology class label.
  - Confidence percentage and band (`low` / `medium` / `high`).

Click a candidate to open the **Decision Canvas**:

| Region  | Content                                                          |
|---------|------------------------------------------------------------------|
| Left    | KG node metadata (type, IRI, first six metadata fields).         |
| Centre  | 8-signal radar (structural, semantic, provenance, temporal,      |
|         | editors, reasoner, KG popularity, OWL coverage).                 |
| Right   | Proposed OWL class label, IRI, definition, broker rationale.     |
| Footer  | `Approve`, `Reject`, `Defer` + optional `Reason` textarea.       |

## Making a decision

1. Select a candidate from the list.
2. Review the radar &mdash; low scores on `provenance_strength` or
   `reasoner_support` are the usual red flags.
3. (Optional) Write a short reason. It is stored verbatim with the decision
   and shows up in audit logs.
4. Click one of:
   - **Approve** &rarr; `POST /api/bridge/{id}/promote`. The broker mints a
     `BRIDGE_TO` edge and emits a kind-30100 migration event. You will see
     a toast in the bottom-right; clicking it pans the camera to the source
     node and pulses the filament amber &rarr; cyan.
   - **Reject** &rarr; `POST /api/bridge/{id}/reject`. Candidate is removed
     from the queue and marked rejected.
   - **Defer** &rarr; `POST /api/bridge/{id}/defer`. Candidate is parked;
     the broker will re-surface it once its signal mix changes.

All three calls are optimistic: the candidate is removed locally immediately
and re-inserted on failure (the error is shown in the panel header).

## Migration event toasts

The `MigrationEventToast` component subscribes to the migration event stream
over `wss://.../api/bridge/events/ws`, with polling fallback every 10 seconds
on `/api/bridge/events`. Each kind-30100 event spawns a 3-second toast in the
bottom-right corner:

- Title: `from_kg -> to_owl` (or a server-supplied summary).
- Metadata: timestamp, confidence percentage.
- Click: dispatches a `visionflow:migration-focus` window event that the
  graph overlay uses to pan the camera.

Toasts are buffered in a 50-entry ring; old events beyond the 3-item visible
stack can be cleared via the `clear` control underneath the stack.

## Visibility controls (ADR-049)

Alongside the broker inbox, owners of a private node see a Publish /
Unpublish toggle wired to the `VisibilityControl` component. Non-owners see
a read-only `Private (owner: npub1...)` badge rendered at 60% opacity.

- **Publish** &rarr; `POST /api/nodes/{id}/publish`. The node flips to
  `public`, a Solid pod record is written, and the URN becomes citable.
- **Unpublish** &rarr; `POST /api/nodes/{id}/unpublish`. The node flips to
  `tombstone` for 5 seconds (red X marker in the graph), then settles to
  `private`.

Both actions require a confirmation modal. The `VISIBILITY_TRANSITIONS`
feature flag must be on.

## Troubleshooting

| Symptom                                              | Fix                                             |
|------------------------------------------------------|-------------------------------------------------|
| Panel never appears                                  | Check that `BRIDGE_EDGE_ENABLED` is `true` in  |
|                                                      | `/api/features`, and reload the page.          |
| "Failed to load bridge candidates"                   | Verify NIP-98 auth; re-authenticate if needed. |
| Toasts never fire                                    | Open devtools, watch `ws://.../bridge/events/ws`.|
| Filament pulse does not pan the camera               | Check the browser console for the dispatched  |
|                                                      | `visionflow:migration-focus` event.            |
| Publish toggle missing for my own node               | Check `VISIBILITY_TRANSITIONS` is on and your  |
|                                                      | pubkey matches the node owner.                 |

## References

- ADR-048 &mdash; Migration event stream (kind-30100) and amber&rarr;cyan
  filament pulse.
- ADR-049 &mdash; Public/private/tombstone node visibility.
- ADR-051 &mdash; Judgment Broker and Decision Canvas.
- `client/src/features/broker/BrokerInbox.tsx`
- `client/src/features/migration/MigrationEventToast.tsx`
- `client/src/features/node/VisibilityControl.tsx`
