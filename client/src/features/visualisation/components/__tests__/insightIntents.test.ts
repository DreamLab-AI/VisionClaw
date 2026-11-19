import { describe, it, expect } from 'vitest';
import {
  planInsightIntent,
  INSIGHT_ENDPOINT_ALLOWLIST,
  INSIGHT_SETTING_PATH_ALLOWLIST,
} from '../insightIntents';
import { ALL_PATHS } from '../../../control-center/registry/settingsRegistry';

const PHRASES: Array<[string, string]> = [
  ['what are the main themes in this graph?', 'communities'],
  ['show me the communities', 'communities'],
  ['what connects blockchain and robotics?', 'bridges'],
  ['which concepts bridge ai and spatial-computing', 'bridges'],
  ['how is machine-learning organised?', 'hierarchy'],
  ['show the backbone of the ontology', 'hierarchy'],
  ['where are the gaps?', 'immature'],
  ['show immature concepts', 'immature'],
  ["what doesn't fit?", 'anomalies'],
  ['find the outliers', 'anomalies'],
  ['what are the most important concepts', 'hubs'],
  ['show the hubs', 'hubs'],
];

describe('planInsightIntent', () => {
  it.each(PHRASES)('%s -> %s', (phrase, id) => {
    const plan = planInsightIntent(phrase);
    expect(plan?.id).toBe(id);
    expect(plan?.answer.length).toBeGreaterThan(20);
  });

  it.each([
    'show hulls', 'hide cluster hulls', 'more repulsion', 'increase spring',
    'focus on blockchain', 'find robotics', 'search transformers', 'flatten the graphs',
    'hide the communities', 'turn off anomalies', 'stop showing hubs', 'layout hierarchy left to right',
    'radial hierarchy layout', 'switch to dark theme', 'increase the gap between nodes', 'make springs weak',
  ])('leaves view command "%s" alone', (phrase) => {
    expect(planInsightIntent(phrase)).toBeNull();
  });

  it('bridges name the two domains in the answer', () => {
    const plan = planInsightIntent('what connects blockchain and robotics?')!;
    expect(plan.answer).toMatch(/blockchain/);
    expect(plan.answer).toMatch(/robotics/);
    expect(plan.domains).toEqual(['blockchain', 'robotics']);
  });

  it('communities run detection, colour by community and show hulls', () => {
    const plan = planInsightIntent('what are the main themes')!;
    expect(plan.requests).toEqual([
      { method: 'POST', endpoint: '/api/analytics/community/detect', body: { algorithm: 'leiden', resolution: 1.0 } },
    ]);
    expect(plan.writes).toEqual(expect.arrayContaining([
      { path: 'visualisation.graphs.knowledge.nodes.colorScheme', value: 'community' },
      { path: 'visualisation.clusterHulls.enabled', value: true },
    ]));
  });

  it('community-style plans clear the anomaly overlay and enable community hulls', () => {
    for (const p of ['what are the main themes', 'show the hubs', 'what connects ai and robotics']) {
      const w = planInsightIntent(p)!.writes;
      expect(w).toContainEqual({ path: 'qualityGates.showAnomalies', value: false });
    }
    expect(planInsightIntent('main themes')!.writes)
      .toContainEqual({ path: 'visualisation.clusterHulls.communityFallback', value: true });
  });

  it('hierarchy never switches layout mode; it names the concept and keeps physics', () => {
    const plan = planInsightIntent('how is robotics organised')!;
    expect(plan.requests).toEqual([]);
    expect(plan.writes.map((w) => w.path)).not.toContain('visualisation.graphs.knowledge.physics.layoutMode');
    expect(plan.concept).toBe('robotics');
    expect(plan.answer).toMatch(/physics/i);
  });

  it('hierarchy without a named concept asks for one', () => {
    const plan = planInsightIntent('show the backbone of the ontology')!;
    expect(plan.concept).toBeUndefined();
    expect(plan.answer).toMatch(/name a concept|select a concept/i);
    expect(plan.writes).toEqual([]);
  });

  it('gaps focus on important-but-immature concepts and leave the node filter untouched', () => {
    const plan = planInsightIntent('where are the gaps')!;
    expect(plan.id).toBe('immature');
    expect(plan.writes.filter((w) => w.path.startsWith('nodeFilter.'))).toEqual([]);
    expect(plan.followUp).toBe('gaps');
    expect(plan.answer).not.toMatch(/grey/i);
  });

  it('anomalies do not promise red nodes before the result is known', () => {
    const plan = planInsightIntent('find the outliers')!;
    expect(plan.requests[0]).toMatchObject({ endpoint: '/api/analytics/anomaly/detect', body: { method: 'lof' } });
    expect(plan.writes).toContainEqual({ path: 'qualityGates.showAnomalies', value: true });
    expect(plan.followUp).toBe('anomalies');
    expect(plan.answer).not.toMatch(/red/i);
  });

  it('bridges and hubs carry a follow-up and communities mention cross-domain hulls', () => {
    expect(planInsightIntent('what connects blockchain and robotics?')!.followUp).toBe('bridges');
    expect(planInsightIntent('show the hubs')!.followUp).toBe('hubs');
    expect(planInsightIntent('main themes')!.answer).toMatch(/cross-domain theme/i);
  });

  it('hubs compute PageRank and colour by centrality', () => {
    const plan = planInsightIntent('show the hubs')!;
    expect(plan.requests[0].endpoint).toBe('/api/analytics/pagerank/compute');
    expect(plan.writes).toContainEqual({ path: 'visualisation.graphs.knowledge.nodes.colorScheme', value: 'centrality' });
  });
});

describe('allow-lists', () => {
  const emitted = PHRASES.map(([p]) => planInsightIntent(p)!);

  it('every request is a mounted server route', () => {
    for (const plan of emitted) {
      for (const r of plan.requests) {
        expect(INSIGHT_ENDPOINT_ALLOWLIST).toContain(`${r.method} ${r.endpoint}`);
      }
    }
  });

  it('every setting write is allow-listed and a registered settings path', () => {
    for (const plan of emitted) {
      for (const w of plan.writes) {
        expect(INSIGHT_SETTING_PATH_ALLOWLIST).toContain(w.path);
        expect(ALL_PATHS as readonly string[]).toContain(w.path);
      }
    }
  });
});
