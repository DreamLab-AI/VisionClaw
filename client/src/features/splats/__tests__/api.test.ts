import { describe, expect, it } from 'vitest';
import { parseSplatListing } from '../api';
import { wireListing } from './fixtures';

describe('parseSplatListing', () => {
  it('parses the camelCase wire listing', () => {
    const listing = parseSplatListing(wireListing());
    expect(listing.assets).toHaveLength(1);
    const a = listing.assets[0];
    expect(a.id).toBe('iss');
    expect(a.licence.spdx).toBe('CC-BY-4.0');
    expect(a.modified).toBe(true);
    expect(a.bounds).toEqual({ min: [-64, -36, -26], max: [64, 35, 43] });
    expect(a.variants.map((v) => v.name)).toEqual(['desktop-hi', 'desktop', 'xr-3']);
    expect(a.variants[0].address).toMatch(/^sha256-12-[0-9a-f]{12}$/);
  });

  it('skips a variant name it does not know instead of failing', () => {
    const raw = wireListing() as { assets: { variants: { name: string }[] }[] };
    raw.assets[0].variants[0].name = 'desktop-ultra';
    expect(parseSplatListing(raw).assets[0].variants.map((v) => v.name)).toEqual(['desktop', 'xr-3']);
  });

  it('rejects a malformed body', () => {
    expect(() => parseSplatListing(null)).toThrow(/body/);
    expect(() => parseSplatListing({})).toThrow(/assets/);
    const noTitle = wireListing() as { assets: Record<string, unknown>[] };
    delete noTitle.assets[0].title;
    expect(() => parseSplatListing(noTitle)).toThrow(/title/);
  });

  it('rejects an address that does not follow the sha256', () => {
    const raw = wireListing() as { assets: { variants: { address: string }[] }[] };
    raw.assets[0].variants[0].address = 'sha256-12-000000000000';
    expect(() => parseSplatListing(raw)).toThrow(/address/);
  });

  it('rejects a blob url that leaves the origin', () => {
    for (const url of ['https://evil.example/blob', '//evil.example/blob', 'blob/relative']) {
      const raw = wireListing() as { assets: { variants: { url: string }[] }[] };
      raw.assets[0].variants[0].url = url;
      expect(() => parseSplatListing(raw)).toThrow(/url/);
    }
  });

  it('rejects inverted bounds', () => {
    const raw = wireListing() as { assets: { bounds: { min: number[] } }[] };
    raw.assets[0].bounds.min = [100, 0, 0];
    expect(() => parseSplatListing(raw)).toThrow(/bounds/);
  });
});
