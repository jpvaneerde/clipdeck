//! Render engine sessions to audio for the library.

use crate::drumsynth::SR;
use crate::music::Params;
use clipdeck_engine::{drums::param as dp, fx::track_param as tp, kind, master_param, return_param, sample::Sample, Clip, Engine, Note};

pub fn n(start: f64, len: f64, pitch: i32, vel: f32) -> Note {
    Note { start, len, pitch: pitch.clamp(0, 127) as u8, vel: vel.clamp(0.05, 1.0) }
}

pub struct PadDef {
    pub sample: Vec<f32>,
    pub gain: f32,
    pub pan: f32,
    pub choke: u8,
}

pub struct Session {
    pub e: Engine,
    next: u32,
    pub bpm: f32,
}

impl Session {
    pub fn new(bpm: f32, swing: f32) -> Self {
        let mut e = Engine::new(SR);
        e.set_bpm(bpm);
        e.set_master_param(master_param::SWING, swing);
        e.set_master_param(master_param::VOLUME, 1.0);
        e.set_return_param(return_param::REVERB_LEVEL, 0.9);
        e.set_return_param(return_param::DELAY_LEVEL, 0.8);
        Self { e, next: 1, bpm }
    }

    pub fn load(&mut self, mono: &[f32]) -> u32 {
        let h = self.next;
        self.next += 1;
        self.e.load_sample(h, Sample::new(mono.to_vec(), 1, SR));
        h
    }

    pub fn drums(&mut self, t: usize, pads: &[(u8, &PadDef)]) {
        self.e.set_track_kind(t, kind::DRUMS);
        for (pad, def) in pads {
            let h = self.load(&def.sample);
            let p = *pad as usize;
            self.e.set_pad_param(t, p, dp::SAMPLE, h as f32);
            self.e.set_pad_param(t, p, dp::GAIN, def.gain);
            self.e.set_pad_param(t, p, dp::PAN, def.pan);
            self.e.set_pad_param(t, p, dp::CHOKE, def.choke as f32);
        }
    }

    pub fn synth(&mut self, t: usize, params: &Params) {
        self.e.set_track_kind(t, kind::SYNTH);
        for (i, v) in params.iter().enumerate() {
            self.e.set_synth_param(t, i, *v);
        }
    }

    pub fn param(&mut self, t: usize, id: usize, v: f32) {
        self.e.set_track_param(t, id, v);
    }

    pub fn clip(&mut self, t: usize, beats: f64, notes: Vec<Note>) {
        self.e.set_clip(t, 0, Some(Clip { length: beats, notes, audio: None }));
    }

    /// Make track `t` pump against an inaudible four-on-the-floor kick.
    pub fn ghost_sidechain(&mut self, t: usize, ghost: usize, beats: f64, amount: f32) {
        let kick: Vec<f32> = (0..(SR * 0.15) as usize).map(|i| (1.0 - i as f32 / (SR * 0.15)).powi(2)).collect();
        let h = self.load(&kick);
        self.e.set_track_kind(ghost, kind::DRUMS);
        self.e.set_pad_param(ghost, 0, dp::SAMPLE, h as f32);
        self.param(ghost, tp::MUTE, 1.0);
        let notes = (0..beats as usize).map(|b| n(b as f64, 0.1, 36, 1.0)).collect();
        self.clip(ghost, beats, notes);
        self.param(t, tp::DUCK_SOURCE, ghost as f32);
        self.param(t, tp::DUCK_AMOUNT, amount);
        self.param(t, tp::DUCK_RELEASE, 60_000.0 / self.bpm * 0.45);
    }

    pub fn frames_for(&self, beats: f64) -> usize {
        (beats * 60.0 / self.bpm as f64 * SR as f64).round() as usize
    }

    /// Render a seamless loop: play it twice and keep the second pass, so
    /// tails from the end of the loop are already present at its start. The
    /// first 20 ms are crossfaded from the start of a third pass (the true
    /// continuation of the loop's end), which removes clicks from
    /// free-running oscillators and LFOs at the wrap point.
    pub fn render_loop(&mut self, beats: f64) -> Vec<f32> {
        self.e.stop();
        self.e.launch_scene(0);
        let first = self.frames_for(beats);
        let total = self.frames_for(beats * 2.0);
        let xf = (0.02 * SR) as usize;
        let (l, r) = self.e.render(total + xf);
        let mut out = Vec::with_capacity((total - first) * 2);
        for i in first..total {
            out.push(l[i]);
            out.push(r[i]);
        }
        let head: Vec<f32> = (total..total + xf).flat_map(|i| [l[i], r[i]]).collect();
        seam_crossfade(&mut out, &head);
        out
    }

    /// Render once from silence with a tail (for one-shots and FX).
    pub fn render_once(&mut self, beats: f64, tail_s: f32) -> Vec<f32> {
        self.e.stop();
        self.e.launch_scene(0);
        let frames = self.frames_for(beats);
        let (l, r) = self.e.render(frames);
        self.e.stop_all_clips();
        let (l2, r2) = self.e.render((tail_s * SR) as usize);
        let mut out = Vec::with_capacity((l.len() + l2.len()) * 2);
        for (a, b) in l.iter().chain(l2.iter()).zip(r.iter().chain(r2.iter())) {
            out.push(*a);
            out.push(*b);
        }
        out
    }
}

/// Peak-normalize interleaved stereo.
pub fn normalize(x: &mut [f32], peak: f32) {
    let m = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if m > 1e-6 {
        let g = peak / m;
        for v in x.iter_mut() {
            *v *= g;
        }
    }
}

/// Trim trailing silence from interleaved stereo and fade out the end.
pub fn trim_stereo(mut x: Vec<f32>) -> Vec<f32> {
    let mut end = x.len() / 2;
    while end > 256 && x[(end - 1) * 2].abs() < 2e-4 && x[(end - 1) * 2 + 1].abs() < 2e-4 {
        end -= 1;
    }
    let end = (end + 64).min(x.len() / 2);
    x.truncate(end * 2);
    let fade = ((0.01 * SR) as usize).min(end);
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        let f = end - 1 - i;
        x[f * 2] *= g;
        x[f * 2 + 1] *= g;
    }
    x
}

pub fn mono_of(x: &[f32]) -> Vec<f32> {
    x.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect()
}

/// Crossfade the start of a loop (interleaved stereo) from `continuation`,
/// the audio that would follow the loop's last frame.
pub fn seam_crossfade(out: &mut [f32], continuation: &[f32]) {
    let frames = (continuation.len() / 2).min(out.len() / 2);
    for i in 0..frames {
        let w = i as f32 / frames as f32;
        for c in 0..2 {
            let k = i * 2 + c;
            out[k] = continuation[k] * (1.0 - w) + out[k] * w;
        }
    }
}
