import { describe, expect, it } from 'vitest';
import { creditsLine } from '../credits';
import { assetWith } from './fixtures';

describe('creditsLine', () => {
  it('matches the server credits line: attribution, licence id and url, modified (decimated)', () => {
    expect(creditsLine(assetWith(['desktop']))).toBe(
      'International Space Station by Tamás Máté (CC-BY-4.0 https://creativecommons.org/licenses/by/4.0/), modified (decimated)',
    );
  });

  it('omits the modification note only when the asset is unmodified', () => {
    const a = { ...assetWith(['desktop']), modified: false };
    expect(creditsLine(a)).not.toContain('modified');
  });
});
