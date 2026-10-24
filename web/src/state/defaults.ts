import { MAX_SCENES, SYNTH_DEFAULTS } from "../audio/ids";
import { presetParams } from "./presets";
import type { Clip, Mixer, Note, Pad, Project, Returns, Track, TrackFx, TrackKind } from "./types";

export const TRACK_COLORS = ["#e8a94a", "#56d9a8", "#7a8cf0", "#e2708a", "#5fbfe0", "#c58ae6", "#9fd36a", "#ef8a5b", "#d9c95a", "#6fd0c4", "#b0a0ff", "#f07fbf"];

export const DRUM_PAD_NAMES = ["Kick", "Snare", "Clap", "Hat", "Open Hat", "Perc 1", "Perc 2", "Shaker", "Tom", "Rim", "Ride", "Crash", "Pad 13", "Pad 14", "Pad 15", "Pad 16"];

export const uid = () => Math.random().toString(36).slice(2, 10);

export const defaultMixer = (): Mixer => ({ volume: 0.8, pan: 0, mute: false, solo: false, sendA: 0, sendB: 0 });

export const defaultFx = (): TrackFx => ({
  filterType: 0,
  cutoff: 20000,
  reso: 0.1,
  drive: 0,
  compThreshold: 0,
  compRatio: 1,
  compAttack: 10,
  compRelease: 120,
  compMakeup: 0,
  duckSource: -1,
  duckAmount: 0,
  duckRelease: 180,
});

export const defaultPads = (): Pad[] =>
  DRUM_PAD_NAMES.map((name, i) => ({ sample: null, name, gain: 1, pitch: 0, pan: 0, decay: 0, choke: i === 3 || i === 4 ? 1 : 0 }));

export const defaultReturns = (): Returns => ({
  reverbSize: 0.75,
  reverbDamp: 0.4,
  reverbWidth: 1,
  reverbLevel: 0.8,
  delayBeats: 0.75,
  delayFeedback: 0.4,
  delayTone: 4500,
  delayPingPong: true,
  delayLevel: 0.7,
});

export function newTrack(kind: TrackKind, index: number, name?: string): Track {
  const label = name ?? { synth: "Synth", drums: "Drums", audio: "Audio" }[kind];
  return {
    id: uid(),
    name: label,
    color: TRACK_COLORS[index % TRACK_COLORS.length],
    kind,
    mixer: defaultMixer(),
    fx: defaultFx(),
    synth: kind === "synth" ? presetParams("Keys") : [...SYNTH_DEFAULTS],
    preset: kind === "synth" ? "Keys" : null,
    pads: defaultPads(),
    clips: Array.from({ length: MAX_SCENES }, () => null),
  };
}

export function emptyClip(length = 4, name = "Clip"): Clip {
  return { name, length, notes: [], audio: null };
}

export function emptyProject(name = "Untitled"): Project {
  return {
    version: 1,
    name,
    bpm: 120,
    swing: 0,
    scenes: Array.from({ length: 8 }, (_, i) => ({ name: `Scene ${i + 1}` })),
    tracks: [],
    returns: defaultReturns(),
    master: { volume: 0.85, quantize: 4 },
    arrangement: [],
  };
}

// ------------------------------------------------------------ theory

export const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
export const noteName = (p: number) => `${NOTE_NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;

export function parseKey(key: string | null | undefined): { tonic: number; minor: boolean } {
  if (!key) return { tonic: 9, minor: true };
  const minor = key.endsWith("m");
  const root = minor ? key.slice(0, -1) : key;
  const tonic = NOTE_NAMES.indexOf(root);
  return { tonic: tonic < 0 ? 9 : tonic, minor };
}

const MINOR = [0, 2, 3, 5, 7, 8, 10];
const MAJOR = [0, 2, 4, 5, 7, 9, 11];

export function degree(tonic: number, minor: boolean, deg: number, octave: number) {
  const s = minor ? MINOR : MAJOR;
  const o = Math.floor(deg / 7);
  const i = ((deg % 7) + 7) % 7;
  return 12 * (octave + 1) + tonic + s[i] + 12 * o;
}

function chordNotes(tonic: number, minor: boolean, deg: number, start: number, len: number, hits: [number, number][], vel = 0.8): Note[] {
  const pitches = [0, 2, 4, 6].map((d) => degree(tonic, minor, deg + d, 3));
  // Keep the voicing compact around middle C.
  const voiced = pitches.map((p) => (p > 72 ? p - 12 : p < 55 ? p + 12 : p)).sort((a, b) => a - b);
  const out: Note[] = [];
  for (const [s, l] of hits) for (const p of voiced) out.push({ s: start + s, l: Math.min(l, len), p, v: vel });
  return out;
}

// ------------------------------------------------------------ demo project

export interface DemoKit {
  bpm: number;
  key: string | null;
  kit: string;
  oneShots: Record<string, string>;
  loops: { id: string; beats: number; category: string; instrument: string }[];
}

const hits = (pitch: number, steps: string, bars = 1, vel = 0.9): Note[] => {
  const out: Note[] = [];
  for (let b = 0; b < bars; b++)
    [...steps].forEach((c, i) => {
      if (c === ".") return;
      out.push({ s: b * 4 + i * 0.25, l: 0.2, p: pitch, v: c === "x" ? vel : c === "o" ? vel * 0.75 : vel * 0.45 });
    });
  return out;
};

/** A ready-to-play session built around one construction kit from the library. */
export function demoProject(kit: DemoKit | null): Project {
  const p = emptyProject(kit ? `${kit.kit.replace(/-kit(\d+)/, " Kit $1").replace(/(^|-)\w/g, (m) => m.toUpperCase().replace("-", " "))} Session` : "First Session");
  p.bpm = kit?.bpm ?? 124;
  p.swing = 0.1;
  p.scenes = ["Intro", "Groove", "Break", "Drop", "Outro", "Scene 6", "Scene 7", "Scene 8"].map((name) => ({ name }));
  const { tonic, minor } = parseKey(kit?.key ?? "Am");
  const prog = minor ? [0, 5, 2, 6] : [0, 4, 5, 3];

  // Drums
  const drums = newTrack("drums", 0, "Drums");
  if (kit) {
    const map: Record<string, number> = { kick: 0, snare: 1, clap: 2, "hat-closed": 3, "hat-open": 4, perc: 5, shaker: 7, tom: 8, rim: 9, ride: 10, crash: 11 };
    for (const [inst, id] of Object.entries(kit.oneShots)) {
      const idx = map[inst];
      if (idx !== undefined) drums.pads[idx] = { ...drums.pads[idx], sample: id };
    }
    drums.pads[3].gain = 0.6;
    drums.pads[4].gain = 0.5;
    drums.pads[7].gain = 0.45;
  }
  const kick = hits(36, "x...x...x...x...", 1);
  const clap = hits(38, "....x.......x...", 1);
  const hat = hits(39, "..x...x...x...x.", 1, 0.8);
  const shaker = hits(43, "gogggogggogggogg", 1, 0.7);
  const ohat = hits(40, "..............o.", 1, 0.7);
  drums.clips[0] = { name: "Kick + Hats", length: 4, notes: [...kick, ...hat] };
  drums.clips[1] = { name: "Groove", length: 4, notes: [...kick, ...clap, ...hat, ...shaker] };
  drums.clips[2] = { name: "Hats Only", length: 4, notes: [...hat, ...shaker] };
  drums.clips[3] = { name: "Full", length: 4, notes: [...kick, ...clap, ...hat, ...shaker, ...ohat] };
  drums.clips[4] = { name: "Kick", length: 4, notes: [...kick] };
  drums.fx = { ...drums.fx, compThreshold: -12, compRatio: 3, compAttack: 8, compMakeup: 2 };
  drums.mixer.sendA = 0.06;

  // Bass
  const bass = newTrack("synth", 1, "Bass");
  bass.synth = presetParams("Pluck Bass");
  bass.preset = "Pluck Bass";
  const bassNotes: Note[] = [];
  prog.forEach((deg, bar) => {
    let root = degree(tonic, minor, deg, 2);
    while (root > 45) root -= 12;
    while (root < 33) root += 12;
    for (let b = 0; b < 4; b++) bassNotes.push({ s: bar * 4 + b + 0.5, l: 0.3, p: root, v: 0.9 });
  });
  bass.clips[1] = { name: "Offbeat", length: 16, notes: bassNotes };
  bass.clips[3] = { name: "Offbeat", length: 16, notes: bassNotes };
  bass.fx = { ...bass.fx, duckSource: 0, duckAmount: 0.35, duckRelease: 160 };
  bass.mixer.volume = 0.75;

  // Chords
  const chords = newTrack("synth", 2, "Chords");
  chords.synth = presetParams("House Stab");
  chords.preset = "House Stab";
  const stab: Note[] = [];
  const pad: Note[] = [];
  prog.forEach((deg, bar) => {
    stab.push(...chordNotes(tonic, minor, deg, bar * 4, 4, [[0.5, 0.22], [1.5, 0.22], [2.5, 0.22], [3.5, 0.22]], 0.85));
    pad.push(...chordNotes(tonic, minor, deg, bar * 4, 4, [[0, 3.9]], 0.7));
  });
  chords.clips[0] = { name: "Pad", length: 16, notes: pad };
  chords.clips[1] = { name: "Stabs", length: 16, notes: stab };
  chords.clips[2] = { name: "Pad", length: 16, notes: pad };
  chords.clips[3] = { name: "Stabs", length: 16, notes: stab };
  chords.fx = { ...chords.fx, duckSource: 0, duckAmount: 0.55, duckRelease: 200 };
  chords.mixer = { ...chords.mixer, volume: 0.65, sendA: 0.22, sendB: 0.12 };

  p.tracks = [drums, bass, chords];

  // Library loops as audio clips.
  if (kit) {
    const pickLoop = (cat: string) => kit.loops.find((l) => l.category === cat);
    const loopTrack = (name: string, idx: number, cat: string, scenes: number[], vol = 0.7) => {
      const l = pickLoop(cat);
      if (!l) return;
      const t = newTrack("audio", idx, name);
      for (const s of scenes) t.clips[s] = { name: l.instrument, length: l.beats, notes: [], audio: { sample: l.id, beats: l.beats, gain: 1 } };
      t.mixer.volume = vol;
      p.tracks.push(t);
    };
    loopTrack("Hook", 3, "hook", [1, 3], 0.6);
    loopTrack("Arp", 4, "arp", [2, 3], 0.5);
    loopTrack("Vox", 5, "vocal", [3], 0.6);
  }

  p.arrangement = [
    { scene: 0, bars: 8 },
    { scene: 1, bars: 16 },
    { scene: 2, bars: 8 },
    { scene: 3, bars: 16 },
    { scene: 4, bars: 4 },
  ];
  return p;
}
