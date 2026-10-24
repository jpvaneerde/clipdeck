import { memo, useRef, useState } from "react";
import { engine } from "../audio/host";
import { STATE_HEADER, STATE_TRACK_STRIDE } from "../audio/ids";
import {
  addSampleAsTrack,
  addScene,
  addTrack,
  deleteTrack,
  dropSampleOnSlot,
  duplicateScene,
  duplicateTrack,
  edit,
  ensureScenes,
  moveTrack,
  newClipAt,
  select,
  transport,
} from "../state/actions";
import { TRACK_COLORS } from "../state/defaults";
import { store, updateTrack, useStore } from "../state/store";
import type { Clip, SampleInfo, Track } from "../state/types";
import { Fader, fmt, Icon, Knob, Meter } from "./controls";
import { SAMPLE_MIME, useFrame, useTransport } from "./hooks";

const CLIP_MIME = "application/x-clipdeck-clip";

function readSample(e: React.DragEvent): SampleInfo | null {
  const raw = e.dataTransfer.getData(SAMPLE_MIME);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as SampleInfo;
  } catch {
    return null;
  }
}

function SlotProgress({ track }: { track: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useFrame(() => {
    const { local, length } = engine.trackBeat(track);
    if (ref.current) ref.current.style.width = length > 0 ? `${(local / length) * 100}%` : "0";
  });
  return <div className="progress" ref={ref} />;
}

const Slot = memo(function Slot({ ti, si, clip, color, kind, playing, queued, selected }: { ti: number; si: number; clip: Clip | null; color: string; kind: Track["kind"]; playing: boolean; queued: boolean; selected: boolean }) {
  const [drop, setDrop] = useState(false);
  const cls = ["slot", clip ? "filled" : "empty", playing && "playing", queued && "queued", selected && "selected", drop && "drop"].filter(Boolean).join(" ");
  return (
    <div
      className={cls}
      style={clip ? { background: clip.color ?? color } : undefined}
      onClick={() => select(ti, si)}
      onDoubleClick={() => !clip && newClipAt(ti, si)}
      draggable={!!clip}
      onDragStart={(e) => {
        e.dataTransfer.setData(CLIP_MIME, JSON.stringify({ ti, si }));
        e.dataTransfer.effectAllowed = "copyMove";
      }}
      onDragOver={(e) => {
        const t = e.dataTransfer.types;
        if (t.includes(SAMPLE_MIME) || t.includes(CLIP_MIME)) {
          e.preventDefault();
          setDrop(true);
        }
      }}
      onDragLeave={() => setDrop(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDrop(false);
        const s = readSample(e);
        if (s) return dropSampleOnSlot(s, ti, si);
        const raw = e.dataTransfer.getData(CLIP_MIME);
        if (!raw) return;
        const src = JSON.parse(raw) as { ti: number; si: number };
        if (src.ti === ti && src.si === si) return;
        const p = store.get().project;
        const srcClip = p.tracks[src.ti]?.clips[src.si];
        if (!srcClip) return;
        if (p.tracks[src.ti].kind !== kind && (kind === "audio") !== !!srcClip.audio) return;
        const copy = e.altKey || e.ctrlKey || e.metaKey;
        edit((p) => {
          let q = updateTrack(p, ti, (t) => ({ ...t, clips: t.clips.map((c, i) => (i === si ? structuredClone(srcClip) : c)) }));
          if (!copy) q = updateTrack(q, src.ti, (t) => ({ ...t, clips: t.clips.map((c, i) => (i === src.si ? null : c)) }));
          return ensureScenes(q, si + 1);
        });
        select(ti, si);
      }}
      title={clip ? `${clip.name} — drag to move, Alt-drag to copy` : kind === "audio" ? "Drop a loop here" : "Double-click to create a clip"}
    >
      <button
        className="launch"
        aria-label={clip ? `Launch ${clip.name}` : "Stop track"}
        onClick={(e) => {
          e.stopPropagation();
          select(ti, si);
          if (clip) transport.launchClip(ti, si);
          else transport.stopTrack(ti);
        }}
      >
        {clip ? Icon.play : Icon.stop}
      </button>
      {clip && <span className="clip-name">{clip.name}</span>}
      {playing && <SlotProgress track={ti} />}
    </div>
  );
});

function TrackMenu({ ti, onClose, onRename }: { ti: number; onClose: () => void; onRename: () => void }) {
  const n = useStore((s) => s.project.tracks.length);
  const item = (label: string, fn: () => void, disabled = false) => (
    <button className="btn ghost" style={{ justifyContent: "flex-start", width: "100%" }} disabled={disabled} onClick={() => { fn(); onClose(); }}>
      {label}
    </button>
  );
  return (
    <div style={{ position: "absolute", top: 34, left: 4, right: 4, zIndex: 10, background: "var(--panel-3)", border: "1px solid var(--line)", borderRadius: 4, padding: 4, display: "grid" }} onMouseLeave={onClose}>
      {item("Rename", onRename)}
      {item("Duplicate", () => duplicateTrack(ti))}
      {item("Move left", () => moveTrack(ti, -1), ti === 0)}
      {item("Move right", () => moveTrack(ti, 1), ti === n - 1)}
      {item("Next color", () =>
        edit((p) => updateTrack(p, ti, (t) => ({ ...t, color: TRACK_COLORS[(TRACK_COLORS.indexOf(t.color) + 1) % TRACK_COLORS.length] }))),
      )}
      {item("Delete track", () => deleteTrack(ti))}
    </div>
  );
}

function MixerStrip({ ti, t }: { ti: number; t: Track }) {
  const setMixer = (patch: Partial<Track["mixer"]>, group: string) => edit((p) => updateTrack(p, ti, (t) => ({ ...t, mixer: { ...t.mixer, ...patch } })), `mix-${ti}-${group}`);
  return (
    <div className="mixer-row">
      <div className="strip-controls">
        <button className={`toggle mute ${t.mixer.mute ? "on" : ""}`} aria-pressed={t.mixer.mute} title="Mute" onClick={() => setMixer({ mute: !t.mixer.mute }, "m")}>
          M
        </button>
        <button className={`toggle solo ${t.mixer.solo ? "on" : ""}`} aria-pressed={t.mixer.solo} title="Solo" onClick={() => setMixer({ solo: !t.mixer.solo }, "s")}>
          S
        </button>
        <button className={`toggle ${store.get().selection.track === ti ? "on" : ""}`} title="Show devices" onClick={() => select(ti, store.get().selection.slot)}>
          ◆
        </button>
      </div>
      <div className="knob-row">
        <Knob label="Pan" value={t.mixer.pan} min={-1} max={1} bipolar defaultValue={0} size={28} format={fmt.pan} onChange={(v) => setMixer({ pan: v }, "pan")} />
        <Knob label="Verb" value={t.mixer.sendA} min={0} max={1} defaultValue={0} size={28} format={fmt.pct} onChange={(v) => setMixer({ sendA: v }, "a")} />
        <Knob label="Delay" value={t.mixer.sendB} min={0} max={1} defaultValue={0} size={28} format={fmt.pct} onChange={(v) => setMixer({ sendB: v }, "b")} />
      </div>
      <div className="fader-wrap">
        <Fader value={t.mixer.volume} onChange={(v) => setMixer({ volume: v }, "vol")} />
        <Meter track={ti} />
      </div>
      <div className="db">{fmt.gainDb(t.mixer.volume)} dB</div>
    </div>
  );
}

function TrackColumn({ ti, t, scenes }: { ti: number; t: Track; scenes: number }) {
  const tr = useTransport();
  const sel = useStore((s) => s.selection);
  const [menu, setMenu] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const playing = tr.slots[ti];
  const queued = tr.queued[ti];
  return (
    <div className={`col ${sel.track === ti ? "selected" : ""}`} style={{ position: "relative" }}>
      <div className="track-head" onClick={() => select(ti, sel.track === ti ? sel.slot : null)} onContextMenu={(e) => { e.preventDefault(); setMenu(true); }}>
        <span className="swatch" style={{ background: t.color }} />
        {renaming ? (
          <input
            id={`track-name-${ti}`}
            aria-label="Track name"
            autoFocus
            value={t.name}
            onChange={(e) => edit((p) => updateTrack(p, ti, (x) => ({ ...x, name: e.target.value })), `name-${ti}`)}
            onBlur={() => setRenaming(false)}
            onKeyDown={(e) => (e.key === "Enter" || e.key === "Escape") && setRenaming(false)}
          />
        ) : (
          <span className="name" title="Double-click to rename" onDoubleClick={() => setRenaming(true)}>
            {t.name}
          </span>
        )}
        <button className="btn ghost icon" style={{ width: 20, height: 20 }} aria-label="Track menu" onClick={(e) => { e.stopPropagation(); setMenu(!menu); }}>
          ⋯
        </button>
      </div>
      {menu && <TrackMenu ti={ti} onClose={() => setMenu(false)} onRename={() => setRenaming(true)} />}
      {Array.from({ length: scenes }, (_, si) => (
        <Slot
          key={si}
          ti={ti}
          si={si}
          clip={t.clips[si] ?? null}
          color={t.color}
          kind={t.kind}
          playing={playing === si}
          queued={queued === si}
          selected={sel.track === ti && sel.slot === si}
        />
      ))}
      <div className="stop-row">
        <button className={`stop-btn ${queued === -2 ? "queued" : ""}`} aria-label={`Stop ${t.name}`} onClick={() => transport.stopTrack(ti)}>
          {Icon.stop}
        </button>
        <span className="kind" style={{ marginLeft: 8, fontSize: 10, color: "var(--dim)", textTransform: "uppercase", letterSpacing: "0.08em" }}>
          {t.kind}
        </span>
      </div>
      <MixerStrip ti={ti} t={t} />
    </div>
  );
}

function SceneColumn({ scenes }: { scenes: { name: string }[] }) {
  const tr = useTransport();
  const returns = useStore((s) => s.project.returns);
  const master = useStore((s) => s.project.master);
  const setRet = (k: keyof typeof returns, v: number) => edit((p) => ({ ...p, returns: { ...p.returns, [k]: v } }), `ret-${k}`);
  return (
    <div className="scene-col">
      <div className="track-head">
        <span>Scenes</span>
        <button className="btn ghost" style={{ height: 22, fontSize: 11 }} onClick={() => transport.stopAll()} title="Stop all clips at the next bar">
          Stop all
        </button>
      </div>
      {scenes.map((s, si) => (
        <div key={si} className={`scene ${tr.playing && tr.lastScene === si ? "last" : ""}`}>
          <button className="launch" aria-label={`Launch scene ${s.name}`} onClick={() => transport.launchScene(si)}>
            {Icon.play}
          </button>
          <input
            id={`scene-name-${si}`}
            aria-label="Scene name"
            value={s.name}
            onChange={(e) => edit((p) => ({ ...p, scenes: p.scenes.map((x, i) => (i === si ? { name: e.target.value } : x)) }), `scene-${si}`)}
          />
          <button className="btn ghost icon" style={{ width: 18, height: 18 }} aria-label="Duplicate scene" title="Duplicate scene" onClick={() => duplicateScene(si)}>
            {Icon.plus}
          </button>
        </div>
      ))}
      <div className="stop-row">
        <button className="btn ghost" style={{ height: 22, fontSize: 11 }} onClick={addScene}>
          + Scene
        </button>
      </div>
      <div className="mixer-row">
        <div className="strip-controls" style={{ fontSize: 11, color: "var(--muted)" }}>Master</div>
        <div className="knob-row">
          <Knob label="Verb" value={returns.reverbLevel} min={0} max={1.5} defaultValue={0.8} size={28} format={fmt.pct} onChange={(v) => setRet("reverbLevel", v)} />
          <Knob label="Size" value={returns.reverbSize} min={0} max={1} defaultValue={0.75} size={28} format={fmt.pct} onChange={(v) => setRet("reverbSize", v)} />
          <Knob label="Delay" value={returns.delayLevel} min={0} max={1.5} defaultValue={0.7} size={28} format={fmt.pct} onChange={(v) => setRet("delayLevel", v)} />
        </div>
        <div className="fader-wrap">
          <Fader value={master.volume} onChange={(v) => edit((p) => ({ ...p, master: { ...p.master, volume: v } }), "master-vol")} />
          <Meter track="master" />
        </div>
        <div className="db">{fmt.gainDb(master.volume)} dB</div>
      </div>
    </div>
  );
}

export function Session() {
  const tracks = useStore((s) => s.project.tracks);
  const scenes = useStore((s) => s.project.scenes);
  const [over, setOver] = useState(false);
  return (
    <div className="session">
      <div className="grid">
        {tracks.map((t, ti) => (
          <TrackColumn key={t.id} ti={ti} t={t} scenes={scenes.length} />
        ))}
        <div
          className="add-col"
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes(SAMPLE_MIME)) {
              e.preventDefault();
              setOver(true);
            }
          }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setOver(false);
            const s = readSample(e);
            if (s) addSampleAsTrack(s);
          }}
        >
          <button className="btn" onClick={() => addTrack("drums")}>{Icon.plus} Drum rack</button>
          <button className="btn" onClick={() => addTrack("synth")}>{Icon.plus} Synth</button>
          <button className="btn" onClick={() => addTrack("audio")}>{Icon.plus} Audio</button>
          <div className={`dropzone ${over ? "over" : ""}`} style={{ margin: 0 }}>
            Drop a loop here for a new track
          </div>
        </div>
        <SceneColumn scenes={scenes} />
      </div>
    </div>
  );
}

/** Peak of a track's level, for anything that wants a number. */
export function trackPeak(ti: number) {
  const o = STATE_HEADER + ti * STATE_TRACK_STRIDE + 4;
  return Math.max(engine.state[o], engine.state[o + 1]);
}
