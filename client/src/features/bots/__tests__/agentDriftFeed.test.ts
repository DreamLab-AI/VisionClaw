import { describe, it, expect } from 'vitest';
import { AgentDriftFeed, populationVertex } from '../agentDriftFeed';
import { triangleFrame, Vertex } from '../../graph/triLayout';
import { AGENT_NODE_FLAG } from '../../../types/binaryProtocol';

const NODES: Record<string, { metadata?: Record<string, unknown>; type?: string }> = {
  '10': { metadata: { type: 'page' } },
  '11': { metadata: { type: 'ontology_node' } },
  '12': { metadata: { type: 'domain_root' } },
  '13': { metadata: { type: 'agent' } },
};
const AGENT = String((AGENT_NODE_FLAG | 0xd001) >>> 0);
const OTHER = String((AGENT_NODE_FLAG | 0xd002) >>> 0);

function feed(now: { t: number }) {
  return new AgentDriftFeed({
    lookupNode: (id) => NODES[id],
    agentKeys: () => [AGENT, OTHER],
    nowS: () => now.t,
  });
}

const beam = (id: number, src: string, target: number, startMs: number) => ({
  id, sourceAgentId: Number(src), targetNodeId: target, startTime: startMs,
});

describe('populationVertex mirrors Node::population', () => {
  it('classifies like the server', () => {
    expect(populationVertex(NODES['10'])).toBe(Vertex.Knowledge);
    expect(populationVertex(NODES['11'])).toBe(Vertex.Ontology);
    expect(populationVertex({ metadata: { type: 'owl_property' } })).toBe(Vertex.Ontology);
    expect(populationVertex(NODES['12'])).toBe(Vertex.Knowledge); // server fallback, not the colour mode
    expect(populationVertex({ metadata: {}, type: 'linked_page' })).toBe(Vertex.Knowledge);
    expect(populationVertex({ metadata: { owl_class_iri: 'mv:Avatar' } })).toBe(Vertex.Ontology);
    expect(populationVertex(NODES['13'])).toBeNull();
    expect(populationVertex(undefined)).toBeNull();
  });
});

describe('AgentDriftFeed', () => {
  it('a beam on an ontology node drifts its agent towards the ontology vertex', () => {
    const now = { t: 0 };
    const f = feed(now);
    f.ingestBeams([beam(1, AGENT, 11, 0)]);
    for (let i = 1; i <= 40; i++) { now.t = i * 0.1; f.step(); }
    const frame = triangleFrame(300);
    const o = f.offset(AGENT, frame);
    expect(o[0]).toBeGreaterThan(10); // ontology is front-right (+X)
    expect(f.offset(OTHER, frame)).toEqual([0, 0, 0]);
  });

  it('ignores beams already seen and agent-to-agent beams', () => {
    const now = { t: 0 };
    const f = feed(now);
    f.ingestBeams([beam(1, AGENT, 13, 0)]);
    f.ingestBeams([beam(1, AGENT, 11, 0)]); // same id: already processed
    for (let i = 1; i <= 20; i++) { now.t = i * 0.1; f.step(); }
    expect(f.size).toBe(0);
  });

  it('a named memory flash credits that agent; an anonymous one is shared', () => {
    const now = { t: 0 };
    const f = feed(now);
    f.ingestMemoryFlash({ agentId: 0xd001 }); // flag added to match agent keys
    expect(f.weights(AGENT)?.[Vertex.Memory]).toBeCloseTo(1);
    f.ingestMemoryFlash({});
    expect(f.weights(AGENT)?.[Vertex.Memory]).toBeCloseTo(1.5);
    expect(f.weights(OTHER)?.[Vertex.Memory]).toBeCloseTo(0.5);
    for (let i = 1; i <= 40; i++) { now.t = i * 0.1; f.step(); }
    expect(f.offset(AGENT, triangleFrame(300))[2]).toBeLessThan(-10); // memory is behind (−Z)
  });
});
