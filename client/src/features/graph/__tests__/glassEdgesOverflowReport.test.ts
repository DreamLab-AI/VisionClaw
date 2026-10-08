/**
 * The edge-ceiling overflow line is reported once per overflow episode. It
 * used to key on the exact edge count, which jitters by a few edges every
 * frame while physics settles (the distance cull), so it logged on nearly
 * every frame of a cold load on a 144k-edge graph.
 */
import { describe, it, expect } from 'vitest'
import { OverflowReporter } from '../components/GlassEdges'

describe('OverflowReporter', () => {
  it('reports once while the count stays above the ceiling, however it jitters', () => {
    const r = new OverflowReporter()
    expect(r.shouldReport(144674, 65536)).toBe(true)
    expect(r.shouldReport(144501, 65536)).toBe(false)
    expect(r.shouldReport(144496, 65536)).toBe(false)
    expect(r.shouldReport(144674, 65536)).toBe(false)
  })

  it('reports again after the count has dropped back within the ceiling', () => {
    const r = new OverflowReporter()
    expect(r.shouldReport(70000, 65536)).toBe(true)
    r.observe(60000, 65536)
    expect(r.shouldReport(70000, 65536)).toBe(true)
  })

  it('stays latched while the count remains above the ceiling', () => {
    const r = new OverflowReporter()
    r.shouldReport(70000, 65536)
    r.observe(69000, 65536)
    expect(r.shouldReport(70001, 65536)).toBe(false)
  })
})
