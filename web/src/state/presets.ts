import { SP, SYNTH_DEFAULTS, type SynthParam } from "../audio/ids";

type Overrides = Partial<Record<SynthParam, number>>;

/** Same sound families the library generator uses (crates/samplegen/src/music.rs). */
const PRESETS: Record<string, { group: string; o: Overrides }> = {
  "Sub Bass": { group: "Bass", o: { osc1Wave: 3, osc2Wave: 2, oscMix: 0.15, osc2Semi: 12, cutoff: 400, filterEnv: 0.5, ampA: 0.004, ampD: 0.4, ampS: 0.85, ampR: 0.06, mono: 1, glide: 0.02, spread: 0, drive: 0.2, level: 1.1, keyTrack: 0.3 } },
  "Pluck Bass": { group: "Bass", o: { osc2Wave: 1, osc2Semi: -12, osc2Detune: 3, cutoff: 300, reso: 0.3, filterEnv: 3.2, filtD: 0.15, filtS: 0, ampD: 0.3, ampS: 0.3, ampR: 0.08, mono: 1, spread: 0, sub: 0.4, level: 1 } },
  "Acid Line": { group: "Bass", o: { oscMix: 0, cutoff: 320, reso: 0.8, filterEnv: 3.8, filtD: 0.18, filtS: 0, ampD: 0.4, ampS: 0.6, ampR: 0.05, mono: 1, glide: 0.06, spread: 0, drive: 0.5, velSens: 0.8, level: 0.8 } },
  "Reese": { group: "Bass", o: { osc2Detune: 25, cutoff: 700, reso: 0.25, filterEnv: 0.5, ampA: 0.01, ampS: 1, ampR: 0.1, sub: 0.6, spread: 0.5, lfoRate: 0.4, lfoCutoff: 0.6, mono: 1, glide: 0.04, drive: 0.4, level: 0.8 } },
  "808": { group: "Bass", o: { osc1Wave: 3, oscMix: 0, cutoff: 3000, filterEnv: 0, keyTrack: 0, ampA: 0.002, ampD: 1.2, ampS: 0.35, ampR: 0.12, mono: 1, glide: 0.07, drive: 0.5, spread: 0, level: 1.3 } },
  "Organ Bass": { group: "Bass", o: { osc1Wave: 1, osc2Wave: 3, osc2Semi: 12, cutoff: 900, filterEnv: 1.5, filtD: 0.15, filtS: 0.2, ampD: 0.2, ampS: 0.5, ampR: 0.06, mono: 1, spread: 0, sub: 0.5 } },
  "Disco Bass": { group: "Bass", o: { cutoff: 700, filterEnv: 2.5, filtD: 0.12, filtS: 0.1, reso: 0.25, ampD: 0.18, ampS: 0.35, ampR: 0.05, mono: 1, spread: 0, sub: 0.3 } },
  "Warm Pad": { group: "Chords", o: { osc2Detune: 12, spread: 0.85, cutoff: 1500, reso: 0.15, filterEnv: 1, filtA: 0.6, filtD: 1.5, filtS: 0.4, ampA: 0.4, ampD: 1, ampS: 0.85, ampR: 1.2, lfoRate: 0.3, lfoCutoff: 0.3, level: 0.55, velSens: 0.2 } },
  "Keys": { group: "Chords", o: { osc1Wave: 3, osc2Wave: 2, osc2Semi: 12, osc2Detune: 2, oscMix: 0.25, cutoff: 3500, filterEnv: 1, filtD: 0.6, filtS: 0.3, keyTrack: 1, ampA: 0.002, ampD: 1.2, ampS: 0.25, ampR: 0.4, lfoRate: 4.5, lfoPitch: 0.03, spread: 0.4, velSens: 0.7, drive: 0.15, level: 0.75 } },
  "Lo-Fi Keys": { group: "Chords", o: { osc1Wave: 3, osc2Wave: 2, osc2Semi: 12, osc2Detune: 2, oscMix: 0.25, cutoff: 1800, filterEnv: 1, filtD: 0.6, filtS: 0.3, keyTrack: 1, ampA: 0.002, ampD: 1.2, ampS: 0.25, ampR: 0.4, lfoRate: 0.6, lfoPitch: 0.12, noise: 0.02, spread: 0.4, velSens: 0.7, drive: 0.3, level: 0.75 } },
  "House Stab": { group: "Chords", o: { osc1Wave: 1, osc2Semi: 12, osc2Detune: 6, cutoff: 1900, reso: 0.2, filterEnv: 1.5, filtD: 0.18, filtS: 0.1, ampD: 0.25, ampS: 0.15, ampR: 0.15, spread: 0.6, level: 0.6 } },
  "Brass": { group: "Chords", o: { osc2Detune: 9, cutoff: 900, reso: 0.15, filterEnv: 2.5, filtA: 0.04, filtD: 0.4, filtS: 0.4, ampA: 0.02, ampS: 0.8, ampR: 0.3, spread: 0.7, level: 0.6 } },
  "Supersaw": { group: "Chords", o: { osc2Detune: 20, spread: 0.95, cutoff: 3500, filterEnv: 1, filtD: 0.5, filtS: 0.6, ampA: 0.01, ampS: 0.8, ampR: 0.35, level: 0.45 } },
  "Pluck": { group: "Lead", o: { osc2Wave: 1, osc2Detune: 8, cutoff: 750, reso: 0.2, filterEnv: 3.8, filtD: 0.16, filtS: 0, ampD: 0.4, ampS: 0, ampR: 0.3, spread: 0.7, level: 0.7 } },
  "Saw Lead": { group: "Lead", o: { osc2Wave: 1, osc2Detune: 6, cutoff: 2600, reso: 0.2, filterEnv: 1.5, filtD: 0.3, filtS: 0.5, ampA: 0.005, ampS: 0.75, ampR: 0.2, lfoRate: 5.5, lfoPitch: 0.12, glide: 0.03, mono: 1, spread: 0.4, level: 0.6 } },
  "Bell": { group: "Lead", o: { osc1Wave: 3, osc2Wave: 3, osc2Semi: 19, osc2Detune: 3, oscMix: 0.35, cutoff: 9000, filterEnv: 0, ampA: 0.001, ampD: 1.2, ampS: 0, ampR: 0.8, spread: 0.5, level: 0.75 } },
  "Flute": { group: "Lead", o: { osc1Wave: 3, osc2Wave: 2, oscMix: 0.2, noise: 0.12, cutoff: 3200, filterEnv: 0.3, ampA: 0.06, ampS: 0.85, ampR: 0.15, lfoRate: 5, lfoPitch: 0.15, mono: 1, glide: 0.04, spread: 0, level: 0.8 } },
  "Chip": { group: "Lead", o: { osc1Wave: 1, oscMix: 0, cutoff: 7000, filterEnv: 0, ampA: 0.001, ampD: 0.2, ampS: 0.6, ampR: 0.05, mono: 1, spread: 0, level: 0.45 } },
};

export const PRESET_NAMES = Object.keys(PRESETS);
export const PRESET_GROUPS = [...new Set(Object.values(PRESETS).map((p) => p.group))];
export const presetsIn = (group: string) => PRESET_NAMES.filter((n) => PRESETS[n].group === group);

export function presetParams(name: string): number[] {
  const p = [...SYNTH_DEFAULTS];
  const o = PRESETS[name]?.o ?? {};
  for (const [k, v] of Object.entries(o)) p[SP[k as SynthParam]] = v as number;
  return p;
}
