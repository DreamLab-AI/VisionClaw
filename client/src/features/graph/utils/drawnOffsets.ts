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

  /** Record the offset for node `idx`; bumps `version` only on a real change. */
  set(idx: number, dx: number, dy: number, dz: number): void {
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
