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

// The agent overlay (membrane, nameplate, trail, collaboration edges, payment labels) is drawn from the
// same array as the gem. It holds no position source and no nudge of its own.
describe('the bots overlay has no second agent path', () => {
  const files = sources(SRC)
  const OVERLAY = [
    'features/bots/components/BotsVisualization.tsx',
    'features/bots/components/BotsNode.tsx',
    'features/bots/components/BotsEdges.tsx',
    'features/bots/components/AgentTrail.tsx',
    'features/bots/chain/ChainPaymentLabels.tsx',
    'features/bots/components/BotsShared.tsx',
  ]
  const read = (rel: string) => readFileSync(join(SRC, rel), 'utf8')

  it.each(OVERLAY)('%s reads no agent.position, no server position map and no nudge', (rel) => {
    const src = read(rel)
    expect(src).not.toMatch(/agent\.position/)
    expect(src).not.toMatch(/nudgeTargets|targetPositionRef|lerpVector3|positionsRef|useAgentTargetStore|initial_calculation/)
  })

  it.each(['features/bots/components/BotsNode.tsx', 'features/bots/components/BotsEdges.tsx', 'features/bots/chain/ChainPaymentLabels.tsx'])(
    '%s takes agent positions from the drawn array through agentDrawnPosition',
    (rel) => {
      expect(read(rel)).toContain('agentDrawnPosition')
    },
  )

  it('agentDrawnPosition is the one resolver, and it resolves through resolveNodeWorldPosition', () => {
    const src = read('features/bots/agentDrawnPosition.ts')
    expect(src).toContain('resolveNodeWorldPosition')
    expect(src).toContain('sharedNodePositions')
  })

  it('nothing else in the client reads agent.position for drawing', () => {
    const hits = files
      .filter(f => f.includes('/features/') && /agent\.position\b/.test(readFileSync(f, 'utf8')))
      .map(f => f.replace(SRC, ''))
      .sort()
    expect(hits).toEqual([])
  })
})
