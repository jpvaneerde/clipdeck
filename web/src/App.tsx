import { useCallback, useEffect, useRef, useState } from "react";
import { engine } from "./audio/host";
import { allNotesOff, keyToPitch, noteOff, noteOn } from "./audio/live";
import { startSync } from "./audio/sync";
import { ensureScenes, setClip, transport } from "./state/actions";
import { api } from "./state/api";
import { demoProject, type DemoKit } from "./state/defaults";
import { lastProjectKey, store, useStore } from "./state/store";
import { Browser } from "./ui/Browser";
import { Detail } from "./ui/Detail";
import { ExportDialog, ProjectsDialog, StartOverlay, UsersDialog } from "./ui/Dialogs";
import { useEngineStatus, useToasts } from "./ui/hooks";
import { Session } from "./ui/Session";
import { TopBar } from "./ui/TopBar";

/** Find a construction kit in the library and describe it for the starter session. */
async function loadKit(random: boolean): Promise<DemoKit | null> {
  try {
    const f = await api.facets({ category: "drum-loop" });
    const kits = f.kits.map((k) => k.value);
    if (!kits.length) return null;
    const kit = random ? kits[Math.floor(Math.random() * kits.length)] : kits.find((k) => k === "house-kit01") ?? kits[0];
    const page = await api.search({ kit, limit: 200 });
    const oneShots: Record<string, string> = {};
    let bpm = 0;
    let key: string | null = null;
    const loops: DemoKit["loops"] = [];
    for (const s of page.items) {
      if (s.category === "drums") {
        if (!oneShots[s.instrument]) oneShots[s.instrument] = s.id;
      } else if (s.beats) {
        loops.push({ id: s.id, beats: s.beats, category: s.category, instrument: s.instrument });
        if (s.bpm) bpm = s.bpm;
        if (s.key) key = s.key;
      }
    }
    return { kit, bpm: bpm || 124, key, oneShots, loops };
  } catch {
    return null;
  }
}

function isTyping(e: KeyboardEvent) {
  const el = e.target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
}

export default function App() {
  const { status, running, error } = useEngineStatus();
  const browserOpen = useStore((s) => s.browserOpen);
  const project = useStore((s) => s.project);
  const projectId = useStore((s) => s.projectId);
  const dirty = useStore((s) => s.dirty);
  const toasts = useToasts();
  const [dialog, setDialog] = useState<null | "export" | "projects" | "users">(null);
  const [saveState, setSaveState] = useState("");
  const started = useRef(false);

  const newDemo = useCallback(async (random: boolean) => {
    engine.cmd("stop");
    const kit = await loadKit(random);
    store.load(demoProject(kit), null);
    localStorage.removeItem(lastProjectKey());
  }, []);

  // Boot: engine, sync, and the last project (or a starter session).
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    engine.init().catch(() => undefined);
    startSync();
    (async () => {
      try {
        store.set({ me: await api.me() });
      } catch {
        return; // not signed in: api already redirected to sign-in
      }
      const id = localStorage.getItem(lastProjectKey());
      if (id) {
        try {
          store.load(await api.project(id), id);
          return;
        } catch {
          localStorage.removeItem(lastProjectKey());
        }
      }
      await newDemo(false);
    })();
  }, [newDemo]);

  // Autosave to the server shortly after edits.
  useEffect(() => {
    if (!dirty) return;
    setSaveState("Unsaved");
    const t = setTimeout(async () => {
      setSaveState("Saving…");
      try {
        const p = store.get().project;
        let id = store.get().projectId;
        if (id) await api.saveProject(id, p);
        else {
          id = (await api.createProject(p)).id;
          localStorage.setItem(lastProjectKey(), id);
        }
        store.set({ projectId: id, dirty: store.get().project !== p, savedAt: Date.now() });
        setSaveState("Saved");
      } catch {
        setSaveState("Not saved (offline)");
      }
    }, 1500);
    return () => clearTimeout(t);
  }, [project, dirty, projectId]);

  // Keyboard: transport, editing, and playing notes.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (isTyping(e)) return;
      const mod = e.ctrlKey || e.metaKey;
      if (e.code === "Space") {
        e.preventDefault();
        if (engine.transport.playing) transport.stop();
        else transport.play();
        return;
      }
      if (mod && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) store.redo();
        else store.undo();
        return;
      }
      if (mod && e.key.toLowerCase() === "y") {
        e.preventDefault();
        store.redo();
        return;
      }
      if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        store.set({ dirty: true, project: { ...store.get().project } });
        return;
      }
      const sel = store.get().selection;
      if (mod && e.key.toLowerCase() === "d" && sel.slot !== null) {
        e.preventDefault();
        const t = store.get().project.tracks[sel.track];
        const clip = t?.clips[sel.slot];
        if (!t || !clip) return;
        let to = sel.slot + 1;
        while (to < 16 && t.clips[to]) to++;
        if (to >= 16) return;
        setClip(sel.track, to, structuredClone(clip));
        store.edit((p) => ensureScenes(p, to + 1));
        store.set({ selection: { track: sel.track, slot: to } });
        return;
      }
      if ((e.key === "Delete" || e.key === "Backspace") && sel.slot !== null) {
        if (store.get().project.tracks[sel.track]?.clips[sel.slot]) {
          e.preventDefault();
          setClip(sel.track, sel.slot, null);
        }
        return;
      }
      if (e.key === "Escape") {
        engine.stopPreview();
        return;
      }
      if (mod || e.altKey) return;
      if (e.code === "KeyZ" || e.code === "KeyX") {
        const o = store.get().octave + (e.code === "KeyX" ? 1 : -1);
        store.set({ octave: Math.max(0, Math.min(8, o)) });
        return;
      }
      if (e.code === "KeyB") {
        store.set({ browserOpen: !store.get().browserOpen });
        return;
      }
      if (e.repeat) return;
      const pitch = keyToPitch(e.code);
      if (pitch !== null) {
        engine.resume();
        noteOn(pitch);
      }
    };
    const up = (e: KeyboardEvent) => {
      const pitch = keyToPitch(e.code);
      if (pitch !== null) noteOff(pitch);
    };
    const blur = () => allNotesOff();
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, []);

  return (
    <div className={`app ${browserOpen ? "" : "no-browser"}`}>
      <TopBar onProjects={() => setDialog("projects")} onExport={() => setDialog("export")} onUsers={() => setDialog("users")} saveState={saveState} />
      <Browser />
      <main className="main">
        <Session />
      </main>
      <Detail />
      {dialog === "export" && <ExportDialog onClose={() => setDialog(null)} />}
      {dialog === "projects" && <ProjectsDialog onClose={() => setDialog(null)} onNewDemo={() => newDemo(true)} />}
      {dialog === "users" && <UsersDialog onClose={() => setDialog(null)} />}
      {!running && <StartOverlay status={status} error={error} onStart={() => engine.resume()} />}
      <div className="toast-stack" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.error ? "err" : ""}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
