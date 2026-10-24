// AudioWorklet processor hosting the Rust/WebAssembly engine.
// Self-contained on purpose: AudioWorkletGlobalScope can't import app modules.

declare const sampleRate: number;
declare function registerProcessor(name: string, ctor: unknown): void;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
  constructor();
}

type Fn = (...args: number[]) => number;
interface Exports {
  memory: WebAssembly.Memory;
  [name: string]: Fn | WebAssembly.Memory;
}

type Msg =
  | { t: "init"; wasm: ArrayBuffer }
  | { t: "batch"; cmds: (string | number)[][] }
  | { t: "notes"; track: number; slot: number; length: number; notes: Float32Array }
  | { t: "sample"; handle: number; data: Float32Array; frames: number; channels: number; rate: number };

class EngineProcessor extends AudioWorkletProcessor {
  private ex: Exports | null = null;
  private queue: Msg[] = [];
  private quanta = 0;
  private stateEvery: number;

  constructor() {
    super();
    this.stateEvery = Math.max(1, Math.round(sampleRate / 128 / 40));
    this.port.onmessage = (e: MessageEvent<Msg>) => this.onMessage(e.data);
  }

  private fn(name: string): Fn {
    return this.ex![name] as Fn;
  }

  private async init(bytes: ArrayBuffer) {
    const mod = await WebAssembly.compile(bytes);
    // The engine imports nothing, but stub anything unexpected rather than fail.
    const imports: Record<string, Record<string, unknown>> = {};
    for (const imp of WebAssembly.Module.imports(mod)) {
      (imports[imp.module] ??= {})[imp.name] = imp.kind === "function" ? () => 0 : undefined;
    }
    const inst = await WebAssembly.instantiate(mod, imports as WebAssembly.Imports);
    this.ex = inst.exports as unknown as Exports;
    this.fn("cd_init")(sampleRate);
    const pending = this.queue;
    this.queue = [];
    for (const m of pending) this.apply(m);
    this.port.postMessage({ t: "ready", sampleRate });
  }

  private onMessage(m: Msg) {
    if (m.t === "init") {
      this.init(m.wasm).catch((err) => this.port.postMessage({ t: "error", message: String(err) }));
      return;
    }
    if (!this.ex) {
      this.queue.push(m);
      return;
    }
    this.apply(m);
  }

  private view(ptr: number, len: number) {
    return new Float32Array(this.ex!.memory.buffer, ptr, len);
  }

  private apply(m: Msg) {
    switch (m.t) {
      case "batch":
        for (const c of m.cmds) {
          const f = this.ex![`cd_${c[0]}`] as Fn | undefined;
          if (typeof f === "function") f(...(c.slice(1) as number[]));
        }
        break;
      case "notes": {
        const n = Math.max(1, m.notes.length);
        const ptr = this.fn("cd_alloc")(n);
        this.view(ptr, m.notes.length).set(m.notes);
        this.fn("cd_set_clip_notes")(m.track, m.slot, m.length, ptr, m.notes.length / 4);
        this.fn("cd_free")(ptr, n);
        break;
      }
      case "sample": {
        const n = Math.max(1, m.data.length);
        const ptr = this.fn("cd_alloc")(n);
        this.view(ptr, m.data.length).set(m.data);
        this.fn("cd_load_sample")(m.handle, ptr, m.frames, m.channels, m.rate);
        this.fn("cd_free")(ptr, n);
        break;
      }
    }
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]) {
    const out = outputs[0];
    if (!this.ex || !out || out.length === 0) return true;
    const n = out[0].length;
    this.fn("cd_process")(n);
    out[0].set(this.view(this.fn("cd_out_l")(), n));
    if (out[1]) out[1].set(this.view(this.fn("cd_out_r")(), n));
    if (++this.quanta % this.stateEvery === 0) {
      const s = this.view(this.fn("cd_state")(), this.fn("cd_state_len")()).slice();
      this.port.postMessage({ t: "state", s }, [s.buffer]);
    }
    return true;
  }
}

registerProcessor("clipdeck-engine", EngineProcessor);
