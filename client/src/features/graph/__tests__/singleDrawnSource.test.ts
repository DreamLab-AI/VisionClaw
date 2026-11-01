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
})
