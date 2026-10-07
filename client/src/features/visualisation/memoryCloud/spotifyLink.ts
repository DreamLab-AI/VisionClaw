/**
 * Strict validation of Spotify share links for the explorer's beat-connect
 * embed (after the RuVector Explorer's Spotify chip, MIT,
 * `docs/explorer/index.html`).
 *
 * Only `https://open.spotify.com/(track|album|playlist|episode)/<22 base62>`
 * is accepted, with an optional `/intl-xx/` locale segment, trailing slash,
 * and a query (`?si=` share tracking) or hash of URL-safe characters only. Everything else — other schemes,
 * other hosts, lookalike hosts, userinfo, ports, extra path segments, URIs —
 * is rejected. The embed URL is rebuilt from the validated kind and id, never
 * from the user's string, so nothing they paste reaches the iframe `src`.
 *
 * The embed plays audio cross-origin, which the page cannot analyse; the beat
 * for Spotify comes from tap tempo (see beatClock.ts).
 */

export type SpotifyKind = 'track' | 'album' | 'playlist' | 'episode';

export interface SpotifyLink {
  kind: SpotifyKind;
  /** 22-character base62 Spotify id */
  id: string;
  /** canonical share URL rebuilt from kind and id */
  url: string;
}

const MAX_INPUT = 2048;
const HOST = 'open.spotify.com';
const LINK_RE =
  /^https:\/\/open\.spotify\.com\/(?:intl-[a-z]{2}(?:-[A-Za-z]{2})?\/)?(track|album|playlist|episode)\/([A-Za-z0-9]{22})\/?(?:[?#][A-Za-z0-9\-._~%&=+:@/?#]*)?$/;

/** Parse a pasted Spotify link, or null when it is not exactly an accepted form. */
export function parseSpotifyLink(input: string): SpotifyLink | null {
  if (typeof input !== 'string') return null;
  const s = input.trim();
  if (!s || s.length > MAX_INPUT) return null;
  const m = LINK_RE.exec(s);
  if (!m) return null;
  // Belt and braces: the WHATWG parser must agree on scheme, host and port.
  let parsed: URL;
  try {
    parsed = new URL(s);
  } catch {
    return null;
  }
  if (parsed.protocol !== 'https:' || parsed.hostname !== HOST || parsed.port !== '' || parsed.username || parsed.password) {
    return null;
  }
  const kind = m[1] as SpotifyKind;
  const id = m[2];
  return { kind, id, url: `https://${HOST}/${kind}/${id}` };
}

/** The official embed player URL for a validated link. */
export function spotifyEmbedUrl(link: SpotifyLink): string {
  return `https://${HOST}/embed/${link.kind}/${link.id}?utm_source=generator&theme=0`;
}
