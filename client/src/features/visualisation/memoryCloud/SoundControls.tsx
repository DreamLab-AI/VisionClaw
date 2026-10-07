/**
 * Sound controls for the memory explorer: the beat source (Off / My audio
 * file / Tap tempo / Spotify), tap tempo with bpm and phase nudges, key B,
 * and the Spotify chip whose popover holds the official embed player.
 *
 * After the RuVector Explorer's music panel and Spotify chip (MIT,
 * https://github.com/ruvnet/RuVector, docs/explorer: `mountPanel`, `spxChip`,
 * `spxPop`, `spxTap`). The Spotify embed plays cross-origin audio the page
 * cannot analyse, so its beat comes from tap tempo; the UI says so.
 */

import React, { useEffect, useId, useState } from 'react';
import { useMemoryCloudStore } from './memoryCloudInstance';
import { loadAudioFile, clearAudio } from './cinematicSession';
import { spotifyEmbedUrl } from './spotifyLink';
import type { BeatSource } from './beatClock';
import { ROUTE_PALETTE } from './routeMath';
import { css } from './panelStyles';

const SOURCES: Array<{ id: BeatSource; label: string }> = [
  { id: 'off', label: 'Off' },
  { id: 'file', label: 'My audio file' },
  { id: 'tap', label: 'Tap tempo' },
  { id: 'spotify', label: 'Spotify' },
];

const PHASE_STEP_MS = 20;
const BPM_STEP = 0.5;

/** Key B taps the tempo anywhere except while typing or with a modifier held. */
export function useTapKey(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'b' && e.key !== 'B') return;
      if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as Element | null;
      if (t && typeof t.closest === 'function' && t.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
      useMemoryCloudStore.getState().tapBeat();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

export const TapControls: React.FC = () => {
  const beat = useMemoryCloudStore((s) => s.beat);
  const tap = () => useMemoryCloudStore.getState().tapBeat();
  const nudge = (n: { bpm?: number; phaseMs?: number }) => useMemoryCloudStore.getState().nudgeBeat(n);
  const locked = beat.phaseAt > 0;
  return (
    <div style={{ marginTop: 6 }}>
      <div style={{ ...css.row, flexWrap: 'wrap' }}>
        <button type="button" style={css.button(true)} onClick={tap}>Tap (B)</button>
        <button type="button" style={css.button()} aria-label="Slower" onClick={() => nudge({ bpm: -BPM_STEP })}>−</button>
        <button type="button" style={css.button()} aria-label="Faster" onClick={() => nudge({ bpm: BPM_STEP })}>+</button>
        <button type="button" style={css.button()} aria-label="Shift beat earlier" disabled={!locked} onClick={() => nudge({ phaseMs: -PHASE_STEP_MS })}>Earlier</button>
        <button type="button" style={css.button()} aria-label="Shift beat later" disabled={!locked} onClick={() => nudge({ phaseMs: PHASE_STEP_MS })}>Later</button>
      </div>
      <div style={{ ...css.mono, marginTop: 4 }} aria-live="polite">
        {locked ? `${beat.bpm.toFixed(1)} bpm · confidence ${beat.confidence.toFixed(2)}` : 'Tap along with the beat.'}
      </div>
    </div>
  );
};

export const SoundSection: React.FC<{ cinematicOn: boolean }> = ({ cinematicOn }) => {
  const source = useMemoryCloudStore((s) => s.beat.source);
  const audio = useMemoryCloudStore((s) => s.cinematic.audio);
  const audioError = useMemoryCloudStore((s) => s.cinematic.audioError);
  const spotifyLink = useMemoryCloudStore((s) => s.spotify.link);
  const ids = useId();

  return (
    <div style={css.section} aria-label="Sound">
      <div style={{ fontWeight: 600, marginBottom: 6 }}>Sound</div>
      <div style={{ ...css.row, flexWrap: 'wrap' }} role="group" aria-label="Sound source">
        {SOURCES.map((s) => (
          <button
            key={s.id}
            type="button"
            aria-pressed={source === s.id}
            style={css.button(source === s.id)}
            onClick={() => useMemoryCloudStore.getState().setBeatSource(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>

      {source === 'file' && (
        <div style={{ marginTop: 6 }}>
          <div style={css.row}>
            <label htmlFor={`${ids}-audio`} style={css.label}>Audio file</label>
            <input
              id={`${ids}-audio`}
              type="file"
              accept="audio/*"
              style={{ fontSize: 11, flex: 1, minWidth: 0 }}
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void loadAudioFile(f);
                e.target.value = '';
              }}
            />
          </div>
          {audio && (
            <div style={{ ...css.row, marginTop: 4, justifyContent: 'space-between' }}>
              <span style={css.mono}>
                {audio.name} · {audio.bpm.toFixed(1)} bpm · confidence {audio.confidence.toFixed(2)}
              </span>
              <button type="button" style={css.button()} onClick={clearAudio}>Remove</button>
            </div>
          )}
          {audioError && <div role="alert" style={{ color: ROUTE_PALETTE.miss, marginTop: 4 }}>{audioError}</div>}
          <p style={{ ...css.label, marginTop: 6 }}>
            The audio file is decoded on this device and never uploaded. It plays, and drives the beat, while the cinematic
            director runs{cinematicOn ? '.' : '; turn on Cinematic in the settings to use it.'}
          </p>
        </div>
      )}

      {source === 'tap' && (
        <>
          <TapControls />
          <p style={{ ...css.label, marginTop: 6 }}>Sync to music playing anywhere: tap along with the beat, or press B.</p>
        </>
      )}

      {source === 'spotify' && (
        <>
          <TapControls />
          <p style={{ ...css.label, marginTop: 6 }}>
            {spotifyLink ? 'Playing in the Spotify player (♪ Spotify, top of this panel).' : 'Paste a link in the Spotify player (♪ Spotify, top of this panel).'}{' '}
            Tap along to set the beat.
          </p>
        </>
      )}
    </div>
  );
};

/** Header chip that opens the Spotify popover; shows a dot while a link is loaded. */
export const SpotifyChip: React.FC<{ open: boolean; onToggle: () => void; popoverId: string }> = ({ open, onToggle, popoverId }) => {
  const hasLink = useMemoryCloudStore((s) => s.spotify.link !== null);
  return (
    <button
      type="button"
      style={{ ...css.button(open || hasLink), display: 'inline-flex', gap: 5, alignItems: 'center' }}
      aria-label="Spotify player"
      aria-expanded={open}
      aria-controls={popoverId}
      onClick={onToggle}
    >
      <span aria-hidden="true">♪</span> Spotify
      {hasLink && <span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: 3, background: ROUTE_PALETTE.tip }} />}
    </button>
  );
};

/**
 * Popover with the link form, the embed and tap controls. While a link is
 * loaded the popover stays mounted even when closed (visually hidden), and it
 * sits outside the collapsible panel body, so playback survives both.
 */
export const SpotifyPopover: React.FC<{ open: boolean; id: string }> = ({ open, id }) => {
  const link = useMemoryCloudStore((s) => s.spotify.link);
  const error = useMemoryCloudStore((s) => s.spotify.error);
  const [draft, setDraft] = useState('');
  if (!open && !link) return null;

  const hidden: React.CSSProperties = {
    position: 'absolute',
    width: 1,
    height: 1,
    overflow: 'hidden',
    opacity: 0,
    pointerEvents: 'none',
  };
  const tall = link && (link.kind === 'album' || link.kind === 'playlist');

  return (
    <div
      id={id}
      role={open ? 'dialog' : undefined}
      aria-label={open ? 'Spotify' : undefined}
      aria-hidden={open ? undefined : true}
      style={open ? { ...css.section, marginTop: 0, marginBottom: 10 } : hidden}
    >
      {open && (
        <form
          style={css.row}
          onSubmit={(e) => {
            e.preventDefault();
            useMemoryCloudStore.getState().setSpotifyLink(draft);
          }}
        >
          <input
            aria-label="Spotify link"
            style={css.input}
            placeholder="https://open.spotify.com/track/…"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
          <button type="submit" style={css.button(true)}>Load</button>
        </form>
      )}
      {open && error && <div role="alert" style={{ color: ROUTE_PALETTE.miss, marginTop: 4 }}>{error}</div>}
      {link && (
        <iframe
          key={`${link.kind}/${link.id}`}
          title="Spotify player"
          src={spotifyEmbedUrl(link)}
          width="100%"
          height={tall ? 352 : 152}
          style={{ border: 0, borderRadius: 12, marginTop: 8, display: 'block' }}
          allow="autoplay; clipboard-write; encrypted-media; fullscreen; picture-in-picture"
          loading="lazy"
        />
      )}
      {open && (
        <>
          {link && (
            <div style={{ ...css.row, justifyContent: 'space-between', marginTop: 4 }}>
              <span style={css.mono}>{link.kind} · {link.id}</span>
              <button
                type="button"
                style={css.button()}
                onClick={() => {
                  useMemoryCloudStore.getState().clearSpotify();
                  setDraft('');
                }}
              >
                Remove link
              </button>
            </div>
          )}
          <TapControls />
          <p style={{ ...css.label, marginTop: 6 }}>
            Spotify plays in its own embedded player. Its audio is cross-origin and cannot be analysed here, so tap along
            (or press B) to set the beat; nudge the phase until the glow lands on the kick.
          </p>
        </>
      )}
    </div>
  );
};
