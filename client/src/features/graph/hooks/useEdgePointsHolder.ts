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
