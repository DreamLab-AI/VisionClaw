import { describe, it, expect } from 'vitest';
import { matchDomainNodeIds } from '../insightDomainHighlight';

describe('matchDomainNodeIds', () => {
  const nodes = [
    { id: 1, metadata: { domain: 'blockchain' } },
    { id: 2, metadata: { domain: 'Spatial Computing' } },
    { id: 3, metadata: { source_domain: 'robotics' } },
    { id: 4, metadata: { domain: 'ai' } },
    { id: 5 },
  ];
  it('matches domain or source_domain, case/space-insensitive', () => {
    expect(matchDomainNodeIds(nodes, ['blockchain', 'spatial-computing', 'robotics'])).toEqual(['1', '2', '3']);
  });
  it('matches nothing for an unknown domain', () => {
    expect(matchDomainNodeIds(nodes, ['nope'])).toEqual([]);
  });
});
