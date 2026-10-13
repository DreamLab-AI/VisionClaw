/**
 * Semantic-force controls, built from `semanticFields.json`: the single description of the
 * `physics.semantic` group, emitted by the Rust definition (`semantic_config::FIELDS`) and
 * pinned to it by a Rust test. Labels, ranges and steps are therefore not maintained here; the
 * XR HUD loads the same document. Every value is stored by the ordinary physics PUT and applied
 * by the GPU exactly as stored: there is no scale or multiplier in between.
 */
import type { RegistryField } from '../types';
import fieldsDoc from '../semanticFields.json';

interface SemanticFieldRow {
  path: string;
  group: string;
  label: string;
  description: string;
  kind: 'slider' | 'toggle';
  default: number | boolean;
  min?: number;
  max?: number;
  step?: number;
}

const P = 'visualisation.graphs.knowledge.physics.semantic.';

export const SEMANTIC_FIELD_ROWS = (fieldsDoc as { fields: SemanticFieldRow[] }).fields;

export const semanticFields: RegistryField[] = SEMANTIC_FIELD_ROWS.map((row) => {
  const base = {
    key: `semantic.${row.path}`,
    subgroup: `Semantic: ${row.group}`,
    label: row.label,
    path: `${P}${row.path}`,
    description: row.description,
  };
  return row.kind === 'toggle'
    ? { ...base, type: 'toggle' as const }
    : { ...base, type: 'slider' as const, min: row.min, max: row.max, step: row.step };
});
