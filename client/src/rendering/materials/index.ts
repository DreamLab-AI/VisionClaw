
// Gem material factories (TSL-augmented PBR, WebGPURenderer on either backend)
export { createGemNodeMaterial, createTslGemMaterial, disposeGemMaterial, createGemGeometry } from './GemNodeMaterial';
export type { GemMaterialResult } from './GemNodeMaterial';
export { createCrystalOrbMaterial, createCrystalOrbGeometry } from './CrystalOrbMaterial';
export type { CrystalOrbMaterialResult } from './CrystalOrbMaterial';
export { createAgentCapsuleMaterial, createAgentCapsuleGeometry } from './AgentCapsuleMaterial';
export type { AgentCapsuleMaterialResult } from './AgentCapsuleMaterial';
export { createGlassEdgeMaterial, createGlassEdgeGeometry } from './GlassEdgeMaterial';
export type { GlassEdgeMaterialResult } from './GlassEdgeMaterial';