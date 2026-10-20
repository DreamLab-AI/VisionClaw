import { describe, expect, it } from 'vitest';
import { summariseConsequences } from '../OntologyProposalList';
import type { ProposalConsequences } from '../../hooks/useOntologyStore';

// ADR-2147: the chip states the semantic size of a change, which the text
// diff cannot show, and says in its title that it is never a gate.
const base: ProposalConsequences = {
  rule_table: 'rdfs',
  verdict: 'not_conservative_under_rule_table',
  new_consequences: 206476,
  lost_consequences: 0,
  new_by_predicate: { type: 206476 },
  new_examples: [],
  lost_examples: [],
  not_compared_blank_node_bearing: 607510,
  base_derivations: 731144,
  proposal_derivations: 937620,
  proposal_asserted_sha256: 'a'.repeat(64),
};

describe('summariseConsequences', () => {
  it('names the count of new consequences and the leading predicate', () => {
    const s = summariseConsequences(base);
    expect(s.conservative).toBe(false);
    expect(s.label).toBe('+206,476 consequences');
    expect(s.title).toContain('206,476 new, 0 lost');
    expect(s.title).toContain('type 206,476');
    expect(s.title).toContain('never a gate');
  });

  it('shows lost consequences with a minus and orders predicates by count', () => {
    const s = summariseConsequences({
      ...base,
      new_consequences: 3,
      lost_consequences: 2,
      new_by_predicate: { subClassOf: 1, type: 2 },
    });
    expect(s.label).toBe('+3 −2 consequences');
    expect(s.title).toContain('new by predicate: type 2, subClassOf 1');
  });

  it('reads as conservative when nothing is added or lost', () => {
    const s = summariseConsequences({
      ...base,
      verdict: 'conservative_under_rule_table',
      new_consequences: 0,
      lost_consequences: 0,
      new_by_predicate: {},
    });
    expect(s.conservative).toBe(true);
    expect(s.label).toBe('No new consequences');
    expect(s.title).toContain('607,510 blank-node-bearing triples not compared');
  });
});
