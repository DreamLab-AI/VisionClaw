/**
 * Voice (microphone + TTS input) availability, as plain issues for the Status
 * surface (ADR-2189 §4). This used to be a fixed warning over the dock dials
 * (audit finding 5); it now lives in the Status flyout, with a dot on the chip.
 */
import { AudioInputService } from '../../../services/AudioInputService';

export function voiceSupportIssues(): string[] {
  let s: ReturnType<typeof AudioInputService.getBrowserSupport>;
  try {
    s = AudioInputService.getBrowserSupport();
  } catch {
    return ['Voice support could not be checked'];
  }
  const issues: string[] = [];
  if (!s.isHttps) issues.push('Secure connection (HTTPS) required');
  if (!s.getUserMedia) issues.push('Microphone access not supported by this browser');
  if (!s.audioContext) issues.push('Web Audio API not supported');
  if (!s.mediaRecorder) issues.push('MediaRecorder API not supported');
  return issues;
}
