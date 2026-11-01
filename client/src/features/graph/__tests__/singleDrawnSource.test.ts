/**
 * Guard for "parallel paths are poison": there is ONE source of drawn positions.
 *
 * The behaviour is pinned in drawnPositions.test.ts; this pins the shape. Only GraphManager
 * reads the worker's shared buffer, and nothing re-adds an offset on top of the drawn array.
 */
import { describe, it, expect } from 'vitest'

// vite-node here cannot resolve static `fs`/`path` imports; take them from the runtime.
type Fs = typeof import('fs')
type PathMod = typeof import('path')
const getBuiltin = (process as unknown as { getBuiltinModule(id: string): unknown }).getBuiltinModule
const { readdirSync, readFileSync, statSync, existsSync } = getBuiltin('fs') as Fs
const { join, resolve } = getBuiltin('path') as PathMod

const SRC = resolve(__dirname, '../../..')

function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name === '__tests__' || name === 'node_modules') continue
      sources(p, out)
    } else if (/\.(ts|tsx)$/.test(name) && !/\.(test|spec)\./.test(name)) out.push(p)
  }
  return out
}

describe('one drawn-positions source', () => {
  const files = sources(SRC)

  it('the side-channel offsets module is gone', () => {
    expect(existsSync(join(SRC, 'features/graph/utils/drawnOffsets.ts'))).toBe(false)
  })

  it.each(['drawnOffsets', 'offsetsVersion', 'resolveDrawnNodePosition', 'DrawnOffsets'])(
    'nothing in the client mentions %s',
    (needle) => {
      const hits = files.filter(f => readFileSync(f, 'utf8').includes(needle)).map(f => f.replace(SRC, ''))
      expect(hits).toEqual([])
    },
  )

  it('only GraphManager (and the proxy that owns it) touches the worker shared buffer', () => {
    const hits = files
      .filter(f => readFileSync(f, 'utf8').includes('getPositionsSync'))
      .map(f => f.replace(SRC, ''))
      .sort()
    expect(hits).toEqual([
      '/features/graph/components/GraphManager.tsx',
      '/features/graph/managers/graphWorkerProxy.ts',
    ])
  })

  it('GemNodes holds no agent offset logic of its own', () => {
    const gem = readFileSync(join(SRC, 'features/graph/components/GemNodes.tsx'), 'utf8')
    expect(gem).not.toMatch(/stepAgentOffset|agentDriftFeed|agentOffsetsEnabled/)
  })

  it('GemNodes and the edge fill take no drag override of their own', () => {
    const gem = readFileSync(join(SRC, 'features/graph/components/GemNodes.tsx'), 'utf8')
    expect(gem).not.toMatch(/currentNodePos3D|dragDataRef|dragLocalIdx/)
    const plan = readFileSync(join(SRC, 'features/graph/hooks/edgeBufferPlan.ts'), 'utf8')
    expect(plan).not.toMatch(/dragIdx|dragPos|dragging|wasDragging/)
    const hook = readFileSync(join(SRC, 'features/graph/hooks/useEdgeBufferComputation.ts'), 'utf8')
    expect(hook).not.toMatch(/currentNodePos3D|dragDataRef|dragIdx|dragPos/)
  })

  it('only GraphManager writes the hand position, and only into the drawn array', () => {
    const hits = files
      .filter(f => readFileSync(f, 'utf8').includes('applyDrag('))
      .map(f => f.replace(SRC, ''))
      .sort()
    expect(hits).toEqual(['/features/graph/components/GraphManager.tsx', '/features/graph/utils/drawnPositions.ts'])
  })
})

// ADR-2142 / ADR-2135 (owner rule, 2026-11-01): agents are not graph nodes. Every agent reaches the desktop only as
// 0x23 frames; the client registry is the one source of embodiment and the marker pass the one position writer.
describe('agents have ONE state registry: agentTargetStore', () => {
  it('the parallel work tracker is gone, with every reference', () => {
    expect(existsSync(join(SRC, 'features/bots/agentWorkTargets.ts'))).toBe(false)
    const hits = sources(SRC)
      .filter(f => /agentWorkTargets|getAgentWork|markAgentDone|AGENT_DONE_ACTIVITY/.test(readFileSync(f, 'utf8')))
      .map(f => f.replace(SRC, ''))
    expect(hits).toEqual([])
  })
})

describe('agents have ONE position path: the marker map', () => {
  const files = sources(SRC)
  const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8')

  it.each([
    'features/bots/components/BotsVisualization.tsx',
    'features/bots/components/BotsNode.tsx',
    'features/bots/components/BotsEdges.tsx',
    'features/bots/agentDrawnPosition.ts',
  ])('%s is deleted', (rel) => {
    expect(existsSync(join(SRC, rel))).toBe(false)
  })

  it('the drawn-positions array has no agent pass and no agent slots', () => {
    const src = read('features/graph/utils/drawnPositions.ts')
    expect(src).not.toMatch(/applyAgents|AgentSlot|resolveAgentSlots|liveAgentPassDeps|stepAgentOffset/)
    expect(read('features/graph/components/GraphManager.tsx')).not.toMatch(/applyAgents|agentSlots|resolveAgentSlots|isVirtualAgentId/)
  })

  it('stepAgentOffset has one caller: the marker pass', () => {
    const hits = files.filter(f => /stepAgentOffset\(/.test(readFileSync(f, 'utf8'))).map(f => f.replace(SRC, '')).sort()
    expect(hits).toEqual(['/features/bots/agentMarkerPass.ts', '/features/graph/utils/agentNudge.ts'])
  })

  it('GraphManager runs the marker pass once, and resolves beam sources from the marker map only', () => {
    const gm = read('features/graph/components/GraphManager.tsx')
    expect(gm.match(/agentMarkers\.step\(/g)).toHaveLength(1)
    expect(gm).toContain('agentMarkerPosition')
    expect(gm).not.toMatch(/resolveVirtualAgentPosition/)
  })

  it('the marker layer draws every registry agent, not the six reserved roles', () => {
    const src = read('features/visualisation/components/VirtualAgentMarkers.tsx')
    expect(src).not.toMatch(/VIRTUAL_ROLES\.forEach|resolveVirtualAgentPosition/)
    expect(src).toContain('agentMarkers')
  })

  it('no features/bots component reads a position other than the marker map', () => {
    const bots = files.filter(f => f.includes('/features/bots/') && f.endsWith('.tsx'))
    for (const f of bots) {
      const src = readFileSync(f, 'utf8')
      expect(src, f).not.toMatch(/agent\.position|agentDrawnPosition|sharedNodePositions|resolveNodeWorldPosition|nodePositionsRef/)
    }
  })

  it('BotsAgent has no position or velocity, and the context has no binary position handler', () => {
    expect(read('features/bots/types/BotsTypes.ts')).not.toMatch(/position\?:|velocity\?:|lastPositionUpdate/)
    expect(read('features/bots/contexts/BotsDataContext.tsx')).not.toMatch(/updateFromBinaryPositions|bots-binary-position-update|parseBinaryFrameData/)
  })

  it('nothing else in the client reads agent.position for drawing', () => {
    const hits = files
      .filter(f => f.includes('/features/') && /agent\.position\b/.test(readFileSync(f, 'utf8')))
      .map(f => f.replace(SRC, ''))
      .sort()
    expect(hits).toEqual([])
  })
})
