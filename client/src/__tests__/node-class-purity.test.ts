/**
 * Phase 6 (ADR-04 D9 / T8) — Node-class purity test.
 *
 * Validates that the three standalone pure-renderer components honour the
 * geometry/material contract:
 *
 *   - CrystalOrb    → SphereGeometry(r=0.5), CrystalOrbMaterial
 *   - AgentCapsule  → CapsuleGeometry(r=0.3, h=0.6), AgentCapsuleMaterial
 *   - Gem geometry  → IcosahedronGeometry(r=0.5), GemNodeMaterial
 *
 * These are factory tests — we exercise the geometry/material factory
 * functions directly. The full rendering integration is covered by visual
 * regression downstream.
 */

import { describe, it, expect } from 'vitest';
import * as THREE from 'three';
import { createCrystalOrbGeometry, createCrystalOrbMaterial } from '../rendering/materials/CrystalOrbMaterial';
import { createAgentCapsuleGeometry, createAgentCapsuleMaterial } from '../rendering/materials/AgentCapsuleMaterial';
import { createGemGeometry, createGemNodeMaterial } from '../rendering/materials/GemNodeMaterial';

describe('Phase 6 (ADR-04 D9/T8) — node class geometry & material contract', () => {
  it('CrystalOrb geometry is SphereGeometry with radius 0.5', () => {
    const geo = createCrystalOrbGeometry();
    expect(geo).toBeInstanceOf(THREE.SphereGeometry);
    // SphereGeometry exposes the constructor params via `parameters`.
    expect((geo as THREE.SphereGeometry).parameters.radius).toBe(0.5);
    geo.dispose();
  });

  it('CrystalOrb material is a Material instance with documented uniforms', () => {
    const result = createCrystalOrbMaterial();
    expect(result.material).toBeInstanceOf(THREE.Material);
    expect(result.uniforms.time.value).toBe(0);
    expect(result.uniforms.glowStrength.value).toBeGreaterThan(0);
    expect(result.uniforms.pulseSpeed.value).toBeGreaterThan(0);
    result.material.dispose();
  });

  it('AgentCapsule geometry is CapsuleGeometry r=0.3 h=0.6', () => {
    const geo = createAgentCapsuleGeometry();
    expect(geo).toBeInstanceOf(THREE.CapsuleGeometry);
    const params = (geo as THREE.CapsuleGeometry).parameters as
      { radius: number; height: number; capSegments: number; radialSegments: number };
    expect(params.radius).toBe(0.3);
    expect(params.height).toBe(0.6);
    geo.dispose();
  });

  it('AgentCapsule material is a Material instance', () => {
    const result = createAgentCapsuleMaterial();
    expect(result.material).toBeInstanceOf(THREE.Material);
    result.material.dispose();
  });

  it('Gem geometry is IcosahedronGeometry with radius 0.5', () => {
    const geo = createGemGeometry();
    expect(geo).toBeInstanceOf(THREE.IcosahedronGeometry);
    expect((geo as THREE.IcosahedronGeometry).parameters.radius).toBe(0.5);
    geo.dispose();
  });

  it('Gem material is a Material instance', () => {
    const result = createGemNodeMaterial();
    expect(result.material).toBeInstanceOf(THREE.Material);
    result.material.dispose();
  });

  it('Geometry classes are distinct — no shared identity', () => {
    const g1 = createCrystalOrbGeometry();
    const g2 = createGemGeometry();
    const g3 = createAgentCapsuleGeometry();
    expect(g1.type).not.toBe(g2.type);
    expect(g2.type).not.toBe(g3.type);
    expect(g1.type).not.toBe(g3.type);
    g1.dispose();
    g2.dispose();
    g3.dispose();
  });
});
