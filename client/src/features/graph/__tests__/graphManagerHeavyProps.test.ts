/**
 * React 19 dev builds diff a component's previous and next props whenever it
 * re-renders with changed props, writing one entry per changed array element,
 * then structured-clone the result into performance.measure(). A 147k-edge
 * graph hands that diff ~880k numbers and 147k edge objects: DataCloneError,
 * "out of memory". These are the props that must never carry bulk data.
 */
import { describe, it, expect } from 'vitest';
import GraphManagerSrc from '../components/GraphManager.tsx?raw';

/** JSX opening tag (props included) of the first `<Name ...>` in the source. */
function openingTag(src: string, name: string): string {
  const start = src.indexOf(`<${name}\n`) >= 0 ? src.indexOf(`<${name}\n`) : src.indexOf(`<${name} `);
  expect(start, `<${name}> not found`).toBeGreaterThan(-1);
  let depth = 0;
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (c === '{') depth++;
    else if (c === '}') depth--;
    else if (c === '>' && depth === 0 && src[i - 1] !== '=') return src.slice(start, i + 1);
  }
  throw new Error(`unterminated <${name}>`);
}

describe('GraphManager: no bulk arrays as component props', () => {
  it('GlassEdges receives a ref + version, not the points array', () => {
    const tag = openingTag(GraphManagerSrc, 'GlassEdges');
    expect(tag).not.toMatch(/\bpoints=/);
    expect(tag).toMatch(/\bpointsRef=/);
    expect(tag).toMatch(/\bpointsVersion=/);
  });

  it('GemNodes receives an edge count, not the edges array', () => {
    const tag = openingTag(GraphManagerSrc, 'GemNodes');
    expect(tag).not.toMatch(/\bedges=/);
    expect(tag).toMatch(/\bedgeCount=/);
  });

  it('KnowledgeRings does not receive the edges array', () => {
    expect(openingTag(GraphManagerSrc, 'KnowledgeRings')).not.toMatch(/\bedges=/);
  });
});
