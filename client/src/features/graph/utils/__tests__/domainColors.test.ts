import { describe, expect, it } from 'vitest';
import { DEFAULT_DOMAIN_COLOR, DOMAIN_COLORS, getDomainColor } from '../domainColors';
import { getDomainColor as getComputationDomainColor } from '../graphComputations';

const legacyDomains = [
  ['artificial-intelligence', 'AI', '#4FC3F7'],
  ['blockchain', 'BC', '#81C784'],
  ['robotics', 'RB', '#FFB74D'],
  ['spatial-computing', 'MV', '#CE93D8'],
  ['distributed-collaboration', 'NGM', '#4DB6AC'],
  ['infrastructure', 'TC', '#FFD54F'],
] as const;

describe('corpus domain colours', () => {
  it.each(legacyDomains)('preserves %s and its %s alias', (canonical, alias, colour) => {
    for (const domain of [canonical, canonical.toUpperCase(), alias, alias.toLowerCase()]) {
      expect(getDomainColor(domain)).toBe(colour);
      expect(getComputationDomainColor(domain)).toBe(colour);
    }
  });
  it('gives both new domains distinct colours matching the corpus export', () => {
    expect(getDomainColor('space-science-and-systems')).toBe('#646b9f');
    expect(getDomainColor('earth-observation-and-geospatial-sensing')).toBe('#438273');
    const colours = [...legacyDomains.map(([domain]) => getDomainColor(domain)),
      getDomainColor('space-science-and-systems'),
      getDomainColor('earth-observation-and-geospatial-sensing')];
    expect(new Set(colours).size).toBe(8);
    expect(colours).not.toContain(DEFAULT_DOMAIN_COLOR);
  });
  it.each([['DT', '#EF5350'], ['SEC', '#FF7043'], ['INFRA', '#78909C']])(
    'preserves the legacy %s colour', (domain, colour) => {
      expect(getDomainColor(domain)).toBe(colour);
    },
  );
  it.each([undefined, '', ' ', 'unrecognised', 'toString', '__proto__', 'constructor'])(
    'uses the fallback for %s', domain => {
      expect(getDomainColor(domain)).toBe(DEFAULT_DOMAIN_COLOR);
    },
  );
  it('accepts surrounding whitespace without mutating the shared palette', () => {
    expect(getDomainColor(' ai ')).toBe('#4FC3F7');
    expect(Object.isFrozen(DOMAIN_COLORS)).toBe(true);
  });
});
