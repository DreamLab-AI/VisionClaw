/**
 * MemoryCameraRig — camera work for the memory explorer, inside the Canvas.
 *
 *  - Click-to-focus: listens for MEMORY_FOCUS_EVENT (dispatched by the
 *    explorer panel through cameraFocus.focusMemoryPoint) and eases the
 *    camera and orbit target to that cloud point.
 *  - Cinematic director: when the store's cinematic run starts, it compiles a
 *    shot timeline over the current route (director.ts), drives the camera and
 *    the route's playback clock from it, plays the loaded audio file with
 *    beat sync (store.beat via beatClock.ts → beatState → glow and comet pulse), and optionally
 *    records the canvas (recorder.ts) for download.
 *
 * Renders nothing; lives inside the cloud group only to share its transform.
 */

import React, { useEffect, useRef } from 'react';
import * as THREE from 'three';
import { useFrame, useThree } from '@react-three/fiber';
import { MEMORY_FOCUS_EVENT, flyPose, type CameraPoseLike, type MemoryFocusDetail } from '../cameraFocus';
import { useMemoryCloudStore } from './memoryCloudInstance';
import { beatState, directorClock, routeChannel } from './memoryCloudStore';
import { compileDirector, type Director } from './director';
import { cinematicSession, ensureAudioContext } from './cinematicSession';
import { beatAt, fromTempoEstimate } from './beatClock';
import { recordCanvas, exportFilename, downloadBlob, RecorderError } from './recorder';
import { useReducedMotion } from './useReducedMotion';
import type { Vec3 } from '../memoryTrajectory/types';

interface ControlsLike {
  target: THREE.Vector3;
  enabled: boolean;
  update: () => void;
}

const FLY_SECONDS = 0.9;
/** distance from a focused point, in cloud-local units */
const FOCUS_DISTANCE = 18;

interface Flight {
  from: CameraPoseLike;
  to: CameraPoseLike;
  t0: number;
}

interface Run {
  director: Director;
  t0: number;
  source: AudioBufferSourceNode | null;
  gain: GainNode | null;
  controlsWereEnabled: boolean;
  /** reached the end of the timeline (a recording then finishes on its own) */
  completed: boolean;
}

export interface MemoryCameraRigProps {
  cloudGroup: React.RefObject<THREE.Group | null>;
  positions: ArrayLike<number>;
}

const MemoryCameraRig: React.FC<MemoryCameraRigProps> = ({ cloudGroup, positions }) => {
  const camera = useThree((s) => s.camera);
  const controls = useThree((s) => s.controls) as unknown as ControlsLike | null;
  const gl = useThree((s) => s.gl);
  const reducedMotion = useReducedMotion();
  const cinematicActive = useMemoryCloudStore((s) => s.cinematic.active);
  const runSeq = useMemoryCloudStore((s) => s.cinematic.runSeq);

  const flight = useRef<Flight | null>(null);
  const run = useRef<Run | null>(null);

  const currentPose = (): CameraPoseLike => {
    const tgt = controls?.target ?? new THREE.Vector3(0, 0, 0);
    return {
      position: [camera.position.x, camera.position.y, camera.position.z],
      target: [tgt.x, tgt.y, tgt.z],
    };
  };

  const applyPose = (p: CameraPoseLike) => {
    camera.position.set(...p.position);
    if (controls) {
      controls.target.set(...p.target);
      controls.update();
    } else {
      camera.lookAt(...p.target);
    }
  };

  // ── click-to-focus ──
  useEffect(() => {
    const onFocus = (e: Event) => {
      const { sampleIndex } = (e as CustomEvent<MemoryFocusDetail>).detail ?? { sampleIndex: -1 };
      const g = cloudGroup.current;
      if (!g || sampleIndex < 0 || sampleIndex * 3 + 2 >= positions.length || run.current) return;
      g.updateMatrixWorld();
      const world = new THREE.Vector3(
        positions[sampleIndex * 3],
        positions[sampleIndex * 3 + 1],
        positions[sampleIndex * 3 + 2],
      ).applyMatrix4(g.matrixWorld);
      const scale = new THREE.Vector3();
      g.getWorldScale(scale);
      const from = currentPose();
      const dir = new THREE.Vector3(...from.position).sub(world);
      if (dir.lengthSq() < 1e-6) dir.set(0, 0, 1);
      dir.normalize().multiplyScalar(FOCUS_DISTANCE * scale.x);
      const to: CameraPoseLike = {
        position: [world.x + dir.x, world.y + dir.y, world.z + dir.z],
        target: [world.x, world.y, world.z],
      };
      if (reducedMotion) {
        applyPose(to);
        flight.current = null;
      } else {
        flight.current = { from, to, t0: performance.now() };
      }
    };
    window.addEventListener(MEMORY_FOCUS_EVENT, onFocus);
    return () => window.removeEventListener(MEMORY_FOCUS_EVENT, onFocus);
    // applyPose/currentPose close over camera + controls, both listed
  }, [cloudGroup, positions, camera, controls, reducedMotion]);

  // ── cinematic director ──
  useEffect(() => {
    if (!cinematicActive) return;
    const store = useMemoryCloudStore.getState();
    const g = cloudGroup.current;
    if (!g) {
      store.stopCinematic();
      return;
    }
    g.updateMatrixWorld();
    const v = new THREE.Vector3();
    const route: Vec3[] = routeChannel.pts.map((p) => {
      v.set(p[0], p[1], p[2]).applyMatrix4(g.matrixWorld);
      return [v.x, v.y, v.z];
    });
    const scale = new THREE.Vector3();
    g.getWorldScale(scale);
    const centre = new THREE.Vector3();
    g.getWorldPosition(centre);
    let radius = 60 * scale.x;
    if (route.length >= 2) {
      const c = route.reduce<Vec3>((a, p) => [a[0] + p[0] / route.length, a[1] + p[1] / route.length, a[2] + p[2] / route.length], [0, 0, 0]);
      radius = Math.max(20 * scale.x, 1.3 * Math.max(...route.map((p) => Math.hypot(p[0] - c[0], p[1] - c[1], p[2] - c[2]))));
    }
    const director = compileDirector({
      route,
      radius,
      speed: store.playback.speed,
      focus: [centre.x, centre.y, centre.z],
      reducedMotion,
    });

    // audio (optional): play the decoded file; with recording, also into the capture stream
    let source: AudioBufferSourceNode | null = null;
    let gain: GainNode | null = null;
    let streamDest: MediaStreamAudioDestinationNode | null = null;
    // the file plays only when it is the chosen beat source
    const playFile = store.beat.source === 'file' && cinematicSession.audioBuffer !== null;
    const ctx = playFile ? ensureAudioContext() : null;
    if (ctx && cinematicSession.audioBuffer) {
      void ctx.resume();
      source = ctx.createBufferSource();
      source.buffer = cinematicSession.audioBuffer;
      source.loop = true;
      gain = ctx.createGain();
      gain.gain.value = 0.9;
      source.connect(gain);
      gain.connect(ctx.destination);
      if (cinematicSession.recordRequested) {
        streamDest = ctx.createMediaStreamDestination();
        gain.connect(streamDest);
      }
      source.start();
      if (cinematicSession.tempo) {
        // lock the shared beat clock to when the audio reaches the speakers
        const latencyMs = ((ctx.outputLatency || 0) + (ctx.baseLatency || 0)) * 1000;
        store.setBeat(fromTempoEstimate(cinematicSession.tempo, Date.now() + latencyMs));
      }
    }

    run.current = {
      director,
      t0: performance.now(),
      source,
      gain,
      controlsWereEnabled: controls?.enabled ?? true,
      completed: false,
    };
    flight.current = null;
    directorClock.active = true;
    directorClock.el = 0;
    if (controls) controls.enabled = false;

    // recording
    if (cinematicSession.recordRequested) {
      cinematicSession.recordRequested = false;
      const abort = new AbortController();
      cinematicSession.exportAbort = abort;
      store.setCinematic({ recording: true, exportProgress: 0, exportError: null });
      recordCanvas(gl.domElement, {
        durationSec: director.duration,
        fps: 30,
        audio: streamDest?.stream ?? null,
        signal: abort.signal,
        onProgress: (f) => useMemoryCloudStore.getState().setCinematic({ exportProgress: f }),
      })
        .then((r) => {
          const name = exportFilename(r.ext);
          const url = URL.createObjectURL(r.blob);
          useMemoryCloudStore.getState().setCinematic({ recording: false, exportProgress: 1, lastExport: { name, url, mime: r.mime } });
          downloadBlob(r.blob, name);
        })
        .catch((e: unknown) => {
          const cancelled = e instanceof RecorderError && e.kind === 'aborted';
          useMemoryCloudStore.getState().setCinematic({
            recording: false,
            exportError: cancelled ? 'Export cancelled' : e instanceof Error ? e.message : String(e),
          });
        })
        .finally(() => {
          if (cinematicSession.exportAbort === abort) cinematicSession.exportAbort = null;
        });
    }

    return () => {
      const r = run.current;
      run.current = null;
      if (r?.source) {
        try {
          r.source.stop();
        } catch {
          /* already stopped */
        }
        r.source.disconnect();
        r.gain?.disconnect();
      }
      // an early stop cancels a recording still running
      if (!r?.completed) cinematicSession.exportAbort?.abort();
      directorClock.active = false;
      const beatNow = useMemoryCloudStore.getState().beat;
      // the file is silent again: unlock its clock
      if (beatNow.source === 'file' && beatNow.phaseAt > 0) useMemoryCloudStore.getState().setBeat({ ...beatNow, phaseAt: 0 });
      if (controls) {
        controls.enabled = r?.controlsWereEnabled ?? true;
        controls.update();
      }
      // leave the route converged where the director left it
      const st = useMemoryCloudStore.getState();
      st.seek(directorClock.el);
      st.setPlaying(false);
    };
    // runSeq restarts a run; the rest is read once per run
  }, [cinematicActive, runSeq]);

  useFrame(() => {
    const now = performance.now();
    // one beat driver for every source (file during the director, tap, Spotify + tap)
    const sample = reducedMotion ? null : beatAt(useMemoryCloudStore.getState().beat, Date.now());
    beatState.on = sample?.on ?? false;
    beatState.pulse = sample?.pulse ?? 0;
    beatState.bar = sample?.bar ?? 0;
    beatState.phase = sample?.phase ?? 0;
    const r = run.current;
    if (r) {
      const t = (now - r.t0) / 1000;
      if (t >= r.director.duration) {
        directorClock.el = r.director.playbackAt(r.director.duration);
        r.completed = true;
        useMemoryCloudStore.getState().stopCinematic();
        return;
      }
      directorClock.el = r.director.playbackAt(t);
      applyPose(r.director.poseAt(t));
      return;
    }
    const f = flight.current;
    if (f) {
      const u = Math.min(1, (now - f.t0) / 1000 / FLY_SECONDS);
      applyPose(flyPose(f.from, f.to, u));
      if (u >= 1) flight.current = null;
    }
  });

  return null;
};

export default MemoryCameraRig;
