// ADR-2204 S2 (E1): the desktop edge colour table is generated from
// crates/visionclaw-domain/data/relations.yaml (relations.generated.json). This pins the colours the
// table held when it was hand-written, so generating it changes nothing.
import { describe, it, expect } from 'vitest'
import {
  EDGE_TYPE_COLORS,
  DEFAULT_EDGE_COLOR,
  getEdgeTypeColor,
} from '../useGraphNodeColors'
import projection from '../../relations.generated.json'

const LEGACY: Record<string, string> = {
  hierarchical: 'ffd700', subclass: 'ffd700',
  structural: '4fc3f7', has_part: '4fc3f7', is_part_of: '4fc3f7',
  dependency: '81c784', requires: '81c784', depends_on: '81c784', enables: '81c784',
  associative: 'ce93d8', relates_to: 'ce93d8',
  bridge: 'ff7043', bridges_to: 'ff7043', bridges_from: 'ff7043',
  provenance: 'ffb300', utilisation: '26c6da', co_citation: '5c6bc0',
  standardisation: '7e8ce0', implements: 'ec407a',
  explicit_link: '5a6470', namespace: '78909c', inferred: 'b0bec5',
  domain_member: 'a1887f', chain_payment: 'e040fb',
}

describe('edge type colours', () => {
  it('keeps every hand-written colour', () => {
    for (const [key, hex] of Object.entries(LEGACY)) {
      expect(getEdgeTypeColor(key).getHexString(), key).toBe(hex)
    }
  })

  it('has exactly the legacy key set (nothing added or lost)', () => {
    expect(Object.keys(EDGE_TYPE_COLORS).sort()).toEqual(Object.keys(LEGACY).sort())
  })

  it('keeps the lowercase fallback and the default colour', () => {
    expect(getEdgeTypeColor('HIERARCHICAL').getHexString()).toBe('ffd700')
    expect(getEdgeTypeColor('no-such-edge').getHexString()).toBe('aaaaaa')
    expect(getEdgeTypeColor(undefined)).toBe(DEFAULT_EDGE_COLOR)
  })

  it('is built from the generated projection', () => {
    expect(Object.keys(projection.colours).sort()).toEqual(Object.keys(LEGACY).sort())
    expect(projection.defaultColour.toLowerCase()).toBe('#aaaaaa')
  })
})
