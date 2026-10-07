/**
 * Turns a search trace into a discovery tree: each node hangs off the node
 * that first evaluated it, kept nodes carry a subtree size, a leaf
 * coordinate and an angular sector, and the path runs from the entry point
 * to the best result.
 *
 * Adapted from the RuVector Explorer's `buildTree`
 * (https://github.com/ruvnet/RuVector, docs/explorer, MIT licence).
 */
import type { SearchResult, SearchTree, SearchTreeNode } from './types';
import { hash01 } from './vec';

/** Half-width of the canopy fan in radians, as in the Explorer. */
export const CANOPY_HALF_ANGLE = 1.42;

/** Spread of a rejected node's leaf coordinate around its parent's. */
const REJECTED_LEAF_SPREAD = 2.2;

function makeNode(id: number, parent: number, depth: number, layer: number, kept: boolean, learned: boolean, distance: number, order: number): SearchTreeNode {
  return {
    id,
    parent,
    depth,
    layer,
    kept,
    learned,
    distance,
    order,
    rank: -1,
    children: [],
    subtreeSize: 1,
    leafX: 0,
    angle: 0,
    hash: hash01(id),
  };
}

/**
 * Builds the tree rooted at `root` (the graph's entry point). Rejected
 * nodes have `rank` -1 and `subtreeSize` 1. When no result is reachable the
 * path is just `[root]`.
 */
export function buildSearchTree(result: SearchResult, root: number): SearchTree {
  const nodes = new Map<number, SearchTreeNode>();
  const rootLayer = result.trace.reduce((m, e) => Math.max(m, e.l), 0);
  const rootNode = makeNode(root, -1, 0, rootLayer, true, false, 0, 0);
  nodes.set(root, rootNode);
  let order = 1;
  for (const e of result.trace) {
    if (e.t !== 'eval') continue;
    const existing = nodes.get(e.n);
    if (existing) {
      if (e.ok) existing.kept = true;
      continue;
    }
    const parent = nodes.get(e.from);
    if (!parent) continue;
    const nd = makeNode(e.n, e.from, parent.depth + 1, e.l, e.ok, !!e.learned, e.d, order++);
    nodes.set(e.n, nd);
    parent.children.push(nd);
  }
  for (const e of result.trace) {
    if (e.t !== 'pop') continue;
    const n = nodes.get(e.n);
    if (!n) continue;
    n.kept = true;
    if (n === rootNode) rootNode.distance = e.d;
  }

  const list = [...nodes.values()].sort((a, b) => a.order - b.order);
  let rank = 0;
  for (const n of list) if (n.kept) n.rank = rank++;

  // leaf coordinates and subtree sizes over kept children (iterative post-order)
  let leaf = 0;
  let maxDepth = 1;
  const keptKids = (n: SearchTreeNode) => n.children.filter((c) => c.kept);
  const stack: { n: SearchTreeNode; kids: SearchTreeNode[]; i: number }[] = [{ n: rootNode, kids: keptKids(rootNode), i: 0 }];
  while (stack.length) {
    const top = stack[stack.length - 1];
    if (top.i < top.kids.length) {
      const k = top.kids[top.i++];
      stack.push({ n: k, kids: keptKids(k), i: 0 });
      continue;
    }
    stack.pop();
    const n = top.n;
    maxDepth = Math.max(maxDepth, n.depth);
    n.subtreeSize = 1;
    if (!top.kids.length) {
      n.leafX = leaf++;
      continue;
    }
    let s = 0;
    for (const k of top.kids) {
      s += k.leafX;
      n.subtreeSize += k.subtreeSize;
    }
    n.leafX = s / top.kids.length;
  }

  // canopy sectors: each kept node owns an angular interval sized by its subtree
  const sectors: [SearchTreeNode, number, number][] = [[rootNode, -CANOPY_HALF_ANGLE, CANOPY_HALF_ANGLE]];
  while (sectors.length) {
    const [n, a0, a1] = sectors.pop()!;
    n.angle = (a0 + a1) / 2;
    const kids = keptKids(n);
    let s = 0;
    for (const k of kids) s += k.subtreeSize;
    let t = a0;
    for (const k of kids) {
      const d = ((a1 - a0) * k.subtreeSize) / (s || 1);
      sectors.push([k, t, t + d]);
      t += d;
    }
  }

  // rejected nodes: a jittered leaf coordinate near the parent, and the parent's angle
  for (const n of list) {
    if (n.kept) continue;
    const p = nodes.get(n.parent);
    n.leafX = (p ? p.leafX : 0) + (n.hash - 0.5) * REJECTED_LEAF_SPREAD;
    n.angle = p ? p.angle : 0;
  }

  const target = result.top.find((t) => nodes.has(t));
  const path: number[] = [];
  let c: number | undefined = target ?? root;
  while (c !== undefined && c !== -1 && nodes.has(c)) {
    path.unshift(c);
    c = nodes.get(c)!.parent;
  }

  return {
    root,
    nodes,
    list,
    path,
    maxDepth,
    leaves: Math.max(leaf, 2),
    keptTotal: Math.max(rank, 1),
  };
}
