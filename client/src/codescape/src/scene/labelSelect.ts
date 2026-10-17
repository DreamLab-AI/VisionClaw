/**
 * Label budget: choose at most `max` labels by importance / distance², rejecting
 * any whose screen box overlaps an already accepted one on a 2D cell grid.
 * Pinned labels (focus, hover, selection) are placed first and always win.
 */

export interface LabelCandidate {
  /** Node index. */
  index: number;
  /** Screen-space anchor (px, origin top-left); the box is centred horizontally above it. */
  x: number;
  y: number;
  /** Box size in px. */
  w: number;
  h: number;
  /** importance / distance² (higher wins). */
  score: number;
  pinned?: boolean;
}

export interface SelectOptions {
  max: number;
  width: number;
  height: number;
  /** Grid cell size in px. */
  cell: number;
}

export function labelScore(importance: number, distance: number): number {
  const d = Math.max(1e-3, distance);
  return (0.05 + importance) / (d * d);
}

/**
 * Pure selection. Candidates are not mutated. Returns accepted node indices in
 * placement order (pinned first, then by score).
 */
export function selectLabels(candidates: readonly LabelCandidate[], opt: SelectOptions): number[] {
  const cols = Math.max(1, Math.ceil(opt.width / opt.cell));
  const rows = Math.max(1, Math.ceil(opt.height / opt.cell));
  const grid = new Uint8Array(cols * rows);
  const order = candidates
    .filter((c) => c.pinned || (c.x + c.w / 2 >= 0 && c.x - c.w / 2 <= opt.width && c.y >= 0 && c.y - c.h <= opt.height))
    .slice()
    .sort((a, b) => (a.pinned === b.pinned ? b.score - a.score : a.pinned ? -1 : 1));
  const out: number[] = [];
  for (const c of order) {
    if (out.length >= opt.max && !c.pinned) break;
    const x0 = Math.floor((c.x - c.w / 2) / opt.cell);
    const x1 = Math.floor((c.x + c.w / 2) / opt.cell);
    const y0 = Math.floor((c.y - c.h) / opt.cell);
    const y1 = Math.floor(c.y / opt.cell);
    let free = true;
    if (!c.pinned) {
      for (let gy = Math.max(0, y0); gy <= Math.min(rows - 1, y1) && free; gy++) {
        for (let gx = Math.max(0, x0); gx <= Math.min(cols - 1, x1); gx++) {
          if (grid[gy * cols + gx]) {
            free = false;
            break;
          }
        }
      }
    }
    if (!free) continue;
    for (let gy = Math.max(0, y0); gy <= Math.min(rows - 1, y1); gy++) {
      for (let gx = Math.max(0, x0); gx <= Math.min(cols - 1, x1); gx++) grid[gy * cols + gx] = 1;
    }
    out.push(c.index);
  }
  return out;
}

/**
 * Keep the `k` highest-scoring candidates without sorting all of them
 * (binary min-heap; O(n log k)). Used to bound the collision pass at 100k nodes.
 */
export function topK<T extends { score: number }>(items: Iterable<T>, k: number): T[] {
  const heap: T[] = [];
  const up = (i: number) => {
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (heap[p].score <= heap[i].score) break;
      [heap[p], heap[i]] = [heap[i], heap[p]];
      i = p;
    }
  };
  const down = (i: number) => {
    for (;;) {
      const l = 2 * i + 1;
      const r = l + 1;
      let m = i;
      if (l < heap.length && heap[l].score < heap[m].score) m = l;
      if (r < heap.length && heap[r].score < heap[m].score) m = r;
      if (m === i) return;
      [heap[m], heap[i]] = [heap[i], heap[m]];
      i = m;
    }
  };
  for (const it of items) {
    if (heap.length < k) {
      heap.push(it);
      up(heap.length - 1);
    } else if (k > 0 && it.score > heap[0].score) {
      heap[0] = it;
      down(0);
    }
  }
  return heap.sort((a, b) => b.score - a.score);
}
