import { useCallback, useRef, useState } from 'react'

/**
 * Holds a bulk edge-point array (6 numbers per edge, ~880k on the 147k-edge
 * graph) behind a stable ref, and tells React it changed with a small integer.
 *
 * React 19 dev builds diff a component's previous and next props on every
 * re-render, writing an entry per changed array element and then
 * structured-cloning the result into `performance.measure()`. Passing the array
 * itself as a prop therefore ends in "DataCloneError: out of memory". Pass
 * `ref` and `version` instead: the ref never changes identity and the version
 * is a primitive, so the diff is O(1) and the consumer still re-runs its
 * effects when the data is replaced.
 */
export function useEdgePointsHolder() {
  const ref = useRef<number[]>([])
  const [version, setVersion] = useState(0)
  const set = useCallback((points: number[]) => {
    ref.current = points
    setVersion(v => v + 1)
  }, [])
  return { ref, version, set }
}

/**
 * The same idea for a value another hook already produces (a memoised edge
 * plan, overview edge data: typed arrays with one entry per edge). The ref is
 * stable; `version` changes exactly when `value`'s identity does, so a child
 * that lists `version` in its dependency arrays re-runs when it should.
 * React's dev props diff expands typed arrays index by index, so never hand
 * one to a component as a prop.
 */
export function useRefVersion<T>(value: T) {
  const ref = useRef<T>(value)
  const version = useRef(0)
  if (ref.current !== value) {
    ref.current = value
    version.current++
  }
  return { ref, version: version.current }
}
