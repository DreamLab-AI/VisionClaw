/**
 * agentDemoMode.ts — Synthetic agent injection for visual verification.
 *
 * Injects 6 demo agent nodes into the graph and cycles them through
 * work→idle→retarget phases so the sprite, momentum-nudge, and fade-to-edge
 * features can be exercised without a live backend swarm. Every injected
 * artefact is tagged `demo` and cleaned up on stop.
 */

import { graphDataManager } from '../graph/managers/graphDataManager';
import { pushTransientBeams } from '../../store/transientBeamStore';
import { useAgentTargetStore } from '../../store/agentTargetStore';
import { AgentActionType } from '../../services/binaryProtocol/frameTypes';
import type { AgentActionEvent } from '../../services/binaryProtocol/frameTypes';
import type { Node } from '../graph/managers/graphWorkerProxy';
import { AGENT_NODE_FLAG } from '../../types/binaryProtocol';

const DEMO_SUFFIX_BASE = 0xD001;

const DEMO_AGENTS = [
  { suffix: 0xD001, label: 'Demo-Architect', agentType: 'architect' },
  { suffix: 0xD002, label: 'Demo-Analyst',   agentType: 'analyst' },
  { suffix: 0xD003, label: 'Demo-Coder',     agentType: 'coder' },
  { suffix: 0xD004, label: 'Demo-Reviewer',   agentType: 'reviewer' },
  { suffix: 0xD005, label: 'Demo-Tester',     agentType: 'tester' },
  { suffix: 0xD006, label: 'Demo-Optimizer',  agentType: 'optimizer' },
] as const;

const WORK_DURATION_MS = 12_000;
const IDLE_DURATION_MS = 8_000;
const BEAM_INTERVAL_MS = 2_000;

const ACTION_TYPES: AgentActionType[] = [
  AgentActionType.Query,
  AgentActionType.Update,
  AgentActionType.Create,
  AgentActionType.Link,
  AgentActionType.Transform,
];

let running = false;
let cycleTimer: ReturnType<typeof setInterval> | null = null;
let beamTimer: ReturnType<typeof setInterval> | null = null;

interface AgentCycleState {
  wireId: number;
  stringId: string;
  targetNodeId: string;
  phase: 'working' | 'idle';
  phaseStart: number;
}

let agentStates: AgentCycleState[] = [];

function pickRandomTarget(): string {
  const data = graphDataManager['lastGraphData'];
  if (!data || data.nodes.length === 0) return '1';
  const nonDemo = data.nodes.filter(
    (n: Node) => !n.metadata?.agentType?.startsWith('demo'),
  );
  if (nonDemo.length === 0) return '1';
  return String(nonDemo[Math.floor(Math.random() * nonDemo.length)].id);
}

function pickRandomActionType(): AgentActionType {
  return ACTION_TYPES[Math.floor(Math.random() * ACTION_TYPES.length)];
}

async function injectDemoNodes(): Promise<void> {
  for (const agent of DEMO_AGENTS) {
    const wireId = (AGENT_NODE_FLAG | agent.suffix) >>> 0;
    const stringId = String(wireId);
    const spread = (agent.suffix - DEMO_SUFFIX_BASE) * 8;
    const angle = spread * 0.7;
    const node: Node = {
      id: stringId,
      label: agent.label,
      position: {
        x: Math.cos(angle) * 30 + (Math.random() - 0.5) * 10,
        y: (Math.random() - 0.5) * 20,
        z: Math.sin(angle) * 30 + (Math.random() - 0.5) * 10,
      },
      metadata: {
        nodeType: 'agent',
        agentType: agent.agentType,
        type: 'agent',
        demo: true,
        health: 0.85 + Math.random() * 0.15,
        status: 'working',
      },
    };
    await graphDataManager.addNode(node);
    agentStates.push({
      wireId,
      stringId,
      targetNodeId: pickRandomTarget(),
      phase: 'working',
      phaseStart: performance.now(),
    });
  }
}

function pushBeamsForWorkingAgents(): void {
  const events: AgentActionEvent[] = [];
  for (const state of agentStates) {
    if (state.phase !== 'working') continue;
    events.push({
      sourceAgentId: state.wireId,
      targetNodeId: parseInt(state.targetNodeId, 10) || 1,
      actionType: pickRandomActionType(),
      timestamp: Date.now(),
      durationMs: BEAM_INTERVAL_MS,
    });
  }
  if (events.length > 0) {
    pushTransientBeams(events);
    useAgentTargetStore.getState().pushActions(events);
  }
}

function cycleAgentPhases(): void {
  const now = performance.now();
  for (const state of agentStates) {
    const elapsed = now - state.phaseStart;
    if (state.phase === 'working' && elapsed >= WORK_DURATION_MS) {
      state.phase = 'idle';
      state.phaseStart = now;
    } else if (state.phase === 'idle' && elapsed >= IDLE_DURATION_MS) {
      state.targetNodeId = pickRandomTarget();
      state.phase = 'working';
      state.phaseStart = now;
    }
  }
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

export async function startDemo(): Promise<void> {
  if (running) return;
  running = true;
  await injectDemoNodes();
  pushBeamsForWorkingAgents();
  beamTimer = setInterval(pushBeamsForWorkingAgents, BEAM_INTERVAL_MS);
  cycleTimer = setInterval(cycleAgentPhases, 1_000);
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
