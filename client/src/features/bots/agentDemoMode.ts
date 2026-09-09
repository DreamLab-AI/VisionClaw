/**
 * agentDemoMode.ts — Synthetic agent injection for visual verification.
 *
 * ALL desktop demo code lives here. The demo is a producer only: it injects six
 * agent nodes at the same entry points a live swarm uses (graphDataManager +
 * the transient-beam store + the agent-target store) and drives the same
 * multi-minute loop grammar as the XR demo director (agent_demo_director.gd):
 *
 *   stagger in → work a real node 9–14 s (beams every 2 s) → 60 % of the time
 *   follow a real edge to a neighbour and work 7–11 s → explicit done → rest
 *   18–26 s → new target (biased toward high-degree nodes, i.e. the mass centre)
 *
 * The synthetic agents play as real agents: nothing they emit or display says
 * "demo" — the Toggle Agent Demo button is the only visible marker. Provenance
 * lives in code alone (reserved wire ids 0x80000000|0xD001..D006), and stop
 * removes exactly those. The renderer's momentum nudge, fade-to-edge and layer
 * toggles have no demo branch — they see ordinary agents.
 */

import { graphDataManager } from '../graph/managers/graphDataManager';
import { pushTransientBeams } from '../../store/transientBeamStore';
import { useAgentTargetStore } from '../../store/agentTargetStore';
import { AgentActionType } from '../../services/binaryProtocol/frameTypes';
import type { AgentActionEvent } from '../../services/binaryProtocol/frameTypes';
import type { Node, Edge } from '../graph/managers/graphWorkerProxy';
import { AGENT_NODE_FLAG } from '../../types/binaryProtocol';
import { markAgentDone } from './agentWorkTargets';

const DEMO_SUFFIX_BASE = 0xD001;

const DEMO_AGENTS = [
  { suffix: 0xD001, label: 'Architect', agentType: 'architect', action: AgentActionType.Create, verb: 'Mapping' },
  { suffix: 0xD002, label: 'Analyst',   agentType: 'analyst',   action: AgentActionType.Query,  verb: 'Analysing' },
  { suffix: 0xD003, label: 'Coder',     agentType: 'coder',     action: AgentActionType.Update, verb: 'Refactoring' },
  { suffix: 0xD004, label: 'Reviewer',  agentType: 'reviewer',  action: AgentActionType.Link,   verb: 'Reviewing' },
  { suffix: 0xD005, label: 'Tester',    agentType: 'tester',    action: AgentActionType.Query,  verb: 'Testing' },
  { suffix: 0xD006, label: 'Optimizer', agentType: 'optimizer', action: AgentActionType.Transform, verb: 'Optimising' },
] as const;

/** Loop timings (ms) — identical to the XR director. */
export const DEMO_TIMING = {
  staggerMs: [0, 1_300, 2_900, 4_600, 6_800, 9_100],
  workMinMs: 9_000, workMaxMs: 14_000,
  followMinMs: 7_000, followMaxMs: 11_000,
  restMinMs: 18_000, restMaxMs: 26_000,
  beamIntervalMs: 2_000,
  followChance: 0.6,
  candidatePool: 80,
} as const;

type Phase = 'wait' | 'working' | 'follow' | 'rest';

interface AgentCycleState {
  wireId: number;
  stringId: string;
  role: (typeof DEMO_AGENTS)[number];
  targetNodeId: string;
  phase: Phase;
  phaseStart: number;
  phaseDur: number;
}

let running = false;
let cycleTimer: ReturnType<typeof setInterval> | null = null;
let beamTimer: ReturnType<typeof setInterval> | null = null;
let agentStates: AgentCycleState[] = [];
let adjacency = new Map<string, string[]>();
let candidates: string[] = [];

function rand(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

function isAgentNode(n: Node): boolean {
  return n.metadata?.nodeType === 'agent' || n.metadata?.type === 'agent';
}

/** Synchronous view of the last graph payload (the async getter would race the injection). */
function currentGraph(): { nodes: Node[]; edges: Edge[] } {
  const data = (graphDataManager as unknown as { lastGraphData?: { nodes: Node[]; edges: Edge[] } | null }).lastGraphData;
  return data ?? { nodes: [], edges: [] };
}

/** Graph (non-agent) nodes ranked by degree (highest first) — the graph's mass centre. */
function buildGraphIndex(): void {
  const data = currentGraph();
  adjacency = new Map();
  const degree = new Map<string, number>();
  for (const e of (data.edges as Edge[]) ?? []) {
    const s = String(e.source), t = String(e.target);
    if (!adjacency.has(s)) adjacency.set(s, []);
    if (!adjacency.has(t)) adjacency.set(t, []);
    adjacency.get(s)!.push(t);
    adjacency.get(t)!.push(s);
    degree.set(s, (degree.get(s) ?? 0) + 1);
    degree.set(t, (degree.get(t) ?? 0) + 1);
  }
  const real = (data.nodes as Node[]).filter(n => !isAgentNode(n)).map(n => String(n.id));
  real.sort((a, b) => (degree.get(b) ?? 0) - (degree.get(a) ?? 0));
  candidates = real.slice(0, DEMO_TIMING.candidatePool);
  if (candidates.length === 0) candidates = real.length ? real : ['1'];
}

function pickTarget(prev?: string): string {
  if (prev) {
    const nb = neighbourOf(prev);
    if (nb && Math.random() < 0.5) return nb;
  }
  return candidates[Math.floor(Math.random() * candidates.length)];
}

function neighbourOf(nodeId: string): string | null {
  const nb = adjacency.get(nodeId);
  if (!nb || nb.length === 0) return null;
  for (let i = 0; i < Math.min(8, nb.length); i++) {
    const pick = nb[Math.floor(Math.random() * nb.length)];
    if (pick !== nodeId && candidates.includes(pick)) return pick;
  }
  const fallback = nb[Math.floor(Math.random() * nb.length)];
  return fallback !== nodeId ? fallback : null;
}

async function injectDemoNodes(): Promise<void> {
  const now = performance.now();
  let i = 0;
  for (const agent of DEMO_AGENTS) {
    const wireId = (AGENT_NODE_FLAG | agent.suffix) >>> 0;
    const stringId = String(wireId);
    const angle = (agent.suffix - DEMO_SUFFIX_BASE) * 1.05;
    const node: Node = {
      id: stringId,
      label: agent.label,
      position: {
        x: Math.cos(angle) * 40 + (Math.random() - 0.5) * 6,
        y: (Math.random() - 0.5) * 16,
        z: Math.sin(angle) * 40 + (Math.random() - 0.5) * 6,
      },
      metadata: {
        nodeType: 'agent',
        agentType: agent.agentType,
        type: 'agent',
        health: 0.85 + Math.random() * 0.15,
        status: 'idle',
      },
    };
    await graphDataManager.addNode(node);
    agentStates.push({
      wireId,
      stringId,
      role: agent,
      targetNodeId: pickTarget(),
      phase: 'wait',
      phaseStart: now,
      phaseDur: DEMO_TIMING.staggerMs[i % DEMO_TIMING.staggerMs.length],
    });
    i++;
  }
}

function beamFor(state: AgentCycleState): AgentActionEvent {
  return {
    sourceAgentId: state.wireId,
    targetNodeId: parseInt(state.targetNodeId, 10) || 1,
    actionType: state.role.action,
    timestamp: Date.now(),
    durationMs: DEMO_TIMING.beamIntervalMs,
    payload: new TextEncoder().encode(JSON.stringify({
      intent: `${state.role.verb}: ${labelOf(state.targetNodeId)}`,
    })),
  };
}

function labelOf(nodeId: string): string {
  const n = currentGraph().nodes.find(x => String(x.id) === nodeId);
  return n?.label || `node ${nodeId}`;
}

function pushBeams(states: AgentCycleState[]): void {
  const events = states.map(beamFor);
  if (events.length === 0) return;
  pushTransientBeams(events);
  useAgentTargetStore.getState().pushActions(events);
}

function pushBeamsForWorkingAgents(): void {
  pushBeams(agentStates.filter(s => s.phase === 'working' || s.phase === 'follow'));
}

function beginWork(state: AgentCycleState, phase: 'working' | 'follow', now: number): void {
  state.phase = phase;
  state.phaseStart = now;
  state.phaseDur = phase === 'working'
    ? rand(DEMO_TIMING.workMinMs, DEMO_TIMING.workMaxMs)
    : rand(DEMO_TIMING.followMinMs, DEMO_TIMING.followMaxMs);
  pushBeams([state]);
}

function cycleAgentPhases(): void {
  const now = performance.now();
  for (const state of agentStates) {
    if (now - state.phaseStart < state.phaseDur) continue;
    switch (state.phase) {
      case 'wait':
        beginWork(state, 'working', now);
        break;
      case 'working': {
        const nb = neighbourOf(state.targetNodeId);
        if (nb && Math.random() < DEMO_TIMING.followChance) {
          state.targetNodeId = nb;
          beginWork(state, 'follow', now);
        } else {
          complete(state, now);
        }
        break;
      }
      case 'follow':
        complete(state, now);
        break;
      case 'rest':
        state.targetNodeId = pickTarget(state.targetNodeId);
        beginWork(state, 'working', now);
        break;
    }
  }
}

function complete(state: AgentCycleState, now: number): void {
  state.phase = 'rest';
  state.phaseStart = now;
  state.phaseDur = rand(DEMO_TIMING.restMinMs, DEMO_TIMING.restMaxMs);
  // Explicit completion — same semantics as the XR registry's `done` status.
  markAgentDone(state.stringId);
  useAgentTargetStore.getState().remove(state.wireId);
}

async function removeDemoNodes(): Promise<void> {
  for (const state of agentStates) {
    await graphDataManager.removeNode(state.stringId);
    useAgentTargetStore.getState().remove(state.wireId);
  }
  agentStates = [];
}

export function isDemoRunning(): boolean {
  return running;
}

/** Current phase per demo agent (diagnostics / verification). */
export function demoPhases(): Record<string, Phase> {
  const out: Record<string, Phase> = {};
  for (const s of agentStates) out[s.role.label] = s.phase;
  return out;
}

export async function startDemo(): Promise<void> {
  if (running) return;
  running = true;
  buildGraphIndex();
  await injectDemoNodes();
  beamTimer = setInterval(pushBeamsForWorkingAgents, DEMO_TIMING.beamIntervalMs);
  cycleTimer = setInterval(cycleAgentPhases, 250);
}

export async function stopDemo(): Promise<void> {
  if (!running) return;
  running = false;
  if (beamTimer) { clearInterval(beamTimer); beamTimer = null; }
  if (cycleTimer) { clearInterval(cycleTimer); cycleTimer = null; }
  await removeDemoNodes();
}

export async function toggleDemo(): Promise<void> {
  if (running) await stopDemo();
  else await startDemo();
}
