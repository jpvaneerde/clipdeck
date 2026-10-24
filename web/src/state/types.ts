// Mirrors crates/model/src/lib.rs (the project file format).

export type TrackKind = "synth" | "drums" | "audio";

export interface Note {
  /** start in beats */
  s: number;
  /** length in beats */
  l: number;
  /** MIDI pitch */
  p: number;
  /** velocity 0..1 */
  v: number;
}

export interface AudioData {
  sample: string;
  /** beats the sample spans at its own tempo; 0 = unsynced one-shot */
  beats: number;
  gain: number;
}

export interface Clip {
  name: string;
  color?: string | null;
  length: number;
  notes: Note[];
  audio?: AudioData | null;
}

export interface Mixer {
  volume: number;
  pan: number;
  mute: boolean;
  solo: boolean;
  sendA: number;
  sendB: number;
}

export interface TrackFx {
  filterType: number;
  cutoff: number;
  reso: number;
  drive: number;
  compThreshold: number;
  compRatio: number;
  compAttack: number;
  compRelease: number;
  compMakeup: number;
  duckSource: number;
  duckAmount: number;
  duckRelease: number;
}

export interface Pad {
  sample: string | null;
  name: string;
  gain: number;
  pitch: number;
  pan: number;
  decay: number;
  choke: number;
}

export interface Track {
  id: string;
  name: string;
  color: string;
  kind: TrackKind;
  mixer: Mixer;
  fx: TrackFx;
  synth: number[];
  preset?: string | null;
  pads: Pad[];
  clips: (Clip | null)[];
}

export interface Returns {
  reverbSize: number;
  reverbDamp: number;
  reverbWidth: number;
  reverbLevel: number;
  delayBeats: number;
  delayFeedback: number;
  delayTone: number;
  delayPingPong: boolean;
  delayLevel: number;
}

export interface Section {
  scene: number;
  bars: number;
}

export interface Project {
  version: number;
  name: string;
  bpm: number;
  swing: number;
  scenes: { name: string }[];
  tracks: Track[];
  returns: Returns;
  master: { volume: number; quantize: number };
  arrangement: Section[];
}

export interface SampleInfo {
  id: string;
  name: string;
  category: string;
  instrument: string;
  genre: string | null;
  bpm: number | null;
  key: string | null;
  bars: number | null;
  beats: number | null;
  kit: string | null;
  tags: string[];
  duration: number;
  channels: number;
  source: string;
  favorite: boolean;
  peaks: number[];
}
