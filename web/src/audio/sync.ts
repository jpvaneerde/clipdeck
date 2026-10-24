// Keeps the engine in step with the project: diffs the previous and next
// project by reference and sends only what changed.

import { engine } from "./host";
import { KIND, MASTER, MAX_SCENES, MAX_TRACKS, PAD, RET, TP } from "./ids";
import { store } from "../state/store";
import type { Clip, Mixer, Pad, Project, Track, TrackFx } from "../state/types";

const kindId = (k: Track["kind"]) => KIND[k];

function syncMixer(ti: number, a: Mixer | undefined, b: Mixer) {
  const fields: [keyof Mixer, number][] = [
    ["volume", TP.volume],
    ["pan", TP.pan],
    ["mute", TP.mute],
    ["solo", TP.solo],
    ["sendA", TP.sendA],
    ["sendB", TP.sendB],
  ];
  for (const [k, id] of fields) if (!a || a[k] !== b[k]) engine.cmd("set_track_param", ti, id, Number(b[k]));
}

function syncFx(ti: number, a: TrackFx | undefined, b: TrackFx) {
  for (const k of Object.keys(b) as (keyof TrackFx)[]) {
    if (a && a[k] === b[k]) continue;
    const id = (TP as Record<string, number>)[k];
    if (id !== undefined) engine.cmd("set_track_param", ti, id, b[k]);
  }
}

function syncSynth(ti: number, a: number[] | undefined, b: number[]) {
  b.forEach((v, i) => {
    if (!a || a[i] !== v) engine.cmd("set_synth_param", ti, i, v);
  });
}

function syncPad(ti: number, pi: number, a: Pad | undefined, b: Pad) {
  const simple: [keyof Pad, number][] = [
    ["gain", PAD.gain],
    ["pitch", PAD.pitch],
    ["pan", PAD.pan],
    ["decay", PAD.decay],
    ["choke", PAD.choke],
  ];
  for (const [k, id] of simple) if (!a || a[k] !== b[k]) engine.cmd("set_pad_param", ti, pi, id, Number(b[k]));
  if (a && a.sample === b.sample) return;
  if (!b.sample) {
    engine.cmd("set_pad_param", ti, pi, PAD.sample, -1);
    return;
  }
  const id = b.sample;
  engine.loadSample(id).then((h) => {
    // Only apply if the pad still wants this sample.
    if (store.get().project.tracks[ti]?.pads[pi]?.sample === id) engine.cmd("set_pad_param", ti, pi, PAD.sample, h);
  });
}

function sendClip(ti: number, slot: number, c: Clip | null) {
  if (!c) {
    engine.cmd("clear_clip", ti, slot);
    return;
  }
  if (c.audio) {
    const audio = c.audio;
    engine.loadSample(audio.sample).then((h) => {
      const cur = store.get().project.tracks[ti]?.clips[slot];
      if (cur !== c) return; // superseded; the newer clip was sent separately
      engine.cmd("set_clip_audio", ti, slot, c.length, h, audio.beats, audio.gain);
    });
    return;
  }
  engine.setClipNotes(ti, slot, c.length, c.notes);
}

function syncTrack(ti: number, a: Track | undefined, b: Track | undefined) {
  if (!b) {
    if (a) {
      engine.cmd("stop_track", ti);
      for (let s = 0; s < MAX_SCENES; s++) if (a.clips[s]) engine.cmd("clear_clip", ti, s);
      engine.cmd("set_track_kind", ti, KIND.none);
    }
    return;
  }
  const fresh = !a || a.kind !== b.kind;
  if (fresh) {
    engine.cmd("set_track_kind", ti, kindId(b.kind));
    a = undefined;
  }
  if (!a || a.mixer !== b.mixer) syncMixer(ti, a?.mixer, b.mixer);
  if (!a || a.fx !== b.fx) syncFx(ti, a?.fx, b.fx);
  if (b.kind === "synth" && (!a || a.synth !== b.synth)) syncSynth(ti, a?.synth, b.synth);
  if (b.kind === "drums" && (!a || a.pads !== b.pads)) b.pads.forEach((p, pi) => (!a || a.pads[pi] !== p) && syncPad(ti, pi, a?.pads[pi], p));
  if (!a || a.clips !== b.clips) {
    for (let s = 0; s < MAX_SCENES; s++) {
      const ca = a?.clips[s] ?? null;
      const cb = b.clips[s] ?? null;
      if (ca !== cb || fresh) {
        if (fresh && !cb) {
          engine.cmd("clear_clip", ti, s);
          continue;
        }
        sendClip(ti, s, cb);
      }
    }
  }
}

export function syncProject(a: Project | null, b: Project) {
  if (!a || a.bpm !== b.bpm) engine.cmd("set_bpm", b.bpm);
  if (!a || a.swing !== b.swing) engine.cmd("set_master_param", MASTER.swing, b.swing);
  if (!a || a.master !== b.master) {
    engine.cmd("set_master_param", MASTER.volume, b.master.volume);
    engine.cmd("set_master_param", MASTER.quantize, b.master.quantize);
  }
  if (!a || a.returns !== b.returns) {
    const r = b.returns;
    for (const k of Object.keys(RET) as (keyof typeof RET)[]) {
      if (a && a.returns[k] === r[k]) continue;
      engine.cmd("set_return_param", RET[k], Number(r[k]));
    }
  }
  if (!a || a.tracks !== b.tracks) {
    for (let ti = 0; ti < MAX_TRACKS; ti++) {
      const ta = a?.tracks[ti];
      const tb = b.tracks[ti];
      if (ta !== tb) syncTrack(ti, ta, tb);
    }
  }
}

/** Wire the store to the engine. Call once at startup. */
export function startSync() {
  let prev: Project | null = null;
  const run = () => {
    const p = store.get().project;
    if (p === prev) return;
    syncProject(prev, p);
    prev = p;
  };
  run();
  store.subscribe(run);
  let metronome = store.get().metronome;
  engine.cmd("set_master_param", MASTER.metronome, metronome ? 1 : 0);
  store.subscribe(() => {
    const m = store.get().metronome;
    if (m !== metronome) {
      metronome = m;
      engine.cmd("set_master_param", MASTER.metronome, m ? 1 : 0);
    }
  });
}

export const MAX_TRACK_COUNT = MAX_TRACKS;
