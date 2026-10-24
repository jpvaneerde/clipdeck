import { useEffect, useState } from "react";
import { edit } from "../state/actions";
import { api } from "../state/api";
import { emptyProject } from "../state/defaults";
import { store, useStore } from "../state/store";
import type { Project, Section } from "../state/types";
import { Icon } from "./controls";
import { toast } from "./hooks";

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title}>
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2>{title}</h2>
          <button className="btn ghost icon" aria-label="Close" onClick={onClose}>
            {Icon.close}
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

// ------------------------------------------------------------ export

export function ExportDialog({ onClose }: { onClose: () => void }) {
  const project = useStore((s) => s.project);
  const [bits, setBits] = useState(24);
  const [rate, setRate] = useState(48000);
  const [tail, setTail] = useState(2);
  const [busy, setBusy] = useState(false);
  const arr = project.arrangement.length ? project.arrangement : [{ scene: 0, bars: 8 }];
  const setArr = (a: Section[]) => edit((p) => ({ ...p, arrangement: a }), "arrangement");
  const bars = arr.reduce((n, s) => n + s.bars, 0);
  const seconds = (bars * 4 * 60) / project.bpm + tail;

  const render = async () => {
    setBusy(true);
    try {
      const p: Project = { ...project, arrangement: arr };
      const blob = await api.render(p, { bits, sampleRate: rate, tail });
      download(blob, `${project.name.replace(/[^\w\- ]+/g, "").trim() || "clipdeck"}.wav`);
      toast("Export finished");
      onClose();
    } catch (e) {
      toast((e as Error).message, true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Export to WAV" onClose={onClose}>
      <p>Scenes play in this order, each for the bars you set. The server renders with the same engine you hear, faster than real time.</p>
      <div className="arr">
        {arr.map((s, i) => (
          <div key={i} className="arr-item">
            <select id={`arr-scene-${i}`} aria-label="Scene" value={s.scene} onChange={(e) => setArr(arr.map((x, k) => (k === i ? { ...x, scene: Number(e.target.value) } : x)))}>
              {project.scenes.map((sc, si) => (
                <option key={si} value={si}>
                  {si + 1}. {sc.name}
                </option>
              ))}
            </select>
            <input
              id={`arr-bars-${i}`}
              aria-label="Bars"
              type="number"
              min={1}
              max={256}
              value={s.bars}
              onChange={(e) => setArr(arr.map((x, k) => (k === i ? { ...x, bars: Math.max(1, Number(e.target.value) || 1) } : x)))}
            />
            <button className="btn ghost icon" aria-label="Remove section" disabled={arr.length === 1} onClick={() => setArr(arr.filter((_, k) => k !== i))}>
              {Icon.close}
            </button>
          </div>
        ))}
        <button className="btn" onClick={() => setArr([...arr, { scene: Math.min(project.scenes.length - 1, (arr[arr.length - 1]?.scene ?? -1) + 1), bars: 8 }])}>
          {Icon.plus} Add section
        </button>
      </div>
      <div className="row">
        <div className="field">
          <label htmlFor="exp-bits">Bits</label>
          <select id="exp-bits" value={bits} onChange={(e) => setBits(Number(e.target.value))}>
            <option value={16}>16</option>
            <option value={24}>24</option>
          </select>
        </div>
        <div className="field">
          <label htmlFor="exp-rate">Rate</label>
          <select id="exp-rate" value={rate} onChange={(e) => setRate(Number(e.target.value))}>
            <option value={44100}>44.1 kHz</option>
            <option value={48000}>48 kHz</option>
            <option value={96000}>96 kHz</option>
          </select>
        </div>
        <div className="field">
          <label htmlFor="exp-tail">Tail</label>
          <input id="exp-tail" type="number" min={0} max={10} step={0.5} value={tail} onChange={(e) => setTail(Number(e.target.value))} />
          <span className="muted">s</span>
        </div>
      </div>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="muted mono">
          {bars} bars · {Math.floor(seconds / 60)}:{String(Math.round(seconds % 60)).padStart(2, "0")}
        </span>
        <button className="btn primary" disabled={busy} onClick={render}>
          {busy ? "Rendering…" : "Render WAV"}
        </button>
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------ projects

export function ProjectsDialog({ onClose, onNewDemo }: { onClose: () => void; onNewDemo: () => void }) {
  const currentId = useStore((s) => s.projectId);
  const [list, setList] = useState<{ id: string; name: string; updated: number }[] | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const refresh = () => api.projects().then(setList).catch((e) => toast(e.message, true));
  useEffect(() => {
    refresh();
  }, []);

  const open = async (id: string) => {
    try {
      const p = await api.project(id);
      store.load(p, id);
      localStorage.setItem("clipdeck.project", id);
      onClose();
    } catch (e) {
      toast((e as Error).message, true);
    }
  };

  const importJson = async (f: File) => {
    try {
      const p = JSON.parse(await f.text()) as Project;
      if (!Array.isArray(p.tracks)) throw new Error("That file is not a Clipdeck project.");
      store.load(p, null);
      onClose();
    } catch (e) {
      toast((e as Error).message, true);
    }
  };

  return (
    <Modal title="Projects" onClose={onClose}>
      <div className="row">
        <button
          className="btn"
          onClick={() => {
            store.load(emptyProject(), null);
            localStorage.removeItem("clipdeck.project");
            onClose();
          }}
        >
          {Icon.plus} Empty project
        </button>
        <button
          className="btn"
          onClick={() => {
            onNewDemo();
            onClose();
          }}
        >
          {Icon.plus} Starter from a random kit
        </button>
        <button className="btn" onClick={() => download(new Blob([JSON.stringify(store.get().project, null, 1)], { type: "application/json" }), `${store.get().project.name || "project"}.clipdeck.json`)}>
          Save as file
        </button>
        <label className="btn">
          Open file
          <input id="import-project" type="file" accept=".json,application/json" hidden onChange={(e) => e.target.files?.[0] && importJson(e.target.files[0])} />
        </label>
      </div>
      <div className="project-list">
        {list === null && <p>Loading…</p>}
        {list?.length === 0 && <p>No saved projects yet. Your session saves itself as you work.</p>}
        {list?.map((p) => (
          <div key={p.id} className={`project-item ${p.id === currentId ? "current" : ""}`}>
            <div>
              <div>{p.name}</div>
              <div className="muted mono" style={{ fontSize: 11 }}>
                {new Date(p.updated * 1000).toLocaleString()}
              </div>
            </div>
            <div className="row">
              {confirm === p.id ? (
                <>
                  <span className="muted">Delete for good?</span>
                  <button
                    className="btn"
                    style={{ borderColor: "var(--rec)" }}
                    onClick={async () => {
                      await api.deleteProject(p.id);
                      if (p.id === currentId) store.set({ projectId: null });
                      setConfirm(null);
                      refresh();
                    }}
                  >
                    Delete
                  </button>
                  <button className="btn ghost" onClick={() => setConfirm(null)}>
                    Keep
                  </button>
                </>
              ) : (
                <>
                  <button className="btn ghost" onClick={() => setConfirm(p.id)}>
                    Delete
                  </button>
                  <button className="btn" disabled={p.id === currentId} onClick={() => open(p.id)}>
                    {p.id === currentId ? "Open now" : "Open"}
                  </button>
                </>
              )}
            </div>
          </div>
        ))}
      </div>
    </Modal>
  );
}

// ------------------------------------------------------------ start

export function StartOverlay({ status, error, onStart }: { status: string; error: string; onStart: () => void }) {
  return (
    <div className="overlay">
      <div className="dialog start">
        <h1>Clipdeck</h1>
        <p>Launch clips, build scenes, and play the synths and drum racks from your keyboard. Browsers only allow sound after you click, so start the audio engine here.</p>
        {status === "error" ? (
          <p style={{ color: "var(--rec)" }}>The audio engine did not load: {error}. Reload the page; if it keeps failing, check that engine.wasm is served by the server.</p>
        ) : (
          <div className="row">
            <button className="btn primary" style={{ height: 36, padding: "0 18px" }} onClick={onStart} autoFocus>
              {Icon.play} Start audio
            </button>
            {status === "loading" && <span className="muted">Loading engine…</span>}
          </div>
        )}
        <p className="muted" style={{ fontSize: 12 }}>
          Space play/stop · A–L play notes · Z/X octave · Ctrl+Z undo · Del delete clip · Ctrl+D duplicate clip
        </p>
      </div>
    </div>
  );
}
