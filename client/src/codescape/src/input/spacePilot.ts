/**
 * 3Dconnexion SpacePilot / SpaceMouse over WebHID.
 *
 * Self-contained on purpose: Codescape may not import from the VisionClaw
 * client tree (the import-isolation test), so the device parsing lives here.
 * It mirrors the report layout VisionClaw's SpaceDriverService decodes:
 *
 * | report id | payload                                   |
 * |-----------|-------------------------------------------|
 * | 1         | 3 × i16 translation; some devices pack rotation as 3 more i16 |
 * | 2         | 3 × i16 rotation                          |
 * | 3         | u16 button bitfield                       |
 *
 * Axis values are raw ±(350..512 typical, 32767 max); callers normalise.
 *
 * WebHID needs a secure context (https or localhost) and a user gesture for
 * `requestDevice`; previously paired devices reconnect from `getDevices()`
 * without a gesture.
 */

/** Logitech (older SpacePilot units) and 3Dconnexion vendor ids. */
export const SUPPORTED_VENDOR_IDS: readonly number[] = [0x046d, 0x256f];

const REPORT_TRANSLATION = 1;
const REPORT_ROTATION = 2;
const REPORT_BUTTONS = 3;

/** Six-axis sample plus the raw button mask, as parsed from one report. */
export interface SpaceSample {
  tx: number;
  ty: number;
  tz: number;
  rx: number;
  ry: number;
  rz: number;
}

export interface ParsedReport {
  /** Axes present in this report (others untouched by the caller). */
  axes?: Partial<SpaceSample>;
  /** Button bitfield when the report carried one. */
  buttons?: number;
}

/**
 * Parse one HID input report into axis deltas and/or a button mask. Pure, so
 * it is unit-tested without a device. Unknown ids yield an empty object.
 */
export function parseReport(reportId: number, data: DataView): ParsedReport {
  const n = Math.floor(data.byteLength / 2);
  const i16 = (k: number) => (k < n ? data.getInt16(k * 2, true) : 0);
  switch (reportId) {
    case REPORT_TRANSLATION: {
      const axes: Partial<SpaceSample> = { tx: i16(0), ty: i16(1), tz: i16(2) };
      if (n >= 6) {
        // Combined 6-axis report (SpaceNavigator and newer SpaceMouse units).
        axes.rx = -i16(3);
        axes.ry = -i16(4);
        axes.rz = i16(5);
      }
      return { axes };
    }
    case REPORT_ROTATION:
      return { axes: { rx: -i16(0), ry: -i16(1), rz: i16(2) } };
    case REPORT_BUTTONS:
      return { buttons: n >= 1 ? data.getUint16(0, true) : 0 };
    default:
      return {};
  }
}

/** Why the device cannot be used right now, or `ok` when it can. */
export type HidAvailability = { ok: true } | { ok: false; reason: string };

export function hidAvailable(): HidAvailability {
  if (typeof navigator === 'undefined' || !('hid' in navigator) || !navigator.hid) {
    return { ok: false, reason: 'WebHID is not available in this browser (Chromium only).' };
  }
  if (typeof window !== 'undefined' && !window.isSecureContext) {
    return { ok: false, reason: 'WebHID needs a secure context: open Codescape over https or on localhost.' };
  }
  return { ok: true };
}

export type SpacePilotState = 'unsupported' | 'idle' | 'connecting' | 'connected';

/**
 * Owns one HID device, decodes its reports into a live six-axis sample and a
 * button mask, and tells listeners when the state changes. Values are the raw
 * integers from the device; `SpacePilotRig` turns them into camera motion.
 */
export class SpacePilotDevice {
  readonly sample: SpaceSample = { tx: 0, ty: 0, tz: 0, rx: 0, ry: 0, rz: 0 };
  buttons = 0;
  state: SpacePilotState = hidAvailable().ok ? 'idle' : 'unsupported';
  productName = '';
  onState: (s: SpacePilotState, productName: string) => void = () => {};
  /** Called on each rising edge of a button bit with the 1-based button index. */
  onButton: (index: number) => void = () => {};

  private device: HIDDevice | null = null;
  private readonly onReport = (evt: HIDInputReportEvent) => this.ingest(evt.reportId, evt.data);
  private readonly onDisconnect = (evt: HIDConnectionEvent) => {
    if (this.device && evt.device === this.device) this.drop('idle');
  };
  private readonly onConnect = (evt: HIDConnectionEvent) => {
    if (!this.device && SUPPORTED_VENDOR_IDS.includes(evt.device.vendorId)) void this.open(evt.device);
  };

  /** Reconnect to an already-paired device without a user gesture. */
  async resume(): Promise<boolean> {
    if (this.state === 'unsupported') return false;
    try {
      const devices = await navigator.hid.getDevices();
      const dev = devices.find((d) => SUPPORTED_VENDOR_IDS.includes(d.vendorId));
      navigator.hid.addEventListener('disconnect', this.onDisconnect);
      navigator.hid.addEventListener('connect', this.onConnect);
      if (!dev) return false;
      return this.open(dev);
    } catch {
      return false;
    }
  }

  /** Ask the browser for a device; must run inside a user gesture. */
  async request(): Promise<boolean> {
    if (this.state === 'unsupported') return false;
    this.set('connecting');
    try {
      const picked = await navigator.hid.requestDevice({ filters: SUPPORTED_VENDOR_IDS.map((vendorId) => ({ vendorId })) });
      if (picked.length === 0) {
        this.set('idle');
        return false;
      }
      navigator.hid.addEventListener('disconnect', this.onDisconnect);
      return this.open(picked[0]);
    } catch {
      this.set('idle');
      return false;
    }
  }

  async disconnect(): Promise<void> {
    const d = this.device;
    this.drop('idle');
    if (d && d.opened) {
      try {
        await d.close();
      } catch {
        /* already gone */
      }
    }
  }

  dispose(): void {
    void this.disconnect();
    if (this.state !== 'unsupported') {
      navigator.hid.removeEventListener('disconnect', this.onDisconnect);
      navigator.hid.removeEventListener('connect', this.onConnect);
    }
  }

  /** Feed a report directly (tests, or a replayed capture). */
  ingest(reportId: number, data: DataView): void {
    const r = parseReport(reportId, data);
    if (r.axes) Object.assign(this.sample, r.axes);
    if (r.buttons !== undefined) {
      const rising = r.buttons & ~this.buttons;
      this.buttons = r.buttons;
      for (let i = 0; i < 16; i++) if (rising & (1 << i)) this.onButton(i + 1);
    }
  }

  private async open(dev: HIDDevice): Promise<boolean> {
    try {
      if (!dev.opened) await dev.open();
      this.device = dev;
      this.productName = dev.productName || 'SpaceMouse';
      dev.addEventListener('inputreport', this.onReport);
      this.set('connected');
      return true;
    } catch {
      this.set('idle');
      return false;
    }
  }

  private drop(next: SpacePilotState): void {
    if (this.device) this.device.removeEventListener('inputreport', this.onReport);
    this.device = null;
    Object.assign(this.sample, { tx: 0, ty: 0, tz: 0, rx: 0, ry: 0, rz: 0 });
    this.buttons = 0;
    this.set(next);
  }

  private set(s: SpacePilotState): void {
    if (this.state === s) return;
    this.state = s;
    this.onState(s, this.productName);
  }
}

let singleton: SpacePilotDevice | null = null;

/** The page's one SpacePilot device (the HID handle outlives React remounts). */
export function spacePilot(): SpacePilotDevice {
  if (!singleton) singleton = new SpacePilotDevice();
  return singleton;
}
