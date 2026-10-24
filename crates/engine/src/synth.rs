//! Polyphonic subtractive synthesizer: two band-limited oscillators, sub,
//! noise, stereo spread, resonant filter with its own envelope, LFO, glide.

use crate::dsp::{self, Adsr, FilterMode, Noise, Svf, Wave};

pub mod param {
    pub const OSC1_WAVE: usize = 0;
    pub const OSC2_WAVE: usize = 1;
    pub const OSC2_SEMI: usize = 2;
    pub const OSC2_DETUNE: usize = 3;
    pub const OSC_MIX: usize = 4;
    pub const SUB: usize = 5;
    pub const NOISE: usize = 6;
    pub const CUTOFF: usize = 7;
    pub const RESO: usize = 8;
    pub const FILTER_ENV: usize = 9;
    pub const KEY_TRACK: usize = 10;
    pub const AMP_A: usize = 11;
    pub const AMP_D: usize = 12;
    pub const AMP_S: usize = 13;
    pub const AMP_R: usize = 14;
    pub const FILT_A: usize = 15;
    pub const FILT_D: usize = 16;
    pub const FILT_S: usize = 17;
    pub const FILT_R: usize = 18;
    pub const LFO_RATE: usize = 19;
    pub const LFO_CUTOFF: usize = 20;
    pub const LFO_PITCH: usize = 21;
    pub const GLIDE: usize = 22;
    pub const MONO: usize = 23;
    pub const SPREAD: usize = 24;
    pub const LEVEL: usize = 25;
    pub const VEL_SENS: usize = 26;
    pub const DRIVE: usize = 27;
    pub const COUNT: usize = 28;
}

pub const DEFAULTS: [f32; param::COUNT] = [
    0.0,    // osc1 saw
    0.0,    // osc2 saw
    0.0,    // osc2 semitones
    8.0,    // osc2 detune cents
    0.5,    // osc mix
    0.0,    // sub
    0.0,    // noise
    2400.0, // cutoff Hz
    0.2,    // resonance
    2.0,    // filter env amount (octaves)
    0.5,    // key tracking
    0.005,  // amp attack
    0.3,    // amp decay
    0.7,    // amp sustain
    0.25,   // amp release
    0.002,  // filter attack
    0.25,   // filter decay
    0.2,    // filter sustain
    0.3,    // filter release
    4.0,    // lfo rate Hz
    0.0,    // lfo -> cutoff (octaves)
    0.0,    // lfo -> pitch (semitones)
    0.0,    // glide seconds
    0.0,    // mono
    0.3,    // stereo spread
    0.8,    // level
    0.5,    // velocity sensitivity
    0.0,    // drive
];

const VOICES: usize = 12;
const CONTROL_INTERVAL: u32 = 16;
pub const GATE_HELD: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    note: u8,
    cur_note: f32,
    target_note: f32,
    vel: f32,
    p1: f32,
    p2: f32,
    psub: f32,
    dt1: f32,
    dt2: f32,
    dtsub: f32,
    amp: Adsr,
    filt: Adsr,
    svf_l: Svf,
    svf_r: Svf,
    gate_left: u32,
    age: u64,
    ctr: u32,
}

impl Voice {
    fn new() -> Self {
        Self {
            active: false,
            note: 0,
            cur_note: 60.0,
            target_note: 60.0,
            vel: 1.0,
            p1: 0.0,
            p2: 0.0,
            psub: 0.0,
            dt1: 0.0,
            dt2: 0.0,
            dtsub: 0.0,
            amp: Adsr::new(),
            filt: Adsr::new(),
            svf_l: Svf::new(),
            svf_r: Svf::new(),
            gate_left: GATE_HELD,
            age: 0,
            ctr: 0,
        }
    }
}

pub struct PolySynth {
    pub params: [f32; param::COUNT],
    voices: [Voice; VOICES],
    sr: f32,
    lfo_phase: f32,
    noise: Noise,
    age: u64,
    envs_dirty: bool,
}

impl PolySynth {
    pub fn new(sr: f32) -> Self {
        Self {
            params: DEFAULTS,
            voices: [Voice::new(); VOICES],
            sr,
            lfo_phase: 0.0,
            noise: Noise::new(0x9e37_79b9),
            age: 0,
            envs_dirty: true,
        }
    }

    pub fn set_param(&mut self, id: usize, value: f32) {
        if id < param::COUNT && value.is_finite() {
            self.params[id] = value;
            if (param::AMP_A..=param::FILT_R).contains(&id) {
                self.envs_dirty = true;
            }
        }
    }

    fn update_envs(&mut self) {
        let p = &self.params;
        let sr = self.sr;
        for v in self.voices.iter_mut() {
            v.amp.set(p[param::AMP_A], p[param::AMP_D], p[param::AMP_S], p[param::AMP_R], sr);
            v.filt.set(p[param::FILT_A], p[param::FILT_D], p[param::FILT_S], p[param::FILT_R], sr);
        }
        self.envs_dirty = false;
    }

    fn is_mono(&self) -> bool {
        self.params[param::MONO] >= 0.5
    }

    /// Start a note. `gate` is its length in samples, or `GATE_HELD` until `note_off`.
    pub fn note_on(&mut self, note: u8, vel: f32, gate: u32) {
        if self.envs_dirty {
            self.update_envs();
        }
        self.age += 1;
        let age = self.age;
        let vel_sens = self.params[param::VEL_SENS];
        let vel_gain = 1.0 - vel_sens + vel_sens * vel.clamp(0.0, 1.0);

        if self.is_mono() {
            let glide = self.params[param::GLIDE] > 0.0005;
            let v = &mut self.voices[0];
            let sounding = v.active && !v.amp.is_releasing();
            v.note = note;
            v.target_note = note as f32;
            if !(glide && sounding) {
                v.cur_note = note as f32;
            }
            if !v.active {
                v.p1 = 0.0;
                v.p2 = 0.25;
                v.psub = 0.0;
                v.svf_l.reset();
                v.svf_r.reset();
            }
            v.active = true;
            v.vel = vel_gain;
            v.gate_left = gate;
            v.age = age;
            v.ctr = 0;
            v.amp.gate_on();
            v.filt.gate_on();
            return;
        }

        // Poly: reuse a voice already on this note, else a free one, else steal.
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                // Prefer stealing released voices, then the oldest.
                let mut best = 0;
                let mut best_score = u64::MAX;
                for (i, v) in self.voices.iter().enumerate() {
                    let score = if v.amp.is_releasing() { v.age / 2 } else { v.age };
                    if score < best_score {
                        best_score = score;
                        best = i;
                    }
                }
                best
            });
        let phase_seed = self.noise.next();
        let v = &mut self.voices[idx];
        if !v.active {
            v.svf_l.reset();
            v.svf_r.reset();
            v.amp.reset();
            v.filt.reset();
        }
        v.active = true;
        v.note = note;
        v.cur_note = note as f32;
        v.target_note = note as f32;
        v.vel = vel_gain;
        v.p1 = 0.0;
        v.p2 = phase_seed.abs();
        v.psub = 0.0;
        v.gate_left = gate;
        v.age = age;
        v.ctr = 0;
        v.amp.gate_on();
        v.filt.gate_on();
    }

    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut() {
            if v.active && v.note == note && v.gate_left == GATE_HELD {
                v.amp.gate_off();
                v.filt.gate_off();
            }
        }
    }

    /// Release every sounding voice (used when a clip stops or switches).
    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            if v.active {
                v.amp.gate_off();
                v.filt.gate_off();
            }
        }
    }

    pub fn panic(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
            v.amp.reset();
            v.filt.reset();
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Add this synth's output into `l`/`r`.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.envs_dirty {
            self.update_envs();
        }
        let n = l.len().min(r.len());
        let p = self.params;
        let sr = self.sr;
        let inv_sr = 1.0 / sr;

        // LFO is shared by all voices; advance it once per block segment.
        let lfo_inc = p[param::LFO_RATE].max(0.0) * inv_sr;
        let lfo_start = self.lfo_phase;
        self.lfo_phase = dsp::wrap_phase(self.lfo_phase + lfo_inc * n as f32);

        let w1 = Wave::from_f32(p[param::OSC1_WAVE]);
        let w2 = Wave::from_f32(p[param::OSC2_WAVE]);
        let mix = p[param::OSC_MIX].clamp(0.0, 1.0);
        let g1 = 1.0 - mix;
        let g2 = mix;
        let sub = p[param::SUB].clamp(0.0, 1.0);
        let noise_amt = p[param::NOISE].clamp(0.0, 1.0);
        let spread = p[param::SPREAD].clamp(0.0, 1.0);
        let reso = p[param::RESO];
        let level = p[param::LEVEL] * 0.28;
        let drive = p[param::DRIVE].clamp(0.0, 1.0);
        let drive_k = 1.0 + drive * 6.0;
        let drive_norm = 1.0 / (1.0 + drive * 1.5);
        let glide_coef = dsp::one_pole_coef(p[param::GLIDE].max(0.0), sr / CONTROL_INTERVAL as f32);
        let mono = self.is_mono();

        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let mut fenv = v.filt.level;
            for i in 0..n {
                if v.ctr == 0 {
                    // Control-rate updates: pitch, glide, filter coefficients.
                    let lfo_phase = dsp::wrap_phase(lfo_start + lfo_inc * i as f32);
                    let lfo = (dsp::TAU * lfo_phase).sin();
                    if mono {
                        v.cur_note += (v.target_note - v.cur_note) * glide_coef;
                    }
                    let base = v.cur_note + lfo * p[param::LFO_PITCH];
                    let f1 = dsp::midi_to_hz(base);
                    let f2 = dsp::midi_to_hz(base + p[param::OSC2_SEMI] + p[param::OSC2_DETUNE] * 0.01);
                    v.dt1 = (f1 * inv_sr).min(0.49);
                    v.dt2 = (f2 * inv_sr).min(0.49);
                    v.dtsub = v.dt1 * 0.5;
                    let octaves = fenv * p[param::FILTER_ENV] + lfo * p[param::LFO_CUTOFF] + (base - 60.0) / 12.0 * p[param::KEY_TRACK];
                    let fc = p[param::CUTOFF] * dsp::fast_exp2(octaves);
                    v.svf_l.set(fc, reso, sr);
                    let c = v.svf_l;
                    v.svf_r.copy_coefs(&c);
                }
                v.ctr = (v.ctr + 1) % CONTROL_INTERVAL;

                if v.gate_left != GATE_HELD {
                    if v.gate_left == 0 {
                        v.amp.gate_off();
                        v.filt.gate_off();
                        v.gate_left = GATE_HELD - 1; // released; never re-trigger
                    } else if v.gate_left != GATE_HELD - 1 {
                        v.gate_left -= 1;
                    }
                }

                let a = v.amp.next();
                fenv = v.filt.next();
                if v.amp.is_idle() {
                    v.active = false;
                    break;
                }

                let o1 = dsp::osc(w1, v.p1, v.dt1) * g1;
                let o2 = dsp::osc(w2, v.p2, v.dt2) * g2;
                v.p1 = dsp::wrap_phase(v.p1 + v.dt1);
                v.p2 = dsp::wrap_phase(v.p2 + v.dt2);
                let mut center = 0.0;
                if sub > 0.0 {
                    center += (dsp::TAU * v.psub).sin() * sub;
                    v.psub = dsp::wrap_phase(v.psub + v.dtsub);
                }
                if noise_amt > 0.0 {
                    center += self.noise.next() * noise_amt * 0.5;
                }
                let in_l = o1 + o2 * (1.0 - spread) + center;
                let in_r = o1 * (1.0 - spread) + o2 + center;
                let mut yl = v.svf_l.process(in_l, FilterMode::Low);
                let mut yr = v.svf_r.process(in_r, FilterMode::Low);
                if drive > 0.0 {
                    yl = dsp::soft_clip(yl * drive_k) * drive_norm;
                    yr = dsp::soft_clip(yr * drive_k) * drive_norm;
                }
                let g = a * v.vel * level;
                l[i] += yl * g;
                r[i] += yr * g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_produces_sound_and_ends() {
        let sr = 48000.0;
        let mut s = PolySynth::new(sr);
        s.note_on(60, 1.0, 4800);
        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        s.render(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.05, "peak {peak}");
        // Let it release.
        for _ in 0..20 {
            let mut l = vec![0.0; 4800];
            let mut r = vec![0.0; 4800];
            s.render(&mut l, &mut r);
        }
        assert_eq!(s.active_voices(), 0);
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn polyphony_steals_when_full() {
        let mut s = PolySynth::new(48000.0);
        for n in 0..20 {
            s.note_on(40 + n, 0.8, GATE_HELD);
        }
        assert_eq!(s.active_voices(), VOICES);
    }
}
