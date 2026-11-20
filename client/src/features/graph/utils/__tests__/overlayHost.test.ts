// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { overlayHost } from '../overlayHost';

/**
 * drei's <Html> mounts into `events.connected` when no portal is given. The
 * GraphCanvas event guard can connect events to the <canvas> itself (a start-up
 * race), and children of a <canvas> are fallback content the browser never
 * draws: every HTML overlay in the scene then silently disappears.
 */
describe('overlayHost', () => {
  it('is the element that contains the canvas, never the canvas', () => {
    const host = document.createElement('div');
    const canvas = document.createElement('canvas');
    host.appendChild(canvas);
    expect(overlayHost(canvas)).toBe(host);
  });

  it('is null for a detached or missing canvas', () => {
    expect(overlayHost(document.createElement('canvas'))).toBeNull();
    expect(overlayHost(null)).toBeNull();
    expect(overlayHost(undefined)).toBeNull();
  });
});

describe('every <Html> overlay names its host', () => {
  const sources = import.meta.glob(
    ['/src/**/*.tsx', '!/src/**/__tests__/**'],
    { query: '?raw', import: 'default', eager: true },
  ) as Record<string, string>;

  it('passes portal to each drei <Html>', () => {
    const offenders: string[] = [];
    for (const [file, src] of Object.entries(sources)) {
      const re = /<Html\b([^>]*)>/g;
      let m: RegExpExecArray | null;
      while ((m = re.exec(src))) {
        if (!/\bportal=/.test(m[1])) offenders.push(`${file}: <Html${m[1].slice(0, 40)}`);
      }
    }
    expect(Object.keys(sources).length).toBeGreaterThan(50);
    expect(offenders).toEqual([]);
  });
});
