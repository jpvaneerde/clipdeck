// Main-thread side of the audio engine: AudioContext, worklet, sample loading,
// and a compact transport snapshot for React.

import workletUrl from "./worklet.ts?worker&url";
import { MAX_TRACKS, STATE_HEADER, STATE_TRACK_STRIDE } from "./ids";
import { api, signIn } from "../state/api";
import type { Note } from "../state/types";

export interface Transport {
  playing: boolean;
  lastScene: number;
  /** per track: playing slot (-1 none) */
  slots: number[];
  /** per track: queued slot (-1 none, -2 stop) */
  queued: number[];
}

type Cmd = (string | number)[];

const EMPTY_STATE = new Float32Array(STATE_HEADER + MAX_TRACKS * STATE_TRACK_STRIDE);

class EngineHost {
  ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  private pending: Cmd[] = [];
  private flushScheduled = false;
  private samples = new Map<string, Promise<number>>();
  private nextHandle = 1;
  /** Latest raw engine state (see crates/engine STATE_* layout). */
  state: Float32Array = EMPTY_STATE;
  private stateTime = 0;
  transport: Transport = { playing: false, lastScene: 0, slots: Array(MAX_TRACKS).fill(-1), queued: Array(MAX_TRACKS).fill(-1) };
  private transportListeners = new Set<() => void>();
  status: "idle" | "loading" | "ready" | "error" = "idle";
  error = "";
  private statusListeners = new Set<() => void>();
  private ready!: Promise<void>;

  /** Create the context and load the engine. Audio stays suspended until `resume`. */
  init(): Promise<void> {
    if (this.ready) return this.ready;
    this.setStatus("loading");
    this.ready = (async () => {
      const ctx = new AudioContext({ latencyHint: "interactive" });
      this.ctx = ctx;
      await ctx.audioWorklet.addModule(workletUrl);
      const node = new AudioWorkletNode(ctx, "clipdeck-engine", { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] });
      node.connect(ctx.destination);
      this.node = node;
      const ready = new Promise<void>((resolve, reject) => {
        node.port.onmessage = (e) => {
          const m = e.data;
          if (m.t === "state") this.onState(m.s);
          else if (m.t === "ready") resolve();
          else if (m.t === "error") reject(new Error(m.message));
        };
      });
      const res = await fetch("/engine.wasm");
      if (!res.ok) throw new Error(`engine.wasm: ${res.status}`);
      const wasm = await res.arrayBuffer();
      node.port.postMessage({ t: "init", wasm }, [wasm]);
      await ready;
      this.setStatus("ready");
      this.flush();
    })().catch((err) => {
      this.error = err instanceof Error ? err.message : String(err);
      this.setStatus("error");
      throw err;
    });
    return this.ready;
  }

  async resume() {
    await this.init().catch(() => undefined);
    if (this.ctx && this.ctx.state !== "running") await this.ctx.resume();
    this.emitStatus();
  }

  get running() {
    return this.ctx?.state === "running";
  }

  private setStatus(s: EngineHost["status"]) {
    this.status = s;
    this.emitStatus();
  }

  private emitStatus() {
    for (const l of this.statusListeners) l();
  }

  onStatus(l: () => void) {
    this.statusListeners.add(l);
    return () => this.statusListeners.delete(l);
  }

  // ---------------------------------------------------------------- commands

  cmd(...c: Cmd) {
    this.pending.push(c);
    if (!this.flushScheduled) {
      this.flushScheduled = true;
      queueMicrotask(() => this.flush());
    }
  }

  private flush() {
    this.flushScheduled = false;
    if (!this.node || this.pending.length === 0) return;
    const cmds = this.pending;
    this.pending = [];
    this.node.port.postMessage({ t: "batch", cmds });
  }

  setClipNotes(track: number, slot: number, length: number, notes: Note[]) {
    this.flush();
    const arr = new Float32Array(notes.length * 4);
    notes.forEach((n, i) => arr.set([n.s, n.l, n.p, n.v], i * 4));
    this.post({ t: "notes", track, slot, length, notes: arr }, [arr.buffer]);
  }

  private post(msg: unknown, transfer: Transferable[] = []) {
    // Before the worklet exists, park structured messages behind the batch queue.
    if (!this.node) {
      this.init()
        .then(() => this.node!.port.postMessage(msg, transfer))
        .catch(() => undefined);
      return;
    }
    this.node.port.postMessage(msg, transfer);
  }

  /** Fetch, decode and upload a library sample; resolves to its engine handle (-1 on failure). */
  loadSample(id: string): Promise<number> {
    const existing = this.samples.get(id);
    if (existing) return existing;
    const p = (async () => {
      await this.init();
      const res = await fetch(api.audioUrl(id));
      if (res.status === 401) signIn();
      if (!res.ok) throw new Error(`${id}: ${res.status}`);
      const buf = await this.ctx!.decodeAudioData(await res.arrayBuffer());
      const ch = Math.min(2, buf.numberOfChannels);
      const frames = buf.length;
      const data = new Float32Array(frames * ch);
      const c0 = buf.getChannelData(0);
      if (ch === 1) data.set(c0);
      else {
        const c1 = buf.getChannelData(1);
        for (let i = 0; i < frames; i++) {
          data[i * 2] = c0[i];
          data[i * 2 + 1] = c1[i];
        }
      }
      const handle = this.nextHandle++;
      this.flush();
      this.post({ t: "sample", handle, data, frames, channels: ch, rate: buf.sampleRate }, [data.buffer]);
      return handle;
    })().catch((err) => {
      console.warn("sample load failed", err);
      this.samples.delete(id);
      return -1;
    });
    this.samples.set(id, p);
    return p;
  }

  async preview(id: string, gain = 0.8) {
    await this.resume();
    const h = await this.loadSample(id);
    if (h >= 0) this.cmd("preview", h, gain);
  }

  stopPreview() {
    this.cmd("stop_preview");
  }

  // ---------------------------------------------------------------- state

  private onState(s: Float32Array) {
    this.state = s;
    this.stateTime = this.ctx?.currentTime ?? 0;
    const t = this.transport;
    const playing = s[1] > 0.5;
    const lastScene = s[7] | 0;
    let changed = playing !== t.playing || lastScene !== t.lastScene;
    const slots = t.slots;
    const queued = t.queued;
    for (let i = 0; i < MAX_TRACKS; i++) {
      const o = STATE_HEADER + i * STATE_TRACK_STRIDE;
      if (slots[i] !== s[o] || queued[i] !== s[o + 1]) {
        changed = true;
        break;
      }
    }
    if (changed) {
      this.transport = {
        playing,
        lastScene,
        slots: Array.from({ length: MAX_TRACKS }, (_, i) => s[STATE_HEADER + i * STATE_TRACK_STRIDE]),
        queued: Array.from({ length: MAX_TRACKS }, (_, i) => s[STATE_HEADER + i * STATE_TRACK_STRIDE + 1]),
      };
      for (const l of this.transportListeners) l();
    }
  }

  onTransport(l: () => void) {
    this.transportListeners.add(l);
    return () => this.transportListeners.delete(l);
  }

  /** Current beat, extrapolated from the last state message. */
  beatNow(): number {
    const s = this.state;
    if (s[1] < 0.5 || !this.ctx) return s[0];
    return s[0] + (this.ctx.currentTime - this.stateTime) * (s[2] / 60);
  }

  /** Clip-local beat of a track, extrapolated. */
  trackBeat(track: number): { local: number; length: number } {
    const o = STATE_HEADER + track * STATE_TRACK_STRIDE;
    const s = this.state;
    const len = s[o + 3];
    if (len <= 0) return { local: 0, length: 0 };
    const dt = s[1] > 0.5 && this.ctx ? (this.ctx.currentTime - this.stateTime) * (s[2] / 60) : 0;
    return { local: (s[o + 2] + dt) % len, length: len };
  }
}

export const engine = new EngineHost();
