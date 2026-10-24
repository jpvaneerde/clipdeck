// Parameter ids shared with the Rust engine (crates/engine).

export const MAX_TRACKS = 16;
export const MAX_SCENES = 16;
export const STATE_HEADER = 8;
export const STATE_TRACK_STRIDE = 8;

export const KIND = { none: 0, synth: 1, drums: 2, audio: 3 } as const;

/** fx::track_param */
export const TP = {
  volume: 0,
  pan: 1,
  mute: 2,
  solo: 3,
  sendA: 4,
  sendB: 5,
  filterType: 6,
  cutoff: 7,
  reso: 8,
  drive: 9,
  compThreshold: 10,
  compRatio: 11,
  compAttack: 12,
  compRelease: 13,
  compMakeup: 14,
  duckSource: 15,
  duckAmount: 16,
  duckRelease: 17,
} as const;

/** drums::param */
export const PAD = { sample: 0, gain: 1, pitch: 2, pan: 3, decay: 4, choke: 5 } as const;

/** master_param */
export const MASTER = { volume: 0, swing: 1, quantize: 2, metronome: 3 } as const;

/** return_param */
export const RET = {
  reverbSize: 0,
  reverbDamp: 1,
  reverbWidth: 2,
  reverbLevel: 3,
  delayBeats: 4,
  delayFeedback: 5,
  delayTone: 6,
  delayPingPong: 7,
  delayLevel: 8,
} as const;

/** synth::param, in index order. */
export const SYNTH_PARAMS = [
  "osc1Wave",
  "osc2Wave",
  "osc2Semi",
  "osc2Detune",
  "oscMix",
  "sub",
  "noise",
  "cutoff",
  "reso",
  "filterEnv",
  "keyTrack",
  "ampA",
  "ampD",
  "ampS",
  "ampR",
  "filtA",
  "filtD",
  "filtS",
  "filtR",
  "lfoRate",
  "lfoCutoff",
  "lfoPitch",
  "glide",
  "mono",
  "spread",
  "level",
  "velSens",
  "drive",
] as const;

export type SynthParam = (typeof SYNTH_PARAMS)[number];
export const SP = Object.fromEntries(SYNTH_PARAMS.map((n, i) => [n, i])) as Record<SynthParam, number>;

export const SYNTH_DEFAULTS = [
  0, 0, 0, 8, 0.5, 0, 0, 2400, 0.2, 2, 0.5, 0.005, 0.3, 0.7, 0.25, 0.002, 0.25, 0.2, 0.3, 4, 0, 0, 0, 0, 0.3, 0.8, 0.5, 0,
];
