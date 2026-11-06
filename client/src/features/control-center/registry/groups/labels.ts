/**
 * Source group — Labels & Text (id `labels`). registry/placement.ts (ADR-2189) decides which section shows each field.
 * All from the legacy Graph tab. `labelLayoutEvery` keeps its `rendering.*` path
 * but lives here for user semantics (see spec §1.4).
 */
import type { GroupData, RegistryField } from '../types';

const L = 'visualisation.graphs.knowledge.labels.';

const fields: RegistryField[] = [
  { key: 'enableLabels', subgroup: 'Labels', label: 'Show Labels', type: 'toggle', path: `${L}enableLabels`, description: 'Display node labels' },
  { key: 'labelColor', subgroup: 'Labels', label: 'Label Color', type: 'color', path: `${L}textColor`, description: 'Color of label text' },
  { key: 'showMetadata', subgroup: 'Labels', label: 'Show Metadata', type: 'toggle', path: `${L}showMetadata`, description: 'Show domain, links, and quality info under labels' },
  { key: 'labelStandoff', subgroup: 'Labels', label: 'Label Standoff', type: 'slider', min: -1.0, max: 3.0, step: 0.05, path: `${L}textPadding`, description: 'Gap between node surface and label' },
  // step 0.0001 (not 0.001): the stored default 0.0074725277 snaps to 0.0075 (loss ~2.7e-5) instead of 0.007 (loss ~4.7e-4) on first touch — least-lossy without altering the frozen path or default — see defect-3.
  { key: 'labelDistanceThreshold', subgroup: 'Labels', label: 'Label Draw Distance', type: 'slider', min: 0, max: 2000, step: 25, path: `${L}labelDistanceThreshold`, description: 'Max camera distance for label visibility' },
  { key: 'maxLabelWidth', subgroup: 'Labels', label: 'Max Label Width', type: 'slider', min: 2, max: 20, step: 0.5, path: `${L}maxLabelWidth`, description: 'Maximum text wrapping width' },
  { key: 'labelLayoutEvery', subgroup: 'Labels', label: 'Label Layout Cadence (frames)', type: 'slider', min: 1, max: 10, step: 1, path: 'visualisation.rendering.labelLayoutEvery', description: 'Frames between full label re-layout passes' },
];

export const labels: GroupData = {
  id: 'labels',
  label: 'Labels & Text',
  description: 'Node label visibility, sizing, colour, outline, and layout cadence.',
  loadPaths: ['visualisation.graphs.knowledge.labels', 'visualisation.rendering'],
  fields,
};
