import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { setLineDrawCount } from '../lineDrawCount';

function line(kind: 'strip' | 'segments', capacity: number) {
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(capacity * 3), 3));
  return kind === 'strip' ? new THREE.Line(g) : new THREE.LineSegments(g);
}

describe('setLineDrawCount (no zero-vertex draws: WebGPU warns on Draw(0))', () => {
  it('hides a LineSegments with no segments and shows it with one', () => {
    const l = line('segments', 4);
    setLineDrawCount(l, 0);
    expect(l.visible).toBe(false);
    setLineDrawCount(l, 2);
    expect(l.visible).toBe(true);
    expect(l.geometry.drawRange.count).toBe(2);
  });

  it('hides a Line strip below two vertices (one vertex is no segment)', () => {
    const l = line('strip', 24);
    setLineDrawCount(l, 1);
    expect(l.visible).toBe(false);
    expect(l.geometry.drawRange.count).toBe(1);
    setLineDrawCount(l, 2);
    expect(l.visible).toBe(true);
  });
});
