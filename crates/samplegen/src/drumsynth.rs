//! Analog-style drum synthesis. Every function returns mono audio and
//! randomizes its character from `rng`, so each call is a new sound.

use crate::rng::Rng;
use clipdeck_engine::dsp::{soft_clip, FilterMode, Svf, TAU};

pub const SR: f32 = 44100.0;

fn buf(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * SR) as usize]
}

#[inline]
fn exp_env(t: f32, tau: f32) -> f32 {
    (-t / tau.max(1e-4)).exp()
}

/// Peak-normalize to `peak` and fade the last few ms.
pub fn finish(mut x: Vec<f32>, peak: f32) -> Vec<f32> {
    // Trim trailing near-silence.
    let thresh = 1e-4;
    let mut end = x.len();
    while end > 64 && x[end - 1].abs() < thresh {
        end -= 1;
    }
    x.truncate((end + 32).min(x.len()));
    let m = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if m > 1e-9 {
        let g = peak / m;
        for v in x.iter_mut() {
            *v *= g;
        }
    }
    let fade = ((0.004 * SR) as usize).min(x.len());
    let n = x.len();
    for i in 0..fade {
        x[n - 1 - i] *= i as f32 / fade as f32;
    }
    x
}

fn metallic(t: f32, base: f32, phases: &mut [f32; 6], detune: f32) -> f32 {
    // TR-808 cymbal oscillator ratios.
    const R: [f32; 6] = [2.0, 3.0, 4.16, 5.43, 6.79, 8.21];
    let mut s = 0.0;
    for (i, p) in phases.iter_mut().enumerate() {
        *p += base * R[i] * (1.0 + detune * i as f32 * 0.01) / SR;
        if *p >= 1.0 {
            *p -= 1.0;
        }
        s += if *p < 0.5 { 1.0 } else { -1.0 };
    }
    let _ = t;
    s / 6.0
}

pub fn kick(rng: &mut Rng, style: &str) -> Vec<f32> {
    let (len, f_end, f_start, pitch_tau, amp_tau, drive, click) = match style {
        "techno" => (
            rng.range(0.35, 0.7),
            rng.range(42.0, 52.0),
            rng.range(180.0, 320.0),
            rng.range(0.025, 0.05),
            rng.range(0.18, 0.32),
            rng.range(2.0, 4.5),
            rng.range(0.1, 0.35),
        ),
        "hiphop" => (
            rng.range(0.3, 0.6),
            rng.range(45.0, 60.0),
            rng.range(120.0, 220.0),
            rng.range(0.02, 0.05),
            rng.range(0.12, 0.25),
            rng.range(1.2, 2.5),
            rng.range(0.15, 0.45),
        ),
        "trap" => (
            rng.range(0.25, 0.45),
            rng.range(50.0, 62.0),
            rng.range(160.0, 300.0),
            rng.range(0.012, 0.03),
            rng.range(0.08, 0.16),
            rng.range(1.5, 3.5),
            rng.range(0.25, 0.6),
        ),
        "dnb" => (
            rng.range(0.2, 0.4),
            rng.range(48.0, 65.0),
            rng.range(200.0, 380.0),
            rng.range(0.01, 0.025),
            rng.range(0.08, 0.15),
            rng.range(1.5, 3.0),
            rng.range(0.3, 0.6),
        ),
        "lofi" => (
            rng.range(0.3, 0.5),
            rng.range(48.0, 62.0),
            rng.range(110.0, 180.0),
            rng.range(0.03, 0.06),
            rng.range(0.1, 0.2),
            rng.range(1.0, 1.8),
            rng.range(0.05, 0.2),
        ),
        _ => (
            rng.range(0.3, 0.6),
            rng.range(44.0, 56.0),
            rng.range(150.0, 260.0),
            rng.range(0.02, 0.045),
            rng.range(0.14, 0.28),
            rng.range(1.5, 3.0),
            rng.range(0.1, 0.4),
        ),
    };
    let mut x = buf(len);
    let mut phase = 0.0f32;
    let mut hp = Svf::new();
    hp.set(rng.range(1500.0, 4000.0), 0.1, SR);
    let click_tau = rng.range(0.001, 0.004);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let f = f_end + (f_start - f_end) * exp_env(t, pitch_tau);
        phase += f / SR;
        let body = (TAU * phase).sin() * exp_env(t, amp_tau);
        let c = hp.process(rng.noise(), FilterMode::Band) * exp_env(t, click_tau) * click;
        *v = soft_clip((body + c) * drive);
    }
    finish(x, 0.95)
}

pub fn snare(rng: &mut Rng, style: &str) -> Vec<f32> {
    let tone_f = rng.range(160.0, 260.0);
    let tone_tau = rng.range(0.03, 0.08);
    let noise_tau = match style {
        "trap" | "dnb" => rng.range(0.06, 0.14),
        "lofi" | "hiphop" => rng.range(0.1, 0.2),
        "synthwave" => rng.range(0.18, 0.35),
        _ => rng.range(0.08, 0.2),
    };
    let tone_mix = rng.range(0.3, 0.8);
    let mut bp = Svf::new();
    bp.set(rng.range(2500.0, 6000.0), rng.range(0.1, 0.4), SR);
    let mut hp = Svf::new();
    hp.set(rng.range(300.0, 900.0), 0.0, SR);
    let mut x = buf(noise_tau * 6.0 + 0.05);
    let mut p1 = 0.0f32;
    let mut p2 = 0.0f32;
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let f = tone_f * (1.0 + 0.5 * exp_env(t, 0.01));
        p1 += f / SR;
        p2 += f * 1.62 / SR;
        let tone = ((TAU * p1).sin() + 0.5 * (TAU * p2).sin()) * exp_env(t, tone_tau) * tone_mix;
        let n = rng.noise();
        let noise = (bp.process(n, FilterMode::Band) * 1.5 + hp.process(n, FilterMode::High) * 0.6) * exp_env(t, noise_tau);
        *v = soft_clip((tone + noise) * 1.6);
    }
    finish(x, 0.9)
}

pub fn clap(rng: &mut Rng) -> Vec<f32> {
    let bursts = rng.int(3, 5);
    let spacing = rng.range(0.008, 0.014);
    let tail_tau = rng.range(0.08, 0.25);
    let mut bp = Svf::new();
    bp.set(rng.range(900.0, 2200.0), rng.range(0.3, 0.6), SR);
    let mut x = buf(tail_tau * 6.0 + 0.1);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let mut env = 0.0;
        for b in 0..bursts {
            let tb = t - b as f32 * spacing;
            if tb >= 0.0 {
                env += if b + 1 == bursts { exp_env(tb, tail_tau) } else { exp_env(tb, 0.006) };
            }
        }
        *v = bp.process(rng.noise(), FilterMode::Band) * env * 2.0;
    }
    finish(x, 0.9)
}

pub fn hat(rng: &mut Rng, open: bool) -> Vec<f32> {
    let base = rng.range(35.0, 60.0);
    let tau = if open { rng.range(0.12, 0.35) } else { rng.range(0.015, 0.05) };
    let noise_mix = rng.range(0.2, 0.7);
    let detune = rng.range(0.0, 1.5);
    let mut hp = Svf::new();
    hp.set(rng.range(6500.0, 9500.0), rng.range(0.1, 0.4), SR);
    let mut hp2 = Svf::new();
    hp2.set(rng.range(5000.0, 8000.0), 0.0, SR);
    let mut phases = [0.0f32; 6];
    for p in phases.iter_mut() {
        *p = rng.f();
    }
    let mut x = buf(tau * 7.0 + 0.02);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let m = metallic(t, base, &mut phases, detune) * (1.0 - noise_mix) + rng.noise() * noise_mix;
        let y = hp2.process(hp.process(m, FilterMode::High), FilterMode::High);
        let attack = (t / 0.0015).min(1.0);
        *v = y * exp_env(t, tau) * attack * 2.0;
    }
    finish(x, 0.8)
}

pub fn cymbal(rng: &mut Rng, ride: bool) -> Vec<f32> {
    let base = rng.range(40.0, 70.0);
    let tau = if ride { rng.range(0.6, 1.2) } else { rng.range(0.9, 1.8) };
    let bell = if ride { rng.range(0.2, 0.5) } else { 0.0 };
    let bell_f = rng.range(600.0, 900.0);
    let noise_mix = if ride { rng.range(0.1, 0.3) } else { rng.range(0.5, 0.8) };
    let mut hp = Svf::new();
    hp.set(rng.range(3500.0, 6000.0), 0.15, SR);
    let mut phases = [0.0f32; 6];
    let mut pb = 0.0f32;
    let mut x = buf(tau * 4.5);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let m = metallic(t, base, &mut phases, 1.0) * (1.0 - noise_mix) + rng.noise() * noise_mix;
        pb += bell_f / SR;
        let b = ((TAU * pb).sin() + (TAU * pb * 2.76).sin() * 0.5) * bell * exp_env(t, tau * 0.4);
        *v = hp.process(m, FilterMode::High) * exp_env(t, tau) * (t / 0.002).min(1.0) * 1.5 + b;
    }
    finish(x, 0.75)
}

pub fn tom(rng: &mut Rng) -> Vec<f32> {
    let f0 = rng.range(80.0, 240.0);
    let tau = rng.range(0.15, 0.4);
    let mut x = buf(tau * 5.0);
    let mut p = 0.0f32;
    let mut bp = Svf::new();
    bp.set(f0 * 4.0, 0.3, SR);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        p += f0 * (1.0 + 0.6 * exp_env(t, 0.03)) / SR;
        *v = (TAU * p).sin() * exp_env(t, tau) + bp.process(rng.noise(), FilterMode::Band) * exp_env(t, 0.02) * 0.3;
    }
    finish(x, 0.9)
}

pub fn rim(rng: &mut Rng) -> Vec<f32> {
    let f = rng.range(400.0, 900.0);
    let mut x = buf(0.08);
    let mut p = 0.0f32;
    let mut bp = Svf::new();
    bp.set(rng.range(1500.0, 3500.0), 0.6, SR);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        p += f / SR;
        let tri = 1.0 - 4.0 * ((p % 1.0) - 0.5).abs();
        *v = tri * exp_env(t, 0.012) + bp.process(rng.noise(), FilterMode::Band) * exp_env(t, 0.006) * 2.0;
    }
    finish(x, 0.85)
}

pub fn shaker(rng: &mut Rng) -> Vec<f32> {
    let attack = rng.range(0.008, 0.03);
    let tau = rng.range(0.03, 0.09);
    let mut bp = Svf::new();
    bp.set(rng.range(5000.0, 9000.0), 0.3, SR);
    let mut x = buf(attack + tau * 6.0);
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let env = if t < attack { t / attack } else { exp_env(t - attack, tau) };
        *v = bp.process(rng.noise(), FilterMode::Band) * env * 2.0;
    }
    finish(x, 0.7)
}

/// Assorted hand percussion and synthetic blips.
pub fn perc(rng: &mut Rng) -> (Vec<f32>, &'static str) {
    let kinds = ["conga", "bongo", "woodblock", "clave", "cowbell", "blip", "tabla", "triangle", "click"];
    let kind = *rng.pick(&kinds);
    let mut x;
    match kind {
        "conga" | "bongo" | "tabla" => {
            let f0 = if kind == "bongo" { rng.range(300.0, 500.0) } else { rng.range(180.0, 330.0) };
            let bend = if kind == "tabla" { rng.range(-0.25, -0.1) } else { rng.range(0.1, 0.3) };
            let tau = rng.range(0.08, 0.2);
            x = buf(tau * 5.0);
            let mut p = 0.0f32;
            let mut bp = Svf::new();
            bp.set(rng.range(1500.0, 3000.0), 0.5, SR);
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                p += f0 * (1.0 + bend * exp_env(t, 0.04)) / SR;
                *v = (TAU * p).sin() * exp_env(t, tau) + bp.process(rng.noise(), FilterMode::Band) * exp_env(t, 0.008);
            }
        }
        "woodblock" | "clave" => {
            let f0 = if kind == "clave" { rng.range(2200.0, 2800.0) } else { rng.range(700.0, 1400.0) };
            x = buf(0.12);
            let mut p = 0.0f32;
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                p += f0 / SR;
                *v = (TAU * p).sin() * exp_env(t, 0.02);
            }
        }
        "cowbell" => {
            let f1 = rng.range(520.0, 600.0);
            let f2 = f1 * rng.range(1.45, 1.52);
            x = buf(0.4);
            let (mut p1, mut p2) = (0.0f32, 0.0f32);
            let mut bp = Svf::new();
            bp.set(rng.range(2000.0, 2800.0), 0.5, SR);
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                p1 = (p1 + f1 / SR) % 1.0;
                p2 = (p2 + f2 / SR) % 1.0;
                let s = (if p1 < 0.5 { 1.0 } else { -1.0 }) + (if p2 < 0.5 { 1.0 } else { -1.0 });
                *v = bp.process(s, FilterMode::Band) * (exp_env(t, 0.015) * 0.6 + exp_env(t, 0.12) * 0.4);
            }
        }
        "blip" => {
            let fc = rng.range(400.0, 2000.0);
            let ratio = *rng.pick(&[1.0f32, 1.5, 2.0, 2.76, 3.5]);
            let index = rng.range(1.0, 6.0);
            let tau = rng.range(0.03, 0.12);
            x = buf(tau * 6.0);
            let (mut pc, mut pm) = (0.0f32, 0.0f32);
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                pm += fc * ratio / SR;
                let m = (TAU * pm).sin() * index * exp_env(t, tau * 0.5);
                pc += fc / SR;
                *v = (TAU * pc + m).sin() * exp_env(t, tau);
            }
        }
        "triangle" => {
            let f = rng.range(3500.0, 5500.0);
            x = buf(1.0);
            let mut p = 0.0f32;
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                p += f / SR;
                *v = ((TAU * p).sin() + (TAU * p * 2.7).sin() * 0.4) * exp_env(t, 0.25);
            }
        }
        _ => {
            x = buf(0.03);
            let mut hp = Svf::new();
            hp.set(rng.range(2000.0, 6000.0), 0.5, SR);
            for (i, v) in x.iter_mut().enumerate() {
                let t = i as f32 / SR;
                *v = hp.process(rng.noise(), FilterMode::Band) * exp_env(t, 0.003) * 3.0;
            }
        }
    }
    (finish(x, 0.85), kind)
}

/// Tuned 808 bass hit at MIDI `note`.
pub fn bass808(rng: &mut Rng, note: u8, timbre: usize) -> Vec<f32> {
    let f = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
    let len = [1.6, 1.2, 2.2, 1.0][timbre % 4] * rng.range(0.9, 1.1);
    let drive = [1.2, 3.0, 1.6, 5.0][timbre % 4];
    let punch = [0.5, 1.0, 0.3, 1.5][timbre % 4];
    let mut x = buf(len);
    let mut p = 0.0f32;
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / SR;
        p += f * (1.0 + punch * exp_env(t, 0.02)) / SR;
        let env = exp_env(t, len * 0.35) * (t / 0.002).min(1.0);
        *v = soft_clip((TAU * p).sin() * drive) * env;
    }
    finish(x, 0.95)
}
