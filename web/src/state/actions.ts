import { engine } from "../audio/host";
import { MAX_SCENES, MAX_TRACKS } from "../audio/ids";
import { emptyClip, newTrack, TRACK_COLORS, uid } from "./defaults";
import { store, updateTrack } from "./store";
import type { Clip, Project, SampleInfo, TrackKind } from "./types";
import { toast } from "../ui/hooks";

export const edit = store.edit.bind(store);

export function select(track: number, slot: number | null = null) {
  store.set({ selection: { track, slot } });
}

export function setClip(ti: number, slot: number, clip: Clip | null, group?: string) {
  edit(
    (p) =>
      updateTrack(p, ti, (t) => {
        const clips = t.clips.slice();
        clips[slot] = clip;
        return { ...t, clips };
      }),
    group,
  );
}

export function updateClip(ti: number, slot: number, fn: (c: Clip) => Clip, group?: string) {
  edit(
    (p) =>
      updateTrack(p, ti, (t) => {
        const c = t.clips[slot];
        if (!c) return t;
        const clips = t.clips.slice();
        clips[slot] = fn(c);
        return { ...t, clips };
      }),
    group,
  );
}

export function addTrack(kind: TrackKind): number | null {
  const p = store.get().project;
  if (p.tracks.length >= MAX_TRACKS) {
    toast(`A session holds up to ${MAX_TRACKS} tracks.`, true);
    return null;
  }
  const idx = p.tracks.length;
  const count = p.tracks.filter((t) => t.kind === kind).length;
  const base = { synth: "Synth", drums: "Drums", audio: "Audio" }[kind];
  const t = newTrack(kind, idx, count ? `${base} ${count + 1}` : base);
  edit((p) => ({ ...p, tracks: [...p.tracks, t] }));
  select(idx, null);
  return idx;
}

export function deleteTrack(ti: number) {
  engine.cmd("stop_track", ti);
  edit((p) => {
    const tracks = p.tracks.filter((_, i) => i !== ti);
    // Re-point sidechain sources at the shifted indices.
    const fixed = tracks.map((t) => {
      const s = t.fx.duckSource;
      if (s === ti) return { ...t, fx: { ...t.fx, duckSource: -1, duckAmount: 0 } };
      if (s > ti) return { ...t, fx: { ...t.fx, duckSource: s - 1 } };
      return t;
    });
    return { ...p, tracks: fixed };
  });
  const sel = store.get().selection;
  select(Math.max(0, Math.min(sel.track, store.get().project.tracks.length - 1)), null);
}

export function duplicateTrack(ti: number) {
  const p = store.get().project;
  if (p.tracks.length >= MAX_TRACKS) return;
  const t = p.tracks[ti];
  const copy = { ...structuredClone(t), id: uid(), name: `${t.name} copy`, color: TRACK_COLORS[(p.tracks.length) % TRACK_COLORS.length] };
  edit((p) => {
    const tracks = p.tracks.slice();
    tracks.splice(ti + 1, 0, copy);
    return { ...p, tracks };
  });
}

export function moveTrack(ti: number, dir: -1 | 1) {
  const to = ti + dir;
  const n = store.get().project.tracks.length;
  if (to < 0 || to >= n) return;
  engine.cmd("stop_track", ti);
  engine.cmd("stop_track", to);
  edit((p) => {
    const tracks = p.tracks.slice();
    [tracks[ti], tracks[to]] = [tracks[to], tracks[ti]];
    const remap = (s: number) => (s === ti ? to : s === to ? ti : s);
    return { ...p, tracks: tracks.map((t) => (t.fx.duckSource >= 0 ? { ...t, fx: { ...t.fx, duckSource: remap(t.fx.duckSource) } } : t)) };
  });
  select(to, store.get().selection.slot);
}

export function ensureScenes(p: Project, n: number): Project {
  if (p.scenes.length >= n) return p;
  const scenes = p.scenes.slice();
  while (scenes.length < Math.min(n, MAX_SCENES)) scenes.push({ name: `Scene ${scenes.length + 1}` });
  return { ...p, scenes };
}

export function addScene() {
  const p = store.get().project;
  if (p.scenes.length >= MAX_SCENES) {
    toast(`A session holds up to ${MAX_SCENES} scenes.`, true);
    return;
  }
  edit((p) => ensureScenes(p, p.scenes.length + 1));
}

export function duplicateScene(si: number) {
  const p = store.get().project;
  if (p.scenes.length >= MAX_SCENES) return;
  edit((p) => {
    const scenes = p.scenes.slice();
    scenes.splice(si + 1, 0, { name: `${p.scenes[si].name} copy` });
    const tracks = p.tracks.map((t) => {
      const clips = t.clips.slice();
      clips.splice(si + 1, 0, t.clips[si] ? structuredClone(t.clips[si]) : null);
      return { ...t, clips: clips.slice(0, MAX_SCENES) };
    });
    return { ...p, scenes, tracks };
  });
}

/** Beats a sample covers at the project tempo (whole beats, ≥ 1). */
export function sampleBeats(s: SampleInfo, bpm: number): { length: number; beats: number } {
  if (s.beats && s.beats > 0) return { length: s.beats, beats: s.beats };
  // One-shot or unknown tempo: play at native speed, loop length rounded up to a bar.
  const raw = (s.duration * bpm) / 60;
  return { length: Math.max(4, Math.ceil(raw / 4) * 4), beats: 0 };
}

function audioClip(s: SampleInfo, bpm: number): Clip {
  const { length, beats } = sampleBeats(s, bpm);
  return { name: s.name.split(" · ").slice(-2).join(" · "), length, notes: [], audio: { sample: s.id, beats, gain: 1 } };
}

/** Put a library sample into the session at a grid position. */
export function dropSampleOnSlot(s: SampleInfo, ti: number, slot: number) {
  const p = store.get().project;
  const t = p.tracks[ti];
  if (!t) return;
  if (t.kind === "audio") {
    edit((p) => ensureScenes(updateTrack(p, ti, (t) => ({ ...t, clips: t.clips.map((c, i) => (i === slot ? audioClip(s, p.bpm) : c)) })), slot + 1));
    engine.loadSample(s.id);
    select(ti, slot);
    return;
  }
  if (t.kind === "drums" && !s.beats) {
    const pad = Math.max(0, t.pads.findIndex((pd) => !pd.sample));
    assignPad(ti, pad, s);
    return;
  }
  toast(`${t.name} plays notes. Drop loops on an audio track, or one-shots on a drum track.`, true);
}

/** New audio track holding `s` in the given scene (or the selected one). */
export function addSampleAsTrack(s: SampleInfo, slot?: number) {
  const idx = addTrack(s.beats ? "audio" : "audio");
  if (idx === null) return;
  const sc = slot ?? store.get().selection.slot ?? 0;
  edit((p) => updateTrack(p, idx, (t) => ({ ...t, name: s.instrument.replace(/-/g, " ").replace(/^\w/, (m) => m.toUpperCase()) })), undefined);
  dropSampleOnSlot(s, idx, sc);
}

export function assignPad(ti: number, pad: number, s: SampleInfo) {
  const short = s.instrument.replace(/-/g, " ").replace(/^\w/, (m) => m.toUpperCase());
  edit((p) =>
    updateTrack(p, ti, (t) => {
      const pads = t.pads.slice();
      pads[pad] = { ...pads[pad], sample: s.id, name: short };
      return { ...t, pads };
    }),
  );
  engine.loadSample(s.id);
}

/** Double-click in the browser: put the sample somewhere sensible. */
export function quickAdd(s: SampleInfo) {
  const st = store.get();
  const t = st.project.tracks[st.selection.track];
  if (t?.kind === "drums" && !s.beats) {
    const pad = t.pads.findIndex((pd) => !pd.sample);
    assignPad(st.selection.track, pad < 0 ? 0 : pad, s);
    toast(`Added to ${t.name}, pad ${pad < 0 ? 1 : pad + 1}`);
    return;
  }
  if (t?.kind === "audio") {
    const slot = st.selection.slot ?? Math.max(0, t.clips.findIndex((c) => !c));
    dropSampleOnSlot(s, st.selection.track, slot);
    return;
  }
  addSampleAsTrack(s);
}

export function newClipAt(ti: number, slot: number) {
  const t = store.get().project.tracks[ti];
  if (!t || t.kind === "audio") return;
  setClip(ti, slot, emptyClip(t.kind === "drums" ? 4 : 16, t.kind === "drums" ? "Beat" : "Clip"));
  edit((p) => ensureScenes(p, slot + 1));
  select(ti, slot);
}

// ------------------------------------------------------------ transport

export const transport = {
  play: () => engine.resume().then(() => engine.cmd("play")),
  stop: () => engine.cmd("stop"),
  launchClip: (ti: number, slot: number) => engine.resume().then(() => engine.cmd("launch_clip", ti, slot)),
  stopTrack: (ti: number) => engine.cmd("stop_track", ti),
  launchScene: (slot: number) => engine.resume().then(() => engine.cmd("launch_scene", slot)),
  stopAll: () => engine.cmd("stop_all_clips"),
};
