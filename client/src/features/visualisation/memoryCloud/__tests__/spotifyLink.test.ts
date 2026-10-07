import { describe, it, expect } from 'vitest';
import { parseSpotifyLink, spotifyEmbedUrl } from '../spotifyLink';

const ID = '4uLU6hMCjMI75M1A2tKUQC'; // 22 base62 characters

describe('parseSpotifyLink — accepts', () => {
  it.each([
    ['track', `https://open.spotify.com/track/${ID}`],
    ['album', `https://open.spotify.com/album/${ID}`],
    ['playlist', `https://open.spotify.com/playlist/${ID}`],
    ['episode', `https://open.spotify.com/episode/${ID}`],
  ])('a %s link', (kind, url) => {
    expect(parseSpotifyLink(url)).toEqual({ kind, id: ID, url: `https://open.spotify.com/${kind}/${ID}` });
  });

  it('share links with ?si= tracking, a trailing slash, a hash or surrounding space', () => {
    for (const u of [
      `https://open.spotify.com/track/${ID}?si=abc123`,
      `https://open.spotify.com/track/${ID}/`,
      `https://open.spotify.com/track/${ID}#x`,
      `  https://open.spotify.com/track/${ID}\n`,
    ]) {
      expect(parseSpotifyLink(u)?.url).toBe(`https://open.spotify.com/track/${ID}`);
    }
  });

  it('a localised /intl-xx/ share link', () => {
    expect(parseSpotifyLink(`https://open.spotify.com/intl-de/track/${ID}`)?.id).toBe(ID);
    expect(parseSpotifyLink(`https://open.spotify.com/intl-pt-BR/album/${ID}`)?.kind).toBe('album');
  });
});

describe('parseSpotifyLink — rejects', () => {
  it.each([
    ['javascript: URL', `javascript:alert(1)//https://open.spotify.com/track/${ID}`],
    ['data: URL', `data:text/html,https://open.spotify.com/track/${ID}`],
    ['plain http', `http://open.spotify.com/track/${ID}`],
    ['lookalike host suffix', `https://open.spotify.com.evil.com/track/${ID}`],
    ['lookalike host prefix', `https://evilopen.spotify.com/track/${ID}`],
    ['other subdomain', `https://play.spotify.com/track/${ID}`],
    ['bare spotify.com', `https://spotify.com/track/${ID}`],
    ['other host', `https://example.com/track/${ID}`],
    ['credentials in the authority', `https://open.spotify.com@evil.com/track/${ID}`],
    ['userinfo before the host', `https://user:pw@open.spotify.com/track/${ID}`],
    ['explicit port', `https://open.spotify.com:8443/track/${ID}`],
    ['unknown kind', `https://open.spotify.com/artist/${ID}`],
    ['already an embed URL', `https://open.spotify.com/embed/track/${ID}`],
    ['short id', 'https://open.spotify.com/track/abc'],
    ['long id', `https://open.spotify.com/track/${ID}x`],
    ['non-base62 id', 'https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQ-'],
    ['extra path segments', `https://open.spotify.com/track/${ID}/extra`],
    ['encoded traversal', `https://open.spotify.com/track/..%2F..%2Fembed/${ID}`],
    ['spotify: URI', `spotify:track:${ID}`],
    ['scheme-less', `open.spotify.com/track/${ID}`],
    ['protocol-relative', `//open.spotify.com/track/${ID}`],
    ['empty', ''],
    ['quote in the query', `https://open.spotify.com/track/${ID}?si=abc"onload=x`],
    ['angle bracket in the hash', `https://open.spotify.com/track/${ID}#<script>`],
    ['whitespace inside', `https://open.spotify.com/track/${ID}?si=a b`],
    ['very long input', `https://open.spotify.com/track/${ID}?${'a'.repeat(5000)}`],
  ])('%s', (_label, input) => {
    expect(parseSpotifyLink(input)).toBeNull();
  });
});

describe('spotifyEmbedUrl', () => {
  it('builds the official embed URL from validated parts only', () => {
    const link = parseSpotifyLink(`https://open.spotify.com/playlist/${ID}?si=zzz`)!;
    expect(spotifyEmbedUrl(link)).toBe(`https://open.spotify.com/embed/playlist/${ID}?utm_source=generator&theme=0`);
  });
});
