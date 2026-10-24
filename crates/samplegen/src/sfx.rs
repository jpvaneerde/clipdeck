//! Transition effects: risers, downlifters, impacts, sweeps, reverse cymbals, zaps.

use crate::drumsynth::{self, SR};
use crate::rng::Rng;
use crate::vox::space;
use clipdeck_engine::dsp::{osc, soft_clip, wrap_phase, FilterMode, Svf, Wave, TAU};

fn lerp_exp(a: f32, b: f32, t: f32) -> f32 {
    a * (b / a).powf(t.clamp(0.0, 1.0))
}

pub fn riser(rng: &mut Rng, seconds: f32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    let mut out = vec![0.0f32; n];
    let mut bp = Svf::new();
    let (f_lo, f_hi) = (rng.range(150.0, 400.0), rng.range(6000.0, 12000.0));
    let tone = rng.range(0.0, 0.6);
    let (p_lo, p_hi) = (rng.range(80.0, 200.0), rng.range(800.0, 2000.0));
    let mut ph = [0.0f32; 3];
    for (i, v) in out.iter_mut().enumerate() {
        let t = i as f32 / n as f32;
        if i % 16 == 0 {
            bp.set(lerp_exp(f_lo, f_hi, t), 0.55, SR);
        }
        let noise = bp.process(rng.noise(), FilterMode::Band);
        let f = lerp_exp(p_lo, p_hi, t.powf(1.5));
        let mut saw = 0.0;
        for (k, p) in ph.iter_mut().enumerate() {
            let dt = f * (1.0 + 0.006 * k as f32) / SR;
            saw += osc(Wave::Saw, *p, dt);
            *p = wrap_phase(*p + dt);
        }
        let env = t.powf(1.6);
        *v = (noise * 1.5 + saw * 0.2 * tone) * env;
    }
    out
}

pub fn downlifter(rng: &mut Rng, seconds: f32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    let mut out = vec![0.0f32; n];
    let mut lp = Svf::new();
    let (f_hi, f_lo) = (rng.range(8000.0, 14000.0), rng.range(150.0, 400.0));
    let mut p = 0.0f32;
    let (p_hi, p_lo) = (rng.range(400.0, 900.0), rng.range(40.0, 80.0));
    for (i, v) in out.iter_mut().enumerate() {
        let t = i as f32 / n as f32;
        if i % 16 == 0 {
            lp.set(lerp_exp(f_hi, f_lo, t), 0.3, SR);
        }
        let f = lerp_exp(p_hi, p_lo, t);
        p = wrap_phase(p + f / SR);
        let env = (1.0 - t).powf(1.3) * (i as f32 / (0.005 * SR)).min(1.0);
        *v = (lp.process(rng.noise(), FilterMode::Low) + (TAU * p).sin() * 0.4) * env;
    }
    out
}

pub fn impact(rng: &mut Rng) -> Vec<f32> {
    let secs = rng.range(1.8, 3.5);
    let n = (secs * SR) as usize;
    let mut out = vec![0.0f32; n];
    let (f0, f1) = (rng.range(70.0, 120.0), rng.range(28.0, 40.0));
    let mut p = 0.0f32;
    let mut lp = Svf::new();
    lp.set(rng.range(1500.0, 4000.0), 0.1, SR);
    let tau = secs * 0.3;
    for (i, v) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let f = f1 + (f0 - f1) * (-t / 0.15).exp();
        p = wrap_phase(p + f / SR);
        let boom = (TAU * p).sin() * (-t / tau).exp();
        let crack = lp.process(rng.noise(), FilterMode::Low) * (-t / 0.25).exp();
        *v = soft_clip((boom * 1.2 + crack * 0.8) * 1.5);
    }
    out
}

pub fn sweep(rng: &mut Rng, seconds: f32, up: bool) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    let mut out = vec![0.0f32; n];
    let mut f = Svf::new();
    let (a, b) = if up { (200.0, 12000.0) } else { (12000.0, 200.0) };
    let reso = rng.range(0.3, 0.75);
    for (i, v) in out.iter_mut().enumerate() {
        let t = i as f32 / n as f32;
        if i % 16 == 0 {
            f.set(lerp_exp(a, b, t), reso, SR);
        }
        let env = (t * 8.0).min(1.0) * ((1.0 - t) * 8.0).min(1.0);
        *v = f.process(rng.noise(), FilterMode::Low) * env;
    }
    out
}

pub fn reverse_crash(rng: &mut Rng) -> Vec<f32> {
    let mut c = drumsynth::cymbal(rng, false);
    c.reverse();
    c
}

pub fn zap(rng: &mut Rng) -> Vec<f32> {
    let secs = rng.range(0.15, 0.5);
    let n = (secs * SR) as usize;
    let (f0, f1) = (rng.range(1500.0, 4000.0), rng.range(60.0, 200.0));
    let mut p = 0.0f32;
    let wave = *rng.pick(&[Wave::Sine, Wave::Square, Wave::Saw]);
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let f = lerp_exp(f0, f1, t.powf(0.5));
            let dt = f / SR;
            let s = osc(wave, p, dt);
            p = wrap_phase(p + dt);
            s * (1.0 - t) * 0.7
        })
        .collect()
}

/// Wrap mono FX in stereo space.
pub fn finish(mono: Vec<f32>, bpm: f32, rng: &mut Rng) -> Vec<f32> {
    let pad = vec![0.0f32; (SR * 1.5) as usize];
    let mut x = mono;
    x.extend_from_slice(&pad);
    space(&x, bpm, rng.range(0.3, 0.8), if rng.chance(0.4) { 0.3 } else { 0.0 })
}
