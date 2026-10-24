//! 16-pad drum rack. MIDI notes 36..=51 trigger pads 0..=15.

use crate::dsp;
use crate::sample::SampleBank;

pub const PADS: usize = 16;
pub const BASE_NOTE: u8 = 36;
const VOICES: usize = 24;

pub mod param {
    pub const SAMPLE: usize = 0;
    pub const GAIN: usize = 1;
    pub const PITCH: usize = 2;
    pub const PAN: usize = 3;
    pub const DECAY: usize = 4;
    pub const CHOKE: usize = 5;
    pub const COUNT: usize = 6;
}

#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub sample: i32,
    pub gain: f32,
    pub pitch: f32,
    pub pan: f32,
    /// Seconds until the pad fades out; 0 plays the whole sample.
    pub decay: f32,
    /// Pads sharing a non-zero choke group cut each other off (hi-hats).
    pub choke: u8,
}

impl Default for Pad {
    fn default() -> Self {
        Self { sample: -1, gain: 1.0, pitch: 0.0, pan: 0.0, decay: 0.0, choke: 0 }
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    pad: usize,
    sample: i32,
    pos: f64,
    rate: f64,
    gl: f32,
    gr: f32,
    env: f32,
    env_coef: f32,
    hold: u32,
    fade: f32,
    fading: bool,
    age: u64,
}

pub struct DrumRack {
    pub pads: [Pad; PADS],
    voices: [Voice; VOICES],
    sr: f32,
    age: u64,
}

impl DrumRack {
    pub fn new(sr: f32) -> Self {
        Self { pads: [Pad::default(); PADS], voices: [Voice::default(); VOICES], sr, age: 0 }
    }

    pub fn set_pad_param(&mut self, pad: usize, id: usize, value: f32) {
        let Some(p) = self.pads.get_mut(pad) else { return };
        if !value.is_finite() {
            return;
        }
        match id {
            param::SAMPLE => p.sample = value as i32,
            param::GAIN => p.gain = value.clamp(0.0, 4.0),
            param::PITCH => p.pitch = value.clamp(-48.0, 48.0),
            param::PAN => p.pan = value.clamp(-1.0, 1.0),
            param::DECAY => p.decay = value.max(0.0),
            param::CHOKE => p.choke = value.clamp(0.0, 255.0) as u8,
            _ => {}
        }
    }

    pub fn trigger(&mut self, note: u8, vel: f32, bank: &SampleBank) {
        if note < BASE_NOTE {
            return;
        }
        let pad_idx = (note - BASE_NOTE) as usize;
        if pad_idx >= PADS {
            return;
        }
        let pad = self.pads[pad_idx];
        let Some(sample) = bank.get(pad.sample) else { return };

        if pad.choke != 0 {
            for v in self.voices.iter_mut() {
                if v.active && self.pads[v.pad].choke == pad.choke {
                    v.fading = true;
                }
            }
        }
        // Retriggering the same pad fades the old hit to avoid clicks.
        for v in self.voices.iter_mut() {
            if v.active && v.pad == pad_idx {
                v.fading = true;
            }
        }

        self.age += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0));
        let (pl, pr) = dsp::pan_gains(pad.pan);
        let g = pad.gain * vel.clamp(0.0, 1.0).powf(0.7);
        let rate = (sample.sample_rate / self.sr) as f64 * dsp::fast_exp2(pad.pitch / 12.0) as f64;
        let (hold, env_coef) = if pad.decay > 0.0 {
            ((pad.decay * 0.3 * self.sr) as u32, dsp::one_pole_coef(pad.decay * 0.7, self.sr))
        } else {
            (u32::MAX, 0.0)
        };
        self.voices[idx] = Voice {
            active: true,
            pad: pad_idx,
            sample: pad.sample,
            pos: 0.0,
            rate,
            gl: g * pl * std::f32::consts::SQRT_2,
            gr: g * pr * std::f32::consts::SQRT_2,
            env: 1.0,
            env_coef,
            hold,
            fade: 1.0,
            fading: false,
            age: self.age,
        };
    }

    pub fn stop_all(&mut self) {
        for v in self.voices.iter_mut() {
            v.fading = true;
        }
    }

    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], bank: &SampleBank) {
        let n = l.len().min(r.len());
        let fade_step = 1.0 / (0.004 * self.sr);
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let Some(s) = bank.get(v.sample) else {
                v.active = false;
                continue;
            };
            for i in 0..n {
                if v.pos >= s.frames as f64 {
                    v.active = false;
                    break;
                }
                if v.hold == 0 {
                    v.env -= v.env * v.env_coef;
                    if v.env < 1e-4 {
                        v.active = false;
                        break;
                    }
                } else if v.hold != u32::MAX {
                    v.hold -= 1;
                }
                if v.fading {
                    v.fade -= fade_step;
                    if v.fade <= 0.0 {
                        v.active = false;
                        break;
                    }
                }
                let (a, b) = s.read(v.pos);
                let g = v.env * v.fade;
                l[i] += a * v.gl * g;
                r[i] += b * v.gr * g;
                v.pos += v.rate;
            }
        }
    }
}
