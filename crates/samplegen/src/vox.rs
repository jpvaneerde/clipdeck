//! Formant "vocal" synthesis for vox chops and vocal-style hooks.

use crate::drumsynth::SR;
use crate::rng::Rng;
use clipdeck_engine::dsp::{midi_to_hz, osc, wrap_phase, FilterMode, Svf, Wave, TAU};
use clipdeck_engine::fx::{Delay, Reverb};

pub const VOWELS: [&str; 5] = ["ah", "eh", "ee", "oh", "oo"];

/// (frequency, gain) for three formants.
const FORMANTS: [[(f32, f32); 3]; 5] = [
    [(800.0, 1.0), (1150.0, 0.5), (2900.0, 0.12)],
    [(400.0, 1.0), (1700.0, 0.2), (2600.0, 0.15)],
    [(270.0, 1.0), (2140.0, 0.25), (2950.0, 0.06)],
    [(450.0, 1.0), (800.0, 0.3), (2830.0, 0.08)],
    [(325.0, 1.0), (700.0, 0.16), (2700.0, 0.03)],
];

#[derive(Clone, Copy, Debug)]
pub struct VoxNote {
    pub start: f32,
    pub dur: f32,
    pub pitch: f32,
    pub from: usize,
    pub to: usize,
    /// Semitones to bend over the last part of the note (negative = fall).
    pub bend: f32,
}

pub fn sing(notes: &[VoxNote], total: f32, rng: &mut Rng, shift: f32) -> Vec<f32> {
    let len = (total * SR) as usize;
    let mut out = vec![0.0f32; len];
    let mut filters = [Svf::new(), Svf::new(), Svf::new()];
    let mut phase = 0.0f32;
    let mut cur_pitch = notes.first().map_or(60.0, |n| n.pitch);
    let vib_rate = rng.range(4.8, 6.0);
    let breath = rng.range(0.03, 0.08);
    let mut gains = [1.0f32, 0.5, 0.1];
    for (ni, note) in notes.iter().enumerate() {
        let s0 = (note.start * SR) as usize;
        let s1 = (((note.start + note.dur) * SR) as usize).min(len);
        let legato = ni > 0 && {
            let p = notes[ni - 1];
            (p.start + p.dur - note.start).abs() < 0.03
        };
        if !legato {
            cur_pitch = note.pitch;
        }
        let release = (0.06 * SR) as usize;
        for i in s0..(s1 + release).min(len) {
            let t = (i - s0) as f32 / SR;
            let frac = ((i - s0) as f32 / (s1 - s0).max(1) as f32).min(1.0);
            // Pitch: glide toward target, vibrato fades in, optional bend at the end.
            cur_pitch += (note.pitch - cur_pitch) * 0.002;
            let vib = (TAU * vib_rate * t).sin() * 0.25 * ((t - 0.15) / 0.3).clamp(0.0, 1.0);
            let bend = if frac > 0.6 { note.bend * ((frac - 0.6) / 0.4).powi(2) } else { 0.0 };
            let f0 = midi_to_hz(cur_pitch + vib + bend);
            let dt = f0 / SR;
            if i % 32 == 0 {
                let a = FORMANTS[note.from];
                let b = FORMANTS[note.to];
                let m = frac.powf(0.7);
                for k in 0..3 {
                    let f = (a[k].0 + (b[k].0 - a[k].0) * m) * shift;
                    filters[k].set(f, 0.92, SR);
                    gains[k] = a[k].1 + (b[k].1 - a[k].1) * m;
                }
            }
            let src = osc(Wave::Saw, phase, dt) * 0.6 + rng.noise() * breath;
            phase = wrap_phase(phase + dt);
            let mut y = 0.0;
            for k in 0..3 {
                y += filters[k].process(src, FilterMode::Band) * gains[k];
            }
            let attack = (t / 0.025).min(1.0);
            let rel = if i >= s1 { 1.0 - (i - s1) as f32 / release as f32 } else { 1.0 };
            out[i] += y * attack * rel * 0.5;
        }
    }
    out
}

/// Mono dry vocal → stereo with reverb (and optional delay).
pub fn space(dry: &[f32], bpm: f32, reverb: f32, delay: f32) -> Vec<f32> {
    let n = dry.len();
    let mut l = dry.to_vec();
    let mut r = dry.to_vec();
    let mut rev = Reverb::new(SR);
    rev.size = 0.8;
    rev.damp = 0.5;
    let mut ol = vec![0.0; n];
    let mut or = vec![0.0; n];
    rev.process(dry, dry, &mut ol, &mut or, reverb);
    if delay > 0.0 {
        let mut d = Delay::new(SR);
        d.beats = 0.75;
        d.feedback = 0.35;
        d.tone = 3500.0;
        d.process(dry, dry, &mut ol, &mut or, bpm, delay);
    }
    for i in 0..n {
        l[i] += ol[i];
        r[i] += or[i];
    }
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        out.push(l[i]);
        out.push(r[i]);
    }
    out
}
