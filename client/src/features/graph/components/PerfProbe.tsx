// PerfProbe — in-client render-loop diagnostics.
//
// Mounted once inside the R3F <Canvas>. Master-gated and OFF by default, so it
// adds zero overhead until armed. Arm it from the DevTools console:
//
//   __perf.on()                 // start continuous sampling + periodic report
//   __perf.report()             // print one snapshot now
//   __perf.dump()               // returns the snapshot object (also on __perfData)
//   __perf.saturation()         // intrusive: no-op rAF fps + setTimeout(0) Hz
//   __perf.ab()                 // intrusive: hide each scene group, measure fps
//   __perf.off()                // stop, restore all wraps
//
// Per-section gates live on __perf.cfg (frameTiming / glInfo / instanced /
// rafCallbacks / census) so you can narrow the report to what matters.
//
// Auto-arms if the page is loaded with ?perf=1 or localStorage.perfProbe==='1'.

import { useThree, useFrame } from '@react-three/fiber';
import { useEffect } from 'react';
import * as THREE from 'three';

type AnyRenderer = THREE.WebGLRenderer & {
  isWebGPURenderer?: boolean;
  info?: {
    render?: { calls?: number; triangles?: number; points?: number; lines?: number };
    memory?: { geometries?: number; textures?: number };
    programs?: unknown[];
    autoReset?: boolean;
    reset?: () => void;
  };
};

interface ProbeCfg {
  frameTiming: boolean;
  glInfo: boolean;
  instanced: boolean;
  rafCallbacks: boolean;
  census: boolean;
  logToConsole: boolean;
  reportIntervalMs: number;
}

interface RafStat { count: number; totalMs: number; maxMs: number }
interface InstStat { key: string; sm: number; sc: number; count: number }

const FRAME_BUF = 240;

class PerfController {
  enabled = false;
  cfg: ProbeCfg = {
    frameTiming: true,
    glInfo: true,
    instanced: true,
    rafCallbacks: true,
    census: true,
    logToConsole: true,
    reportIntervalMs: 4000,
  };

  // live scene handles, set by the mounted component
  gl: AnyRenderer | null = null;
  scene: THREE.Scene | null = null;

  // frame-interval ring buffer (real R3F frame period → fps / frame time)
  private frameBuf = new Float64Array(FRAME_BUF);
  private frameIdx = 0;
  private frameFilled = 0;
  private lastFrameTs = 0;

  // gl.info snapshot from the previously rendered frame
  glSnap = { calls: 0, triangles: 0, programs: 0, geometries: 0, textures: 0 };

  // rAF callback aggregation (reset each report)
  private rafOrig: typeof window.requestAnimationFrame | null = null;
  private rafStats = new Map<string, RafStat>();

  // instanced setMatrixAt/setColorAt counters (reset each report)
  private instCounts = new Map<THREE.InstancedMesh, InstStat>();
  private instWrapped = new WeakSet<THREE.InstancedMesh>();
  private lastRescan = 0;

  private reportTimer: ReturnType<typeof setInterval> | null = null;

  // ---- lifecycle --------------------------------------------------------

  on() {
    if (this.enabled) return '[perf] already on';
    this.enabled = true;
    this.resetFrames();
    if (this.cfg.rafCallbacks) this.installRafWrap();
    if (this.cfg.instanced) this.rescanInstanced(true);
    if (this.gl?.info) this.gl.info.autoReset = false;
    if (this.cfg.logToConsole) {
      this.reportTimer = setInterval(() => this.report(), this.cfg.reportIntervalMs);
    }
    // eslint-disable-next-line no-console
    console.log('%c[perf] armed', 'color:#0af;font-weight:bold', { ...this.cfg });
    return '[perf] on';
  }

  off() {
    this.enabled = false;
    this.uninstallRafWrap();
    this.unwrapInstanced();
    if (this.reportTimer) { clearInterval(this.reportTimer); this.reportTimer = null; }
    // eslint-disable-next-line no-console
    console.log('%c[perf] disarmed', 'color:#888');
    return '[perf] off';
  }

  // ---- per-frame hook (called from the component's useFrame) ------------

  tick() {
    if (!this.enabled) return;
    const now = performance.now();

    if (this.cfg.frameTiming && this.lastFrameTs) {
      this.frameBuf[this.frameIdx] = now - this.lastFrameTs;
      this.frameIdx = (this.frameIdx + 1) % FRAME_BUF;
      if (this.frameFilled < FRAME_BUF) this.frameFilled++;
    }
    this.lastFrameTs = now;

    // read gl.info for the frame that was rendered last, then reset for the next
    if (this.cfg.glInfo && this.gl?.info?.render) {
      const r = this.gl.info.render;
      this.glSnap.calls = r.calls ?? 0;
      this.glSnap.triangles = r.triangles ?? 0;
      this.glSnap.programs = this.gl.info.programs?.length ?? 0;
      this.glSnap.geometries = this.gl.info.memory?.geometries ?? 0;
      this.glSnap.textures = this.gl.info.memory?.textures ?? 0;
      this.gl.info.reset?.();
    }

    if (this.cfg.instanced && now - this.lastRescan > 1000) {
      this.rescanInstanced(false);
      this.lastRescan = now;
    }
  }

  // ---- frame timing helpers --------------------------------------------

  private resetFrames() {
    this.frameIdx = 0; this.frameFilled = 0; this.lastFrameTs = 0;
  }

  private frameStats() {
    const n = this.frameFilled;
    if (!n) return { fps: 0, avgMs: 0, p50: 0, p90: 0, p99: 0, maxMs: 0 };
    const a = Array.prototype.slice.call(this.frameBuf, 0, n).sort((x: number, y: number) => x - y);
    const sum = a.reduce((s: number, v: number) => s + v, 0);
    const avg = sum / n;
    const q = (p: number) => a[Math.min(n - 1, Math.floor(n * p))];
    return {
      fps: +(1000 / avg).toFixed(1),
      avgMs: +avg.toFixed(2),
      p50: +q(0.5).toFixed(2),
      p90: +q(0.9).toFixed(2),
      p99: +q(0.99).toFixed(2),
      maxMs: +a[n - 1].toFixed(2),
    };
  }

  // ---- rAF callback wrap (which callback blocks the frame) --------------

  private installRafWrap() {
    if (this.rafOrig) return;
    const orig = window.requestAnimationFrame.bind(window);
    this.rafOrig = orig;
    const stats = this.rafStats;
    window.requestAnimationFrame = (cb: FrameRequestCallback): number =>
      orig((t: number) => {
        const s = performance.now();
        cb(t);
        const d = performance.now() - s;
        const name = (cb as { name?: string }).name || 'anon';
        let st = stats.get(name);
        if (!st) { st = { count: 0, totalMs: 0, maxMs: 0 }; stats.set(name, st); }
        st.count++; st.totalMs += d; if (d > st.maxMs) st.maxMs = d;
      });
  }

  private uninstallRafWrap() {
    if (this.rafOrig) { window.requestAnimationFrame = this.rafOrig; this.rafOrig = null; }
    this.rafStats.clear();
  }

  private rafReport() {
    const rows = [...this.rafStats.entries()]
      .map(([name, s]) => ({
        name,
        calls: s.count,
        totalMs: +s.totalMs.toFixed(0),
        avgMs: +(s.totalMs / Math.max(1, s.count)).toFixed(2),
        maxMs: +s.maxMs.toFixed(1),
      }))
      .sort((a, b) => b.totalMs - a.totalMs)
      .slice(0, 8);
    this.rafStats.clear();
    return rows;
  }

  // ---- instanced setMatrixAt/setColorAt counters ------------------------

  private rescanInstanced(initial: boolean) {
    if (!this.scene) return;
    this.scene.traverse((o) => {
      const im = o as THREE.InstancedMesh;
      if (!im.isInstancedMesh || this.instWrapped.has(im)) return;
      this.instWrapped.add(im);
      const key = im.name || (im.geometry?.type ?? 'inst') + ':' + im.count;
      const stat: InstStat = { key, sm: 0, sc: 0, count: im.count };
      this.instCounts.set(im, stat);

      const oSM = im.setMatrixAt.bind(im);
      (im as unknown as { __oSM?: typeof oSM }).__oSM = oSM;
      im.setMatrixAt = function (i: number, m: THREE.Matrix4) { stat.sm++; return oSM(i, m); };

      if (im.setColorAt) {
        const oSC = im.setColorAt.bind(im);
        (im as unknown as { __oSC?: typeof oSC }).__oSC = oSC;
        im.setColorAt = function (i: number, c: THREE.Color) { stat.sc++; return oSC(i, c); };
      }
    });
    if (initial) this.lastRescan = performance.now();
  }

  private unwrapInstanced() {
    this.instCounts.forEach((_stat, im) => {
      const holder = im as unknown as { __oSM?: THREE.InstancedMesh['setMatrixAt']; __oSC?: NonNullable<THREE.InstancedMesh['setColorAt']> };
      if (holder.__oSM) { im.setMatrixAt = holder.__oSM; delete holder.__oSM; }
      if (holder.__oSC) { im.setColorAt = holder.__oSC; delete holder.__oSC; }
      this.instWrapped.delete(im);
    });
    this.instCounts.clear();
  }

  private instReport(fps: number) {
    const f = Math.max(1, fps) * (this.cfg.reportIntervalMs / 1000);
    const rows: Array<Record<string, number | string>> = [];
    this.instCounts.forEach((s, im) => {
      // drop meshes that left the scene
      if (!im.parent) return;
      s.count = im.count;
      rows.push({
        mesh: s.key,
        instances: s.count,
        setMatrixAt_perFrame: +(s.sm / f).toFixed(0),
        setColorAt_perFrame: +(s.sc / f).toFixed(0),
      });
      s.sm = 0; s.sc = 0;
    });
    return rows.sort((a, b) => (b.setColorAt_perFrame as number) - (a.setColorAt_perFrame as number));
  }

  // ---- census -----------------------------------------------------------

  private censusReport() {
    const c = { meshes: 0, instancedMeshes: 0, totalInstances: 0, transparent: 0, transmission: 0, materials: {} as Record<string, number> };
    this.scene?.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh && !(o as THREE.InstancedMesh).isInstancedMesh) return;
      c.meshes++;
      const m = (Array.isArray(mesh.material) ? mesh.material[0] : mesh.material) as THREE.MeshPhysicalMaterial | undefined;
      const mt = m?.type ?? 'none';
      c.materials[mt] = (c.materials[mt] ?? 0) + 1;
      if (m?.transparent) c.transparent++;
      if ((m?.transmission ?? 0) > 0) c.transmission++;
      const im = o as THREE.InstancedMesh;
      if (im.isInstancedMesh) { c.instancedMeshes++; c.totalInstances += im.count; }
    });
    return c;
  }

  private backendReport() {
    const out: Record<string, unknown> = {
      renderer: this.gl ? (this.gl.isWebGPURenderer ? 'WebGPURenderer' : this.gl.constructor?.name) : 'unknown',
    };
    try {
      const tmp = document.createElement('canvas').getContext('webgl2');
      const ext = tmp?.getExtension('WEBGL_debug_renderer_info');
      if (tmp && ext) out.webglUnmasked = tmp.getParameter(ext.UNMASKED_RENDERER_WEBGL);
    } catch { /* ignore */ }
    // @types/three 0.187 no longer pulls in @webgpu/types, so `gpu` is absent from Navigator.
    out.hasWebGPU = !!(navigator as Navigator & { gpu?: unknown }).gpu;
    return out;
  }

  // ---- snapshot / report ------------------------------------------------

  dump() {
    const frame = this.cfg.frameTiming ? this.frameStats() : undefined;
    const snap: Record<string, unknown> = {
      ts: new Date().toISOString(),
      backend: this.backendReport(),
    };
    if (frame) snap.frame = frame;
    if (this.cfg.glInfo) snap.gl = { ...this.glSnap };
    if (this.cfg.rafCallbacks) snap.rafCallbacks = this.rafReport();
    if (this.cfg.instanced) snap.instanced = this.instReport(frame?.fps ?? 0);
    if (this.cfg.census) snap.census = this.censusReport();
    (window as unknown as { __perfData?: unknown }).__perfData = snap;
    return snap;
  }

  report() {
    const s = this.dump();
    /* eslint-disable no-console */
    console.groupCollapsed('%c[perf] frame report', 'color:#0af;font-weight:bold');
    console.log('backend', s.backend);
    if (s.frame) console.log('frame', s.frame);
    if (s.gl) console.log('gl (per rendered frame)', s.gl);
    if (s.rafCallbacks) { console.log('rAF callbacks (which loop blocks):'); console.table(s.rafCallbacks); }
    if (s.instanced) { console.log('instanced uploads (per frame):'); console.table(s.instanced); }
    if (s.census) console.log('census', s.census);
    console.log('%ccopy(__perfData) to send the JSON', 'color:#888');
    console.groupEnd();
    /* eslint-enable no-console */
    return s;
  }

  // ---- intrusive on-demand tests ---------------------------------------

  async saturation() {
    const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
    let f = 0, stop = false;
    const t0 = performance.now();
    const tick = () => { if (!stop) { f++; requestAnimationFrame(tick); } };
    requestAnimationFrame(tick);
    await sleep(2000); stop = true;
    const rafFps = +(f / ((performance.now() - t0) / 1000)).toFixed(1);

    let n = 0, stop2 = false;
    const t1 = performance.now();
    const drain = () => { if (!stop2) { n++; setTimeout(drain, 0); } };
    drain();
    await sleep(1000); stop2 = true;
    const r = {
      visibility: document.visibilityState,
      focused: document.hasFocus(),
      noopRafFps: rafFps,
      setTimeoutHz: +(n / ((performance.now() - t1) / 1000)).toFixed(0),
      note: 'noopRafFps≈60 + setTimeoutHz in the thousands → thread is free; low values → main-thread saturated',
    };
    // eslint-disable-next-line no-console
    console.log('%c[perf] saturation', 'color:#0af', r);
    return r;
  }

  async ab() {
    if (!this.scene) return '[perf] no scene';
    const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
    const measure = async (label: string) => {
      let f = 0, stop = false; const t = performance.now();
      const tick = () => { if (!stop) { f++; requestAnimationFrame(tick); } };
      requestAnimationFrame(tick); await sleep(1200); stop = true;
      return { label, fps: +(f / ((performance.now() - t) / 1000)).toFixed(1) };
    };
    const groups = this.scene.children.filter(
      (c) => c.visible && ((c as THREE.Mesh).isMesh || (c as THREE.Group).isGroup || (c as THREE.InstancedMesh).isInstancedMesh),
    );
    const rows = [await measure('baseline')];
    for (const g of groups) {
      g.visible = false;
      rows.push(await measure('hide:' + (g.name || g.type)));
      g.visible = true;
    }
    const prev = groups.map((g) => g.visible);
    groups.forEach((g) => { g.visible = false; });
    rows.push(await measure('hide:ALL'));
    groups.forEach((g, i) => { g.visible = prev[i]; });
    // eslint-disable-next-line no-console
    console.log('%c[perf] visibility A/B (hide:ALL≈baseline → CPU-bound; recovers → GPU-bound)', 'color:#0af');
    // eslint-disable-next-line no-console
    console.table(rows);
    return rows;
  }
}

// module-level singleton so console handle survives component remounts
const controller = new PerfController();
if (typeof window !== 'undefined') {
  (window as unknown as { __perf: PerfController }).__perf = controller;
}

const PerfProbe: React.FC = () => {
  const gl = useThree((s) => s.gl) as unknown as AnyRenderer;
  const scene = useThree((s) => s.scene);

  useEffect(() => {
    controller.gl = gl;
    controller.scene = scene;
    // auto-arm from query param / localStorage
    const auto =
      new URLSearchParams(window.location.search).get('perf') === '1' ||
      (typeof localStorage !== 'undefined' && localStorage.getItem('perfProbe') === '1');
    if (auto && !controller.enabled) controller.on();
    return () => {
      controller.gl = null;
      controller.scene = null;
    };
  }, [gl, scene]);

  // runs first each frame (very negative priority); marks frame period + reads gl.info
  useFrame(() => { controller.tick(); }, -10000);

  return null;
};

export default PerfProbe;
