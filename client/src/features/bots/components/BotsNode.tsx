/**
 * BotsNode.tsx
 * Per-agent 3-D node with:
 *   - Dynamic geometry (shape encodes status/type)
 *   - Organic breathing/metabolic useFrame animation
 *   - Bioluminescent membrane + nucleus glow
 *   - Queen corona ring
 *   - High-token-rate vibration / float / memory-pressure shake
 *   - Billboard label with 5 display modes (click to cycle)
 *   - AgentStatusBadges HTML overlay on hover / active
 *
 * Zero-alloc contract: all THREE objects used inside useFrame are refs
 * (currentPositionRef, targetPositionRef, lastPositionRef) — never allocated
 * per frame.
 */
import React, { useRef, useEffect, useState, useMemo } from 'react';
import { useFrame } from '@react-three/fiber';
import * as THREE from 'three';
import { Html, Text, Billboard } from '@react-three/drei';
import { BotsAgent } from '../types/BotsTypes';
import { useTelemetry, useThreeJSTelemetry } from '../../../telemetry/useTelemetry';
import { useSettingsStore } from '../../../store/settingsStore';
import {
  lerpVector3,
  formatProcessingLogs,
  applySwarmTint,
  ADDITIVE_BLENDING,
  BACK_SIDE,
} from './BotsShared';
import { healthGlowColor, agentStatusColor } from '../agentVisualConstants';
import { shortDid } from '../agentIdentity';
import { isWebGPURenderer } from '../../../rendering/rendererFactory';
import { AgentStatusBadges } from './AgentStatusBadges';
import { AgentTrail, TRAIL_DEFAULT_LENGTH } from './AgentTrail';

/**
 * Nameplate level-of-detail tiers (W3D). A 3-line HTML nameplate per agent
 * overlaps illegibly once ~19 agents cluster, so each agent's label is gated to
 * one of three tiers by camera distance:
 *   - 'full'   3-line nameplate (name / type / status|health)
 *   - 'name'   single name line
 *   - 'hidden' no nameplate
 */
export type NameplateTier = 'full' | 'name' | 'hidden';

export interface NameplateTierOpts {
  /** D1 — camera distance under which the full nameplate shows. */
  fullDistance: number;
  /** D2 — camera distance under which the name-only line shows; hidden beyond it. */
  nameDistance: number;
  /** Fractional dead-band applied to BOTH boundaries (0.1 = ±10%). */
  hysteresis: number;
  /** Pin to 'full' regardless of distance (hovered / selected / queen). */
  forceFull?: boolean;
  /** Screen-density guard: cap a would-be 'full' tier at 'name' (never overrides
   *  forceFull). Set when too many agents crowd the near field. */
  capName?: boolean;
}

/**
 * Pure distance→tier classifier with directional hysteresis. A label only
 * *promotes* (richer tier) once distance drops below boundary·(1−h) and only
 * *demotes* once it rises above boundary·(1+h); inside a dead-band it holds
 * prevTier, so an agent sitting on a boundary never flickers frame-to-frame.
 * Assumes fullDistance < nameDistance with non-overlapping bands (guaranteed by
 * the 2.25× spacing the caller uses against the ±10% band).
 */
export function computeNameplateTier(
  distance: number,
  prevTier: NameplateTier,
  opts: NameplateTierOpts,
): NameplateTier {
  if (opts.forceFull) return 'full';

  const { fullDistance: d1, nameDistance: d2, hysteresis: h } = opts;
  const d1Lo = d1 * (1 - h);
  const d1Hi = d1 * (1 + h);
  const d2Lo = d2 * (1 - h);
  const d2Hi = d2 * (1 + h);

  let tier: NameplateTier;
  if (distance < d1Lo) {
    tier = 'full';
  } else if (distance >= d2Hi) {
    tier = 'hidden';
  } else if (distance >= d1Hi && distance < d2Lo) {
    tier = 'name';
  } else if (distance < d1Hi) {
    // Dead-band around D1: hold full↔name.
    tier = prevTier === 'full' ? 'full' : 'name';
  } else {
    // Dead-band around D2 (d2Lo ≤ distance < d2Hi): hold name↔hidden.
    tier = prevTier === 'hidden' ? 'hidden' : 'name';
  }

  if (opts.capName && tier === 'full') tier = 'name';
  return tier;
}

/** Spacing of the name-only band relative to the full-nameplate distance (D1):
 *  D2 = D1 × this. 2.25× turns the default 40u full radius into a ~90u name band. */
const NAMEPLATE_NAME_DISTANCE_FACTOR = 2.25;
/** ±10% hysteresis dead-band on each LOD boundary. */
const NAMEPLATE_HYSTERESIS = 0.1;
/** More than this many agents inside D1 → non-queen/non-hovered agents drop to
 *  name-only (near-field declutter). */
const NAMEPLATE_DENSITY_LIMIT = 8;

/**
 * Frame-shared near-field agent tally. Every BotsNode increments `accum` when it
 * sits within D1 of the camera; all nodes read `published`, which holds LAST
 * frame's total. This is intentionally ONE FRAME STALE — R3F runs each node's
 * useFrame sequentially with no barrier, so a live count would depend on render
 * order and could itself induce flicker. The ±1-frame lag is imperceptible at
 * 60fps and the crowded state is stable while a swarm stays clustered.
 */
const nameplateDensity = { frameTime: -1, accum: 0, published: 0 };

function tickNameplateDensity(frameTime: number, withinFull: boolean): number {
  if (frameTime !== nameplateDensity.frameTime) {
    // First node of a new frame: publish the previous tally, reset the accumulator.
    nameplateDensity.published = nameplateDensity.accum;
    nameplateDensity.accum = 0;
    nameplateDensity.frameTime = frameTime;
  }
  if (withinFull) nameplateDensity.accum += 1;
  return nameplateDensity.published;
}

export interface BotsNodeProps {
  agent: BotsAgent;
  position: THREE.Vector3;
  index: number;
  color: string;
  /**
   * When true (default) the body colour is hue-rotated by a stable per-swarmId
   * offset so multi-swarm scenes read as families. Pass false to render every
   * swarm on the flat type colour.
   */
  swarmTint?: boolean;
  /**
   * Shared ref map: agent wire id → target KG node world position. When a
   * target is present the node drifts toward it at medium speed, giving the
   * visual impression that agents cluster around the area of the graph they
   * are actively working on.
   */
  nudgeTargetsRef?: React.RefObject<Map<string, THREE.Vector3>>;
}

export const BotsNode: React.FC<BotsNodeProps> = ({ agent, position, index, color, swarmTint = true, nudgeTargetsRef }) => {
  const groupRef   = useRef<THREE.Group>(null);
  const meshRef    = useRef<THREE.Mesh>(null);
  const glowRef    = useRef<THREE.Mesh>(null);
  const nucleusRef = useRef<THREE.Mesh>(null);
  const coronaRef  = useRef<THREE.Mesh>(null);
  // declaredIntent pre-action flash: clock time the current flash began (-1 idle).
  const flashStartRef = useRef(-1);
  const prevIntentRef = useRef<string | undefined>(agent.declaredIntent);
  const [hover, setHover] = useState(false);
  const [displayMode, setDisplayMode] = useState<
    'overview' | 'performance' | 'tasks' | 'network' | 'resources'
  >('overview');
  // Nameplate LOD tier (W3D). Ref is the per-frame source of truth (no re-render);
  // state mirrors it only on an actual tier change so the render churn stays zero
  // while the camera holds still.
  const [nameplateTier, setNameplateTier] = useState<NameplateTier>('full');
  const nameplateTierRef = useRef<NameplateTier>('full');
  const telemetry      = useTelemetry(`BotsNode-${agent.id}`);
  const threeJSTelemetry = useThreeJSTelemetry(agent.id);
  const lastPositionRef    = useRef<THREE.Vector3 | undefined>(undefined);
  const currentPositionRef = useRef<THREE.Vector3>(position.clone());
  const targetPositionRef  = useRef<THREE.Vector3>(position.clone());
  const elapsedTimeRef     = useRef(0);
  const settings = useSettingsStore(state => state.settings);

  const healthColors = settings?.visualisation?.graphTypeVisuals?.agent?.healthColors;
  const glowColor = useMemo(
    () => healthGlowColor(agent.health || 0, healthColors),
    [agent.health, healthColors],
  );

  const isQueen = agent.type === 'queen';

  const statusColor = useMemo(() => agentStatusColor(agent.status), [agent.status]);

  // Swarm partition tint: stable per-swarmId hue rotation of the base body colour
  // so agents from the same swarm read as a family (default ON via swarmTint).
  const bodyColor = useMemo(
    () => (swarmTint ? applySwarmTint(color, agent.swarmId) : color),
    [swarmTint, color, agent.swarmId],
  );

  // Agent trail ribbons (control-centre Agents → Behaviour). Client-only, default on.
  // Trail colour tracks the *status* colour (not the type body colour) so the fading
  // ribbon reads the agent's live activity, tinted per-swarm to match the body when
  // swarmTint is on.
  const agentVisuals = settings?.visualisation?.graphTypeVisuals?.agent;
  const showTrails = agentVisuals?.showTrails ?? true;
  const trailLength = agentVisuals?.trailLength ?? TRAIL_DEFAULT_LENGTH;
  const nameplateLod = agentVisuals?.nameplateLod ?? true;
  const nameplateFullDistance = agentVisuals?.nameplateFullDistance ?? 40;
  const trailColor = useMemo(
    () => (swarmTint ? applySwarmTint(statusColor, agent.swarmId) : statusColor),
    [swarmTint, statusColor, agent.swarmId],
  );

  const baseSize      = 1.0;
  const cpuScale      = agent.cpuUsage    ? (agent.cpuUsage / 100) * 0.8 : 0;
  const workloadScale = agent.workload    ? agent.workload * 0.6           : 0;
  const activityScale = agent.activity   ? agent.activity * 0.4           : 0;
  const tokenScale    = agent.tokenRate  ? Math.min(agent.tokenRate / 50, 0.5) : 0;
  const clampedSize   = Math.max(0.5, Math.min(
    baseSize + cpuScale + workloadScale + activityScale + tokenScale,
    3.0,
  ));

  const geometry = useMemo(() => {
    const r = clampedSize;
    switch (agent.status) {
      case 'error':        return new THREE.TetrahedronGeometry(r * 1.2);
      case 'terminating':  return new THREE.OctahedronGeometry(r);
      case 'initializing': return new THREE.BoxGeometry(r, r, r);
      case 'idle':         return new THREE.SphereGeometry(r * 0.8, 8, 6);
      case 'offline':      return new THREE.CylinderGeometry(r * 0.5, r * 0.5, r);
      case 'busy':
        switch (agent.type) {
          case 'queen':       return new THREE.IcosahedronGeometry(r * 1.3, 1);
          case 'coordinator': return new THREE.DodecahedronGeometry(r * 1.1);
          case 'architect':   return new THREE.ConeGeometry(r, r * 1.5, 8);
          default:            return new THREE.SphereGeometry(r, 10, 8);
        }
      case 'active':
      default:
        return new THREE.SphereGeometry(r, 10, 8);
    }
  }, [agent.status, agent.type, clampedSize]);

  useEffect(() => {
    return () => { geometry?.dispose(); };
  }, [geometry]);

  useFrame((state) => {
    if (!groupRef.current || !meshRef.current || !glowRef.current) return;

    telemetry.startRender();

    if (!lastPositionRef.current || !lastPositionRef.current.equals(position)) {
      threeJSTelemetry.logPositionUpdate(
        { x: position.x, y: position.y, z: position.z },
        { agentType: agent.type, agentStatus: agent.status },
      );
      if (!lastPositionRef.current) {
        lastPositionRef.current = position.clone();
      } else {
        lastPositionRef.current.copy(position);
      }
    }

    targetPositionRef.current.copy(position);

    // Momentum nudge: if this agent has a target KG node, bias the target
    // position toward it so the sprite drifts into the working area of the
    // graph. The blend factor (0.35) gives a medium-speed drift — fast enough
    // to trend quickly to the mass centre, gentle enough to remain smooth.
    if (nudgeTargetsRef?.current) {
      const kgTarget = nudgeTargetsRef.current.get(agent.id);
      if (kgTarget) {
        targetPositionRef.current.lerp(kgTarget, 0.35);
      }
    }

    lerpVector3(currentPositionRef.current, targetPositionRef.current, 0.15);
    groupRef.current.position.copy(currentPositionRef.current);

    const elapsedTime = state.clock.elapsedTime;
    elapsedTimeRef.current = elapsedTime;

    // Nameplate LOD (W3D): pick this agent's label tier from camera distance with
    // hysteresis, plus a one-frame-stale near-field density guard so a clustered
    // swarm doesn't stack 19 overlapping 3-line HTML nameplates. Queen/hovered
    // agents are pinned to the full nameplate. setState fires only on an actual
    // tier change, so the per-frame path stays render-churn-free.
    if (nameplateLod) {
      const camDist = state.camera.position.distanceTo(currentPositionRef.current);
      const crowd = tickNameplateDensity(elapsedTime, camDist < nameplateFullDistance);
      const nextTier = computeNameplateTier(camDist, nameplateTierRef.current, {
        fullDistance: nameplateFullDistance,
        nameDistance: nameplateFullDistance * NAMEPLATE_NAME_DISTANCE_FACTOR,
        hysteresis: NAMEPLATE_HYSTERESIS,
        forceFull: hover || isQueen,
        capName: crowd > NAMEPLATE_DENSITY_LIMIT,
      });
      if (nextTier !== nameplateTierRef.current) {
        nameplateTierRef.current = nextTier;
        setNameplateTier(nextTier);
      }
    } else if (nameplateTierRef.current !== 'full') {
      // LOD disabled: everything reverts to the always-on full nameplate.
      nameplateTierRef.current = 'full';
      setNameplateTier('full');
    }

    // declaredIntent pre-action flash: a new non-empty declared intent starts a
    // brief (~600ms) aura spike — the "about to act" cue before the agent moves.
    const intent = agent.declaredIntent;
    if (intent && intent !== prevIntentRef.current) {
      flashStartRef.current = elapsedTime;
    }
    prevIntentRef.current = intent;
    let flashEnv = 0;
    if (flashStartRef.current >= 0) {
      const t = (elapsedTime - flashStartRef.current) / 0.6;
      if (t >= 1) flashStartRef.current = -1;
      else flashEnv = 1 - t; // linear decay from the intent instant
    }

    const activity   = agent.activity ?? 0;
    const healthPulse = agent.health ? (agent.health / 100) : 0.5;
    const tokenGlow  = agent.tokenRate ? Math.min(agent.tokenRate / 20, 2) : 0;

    // Organic breathing & metabolic pulse
    if (agent.status === 'active' || agent.status === 'busy') {
      const tokenMultiplier  = agent.tokenRate ? Math.min(agent.tokenRate / 10, 3) : 1;
      const healthMultiplier = agent.health    ? Math.max(0.3, agent.health / 100)  : 1;
      const pulseSpeed       = 2 * tokenMultiplier * healthMultiplier;

      const breathCycle = Math.sin(elapsedTime * pulseSpeed * 0.8 + index);
      const breathScale = breathCycle > 0
        ? 1 + breathCycle * 0.08   // gentle inhale
        : 1 + breathCycle * 0.04;  // slower exhale
      meshRef.current.scale.setScalar(breathScale * clampedSize);

      const membraneScale  = isQueen ? 1.5 : 1.3;
      const membraneBreath = 0.08 + healthPulse * 0.04;
      const glowBreathScale = membraneScale
        + Math.sin(elapsedTime * pulseSpeed * 0.7 + index + 0.3) * membraneBreath;
      const statusGlow    = agent.status === 'busy' ? 1.5 : 1.0;
      const glowIntensity = (tokenGlow > 0 ? tokenGlow : 1) * healthPulse * statusGlow;
      glowRef.current.scale.setScalar(glowBreathScale * glowIntensity);

      if (nucleusRef.current) {
        const nucleusGlow = Math.pow(Math.sin(elapsedTime * 1.2 + 0.5 + index) * 0.5 + 0.5, 2);
        const nucleusMat  = nucleusRef.current.material as THREE.MeshBasicMaterial;
        if (nucleusMat) {
          nucleusMat.opacity = 0.3 + activity * 0.3 + nucleusGlow * 0.2;
        }
        nucleusRef.current.scale.setScalar(0.4 + nucleusGlow * 0.05);
      }

      const glowMat = glowRef.current.material as THREE.MeshStandardMaterial;
      if (glowMat && glowMat.opacity !== undefined) {
        glowMat.emissiveIntensity = 0.3 + tokenGlow * 0.2;
      }
    } else if (agent.status === 'error') {
      const distress  = Math.sin(elapsedTime * 8 + index) * Math.sin(elapsedTime * 5.3 + index) * 0.2;
      const errorPulse = 1 + Math.abs(distress) + Math.sin(elapsedTime * 8 + index) * 0.15;
      meshRef.current.scale.setScalar(errorPulse * clampedSize);
      glowRef.current.scale.setScalar(errorPulse * 2.0);
      if (nucleusRef.current) {
        const flickerMat = nucleusRef.current.material as THREE.MeshBasicMaterial;
        if (flickerMat) {
          flickerMat.opacity = 0.2 + Math.abs(Math.sin(elapsedTime * 12 + index)) * 0.5;
        }
      }
    } else {
      if (nucleusRef.current) {
        const idleMat = nucleusRef.current.material as THREE.MeshBasicMaterial;
        if (idleMat) {
          idleMat.opacity = 0.15 + Math.sin(elapsedTime * 0.5 + index) * 0.05;
        }
      }
    }

    // Busy cytoplasm churn
    if (agent.status === 'busy') {
      if (isQueen) {
        meshRef.current.rotation.y += 0.005;
      } else {
        const rotationSpeed = agent.tokenRate ? 0.01 * (1 + agent.tokenRate / 50) : 0.01;
        meshRef.current.rotation.y += rotationSpeed;
      }
      groupRef.current.rotation.x += Math.sin(elapsedTime * 0.7 + index) * 0.02 * 0.1;
      groupRef.current.rotation.z += Math.cos(elapsedTime * 0.5 + index * 0.7) * 0.02 * 0.1;
    }

    // Queen corona
    if (isQueen && coronaRef.current) {
      coronaRef.current.rotation.y -= 0.003;
      coronaRef.current.rotation.z  = Math.sin(elapsedTime * 0.4) * 0.05;
      const coronaMat = coronaRef.current.material as THREE.MeshBasicMaterial;
      if (coronaMat) {
        coronaMat.opacity = 0.12 + Math.sin(elapsedTime * 0.8) * 0.04;
      }
    }

    // High token-rate vibration + float
    if (agent.tokenRate && agent.tokenRate > 30) {
      meshRef.current.position.y += Math.sin(elapsedTime * 15 + index) * 0.03
        + Math.cos(elapsedTime * 3 + index) * 0.1;
    }

    // Memory pressure shake
    if (agent.memoryUsage && agent.memoryUsage > 80) {
      const shake = Math.sin(elapsedTime * 25) * 0.01;
      meshRef.current.position.x += shake;
      meshRef.current.position.z += shake * 0.7;
    }

    // Critical health alarm pulse
    if (agent.health && agent.health < 25) {
      meshRef.current.scale.multiplyScalar(Math.sin(elapsedTime * 12) * 0.5 + 1);
    }

    // declaredIntent flash: brief whole-node swell + core brighten. Group scale is
    // set absolutely (identity when idle) so the spike never accumulates; the
    // nucleus opacity was set absolutely in the status branch above, so adding the
    // spike here is safe and resets next frame.
    groupRef.current.scale.setScalar(1 + flashEnv * 0.18);
    if (flashEnv > 0 && nucleusRef.current) {
      const flashMat = nucleusRef.current.material as THREE.MeshBasicMaterial;
      if (flashMat) flashMat.opacity = Math.min(1, flashMat.opacity + flashEnv * 0.6);
    }

    telemetry.endRender();
  });

  const processingLogs = formatProcessingLogs(agent.processingLogs);
  // Nameplate LOD gates: hidden → render nothing; name → single name line;
  // full → the complete 3-line (+did) nameplate. Driven off the tier state so a
  // change re-renders exactly once. LOD off leaves nameplateTier pinned at 'full'.
  const showNameplate = nameplateTier !== 'hidden';
  const showNameplateFull = nameplateTier === 'full';

  return (
    <>
      {/* Motion-trail ribbon — a SIBLING of the moving group so its sampled world
          positions stay put while the agent flies on (see AgentTrail). */}
      {showTrails && (
        <AgentTrail positionRef={currentPositionRef} color={trailColor} length={trailLength} />
      )}
      <group ref={groupRef}>
      {/* Outer membrane */}
      <mesh ref={glowRef} scale={[isQueen ? 1.5 : 1.3, isQueen ? 1.5 : 1.3, isQueen ? 1.5 : 1.3]}>
        <sphereGeometry args={[clampedSize * 0.75, 10, 8]} />
        <meshStandardMaterial
          color={isQueen ? '#FFD700' : glowColor}
          transparent
          opacity={0.08 + (hover ? 0.06 : 0)
            + (agent.tokenRate ? Math.min(agent.tokenRate / 100, 0.12) : 0)}
          side={BACK_SIDE}
          depthWrite={false}
          emissive={isQueen ? '#FFD700' : glowColor}
          emissiveIntensity={0.3 + (agent.tokenRate ? Math.min(agent.tokenRate / 20, 2) * 0.2 : 0)}
        />
      </mesh>

      {/* Inner nucleus glow */}
      <mesh ref={nucleusRef} scale={[0.4, 0.4, 0.4]}>
        <sphereGeometry args={[clampedSize * 0.8, 12, 12]} />
        <meshBasicMaterial
          color={isQueen ? '#FFD700' : statusColor}
          transparent
          opacity={0.3 + (agent.activity ?? 0) * 0.3}
          blending={ADDITIVE_BLENDING}
          depthWrite={false}
        />
      </mesh>

      {/* Queen golden corona ring */}
      {isQueen && (
        <mesh ref={coronaRef} rotation={[Math.PI / 2, 0, 0]}>
          <torusGeometry args={[clampedSize * 1.8, clampedSize * 0.08, 16, 48]} />
          <meshBasicMaterial
            color="#FFD700"
            transparent
            opacity={0.14}
            blending={ADDITIVE_BLENDING}
            depthWrite={false}
          />
        </mesh>
      )}

      {/* Main agent body */}
      <mesh
        ref={meshRef}
        geometry={geometry}
        onPointerOver={() => {
          setHover(true);
          telemetry.logInteraction('hover_start', {
            agentId: agent.id, agentType: agent.type,
            health: agent.health, cpuUsage: agent.cpuUsage,
            tokenRate: agent.tokenRate, status: agent.status, nodeSize: clampedSize,
          });
        }}
        onPointerOut={() => {
          setHover(false);
          telemetry.logInteraction('hover_end', { agentId: agent.id, agentType: agent.type, hoverDuration: 'hover_ended' });
        }}
        onClick={() => {
          const modes: Array<'overview' | 'performance' | 'tasks' | 'network' | 'resources'> =
            ['overview', 'performance', 'tasks', 'network', 'resources'];
          const nextMode = modes[(modes.indexOf(displayMode) + 1) % modes.length];
          setDisplayMode(nextMode);
          telemetry.logInteraction('click', {
            agentId: agent.id, agentType: agent.type, displayMode: nextMode,
            position: { x: position.x, y: position.y, z: position.z },
            health: agent.health, status: agent.status, currentTask: agent.currentTask,
            capabilities: agent.capabilities?.slice(0, 3),
          });
        }}
      >
        <meshStandardMaterial
          color={bodyColor}
          emissive={glowColor}
          emissiveIntensity={(() => {
            const glowSettings = settings?.visualisation?.glow;
            const baseIntensity = glowSettings?.nodeGlowStrength ?? 0.7;
            return (agent.status === 'active' || agent.status === 'busy')
              ? baseIntensity * 0.7
              : baseIntensity * 0.3;
          })()}
          metalness={0.3}
          roughness={0.7}
          transparent={agent.status === 'error' || agent.status === 'terminating'}
          opacity={agent.status === 'error' || agent.status === 'terminating' ? 0.7 : 1.0}
        />
      </mesh>

      {/* HTML overlay */}
      {(hover || agent.status === 'active' || agent.status === 'busy') && (
        <Html
          center
          distanceFactor={8}
          style={{
            transition: 'all 0.3s ease-in-out',
            opacity: hover ? 1 : 0.85,
            pointerEvents: 'none',
            position: 'absolute',
            top: `${-clampedSize * 25}px`,
            left: '0',
            transform: hover ? 'scale(1.05)' : 'scale(1)',
            filter: hover ? 'drop-shadow(0 4px 8px rgba(0,0,0,0.3))' : 'none',
          }}
        >
          <AgentStatusBadges agent={agent} logs={processingLogs} />
        </Html>
      )}

      {/* High-activity ring + token particles */}
      {((agent.tokenRate ?? 0) > 30 || agent.cpuUsage > 80) && (
        <group>
          <mesh rotation={[Math.PI / 2, 0, 0]} position={[0, clampedSize + 0.2, 0]}>
            <ringGeometry args={[clampedSize * 1.1, clampedSize * 1.3, 16]} />
            <meshBasicMaterial
              color={agent.cpuUsage > 90 ? '#E74C3C' : agent.cpuUsage > 70 ? '#F39C12' : '#2ECC71'}
              transparent opacity={0.6} side={THREE.DoubleSide}
            />
          </mesh>

          {(agent.tokenRate ?? 0) > 50 && [
            ...Array(Math.min(Math.floor((agent.tokenRate ?? 0) / 10), 8))
          ].map((_, i) => {
            const angle  = (i / 8) * Math.PI * 2;
            const radius = clampedSize * 2;
            const x = Math.cos(angle + elapsedTimeRef.current) * radius;
            const z = Math.sin(angle + elapsedTimeRef.current) * radius;
            return (
              <mesh key={i} position={[x, 0, z]}>
                <sphereGeometry args={[0.03, 6, 6]} />
                <meshBasicMaterial color="#F39C12" transparent opacity={0.8} />
              </mesh>
            );
          })}
        </group>
      )}

      {/* Billboard labels — Html whenever troika Text cannot run: on WebGPU its
          Line2 geometry triggers drawIndexed(Infinity) and kills the render
          pass, and under COOP/COEP cross-origin isolation (always on in this
          deployment, for the SAB physics pipeline) Chromium blocks troika's
          blob-worker bootstrap (crbug.com/1084951) — including the copy inlined
          in drei's bundle, which configureTextBuilder cannot reach. The full
          display-mode Text cluster remains for non-isolated WebGL contexts. */}
      {showNameplate && (isWebGPURenderer || (typeof self !== 'undefined' && self.crossOriginIsolated) ? (
        <Html position={[0, clampedSize + 0.9, 0]} center style={{ pointerEvents: 'none', whiteSpace: 'nowrap', textAlign: 'center' }}>
          <div style={{ color: 'white', fontSize: '12px', fontWeight: 'bold', textShadow: '0 0 4px black' }}>
            {agent.name || String(agent.id).slice(0, 8)}
          </div>
          {/* type / status|health / did collapse away below the full-LOD distance
              (W3D) — the name line above is the sole survivor at range. */}
          {showNameplateFull && (<>
            <div style={{ color, fontSize: '10px', textShadow: '0 0 3px black' }}>
              {agent.type.toUpperCase()}
            </div>
            <div style={{ color: glowColor, fontSize: '9px', textShadow: '0 0 3px black' }}>
              {agent.status} | {agent.health ? `${agent.health.toFixed(0)}%` : 'N/A'}
              {(agent.tokenRate ?? 0) > 0 ? ` | ${agent.tokenRate!.toFixed(0)} tok/min` : ''}
            </div>
            {/* Sovereign identity nameplate (COM-14 / ADR-125) — sole did:nostr
                renderer since AgentNodesLayer was retired. */}
            {agent.did_nostr && (
              <div style={{ color: '#7dd3fc', fontSize: '9px', fontFamily: 'monospace', textShadow: '0 0 3px black' }}>
                {shortDid(agent.did_nostr)}
              </div>
            )}
          </>)}
        </Html>
      ) : (
      <Billboard follow lockX={false} lockY={false} lockZ={false}>
        {/* Sovereign identity nameplate (COM-14 / ADR-125) — sole did:nostr
            renderer since AgentNodesLayer was retired. Sits above the mode
            indicator so it never collides with the display-mode cluster below.
            Part of the full-LOD cluster (W3D). */}
        {showNameplateFull && agent.did_nostr && (
          <Text position={[0, clampedSize + 1.15, 0]} fontSize={0.16} color="#7dd3fc"
            anchorX="center" anchorY="middle" outlineWidth={0.015} outlineColor="black">
            {shortDid(agent.did_nostr)}
          </Text>
        )}
        {showNameplateFull && (
          <Text position={[0, clampedSize + 0.8, 0]} fontSize={0.18} color="#3498DB"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            [{displayMode.toUpperCase()}]
          </Text>
        )}
        <Text position={[0, -clampedSize - 0.7, 0]} fontSize={0.4} color="white"
          anchorX="center" anchorY="middle" outlineWidth={0.05} outlineColor="black">
          {agent.name || String(agent.id).slice(0, 8)}
        </Text>

        {showNameplateFull && displayMode === 'overview' && (<>
          <Text position={[0, -clampedSize - 1.1, 0]} fontSize={0.25} color={color}
            anchorX="center" anchorY="middle" outlineWidth={0.03} outlineColor="black">
            {agent.type.toUpperCase()}
          </Text>
          <Text position={[0, -clampedSize - 1.4, 0]} fontSize={0.2} color={glowColor}
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Health: {agent.health ? `${agent.health.toFixed(0)}%` : 'N/A'}
          </Text>
          <Text position={[0, -clampedSize - 1.7, 0]} fontSize={0.15} color="#95A5A6"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Status: {agent.status}
          </Text>
        </>)}

        {showNameplateFull && displayMode === 'performance' && (<>
          <Text position={[0, -clampedSize - 1.1, 0]} fontSize={0.2}
            color={agent.cpuUsage > 80 ? '#E74C3C' : agent.cpuUsage > 50 ? '#F39C12' : '#2ECC71'}
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            CPU: {agent.cpuUsage?.toFixed(0) || 0}%
          </Text>
          <Text position={[0, -clampedSize - 1.4, 0]} fontSize={0.2} color="#9B59B6"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            MEM: {agent.memoryUsage?.toFixed(0) || 0}%
          </Text>
          <Text position={[0, -clampedSize - 1.7, 0]} fontSize={0.18}
            color={(agent.tokenRate ?? 0) > 20 ? '#E67E22' : '#3498DB'}
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Tokens: {agent.tokenRate?.toFixed(1) || 0}/min
          </Text>
          <Text position={[0, -clampedSize - 2.0, 0]} fontSize={0.15} color="#F39C12"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Total: {agent.tokens?.toLocaleString() || 0}
          </Text>
        </>)}

        {showNameplateFull && displayMode === 'tasks' && (<>
          <Text position={[0, -clampedSize - 1.1, 0]} fontSize={0.2} color="#2ECC71"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Active: {agent.tasksActive || 0}
          </Text>
          <Text position={[0, -clampedSize - 1.4, 0]} fontSize={0.2} color="#3498DB"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Done: {agent.tasksCompleted || 0}
          </Text>
          <Text position={[0, -clampedSize - 1.7, 0]} fontSize={0.15} color="#95A5A6"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            {agent.currentTask ? agent.currentTask.substring(0, 20) + '...' : 'Idle'}
          </Text>
          {agent.successRate !== undefined && (
            <Text position={[0, -clampedSize - 2.0, 0]} fontSize={0.15}
              color={agent.successRate > 0.8 ? '#27AE60' : agent.successRate > 0.6 ? '#F39C12' : '#E74C3C'}
              anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
              Success: {(agent.successRate * 100).toFixed(0)}%
            </Text>
          )}
        </>)}

        {showNameplateFull && displayMode === 'network' && (<>
          <Text position={[0, -clampedSize - 1.1, 0]} fontSize={0.18} color="#E67E22"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Swarm: {agent.swarmId?.substring(0, 8) || 'None'}
          </Text>
          <Text position={[0, -clampedSize - 1.4, 0]} fontSize={0.18} color="#F39C12"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Mode: {agent.agentMode || 'Default'}
          </Text>
          {agent.parentQueenId && (
            <Text position={[0, -clampedSize - 1.7, 0]} fontSize={0.15} color="#FFD700"
              anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
              Queen: {agent.parentQueenId.substring(0, 8)}
            </Text>
          )}
          <Text position={[0, -clampedSize - 2.0, 0]} fontSize={0.15} color="#95A5A6"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Age: {agent.age ? Math.floor(agent.age / 1000 / 60) : 0}m
          </Text>
        </>)}

        {showNameplateFull && displayMode === 'resources' && (<>
          <Text position={[0, -clampedSize - 1.1, 0]} fontSize={0.18} color="#3498DB"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Workload: {((agent.workload ?? 0) * 100).toFixed(0)}%
          </Text>
          <Text position={[0, -clampedSize - 1.4, 0]} fontSize={0.18} color="#2ECC71"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            Activity: {((agent.activity ?? 0) * 100).toFixed(0)}%
          </Text>
          {agent.capabilities && agent.capabilities.length > 0 && (
            <Text position={[0, -clampedSize - 1.7, 0]} fontSize={0.15} color="#9B59B6"
              anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
              Caps: {agent.capabilities.length} total
            </Text>
          )}
          <Text position={[0, -clampedSize - 2.0, 0]} fontSize={0.13} color="#95A5A6"
            anchorX="center" anchorY="middle" outlineWidth={0.02} outlineColor="black">
            {agent.capabilities?.[0]?.replace(/_/g, ' ') || 'None'}
          </Text>
        </>)}
      </Billboard>
      ))}
      </group>
    </>
  );
};
