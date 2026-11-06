/**
 * Source group — Provenance (id `provenance`). registry/placement.ts (ADR-2189) decides which section shows each field.
 *
 * Phase-1 W-G client surface for the assertion-version provenance subsystem
 * (ADR-049). Both fields are CLIENT-ONLY and DEFAULT-OFF: they gate whether the
 * node detail panel renders attribution (did:nostr / activity URN /
 * prov:generatedAtTime / signature) and whether the proposal list renders the
 * per-gate status chips (conflict / Whelk consistency / ACSP).
 *
 * The Whelk chip reports classifier consistency of the asserted projection
 * (urn:ngm:graph:ontology:assert) — never reachability, never "Whelk-classified"
 * transitive truth. Paths carry no server bucket (serverBucketFor → null).
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  {
    key: 'showAttribution',
    subgroup: 'Attribution',
    label: 'Show Node Attribution',
    type: 'toggle',
    path: 'provenance.showAttribution',
    default: false,
    description: 'Show the provenance attribution section (did:nostr, activity URN, generated-at time, signature status) on the node detail panel.',
  },
  {
    key: 'showGateChips',
    subgroup: 'Governance Gates',
    label: 'Show Gate Chips',
    type: 'toggle',
    path: 'provenance.showGateChips',
    default: false,
    description: 'Show per-proposal governance gate chips (integrity conflict, Whelk asserted-projection consistency, ACSP) on the proposal list.',
  },
  {
    key: 'enableTimeline',
    subgroup: 'Timeline',
    label: 'Enable Timeline Scrubber',
    type: 'toggle',
    path: 'provenance.enableTimeline',
    default: false,
    description: 'Mount the bottom-docked bi-temporal timeline scrubber (ADR-049 state-at). Fades/highlights nodes by the runtime assertion subjects valid at the scrubbed instant. Overlays the atemporal corpus backdrop; does not replace it.',
  },
  {
    key: 'diffMode',
    subgroup: 'Timeline',
    label: 'Timeline diff',
    type: 'toggle',
    path: 'provenance.diffMode',
    default: false,
    description: 'Compare two instants: marks subjects added (green) or retracted (red) between t1 and t2. The same control as the Diff checkbox on the timeline scrubber.',
  },
];

export const provenance: GroupData = {
  id: 'provenance',
  label: 'Provenance',
  description: 'Client-only overlays for assertion-version attribution and governance-gate status chips.',
  loadPaths: ['provenance'],
  fields,
};
