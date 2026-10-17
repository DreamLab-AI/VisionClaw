/**
 * Presence: each actor is a coloured halo that glides to the node it last
 * touched, followed by a fading trail through its recent nodes; the speaker's
 * caption floats as a speech bubble at their focus.
 */
import { Html } from '@react-three/drei';
import { useFrame } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import * as THREE from 'three';
import { hslToRgb } from '../graph';
import { useCs, type Actor } from '../store';
import { actorFocusAt, trailOf } from '../timeline';
import { useScene } from './context';
import { springFactor } from './lod';

export function actorHue(id: number): number {
  return (id * 137) % 360;
}
export function actorCss(id: number): string {
  return `hsl(${actorHue(id)} 85% 62%)`;
}

const TRAIL_LEN = 12;

function haloTexture(): THREE.CanvasTexture {
  const c = document.createElement('canvas');
  c.width = c.height = 128;
  const g = c.getContext('2d');
  if (g) {
    const grd = g.createRadialGradient(64, 64, 20, 64, 64, 62);
    grd.addColorStop(0, 'rgba(255,255,255,0)');
    grd.addColorStop(0.55, 'rgba(255,255,255,0.15)');
    grd.addColorStop(0.78, 'rgba(255,255,255,0.95)');
    grd.addColorStop(0.86, 'rgba(255,255,255,0.35)');
    grd.addColorStop(1, 'rgba(255,255,255,0)');
    g.fillStyle = grd;
    g.fillRect(0, 0, 128, 128);
  }
  return new THREE.CanvasTexture(c);
}

function ActorHalo({ actor, texture, focusId }: { actor: Actor; texture: THREE.Texture; focusId: number | null }) {
  const { frame } = useScene();
  const caption = useCs((s) => s.captions[actor.id]);
  const sprite = useRef<THREE.Sprite>(null);
  const pos = useRef<THREE.Vector3 | null>(null);
  const bubble = useRef<THREE.Group>(null);
  const [r, g, b] = hslToRgb(actorHue(actor.id), 0.85, 0.62);
  const material = useMemo(() => new THREE.SpriteMaterial({ map: texture, color: new THREE.Color(r, g, b), transparent: true, depthWrite: false, depthTest: false }), [texture, r, g, b]);
  useEffect(() => () => material.dispose(), [material]);

  useFrame((state, dt) => {
    const m = frame.model;
    if (!m || !sprite.current) return;
    const i = focusId !== null ? m.index.get(focusId) : undefined;
    if (i === undefined) {
      sprite.current.visible = false;
      return;
    }
    const target = new THREE.Vector3(frame.display[i * 3], frame.display[i * 3 + 1], frame.display[i * 3 + 2]);
    if (!pos.current) pos.current = target.clone();
    pos.current.lerp(target, springFactor(dt, 0.25));
    sprite.current.visible = true;
    sprite.current.position.copy(pos.current);
    const s = (frame.sceneRadius / 400) * (14 + 2 * Math.sin(state.clock.elapsedTime * 3 + actor.id));
    sprite.current.scale.set(s, s, 1);
    bubble.current?.position.copy(pos.current);
  });

  const recentCaption = caption && Date.now() - caption.ts_ms < 8000 ? caption : null;
  return (
    <>
      <sprite ref={sprite} material={material} renderOrder={5} />
      <group ref={bubble}>
        <Html center zIndexRange={[20, 0]} style={{ pointerEvents: 'none', transform: 'translateY(-34px)' }}>
          <div className="cs-actor-tag" style={{ borderColor: actorCss(actor.id) }}>
            <span className="cs-actor-dot" style={{ background: actorCss(actor.id) }} />
            {actor.label}
            {recentCaption && <div className={`cs-bubble${recentCaption.partial ? ' partial' : ''}`}>{recentCaption.text}</div>}
          </div>
        </Html>
      </group>
    </>
  );
}

function Trails({ actors, t }: { actors: Actor[]; t: number | null }) {
  const { frame } = useScene();
  const presence = useCs((s) => s.presence);
  const material = useMemo(() => new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, opacity: 0.9 }), []);
  const geometry = useMemo(() => {
    const g = new THREE.BufferGeometry();
    const max = 32 * TRAIL_LEN * 2;
    g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(max * 3), 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('color', new THREE.BufferAttribute(new Float32Array(max * 4), 4).setUsage(THREE.DynamicDrawUsage));
    g.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 1e9);
    return g;
  }, []);
  useEffect(() => () => {
    geometry.dispose();
    material.dispose();
  }, [geometry, material]);
  const trails = useMemo(() => {
    const now = t ?? Date.now();
    return actors.slice(0, 32).map((a) => ({ actor: a, nodes: trailOf(presence, a.id, now, TRAIL_LEN) }));
  }, [actors, presence, t]);

  useFrame(() => {
    const m = frame.model;
    if (!m) return;
    const pos = geometry.getAttribute('position').array as Float32Array;
    const col = geometry.getAttribute('color').array as Float32Array;
    let v = 0;
    const now = t ?? Date.now();
    for (const { actor, nodes } of trails) {
      const [r, g, b] = hslToRgb(actorHue(actor.id), 0.85, 0.62);
      for (let k = 1; k < nodes.length; k++) {
        const a = m.index.get(nodes[k - 1].id);
        const c = m.index.get(nodes[k].id);
        if (a === undefined || c === undefined) continue;
        const age = Math.max(0, now - nodes[k].ts) / 120_000;
        const fade = (k / nodes.length) * Math.max(0, 1 - age);
        for (const [idx, f] of [
          [a, fade * 0.6],
          [c, fade],
        ] as const) {
          pos[v * 3] = frame.display[idx * 3];
          pos[v * 3 + 1] = frame.display[idx * 3 + 1];
          pos[v * 3 + 2] = frame.display[idx * 3 + 2];
          col[v * 4] = r;
          col[v * 4 + 1] = g;
          col[v * 4 + 2] = b;
          col[v * 4 + 3] = f;
          v++;
        }
      }
    }
    geometry.getAttribute('position').needsUpdate = true;
    geometry.getAttribute('color').needsUpdate = true;
    geometry.setDrawRange(0, v);
  });

  return <lineSegments geometry={geometry} material={material} frustumCulled={false} renderOrder={4} raycast={() => null} />;
}

export function Presence() {
  const actorsMap = useCs((s) => s.actors);
  const timeline = useCs((s) => s.timeline);
  const presence = useCs((s) => s.presence);
  const texture = useMemo(() => haloTexture(), []);
  useEffect(() => () => texture.dispose(), [texture]);
  const t = timeline.mode === 'live' ? null : timeline.t;
  const actors = useMemo(() => Object.values(actorsMap), [actorsMap]);
  const focusAt = useMemo(() => (t === null ? null : actorFocusAt(presence, t)), [presence, t]);
  return (
    <>
      {actors.map((a) => {
        const focusId = focusAt ? (focusAt.get(a.id) ?? null) : a.focus;
        return <ActorHalo key={a.id} actor={a} texture={texture} focusId={focusId} />;
      })}
      <Trails actors={actors} t={t} />
    </>
  );
}
