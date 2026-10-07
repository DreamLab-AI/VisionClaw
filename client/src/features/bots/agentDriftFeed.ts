/**
 * agentDriftFeed.ts — feeds the separated-layout agent drift (ADR-2135) on
 * the desktop.
 *
 * Every agent the desktop draws is positioned client-side (demo agents are
 * injected into the graph, live swarm agents come from the bots graph), so
 * the drift that moves agents between the knowledge, ontology and memory
 * vertices is applied here, with the rule shared with the XR client
 * (`agentDrift.ts`, a port of `crates/visionclaw-tri-layout`):
 *
 * - a 0x23 beam credits the vertex of its target node's population (the
 *   server's `Node::population`, so the vertex is where the server draws it);
 * - a `memory_flash` credits memory: the agent named by its optional `agentId`,
 *   or every agent present equally.
 *
 * Agents are keyed exactly as agentWorkTargets keys them (`String(wire id)`,
 * flag bit included), so GemNodes looks both up with `String(node.id)`.
 */

import { DriftField } from '../graph/agentDrift';
import { Vertex, type TriangleFrame, type Vec3 } from '../graph/triLayout';
import { useTransientBeamStore, type TransientBeam } from '../../store/transientBeamStore';
import { useWebSocketStore } from '../../store/websocketStore';
import { graphDataManager } from '../graph/managers/graphDataManager';
import { AGENT_NODE_FLAG } from '../../types/binaryProtocol';

const NODE_ID_MASK = 0x03ffffff;

interface NodeLike {
  metadata?: Record<string, unknown>;
  type?: string;
}

/**
 * The triangle vertex a node lives on, mirroring the server's
 * `Node::population` (crates/visionclaw-domain/src/models/node.rs): the origin
 * is `metadata.type`, else the top-level type; agents have none.
 */
export function populationVertex(node: NodeLike | undefined): Vertex | null {
  if (!node) return null;
  const t = (node.metadata?.type as string | undefined) || node.type || '';
  switch (t) {
    case 'agent':
    case 'bot':
      return null;
    case 'owl_class':
    case 'ontology_node':
    case 'owl_individual':
    case 'owl_property':
      return Vertex.Ontology;
    case 'page':
    case 'linked_page':
      return Vertex.Knowledge;
    default: {
      const iri = node.metadata?.owl_class_iri ?? node.metadata?.owlClassIri;
      return iri ? Vertex.Ontology : Vertex.Knowledge;
    }
  }
}

export interface AgentDriftDeps {
  /** graph node by id string */
  lookupNode: (id: string) => NodeLike | undefined;
  /** keys of every agent present, for anonymous memory flashes */
  agentKeys: () => string[];
  /** monotonic seconds */
  nowS: () => number;
}

type BeamLike = Pick<TransientBeam, 'id' | 'sourceAgentId' | 'targetNodeId'>;

export class AgentDriftFeed {
  private field = new DriftField<string>();
  private lastBeamId = -1;

  constructor(private deps: AgentDriftDeps) {}

  ingestBeams(beams: ReadonlyArray<BeamLike>): void {
    for (const b of beams) {
      if (b.id <= this.lastBeamId) continue;
      this.lastBeamId = b.id;
      const raw = String(b.targetNodeId);
      const node = this.deps.lookupNode(raw) ?? this.deps.lookupNode(String(b.targetNodeId & NODE_ID_MASK));
      const v = populationVertex(node);
      if (v === null) continue;
      this.field.recordAction(String(b.sourceAgentId), v, this.deps.nowS());
    }
  }

  ingestMemoryFlash(data: { agentId?: unknown } | null | undefined): void {
    const id = data?.agentId;
    const now = this.deps.nowS();
    if (typeof id === 'number' && Number.isFinite(id)) {
      this.field.recordMemory(String((id | AGENT_NODE_FLAG) >>> 0), [], now);
    } else {
      this.field.recordMemory(null, this.deps.agentKeys(), now);
    }
  }

  step(): void {
    this.field.step(this.deps.nowS());
  }

  offset(agentKey: string, frame: TriangleFrame): Vec3 {
    return this.field.offset(agentKey, frame);
  }

  /** decayed credit per vertex now (tests, diagnostics) */
  weights(agentKey: string): [number, number, number] | undefined {
    return this.field.get(agentKey)?.weightsAtTime(this.deps.nowS());
  }

  get size(): number {
    return this.field.size;
  }
}

// ── the app's feed ──

function graphAgentKeys(): string[] {
  const nodes = graphDataManager.getLastGraphData()?.nodes ?? [];
  const out: string[] = [];
  for (const n of nodes) {
    const t = (n.metadata?.type as string | undefined) ?? (n as unknown as NodeLike).type;
    if (t === 'agent' || t === 'bot') out.push(String(n.id));
  }
  return out;
}

function graphLookup(id: string): NodeLike | undefined {
  const nodes = graphDataManager.getLastGraphData()?.nodes;
  if (!nodes) return undefined;
  if (indexedFrom !== nodes) {
    index.clear();
    for (const n of nodes) index.set(String(n.id), n as unknown as NodeLike);
    indexedFrom = nodes;
  }
  return index.get(id);
}
const index = new Map<string, NodeLike>();
let indexedFrom: unknown = null;

let shared: AgentDriftFeed | null = null;
let unsubs: Array<() => void> = [];

/**
 * The app-wide feed, subscribed on first use to the transient-beam store and
 * the `memoryFlash` socket event.
 */
export function agentDriftFeed(): AgentDriftFeed {
  if (shared) return shared;
  const feed = new AgentDriftFeed({
    lookupNode: graphLookup,
    agentKeys: graphAgentKeys,
    nowS: () => performance.now() / 1000,
  });
  shared = feed;
  unsubs.push(useTransientBeamStore.subscribe((s) => feed.ingestBeams(s.beams)));
  // zustand fires only on later changes: drain what is already there
  feed.ingestBeams(useTransientBeamStore.getState().beams);
  unsubs.push(useWebSocketStore.getState().on('memoryFlash', (d: unknown) => feed.ingestMemoryFlash(d as { agentId?: unknown })));
  return feed;
}

/** Test-only: drop the shared feed and its subscriptions. */
export function resetAgentDriftFeed(): void {
  for (const u of unsubs) u();
  unsubs = [];
  shared = null;
  index.clear();
  indexedFrom = null;
}
