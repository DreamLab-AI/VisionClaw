/**
 * The ONE drawn-positions array of the desktop client (graph nodes).
 *
 * Each frame GraphManager copies the worker's shared position buffer once into this array and applies the
 * drag hand position to the copy once. Everything that needs to know where a node is drawn reads it through
 * `nodePositionsRef`: the gems, the edge fill, inferred edges, beams' target ends, the edge skip gate, labels,
 * picking, fly-to, hulls and rings. No consumer adds an offset of its own.
 *
 * Agents are not graph nodes (ADR-2142 / ADR-2135, owner rule 2026-11-01) and have no slot here: their drawn
 * position is the marker pass's map (`features/bots/agentMarkerPass.ts`), the one writer of an agent point.
 *
 * The shared buffer is never written here. The worker rewrites it on its own 16 ms timer,
 * so a main-thread `+=` would be applied twice on any frame the worker skipped (and would
 * race the worker); writing the copy cannot.
 */

export class DrawnPositions {
  private drawn = new Float32Array(0)

  /** The drawn positions. Exactly as long as the shared buffer, so every length check stays honest. */
  get positions(): Float32Array {
    return this.drawn
  }

  /** Copy this frame's shared buffer in. Reallocates only when its length changes. */
  copy(sab: Float32Array): Float32Array {
    if (this.drawn.length !== sab.length) this.drawn = new Float32Array(sab.length)
    this.drawn.set(sab)
    return this.drawn
  }

  /**
   * Draw the dragged node at the hand. It is the one place the hand position meets the drawn array: the gem, its edges,
   * the label, inferred edges, picking and the rest all read the result, the same frame.
   */
  applyDrag(index: number, hand: { x: number; y: number; z: number }): void {
    const i3 = index * 3
    if (index < 0 || i3 + 2 >= this.drawn.length) return
    this.drawn[i3] = hand.x
    this.drawn[i3 + 1] = hand.y
    this.drawn[i3 + 2] = hand.z
  }
}

/** The shared instance GraphManager drives; tests build their own. */
export const drawnPositions = new DrawnPositions()

/** Copy the first `count` drawn positions into the label array (`labelPositionsRef`). */
export function fillLabelPositions(
  drawn: Float32Array,
  count: number,
  out: Array<{ x: number; y: number; z: number }>,
): void {
  for (let i = 0; i < count; i++) {
    const i3 = i * 3
    out[i].x = drawn[i3]
    out[i].y = drawn[i3 + 1]
    out[i].z = drawn[i3 + 2]
  }
}
