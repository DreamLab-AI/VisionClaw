/**
 * Group 7 — Intelligence (AI) (id `ai`, hotkey 7, 6 fields).
 * All from the legacy AI tab: Perplexity search + PocketTts TTS. Local-only.
 */
import type { GroupData, RegistryField } from '../types';

const fields: RegistryField[] = [
  { key: 'perplexityModel', subgroup: 'Perplexity', label: 'Perplexity Model', type: 'text', path: 'perplexity.model', description: 'Model selection' },
  { key: 'perplexityMaxTokens', subgroup: 'Perplexity', label: 'Max Tokens', type: 'slider', min: 100, max: 4096, step: 100, path: 'perplexity.maxTokens', description: 'Maximum response tokens' },
  { key: 'perplexityTemperature', subgroup: 'Perplexity', label: 'Temperature', type: 'slider', min: 0, max: 2, step: 0.1, path: 'perplexity.temperature', description: 'Response randomness' },
  { key: 'pocketTtsApiUrl', subgroup: 'PocketTts TTS', label: 'PocketTts API URL', type: 'text', path: 'pocketTts.apiUrl', description: 'TTS endpoint' },
  { key: 'pocketTtsVoice', subgroup: 'PocketTts TTS', label: 'Default Voice', type: 'text', path: 'pocketTts.defaultVoice', description: 'Voice selection' },
  { key: 'pocketTtsSpeed', subgroup: 'PocketTts TTS', label: 'Speech Speed', type: 'slider', min: 0.5, max: 2, step: 0.1, path: 'pocketTts.defaultSpeed', description: 'Playback speed' },
];

export const intelligence: GroupData = {
  id: 'ai',
  label: 'Intelligence (AI)',
  description: 'Perplexity search and PocketTts text-to-speech integrations.',
  hotkey: '7',
  loadPaths: ['perplexity', 'pocketTts'],
  fields,
};
