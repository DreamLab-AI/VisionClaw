/**
 * Client-side offsets between a node's server position and where it is DRAWN.
 *
 * Agent nodes are drawn at `SAB position + drift + momentum nudge` (GemNodes). Edges, the
 * highlight and inferred edges read the raw SAB, so without this they ended at a point the
 * node had already left. GemNodes publishes each agent's total offset here every frame;
 * the edge fills add it to the endpoint. Indexed by SAB node index, 3 floats per node.
 */
export class DrawnOffsets {
  /** `[dx,dy,dz]` per node index; zero means drawn at the server position. */
  data = new Float32Array(0)
  /** Bumped whenever any stored offset actually changes (feeds the edge skip gate). */
  version = 0
  private written = new Set<number>()
  private lastWritten = new Set<number>()

  /** Start an agent pass: remember which slots this pass writes. */
  beginFrame(): void {
    this.written.clear()
  }

  /** End an agent pass: slots written last pass but not this one go back to zero. */
  endFrame(): void {
    for (const idx of this.lastWritten) {
      if (!this.written.has(idx)) this.set(idx, 0, 0, 0)
    }
    const done = this.written
    this.written = this.lastWritten
    this.lastWritten = done
  }

  /** Record the offset for node `idx`; bumps `version` only on a real change. */
  set(idx: number, dx: number, dy: number, dz: number): void {
    this.written.add(idx)
    const o = idx * 3
    if (o + 3 > this.data.length) {
      const grown = new Float32Array(Math.max(o + 3, this.data.length * 2))
      grown.set(this.data)
      this.data = grown
    }
    const d = this.data
    if (d[o] === dx && d[o + 1] === dy && d[o + 2] === dz) return
    d[o] = dx
    d[o + 1] = dy
    d[o + 2] = dz
    this.version++
  }
}

/** Shared by GemNodes (writer) and the edge, highlight and inferred-edge layers (readers). */
export const drawnOffsets = new DrawnOffsets()
