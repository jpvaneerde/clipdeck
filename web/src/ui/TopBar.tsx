import { useRef, useState } from "react";
import { engine } from "../audio/host";
import { connectMidi } from "../audio/live";
import { edit, transport } from "../state/actions";
import { store, useStore } from "../state/store";
import { Icon } from "./controls";
import { toast, useFrame, useTransport } from "./hooks";

function Position() {
  const ref = useRef<HTMLSpanElement>(null);
  useFrame(() => {
    if (!ref.current) return;
    const b = Math.max(0, engine.beatNow());
    const bar = Math.floor(b / 4) + 1;
    const beat = Math.floor(b % 4) + 1;
    const six = Math.floor((b % 1) * 4) + 1;
    const txt = `${bar}.${beat}.${six}`;
    if (ref.current.textContent !== txt) ref.current.textContent = txt;
  });
  return <span ref={ref} className="mono beat-display" aria-label="Position (bar.beat.sixteenth)">1.1.1</span>;
}

function UserMenu({ onUsers }: { onUsers: () => void }) {
  const me = useStore((s) => s.me);
  const [open, setOpen] = useState(false);
  if (!me) return null;
  const initials = me.name
    .split(/\s+/)
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <div style={{ position: "relative" }}>
      <button className="btn" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)} title={`${me.name} (${me.email})`}>
        <span className="avatar" aria-hidden="true">{initials}</span>
        <span className="hide-narrow">{me.name.split(" ")[0]}</span>
        {me.role === "admin" && <span className="chip on" style={{ fontSize: 10, padding: "0 6px" }}>admin</span>}
      </button>
      {open && (
        <div className="menu" role="menu" onMouseLeave={() => setOpen(false)}>
          <div className="menu-head">
            <div>{me.name}</div>
            <div className="muted mono" style={{ fontSize: 11 }}>{me.email}</div>
          </div>
          {me.role === "admin" && (
            <button className="btn ghost" role="menuitem" onClick={() => { setOpen(false); onUsers(); }}>
              Users
            </button>
          )}
          {/* Sign-out is a POST so other sites can't sign you out with a link. */}
          <form method="post" action="/auth/logout">
            <button className="btn ghost" role="menuitem" type="submit" style={{ width: "100%" }}>
              Sign out
            </button>
          </form>
        </div>
      )}
    </div>
  );
}

export function TopBar({ onProjects, onExport, onUsers, saveState }: { onProjects: () => void; onExport: () => void; onUsers: () => void; saveState: string }) {
  const name = useStore((s) => s.project.name);
  const bpm = useStore((s) => s.project.bpm);
  const swing = useStore((s) => s.project.swing);
  const quantize = useStore((s) => s.project.master.quantize);
  const rec = useStore((s) => s.recordArmed);
  const metro = useStore((s) => s.metronome);
  const browserOpen = useStore((s) => s.browserOpen);
  const octave = useStore((s) => s.octave);
  useStore((s) => s.project); // re-render undo/redo availability
  const tr = useTransport();

  return (
    <header className="topbar">
      <div className="brand">
        {Icon.logo}
        <span className="hide-narrow">Clipdeck</span>
      </div>
      <button className={`btn icon ${browserOpen ? "on" : ""}`} aria-label="Toggle sample browser" title="Sample browser (B)" onClick={() => store.set({ browserOpen: !browserOpen })}>
        {Icon.panel}
      </button>
      <input id="project-name" className="project-name" aria-label="Project name" value={name} onChange={(e) => edit((p) => ({ ...p, name: e.target.value }), "project-name")} />
      <span className="save-state hide-narrow">{saveState}</span>
      <div className="sep hide-narrow" />
      <div className="group">
        <button className={`btn icon ${tr.playing ? "on" : ""}`} aria-label={tr.playing ? "Stop" : "Play"} title="Play / stop (Space)" onClick={() => (tr.playing ? transport.stop() : transport.play())}>
          {tr.playing ? Icon.stop : Icon.play}
        </button>
        <button className={`btn icon rec ${rec ? "on" : ""}`} aria-pressed={rec} aria-label="Record into the selected clip" title="Overdub notes into the selected playing clip" onClick={() => store.set({ recordArmed: !rec })}>
          {Icon.rec}
        </button>
        <button className={`btn icon ${metro ? "on" : ""}`} aria-pressed={metro} aria-label="Metronome" title="Metronome" onClick={() => store.set({ metronome: !metro })}>
          {Icon.metro}
        </button>
        <Position />
      </div>
      <div className="group">
        <div className="field">
          <label htmlFor="bpm">BPM</label>
          <input
            id="bpm"
            className="mono"
            type="number"
            min={40}
            max={300}
            step={0.5}
            value={bpm}
            onChange={(e) => {
              const v = Number(e.target.value);
              if (v >= 20 && v <= 999) edit((p) => ({ ...p, bpm: v }), "bpm");
            }}
          />
        </div>
        <div className="field hide-narrow">
          <label htmlFor="swing">Swing</label>
          <input id="swing" type="range" min={0} max={1} step={0.01} value={swing} onChange={(e) => edit((p) => ({ ...p, swing: Number(e.target.value) }), "swing")} style={{ width: 56 }} />
          <span className="mono" style={{ fontSize: 11, width: 30 }}>{Math.round(swing * 100)}%</span>
        </div>
        <div className="field hide-narrow">
          <label htmlFor="quant">Launch</label>
          <select id="quant" value={quantize} onChange={(e) => edit((p) => ({ ...p, master: { ...p.master, quantize: Number(e.target.value) } }))}>
            <option value={0}>Now</option>
            <option value={1}>1 beat</option>
            <option value={2}>1/2 bar</option>
            <option value={4}>1 bar</option>
            <option value={8}>2 bars</option>
            <option value={16}>4 bars</option>
          </select>
        </div>
        <div className="field hide-medium" title="Computer keyboard octave (Z / X)">
          <label>Oct</label>
          <span className="mono">{octave}</span>
        </div>
      </div>
      <div className="group">
        <button className="btn icon" aria-label="Undo" title="Undo (Ctrl+Z)" disabled={!store.canUndo()} onClick={() => store.undo()}>
          {Icon.undo}
        </button>
        <button className="btn icon" aria-label="Redo" title="Redo (Ctrl+Shift+Z)" disabled={!store.canRedo()} onClick={() => store.redo()}>
          {Icon.redo}
        </button>
      </div>
      <span className="spacer" />
      <div className="group">
        <button
          className="btn hide-narrow"
          title="Play the selected track from a MIDI keyboard"
          onClick={async () => {
            try {
              const names = await connectMidi();
              toast(names.length ? `MIDI connected: ${names.join(", ")}` : "MIDI is on, but no input device is connected.");
            } catch (e) {
              toast((e as Error).message, true);
            }
          }}
        >
          {Icon.midi} MIDI
        </button>
        <button className="btn" onClick={onProjects}>
          {Icon.folder} <span className="hide-narrow">Projects</span>
        </button>
        <button className="btn primary" onClick={onExport}>
          {Icon.export} Export
        </button>
        <UserMenu onUsers={onUsers} />
      </div>
    </header>
  );
}
