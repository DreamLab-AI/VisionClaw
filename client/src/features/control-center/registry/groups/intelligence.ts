/**
 * Source group — Intelligence (id `ai`): the voice settings the client reads. registry/placement.ts (ADR-2189) decides which section shows each field.
 * Perplexity settings were deleted by ADR-2189: the server reads its own Perplexity config.
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  { key: 'pocketTtsVoice', subgroup: 'PocketTts TTS', label: 'Voice', type: 'text', path: 'pocketTts.defaultVoice', description: 'Voice selection' },
  { key: 'pocketTtsSpeed', subgroup: 'PocketTts TTS', label: 'Speech Speed', type: 'slider', min: 0.5, max: 2, step: 0.1, path: 'pocketTts.defaultSpeed', description: 'Playback speed' },
];

export const intelligence: GroupData = {
  id: 'ai',
  label: 'Intelligence (AI)',
  description: 'PocketTts voice and speed for spoken responses.',
  loadPaths: ['pocketTts'],
  fields,
};
