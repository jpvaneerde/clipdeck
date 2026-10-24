import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { engine } from "../audio/host";
import { setClip, updateClip } from "../state/actions";
import { api } from "../state/api";
import { noteName } from "../state/defaults";
import { useStore } from "../state/store";
import type { Clip, Note, SampleInfo, Track } from "../state/types";
import { fmt, Knob, Waveform } from "./controls";
import { useFrame, useTransport } from "./hooks";

const LENGTHS = [1, 2, 4, 8, 16, 32, 64];
const SNAPS: [number, string][] = [
  [1, "1/4"],
  [0.5, "1/8"],
  [0.25, "1/16"],
  [0.125, "1/32"],
  [1 / 3, "1/4T"],
  [1 / 6, "1/8T"],
];

const LOW = 24;
const HIGH = 96;
const ROW = 14;

function audition(track: number, pitch: number, vel = 0.8) {
  engine.cmd("note_on", track, pitch, vel);
  setTimeout(() => engine.cmd("note_off", track, pitch), 220);
}

function Playhead({ ti, slot, pxPerBeat, offset = 0 }: { ti: number; slot: number; pxPerBeat: number; offset?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const tr = useTransport();
  const active = tr.slots[ti] === slot;
  useFrame(() => {
    if (!ref.current) return;
    if (!active) {
      ref.current.style.display = "none";
      return;
    }
    const { local, length } = engine.trackBeat(ti);
    ref.current.style.display = "block";
    // pxPerBeat 0 means "position as a fraction of the clip".
    ref.current.style.left = pxPerBeat > 0 ? `${offset + local * pxPerBeat}px` : `${length > 0 ? (local / length) * 100 : 0}%`;
  });
  return <div className="playhead" ref={ref} />;
}

function ClipHeader({ ti, slot, clip, children }: { ti: number; slot: number; clip: Clip; children?: React.ReactNode }) {
  return (
    <div className="pane-head">
      <input
        id="clip-name"
        aria-label="Clip name"
        value={clip.name}
        onChange={(e) => updateClip(ti, slot, (c) => ({ ...c, name: e.target.value }), "clip-name")}
        style={{ background: "transparent", border: 0, outline: 0, fontWeight: 600, width: 120 }}
      />
      <div className="field">
        <label htmlFor="clip-len">Length</label>
        <select
          id="clip-len"
          value={clip.length}
          onChange={(e) => updateClip(ti, slot, (c) => ({ ...c, length: Number(e.target.value) }))}
        >
          {[...new Set([...LENGTHS, clip.length])].sort((a, b) => a - b).map((l) => (
            <option key={l} value={l}>
              {l % 4 === 0 ? `${l / 4} bar${l > 4 ? "s" : ""}` : `${l} beat${l > 1 ? "s" : ""}`}
            </option>
          ))}
        </select>
      </div>
      {children}
      <span className="spacer" />
      <button className="btn ghost" onClick={() => setClip(ti, slot, null)} title="Delete clip (Del)">
        Delete
      </button>
    </div>
  );
}

// ------------------------------------------------------------ piano roll

type Drag =
  | { kind: "move"; idx: number[]; startBeat: number; startPitch: number; orig: Note[] }
  | { kind: "resize"; idx: number[]; startBeat: number; orig: Note[] };

export function PianoRoll({ ti, slot, clip, track }: { ti: number; slot: number; clip: Clip; track: Track }) {
  const [snap, setSnap] = useState(0.25);
  const [zoom, setZoom] = useState(48);
  const [sel, setSel] = useState<Set<number>>(new Set());
  const [lastLen, setLastLen] = useState(0.25);
  const bodyRef = useRef<HTMLDivElement>(null);
  const lanesRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const pitches = useMemo(() => Array.from({ length: HIGH - LOW + 1 }, (_, i) => HIGH - i), []);
  const width = clip.length * zoom;

  // Scroll so the notes (or middle C) are visible when a clip opens.
  useLayoutEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    const ps = clip.notes.map((n) => n.p);
    const center = ps.length ? (Math.max(...ps) + Math.min(...ps)) / 2 : 60;
    body.scrollTop = (HIGH - center) * ROW - body.clientHeight / 2;
    setSel(new Set());
  }, [ti, slot]);

  const at = (e: { clientX: number; clientY: number }) => {
    const r = lanesRef.current!.getBoundingClientRect();
    const beat = (e.clientX - r.left) / zoom;
    const pitch = HIGH - Math.floor((e.clientY - r.top) / ROW);
    return { beat, pitch };
  };
  const q = (b: number) => Math.round(b / snap) * snap;
  const qFloor = (b: number) => Math.floor(b / snap + 1e-6) * snap;

  const setNotes = (notes: Note[], group?: string) => updateClip(ti, slot, (c) => ({ ...c, notes }), group);

  const onDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    const noteEl = target.closest("[data-note]") as HTMLElement | null;
    lanesRef.current!.setPointerCapture(e.pointerId);
    const { beat, pitch } = at(e);
    if (noteEl) {
      const idx = Number(noteEl.dataset.note);
      let next = sel;
      if (e.shiftKey) {
        next = new Set(sel);
        next.has(idx) ? next.delete(idx) : next.add(idx);
      } else if (!sel.has(idx)) next = new Set([idx]);
      setSel(next);
      const ids = [...next];
      if (target.classList.contains("resize")) drag.current = { kind: "resize", idx: ids, startBeat: beat, orig: clip.notes };
      else {
        drag.current = { kind: "move", idx: ids, startBeat: beat, startPitch: pitch, orig: clip.notes };
        audition(ti, clip.notes[idx].p, clip.notes[idx].v);
      }
      return;
    }
    // Empty space: add a note and start stretching it.
    if (pitch < LOW || pitch > HIGH || beat < 0 || beat >= clip.length) return;
    const n: Note = { s: qFloor(beat), l: lastLen, p: pitch, v: 0.8 };
    const notes = [...clip.notes, n];
    setNotes(notes, "roll-add");
    const idx = notes.length - 1;
    setSel(new Set([idx]));
    drag.current = { kind: "resize", idx: [idx], startBeat: n.s + n.l, orig: notes };
    audition(ti, pitch);
  };

  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const { beat, pitch } = at(e);
    if (d.kind === "move") {
      const db = q(beat - d.startBeat);
      const dp = pitch - d.startPitch;
      const notes = d.orig.map((n, i) =>
        d.idx.includes(i) ? { ...n, s: Math.min(clip.length - snap, Math.max(0, n.s + db)), p: Math.min(HIGH, Math.max(LOW, n.p + dp)) } : n,
      );
      setNotes(notes, "roll-drag");
    } else {
      const dl = q(beat - d.startBeat);
      const notes = d.orig.map((n, i) => (d.idx.includes(i) ? { ...n, l: Math.max(snap, Math.min(clip.length - n.s, n.l + dl)) } : n));
      setNotes(notes, "roll-drag");
      if (d.idx.length === 1) setLastLen(notes[d.idx[0]].l);
    }
  };

  const deleteSel = () => {
    if (!sel.size) return;
    setNotes(clip.notes.filter((_, i) => !sel.has(i)));
    setSel(new Set());
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      e.stopPropagation();
      deleteSel();
    } else if ((e.key === "a" || e.key === "A") && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      setSel(new Set(clip.notes.map((_, i) => i)));
    } else if ((e.key === "ArrowUp" || e.key === "ArrowDown") && sel.size) {
      e.preventDefault();
      e.stopPropagation();
      const d = (e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1);
      setNotes(clip.notes.map((n, i) => (sel.has(i) ? { ...n, p: Math.min(HIGH, Math.max(LOW, n.p + d)) } : n)), "roll-transpose");
    }
  };

  const selVel = sel.size ? [...sel].reduce((a, i) => a + (clip.notes[i]?.v ?? 0), 0) / sel.size : 0.8;

  return (
    <>
      <ClipHeader ti={ti} slot={slot} clip={clip}>
        <div className="field">
          <label htmlFor="snap">Grid</label>
          <select id="snap" value={snap} onChange={(e) => setSnap(Number(e.target.value))}>
            {SNAPS.map(([v, l]) => (
              <option key={l} value={v}>
                {l}
              </option>
            ))}
          </select>
        </div>
        <div className="field hide-narrow">
          <label htmlFor="zoom">Zoom</label>
          <input id="zoom" type="range" min={16} max={160} value={zoom} onChange={(e) => setZoom(Number(e.target.value))} style={{ width: 70 }} />
        </div>
        <button className="btn ghost" title="Double the clip and copy its notes" onClick={() => updateClip(ti, slot, (c) => ({ ...c, length: c.length * 2, notes: [...c.notes, ...c.notes.map((n) => ({ ...n, s: n.s + c.length }))] }))}>
          ×2
        </button>
        <button className="btn ghost" title="Snap selected notes (or all) to the grid" onClick={() => setNotes(clip.notes.map((n, i) => (!sel.size || sel.has(i) ? { ...n, s: Math.min(clip.length - snap, q(n.s)), l: Math.max(snap, q(n.l)) } : n)))}>
          Quantize
        </button>
        {sel.size > 0 && (
          <div style={{ transform: "scale(0.8)", marginTop: 10 }}>
            <Knob label="Vel" value={selVel} min={0.05} max={1} size={24} format={fmt.pct} onChange={(v) => setNotes(clip.notes.map((n, i) => (sel.has(i) ? { ...n, v } : n)), "roll-vel")} />
          </div>
        )}
      </ClipHeader>
      <div className="pane-body" ref={bodyRef} tabIndex={0} onKeyDown={onKey} aria-label="Piano roll">
        <div className="roll" style={{ width: 44 + width }}>
          <div className="keys">
            {pitches.map((p) => {
              const black = [1, 3, 6, 8, 10].includes(p % 12);
              return (
                <div key={p} className={`key ${black ? "black" : ""} ${p % 12 === 0 ? "c" : ""}`} onPointerDown={() => audition(ti, p)}>
                  {p % 12 === 0 ? noteName(p) : ""}
                </div>
              );
            })}
          </div>
          <div
            className="lanes"
            ref={lanesRef}
            style={{ width }}
            onPointerDown={onDown}
            onPointerMove={onMove}
            onPointerUp={() => (drag.current = null)}
            onContextMenu={(e) => {
              e.preventDefault();
              const el = (e.target as HTMLElement).closest("[data-note]") as HTMLElement | null;
              if (el) setNotes(clip.notes.filter((_, i) => i !== Number(el.dataset.note)));
            }}
            onDoubleClick={(e) => {
              const el = (e.target as HTMLElement).closest("[data-note]") as HTMLElement | null;
              if (el) {
                setNotes(clip.notes.filter((_, i) => i !== Number(el.dataset.note)));
                setSel(new Set());
              }
            }}
          >
            {pitches.map((p) => (
              <div key={p} className={`lane ${[1, 3, 6, 8, 10].includes(p % 12) ? "black" : ""}`} />
            ))}
            <svg className="gridlines" width={width} height={pitches.length * ROW}>
              {Array.from({ length: Math.ceil(clip.length / snap) + 1 }, (_, i) => {
                const b = i * snap;
                const x = b * zoom + 0.5;
                const bar = Math.abs(b % 4) < 1e-6;
                const beat = Math.abs(b - Math.round(b)) < 1e-6;
                if (!beat && zoom * snap < 6) return null;
                return <line key={i} x1={x} x2={x} y1={0} y2="100%" stroke={bar ? "#3b444b" : beat ? "#2a3237" : "#20262a"} />;
              })}
            </svg>
            {clip.notes.map((n, i) => (
              <div
                key={i}
                data-note={i}
                className={`note ${sel.has(i) ? "sel" : ""}`}
                style={{
                  left: n.s * zoom,
                  top: (HIGH - n.p) * ROW,
                  width: Math.max(4, n.l * zoom - 1),
                  background: clip.color ?? track.color,
                  opacity: 0.35 + 0.65 * n.v,
                }}
                title={`${noteName(n.p)} · vel ${Math.round(n.v * 100)}%`}
              >
                <span className="resize" />
              </div>
            ))}
            <Playhead ti={ti} slot={slot} pxPerBeat={zoom} />
          </div>
        </div>
      </div>
    </>
  );
}

// ------------------------------------------------------------ step sequencer

export function StepGrid({ ti, slot, clip, track, selectedPad, onSelectPad }: { ti: number; slot: number; clip: Clip; track: Track; selectedPad: number; onSelectPad: (p: number) => void }) {
  const [showAll, setShowAll] = useState(false);
  const steps = Math.round(clip.length * 4);
  const rows = showAll ? 16 : Math.max(12, ...track.pads.map((p, i) => (p.sample ? i + 1 : 0)));
  const [cur, setCur] = useState(-1);
  const tr = useTransport();
  const active = tr.slots[ti] === slot;
  useFrame(() => {
    const s = active ? Math.floor(engine.trackBeat(ti).local * 4) : -1;
    if (s !== cur) setCur(s);
  });

  const find = (pad: number, step: number) =>
    clip.notes.findIndex((n) => n.p === 36 + pad && n.s >= step * 0.25 - 0.06 && n.s < (step + 1) * 0.25 - 0.06);

  const toggle = (pad: number, step: number, accent: boolean) => {
    const i = find(pad, step);
    if (i >= 0 && !accent) {
      updateClip(ti, slot, (c) => ({ ...c, notes: c.notes.filter((_, k) => k !== i) }));
      return;
    }
    if (i >= 0 && accent) {
      updateClip(ti, slot, (c) => ({ ...c, notes: c.notes.map((n, k) => (k === i ? { ...n, v: n.v > 0.85 ? 0.55 : 1 } : n)) }));
      return;
    }
    audition(ti, 36 + pad);
    updateClip(ti, slot, (c) => ({ ...c, notes: [...c.notes, { s: step * 0.25, l: 0.25, p: 36 + pad, v: accent ? 1 : 0.8 }] }));
  };

  return (
    <>
      <ClipHeader ti={ti} slot={slot} clip={clip}>
        <button className="btn ghost" onClick={() => setShowAll(!showAll)}>
          {showAll ? "Used pads" : "All 16 pads"}
        </button>
        <button
          className="btn ghost"
          title="Double the clip and copy its steps"
          onClick={() => updateClip(ti, slot, (c) => ({ ...c, length: c.length * 2, notes: [...c.notes, ...c.notes.map((n) => ({ ...n, s: n.s + c.length }))] }))}
        >
          ×2
        </button>
        <span className="muted hide-narrow" style={{ fontSize: 11 }}>Shift-click for accent</span>
      </ClipHeader>
      <div className="pane-body">
        <div className="steps" style={{ minWidth: 110 + steps * 22 }}>
          <div>
            {Array.from({ length: rows }, (_, pad) => {
              const used = clip.notes.some((n) => n.p === 36 + pad);
              return (
                <div
                  key={pad}
                  className={`pad-label ${selectedPad === pad ? "sel" : ""}`}
                  onPointerDown={() => {
                    onSelectPad(pad);
                    audition(ti, 36 + pad);
                  }}
                >
                  <span className={`dot ${used ? "on" : ""}`} />
                  {track.pads[pad]?.name || `Pad ${pad + 1}`}
                </div>
              );
            })}
          </div>
          <div style={{ ["--clip" as string]: clip.color ?? track.color }}>
            {Array.from({ length: rows }, (_, pad) => (
              <div key={pad} className="step-row" style={{ gridTemplateColumns: `repeat(${steps}, minmax(16px, 1fr))` }}>
                {Array.from({ length: steps }, (_, st) => {
                  const i = find(pad, st);
                  const v = i >= 0 ? clip.notes[i].v : 0;
                  return (
                    <div
                      key={st}
                      className={`step ${st % 4 === 0 ? "beat" : ""} ${i >= 0 ? "on" : ""} ${st === cur ? "now" : ""}`}
                      onPointerDown={(e) => toggle(pad, st, e.shiftKey)}
                      role="checkbox"
                      aria-checked={i >= 0}
                      aria-label={`${track.pads[pad]?.name || `Pad ${pad + 1}`} step ${st + 1}`}
                    >
                      <i style={i >= 0 ? { opacity: 0.4 + 0.6 * v } : undefined} />
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </div>
    </>
  );
}

// ------------------------------------------------------------ audio clips

export function AudioClipView({ ti, slot, clip }: { ti: number; slot: number; clip: Clip }) {
  const audio = clip.audio!;
  const bpm = useStore((s) => s.project.bpm);
  const [info, setInfo] = useState<SampleInfo | null>(null);
  useEffect(() => {
    let live = true;
    api.sample(audio.sample).then((s) => live && setInfo(s)).catch(() => live && setInfo(null));
    return () => {
      live = false;
    };
  }, [audio.sample]);
  const synced = audio.beats > 0;
  const ratio = synced && info ? (info.duration * bpm) / 60 / audio.beats : 1;
  return (
    <>
      <ClipHeader ti={ti} slot={slot} clip={clip} />
      <div className="pane-body">
        <div className="audio-view">
          <div style={{ position: "relative", background: "var(--bg)", borderRadius: 4, padding: "8px 0" }}>
            <Waveform peaks={info?.peaks ?? []} color={clip.color ?? "var(--accent)"} height={90} className="wavebig" />
            <Playhead ti={ti} slot={slot} pxPerBeat={0} />
          </div>
          <div className="row" style={{ display: "flex", gap: 18, alignItems: "center", flexWrap: "wrap" }}>
            <Knob label="Gain" value={audio.gain} min={0} max={2} defaultValue={1} format={fmt.gainDb} onChange={(v) => updateClip(ti, slot, (c) => ({ ...c, audio: { ...c.audio!, gain: v } }), "clip-gain")} />
            <div className="field">
              <label htmlFor="clip-sync">Tempo</label>
              <select
                id="clip-sync"
                value={synced ? "sync" : "free"}
                onChange={(e) => {
                  const beats = e.target.value === "sync" ? (info?.beats ?? (info?.bpm ? Math.round((info.duration * info.bpm) / 60) : clip.length)) : 0;
                  updateClip(ti, slot, (c) => ({ ...c, audio: { ...c.audio!, beats } }));
                }}
              >
                <option value="sync">Follow project tempo</option>
                <option value="free">Original speed</option>
              </select>
            </div>
            {synced && (
              <div className="field">
                <label htmlFor="clip-beats">Sample spans</label>
                <input
                  id="clip-beats"
                  type="number"
                  min={0.25}
                  step={0.25}
                  value={audio.beats}
                  onChange={(e) => updateClip(ti, slot, (c) => ({ ...c, audio: { ...c.audio!, beats: Math.max(0.25, Number(e.target.value) || 1) } }), "clip-beats")}
                />
                <span className="muted">beats</span>
              </div>
            )}
          </div>
          <p className="muted" style={{ margin: 0, maxWidth: "60ch" }}>
            {info ? (
              <>
                {info.name}
                {info.bpm ? ` · recorded at ${Math.round(info.bpm)} BPM` : ""}
                {info.key ? ` · ${info.key}` : ""}.{" "}
                {synced
                  ? `Plays ${ratio > 1.001 ? "faster" : ratio < 0.999 ? "slower" : "at its own speed"} (×${ratio.toFixed(3)}) to lock to ${Math.round(bpm)} BPM; pitch follows speed.`
                  : "Plays at its recorded speed from the start of every loop."}
              </>
            ) : (
              "Loading sample details…"
            )}
          </p>
        </div>
      </div>
    </>
  );
}
