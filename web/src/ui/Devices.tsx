import { useState } from "react";
import { engine } from "../audio/host";
import { SP, type SynthParam } from "../audio/ids";
import { assignPad, edit } from "../state/actions";
import { presetParams, PRESET_GROUPS, presetsIn } from "../state/presets";
import { updateTrack, useStore } from "../state/store";
import type { Pad, SampleInfo, Track, TrackFx } from "../state/types";
import { fmt, Knob, Seg } from "./controls";
import { SAMPLE_MIME } from "./hooks";

// ------------------------------------------------------------ synth

const WAVES: [number, string][] = [
  [0, "Saw"],
  [1, "Sqr"],
  [2, "Tri"],
  [3, "Sin"],
];

export function SynthDevice({ ti, t }: { ti: number; t: Track }) {
  const p = t.synth;
  const set = (name: SynthParam, v: number) =>
    edit(
      (proj) =>
        updateTrack(proj, ti, (x) => {
          const synth = x.synth.slice();
          synth[SP[name]] = v;
          return { ...x, synth, preset: null };
        }),
      `synth-${ti}-${name}`,
    );
  const k = (name: SynthParam, label: string, min: number, max: number, extra: Partial<Parameters<typeof Knob>[0]> = {}) => (
    <Knob label={label} value={p[SP[name]]} min={min} max={max} onChange={(v) => set(name, v)} {...extra} />
  );
  return (
    <div className="device">
      <h4>
        Synth
        <select
          id={`preset-${ti}`}
          aria-label="Preset"
          value={t.preset ?? ""}
          onChange={(e) => {
            const name = e.target.value;
            if (name) edit((proj) => updateTrack(proj, ti, (x) => ({ ...x, synth: presetParams(name), preset: name })));
          }}
          style={{ background: "var(--bg)", border: "1px solid var(--line)", borderRadius: 4, height: 24, textTransform: "none", letterSpacing: 0, color: "var(--text)" }}
        >
          <option value="">{t.preset ? "Choose preset" : "Custom sound"}</option>
          {PRESET_GROUPS.map((g) => (
            <optgroup key={g} label={g}>
              {presetsIn(g).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </h4>
      <div className="subgroup">
        <span>Oscillators</span>
        <div className="row" style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <Seg value={Math.round(p[SP.osc1Wave])} options={WAVES} onChange={(v) => set("osc1Wave", v)} />
          <Seg value={Math.round(p[SP.osc2Wave])} options={WAVES} onChange={(v) => set("osc2Wave", v)} />
        </div>
        <div className="knobs">
          {k("osc2Semi", "Osc2 pitch", -24, 24, { step: 1, format: fmt.semi, bipolar: true, defaultValue: 0 })}
          {k("osc2Detune", "Detune", 0, 50, { format: (v) => `${v.toFixed(0)}c`, defaultValue: 8 })}
          {k("oscMix", "Mix", 0, 1, { format: fmt.pct, defaultValue: 0.5 })}
          {k("sub", "Sub", 0, 1, { format: fmt.pct, defaultValue: 0 })}
          {k("noise", "Noise", 0, 1, { format: fmt.pct, defaultValue: 0 })}
          {k("spread", "Width", 0, 1, { format: fmt.pct, defaultValue: 0.3 })}
        </div>
      </div>
      <div className="subgroup">
        <span>Filter</span>
        <div className="knobs">
          {k("cutoff", "Cutoff", 30, 18000, { curve: "log", format: fmt.hz, defaultValue: 2400 })}
          {k("reso", "Reso", 0, 1, { format: fmt.pct, defaultValue: 0.2 })}
          {k("filterEnv", "Env amt", -4, 8, { format: (v) => `${v.toFixed(1)}oct`, bipolar: true, defaultValue: 2 })}
          {k("keyTrack", "Key trk", 0, 1, { format: fmt.pct, defaultValue: 0.5 })}
          {k("drive", "Drive", 0, 1, { format: fmt.pct, defaultValue: 0 })}
        </div>
      </div>
      <div className="subgroup">
        <span>Amp envelope</span>
        <div className="knobs">
          {k("ampA", "Attack", 0.0005, 4, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.005 })}
          {k("ampD", "Decay", 0.005, 4, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.3 })}
          {k("ampS", "Sustain", 0, 1, { format: fmt.pct, defaultValue: 0.7 })}
          {k("ampR", "Release", 0.005, 6, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.25 })}
        </div>
      </div>
      <div className="subgroup">
        <span>Filter envelope</span>
        <div className="knobs">
          {k("filtA", "Attack", 0.0005, 4, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.002 })}
          {k("filtD", "Decay", 0.005, 4, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.25 })}
          {k("filtS", "Sustain", 0, 1, { format: fmt.pct, defaultValue: 0.2 })}
          {k("filtR", "Release", 0.005, 6, { curve: { pow: 3 }, format: fmt.sec, defaultValue: 0.3 })}
        </div>
      </div>
      <div className="subgroup">
        <span>LFO and voice</span>
        <div className="knobs">
          {k("lfoRate", "LFO rate", 0.05, 20, { curve: "log", format: (v) => `${v.toFixed(2)}Hz`, defaultValue: 4 })}
          {k("lfoCutoff", "→ Cutoff", 0, 4, { format: (v) => `${v.toFixed(1)}oct`, defaultValue: 0 })}
          {k("lfoPitch", "Vibrato", 0, 1, { format: (v) => `${v.toFixed(2)}st`, defaultValue: 0 })}
          {k("glide", "Glide", 0, 0.5, { curve: { pow: 2 }, format: fmt.sec, defaultValue: 0 })}
          {k("velSens", "Vel sens", 0, 1, { format: fmt.pct, defaultValue: 0.5 })}
          {k("level", "Level", 0, 2, { format: fmt.gainDb, defaultValue: 0.8 })}
          <div className="knob">
            <Seg value={p[SP.mono] >= 0.5 ? 1 : 0} options={[[0, "Poly"], [1, "Mono"]]} onChange={(v) => set("mono", v)} />
            <span className="label">Voices</span>
          </div>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------ drum rack

export function DrumDevice({ ti, t, selectedPad, onSelectPad }: { ti: number; t: Track; selectedPad: number; onSelectPad: (p: number) => void }) {
  const [over, setOver] = useState(-1);
  const pad = t.pads[selectedPad];
  const setPad = (patch: Partial<Pad>, group: string) =>
    edit((proj) => updateTrack(proj, ti, (x) => ({ ...x, pads: x.pads.map((p, i) => (i === selectedPad ? { ...p, ...patch } : p)) })), `pad-${ti}-${selectedPad}-${group}`);
  const hit = (i: number) => {
    engine.resume();
    engine.cmd("note_on", ti, 36 + i, 0.9);
  };
  return (
    <div className="device">
      <h4>Drum rack</h4>
      <div className="pads">
        {[12, 13, 14, 15, 8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3].map((i) => {
          const p = t.pads[i];
          return (
            <button
              key={i}
              className={`pad ${selectedPad === i ? "sel" : ""} ${over === i ? "drop" : ""}`}
              onPointerDown={() => {
                onSelectPad(i);
                hit(i);
              }}
              onDragOver={(e) => {
                if (e.dataTransfer.types.includes(SAMPLE_MIME)) {
                  e.preventDefault();
                  setOver(i);
                }
              }}
              onDragLeave={() => setOver(-1)}
              onDrop={(e) => {
                e.preventDefault();
                setOver(-1);
                const raw = e.dataTransfer.getData(SAMPLE_MIME);
                if (!raw) return;
                assignPad(ti, i, JSON.parse(raw) as SampleInfo);
                onSelectPad(i);
              }}
              title={p.sample ? `${p.name}: ${p.sample}` : "Drop a one-shot from the browser"}
            >
              <span className="pn">{i + 1}</span>
              <span className="ps">{p.sample ? p.name : "empty"}</span>
            </button>
          );
        })}
      </div>
      {pad && (
        <div className="subgroup">
          <span>
            Pad {selectedPad + 1}
            {pad.sample ? ` · ${pad.sample.split("/").pop()}` : " · drop a one-shot here or on a pad"}
          </span>
          <div className="knobs">
            <Knob label="Gain" value={pad.gain} min={0} max={2} defaultValue={1} format={fmt.gainDb} onChange={(v) => setPad({ gain: v }, "gain")} />
            <Knob label="Pitch" value={pad.pitch} min={-24} max={24} step={1} bipolar defaultValue={0} format={fmt.semi} onChange={(v) => setPad({ pitch: v }, "pitch")} />
            <Knob label="Pan" value={pad.pan} min={-1} max={1} bipolar defaultValue={0} format={fmt.pan} onChange={(v) => setPad({ pan: v }, "pan")} />
            <Knob label="Decay" value={pad.decay} min={0} max={2} curve={{ pow: 2 }} defaultValue={0} format={(v) => (v <= 0.001 ? "full" : fmt.sec(v))} onChange={(v) => setPad({ decay: v }, "decay")} />
            <div className="knob">
              <Seg value={pad.choke} options={[[0, "–"], [1, "A"], [2, "B"]]} onChange={(v) => setPad({ choke: v }, "choke")} />
              <span className="label">Choke</span>
            </div>
            {pad.sample && (
              <button className="btn ghost" onClick={() => setPad({ sample: null, name: `Pad ${selectedPad + 1}` }, "clear")}>
                Clear pad
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------ track inserts

export function FxDevice({ ti, t }: { ti: number; t: Track }) {
  const tracks = useStore((s) => s.project.tracks);
  const fx = t.fx;
  const set = (patch: Partial<TrackFx>, group: string) => edit((p) => updateTrack(p, ti, (x) => ({ ...x, fx: { ...x.fx, ...patch } })), `fx-${ti}-${group}`);
  return (
    <div className="device">
      <h4>Track effects</h4>
      <div className="subgroup">
        <span>Filter</span>
        <div className="knobs">
          <div className="knob">
            <Seg value={fx.filterType} options={[[0, "Off"], [1, "LP"], [2, "HP"], [3, "BP"]]} onChange={(v) => set({ filterType: v }, "ft")} />
            <span className="label">Type</span>
          </div>
          <Knob label="Cutoff" value={fx.cutoff} min={20} max={20000} curve="log" defaultValue={fx.filterType === 2 ? 20 : 20000} format={fmt.hz} onChange={(v) => set({ cutoff: v }, "fc")} />
          <Knob label="Reso" value={fx.reso} min={0} max={1} defaultValue={0.1} format={fmt.pct} onChange={(v) => set({ reso: v }, "fr")} />
          <Knob label="Drive" value={fx.drive} min={0} max={1} defaultValue={0} format={fmt.pct} onChange={(v) => set({ drive: v }, "dr")} />
        </div>
      </div>
      <div className="subgroup">
        <span>Compressor</span>
        <div className="knobs">
          <Knob label="Thresh" value={fx.compThreshold} min={-48} max={0} defaultValue={0} format={fmt.db} onChange={(v) => set({ compThreshold: v }, "ct")} />
          <Knob label="Ratio" value={fx.compRatio} min={1} max={20} curve={{ pow: 2 }} defaultValue={1} format={fmt.ratio} onChange={(v) => set({ compRatio: v }, "cr")} />
          <Knob label="Attack" value={fx.compAttack} min={0.1} max={100} curve="log" defaultValue={10} format={fmt.ms} onChange={(v) => set({ compAttack: v }, "ca")} />
          <Knob label="Release" value={fx.compRelease} min={10} max={1000} curve="log" defaultValue={120} format={fmt.ms} onChange={(v) => set({ compRelease: v }, "cl")} />
          <Knob label="Makeup" value={fx.compMakeup} min={0} max={24} defaultValue={0} format={fmt.db} onChange={(v) => set({ compMakeup: v }, "cm")} />
        </div>
      </div>
      <div className="subgroup">
        <span>Sidechain duck</span>
        <div className="knobs" style={{ alignItems: "center" }}>
          <div className="field">
            <label htmlFor={`duck-src-${ti}`}>From</label>
            <select id={`duck-src-${ti}`} value={fx.duckSource} onChange={(e) => set({ duckSource: Number(e.target.value), duckAmount: fx.duckAmount || 0.5 }, "ds")} style={{ width: 110 }}>
              <option value={-1}>Off</option>
              {tracks.map((x, i) =>
                i === ti ? null : (
                  <option key={x.id} value={i}>
                    {x.name}
                  </option>
                ),
              )}
            </select>
          </div>
          <Knob label="Amount" value={fx.duckAmount} min={0} max={1} defaultValue={0.5} format={fmt.pct} onChange={(v) => set({ duckAmount: v }, "da")} />
          <Knob label="Recover" value={fx.duckRelease} min={20} max={800} curve="log" defaultValue={180} format={fmt.ms} onChange={(v) => set({ duckRelease: v }, "dl")} />
        </div>
      </div>
    </div>
  );
}

export function ReturnsDevice() {
  const r = useStore((s) => s.project.returns);
  const set = (k: keyof typeof r, v: number | boolean) => edit((p) => ({ ...p, returns: { ...p.returns, [k]: v } }), `ret-${k}`);
  return (
    <div className="device">
      <h4>Return effects</h4>
      <div className="subgroup">
        <span>Reverb (send A)</span>
        <div className="knobs">
          <Knob label="Size" value={r.reverbSize} min={0} max={1} defaultValue={0.75} format={fmt.pct} onChange={(v) => set("reverbSize", v)} />
          <Knob label="Damp" value={r.reverbDamp} min={0} max={1} defaultValue={0.4} format={fmt.pct} onChange={(v) => set("reverbDamp", v)} />
          <Knob label="Width" value={r.reverbWidth} min={0} max={1} defaultValue={1} format={fmt.pct} onChange={(v) => set("reverbWidth", v)} />
          <Knob label="Level" value={r.reverbLevel} min={0} max={1.5} defaultValue={0.8} format={fmt.pct} onChange={(v) => set("reverbLevel", v)} />
        </div>
      </div>
      <div className="subgroup">
        <span>Delay (send B)</span>
        <div className="knobs">
          <div className="knob">
            <select
              id="delay-time"
              aria-label="Delay time"
              value={r.delayBeats}
              onChange={(e) => set("delayBeats", Number(e.target.value))}
              style={{ background: "var(--bg)", border: "1px solid var(--line)", borderRadius: 4, height: 24 }}
            >
              {[
                [0.25, "1/16"],
                [0.5, "1/8"],
                [0.75, "1/8 dotted"],
                [1, "1/4"],
                [1.5, "1/4 dotted"],
                [2, "1/2"],
                [1 / 3, "1/8 triplet"],
                [2 / 3, "1/4 triplet"],
              ].map(([v, l]) => (
                <option key={String(l)} value={v}>
                  {l}
                </option>
              ))}
            </select>
            <span className="label">Time</span>
          </div>
          <Knob label="Feedback" value={r.delayFeedback} min={0} max={0.95} defaultValue={0.4} format={fmt.pct} onChange={(v) => set("delayFeedback", v)} />
          <Knob label="Tone" value={r.delayTone} min={300} max={16000} curve="log" defaultValue={4500} format={fmt.hz} onChange={(v) => set("delayTone", v)} />
          <div className="knob">
            <Seg value={r.delayPingPong ? 1 : 0} options={[[1, "Ping-pong"], [0, "Stereo"]]} onChange={(v) => set("delayPingPong", v === 1)} />
            <span className="label">Mode</span>
          </div>
          <Knob label="Level" value={r.delayLevel} min={0} max={1.5} defaultValue={0.7} format={fmt.pct} onChange={(v) => set("delayLevel", v)} />
        </div>
      </div>
    </div>
  );
}
