//! Effects: Freeverb-style reverb, tempo-synced ping-pong delay,
//! compressor, master limiter and the per-track insert chain.

use crate::dsp::{self, FilterMode, Smoother, Svf};

// ---------------------------------------------------------------- Reverb

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.idx];
        self.store = dsp::undenorm(y * (1.0 - damp) + self.store * damp);
        self.buf[self.idx] = x + self.store * feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0 }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let y = b - x;
        self.buf[self.idx] = dsp::undenorm(x + b * 0.5);
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        y
    }
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

pub struct Reverb {
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    ap_l: Vec<Allpass>,
    ap_r: Vec<Allpass>,
    pre_hp: Svf,
    pub size: f32,
    pub damp: f32,
    pub width: f32,
    sr: f32,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let scale = sr / 44100.0;
        let s = |n: usize| ((n as f32) * scale) as usize;
        let mut pre_hp = Svf::new();
        pre_hp.set(180.0, 0.0, sr);
        Self {
            combs_l: COMB_TUNING.iter().map(|&n| Comb::new(s(n))).collect(),
            combs_r: COMB_TUNING.iter().map(|&n| Comb::new(s(n + STEREO_SPREAD))).collect(),
            ap_l: ALLPASS_TUNING.iter().map(|&n| Allpass::new(s(n))).collect(),
            ap_r: ALLPASS_TUNING.iter().map(|&n| Allpass::new(s(n + STEREO_SPREAD))).collect(),
            pre_hp,
            size: 0.75,
            damp: 0.4,
            width: 1.0,
            sr,
        }
    }

    pub fn process(&mut self, inl: &[f32], inr: &[f32], outl: &mut [f32], outr: &mut [f32], level: f32) {
        let fb = self.size.clamp(0.0, 1.0) * 0.28 + 0.7;
        let damp = self.damp.clamp(0.0, 1.0) * 0.4;
        let wet1 = level * (self.width * 0.5 + 0.5);
        let wet2 = level * ((1.0 - self.width) * 0.5);
        let _ = self.sr;
        for i in 0..inl.len() {
            let x = self.pre_hp.process((inl[i] + inr[i]) * 0.015, FilterMode::High);
            let mut l = 0.0;
            let mut r = 0.0;
            for c in self.combs_l.iter_mut() {
                l += c.process(x, fb, damp);
            }
            for c in self.combs_r.iter_mut() {
                r += c.process(x, fb, damp);
            }
            for a in self.ap_l.iter_mut() {
                l = a.process(l);
            }
            for a in self.ap_r.iter_mut() {
                r = a.process(r);
            }
            outl[i] += l * wet1 + r * wet2;
            outr[i] += r * wet1 + l * wet2;
        }
    }
}

// ---------------------------------------------------------------- Delay

pub struct Delay {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    w: usize,
    time: Smoother,
    lp_l: f32,
    lp_r: f32,
    pub beats: f32,
    pub feedback: f32,
    pub tone: f32,
    pub ping_pong: bool,
    sr: f32,
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        let len = (sr * 4.0) as usize;
        Self {
            buf_l: vec![0.0; len],
            buf_r: vec![0.0; len],
            w: 0,
            time: Smoother::new(sr * 0.375, 0.08, sr),
            lp_l: 0.0,
            lp_r: 0.0,
            beats: 0.75,
            feedback: 0.4,
            tone: 4500.0,
            ping_pong: true,
            sr,
        }
    }

    #[inline]
    fn read(buf: &[f32], w: usize, delay: f32) -> f32 {
        let len = buf.len();
        let pos = w as f32 - delay;
        let pos = if pos < 0.0 { pos + len as f32 } else { pos };
        let i = pos as usize % len;
        let t = pos - pos.floor();
        let a = buf[i];
        let b = buf[(i + 1) % len];
        a + (b - a) * t
    }

    pub fn process(&mut self, inl: &[f32], inr: &[f32], outl: &mut [f32], outr: &mut [f32], bpm: f32, level: f32) {
        let max = (self.buf_l.len() - 2) as f32;
        let target = (self.beats.max(0.0625) * 60.0 / bpm.max(20.0) * self.sr).clamp(1.0, max);
        self.time.set(target);
        let fb = self.feedback.clamp(0.0, 0.95);
        let lp = 1.0 - (-dsp::TAU * self.tone.clamp(200.0, 18000.0) / self.sr).exp();
        let len = self.buf_l.len();
        for i in 0..inl.len() {
            let d = self.time.next();
            let yl = Self::read(&self.buf_l, self.w, d);
            let yr = Self::read(&self.buf_r, self.w, d);
            self.lp_l = dsp::undenorm(self.lp_l + (yl - self.lp_l) * lp);
            self.lp_r = dsp::undenorm(self.lp_r + (yr - self.lp_r) * lp);
            let (nl, nr) = if self.ping_pong {
                ((inl[i] + inr[i]) * 0.5 + self.lp_r * fb, self.lp_l * fb)
            } else {
                (inl[i] + self.lp_l * fb, inr[i] + self.lp_r * fb)
            };
            self.buf_l[self.w] = dsp::soft_clip(nl);
            self.buf_r[self.w] = dsp::soft_clip(nr);
            self.w = (self.w + 1) % len;
            outl[i] += yl * level;
            outr[i] += yr * level;
        }
    }
}

// ---------------------------------------------------------------- Compressor

#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    env: f32,
    pub gain_reduction_db: f32,
}

impl Compressor {
    pub fn new() -> Self {
        Self { threshold_db: 0.0, ratio: 1.0, attack_ms: 10.0, release_ms: 120.0, makeup_db: 0.0, env: 0.0, gain_reduction_db: 0.0 }
    }

    pub fn enabled(&self) -> bool {
        self.ratio > 1.01 && self.threshold_db < -0.1
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        if !self.enabled() {
            self.gain_reduction_db = 0.0;
            return;
        }
        let att = dsp::one_pole_coef(self.attack_ms * 0.001, sr);
        let rel = dsp::one_pole_coef(self.release_ms * 0.001, sr);
        let slope = 1.0 - 1.0 / self.ratio;
        let knee = 6.0;
        let makeup = dsp::db_to_gain(self.makeup_db);
        let mut max_gr: f32 = 0.0;
        let mut g = dsp::db_to_gain(-self.gain_reduction_db) * makeup;
        for i in 0..l.len() {
            let x = l[i].abs().max(r[i].abs());
            let c = if x > self.env { att } else { rel };
            self.env += (x - self.env) * c;
            // The gain computer runs every 4 samples; the envelope is already smooth.
            if i & 3 == 0 {
                let over = dsp::gain_to_db(self.env) - self.threshold_db;
                let gr = if over <= -knee * 0.5 {
                    0.0
                } else if over >= knee * 0.5 {
                    over * slope
                } else {
                    let o = over + knee * 0.5;
                    slope * o * o / (2.0 * knee)
                };
                max_gr = max_gr.max(gr);
                g = dsp::db_to_gain(-gr) * makeup;
            }
            l[i] *= g;
            r[i] *= g;
        }
        self.gain_reduction_db = max_gr;
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- Limiter

/// Lookahead peak limiter. The needed gain is min-filtered over the lookahead
/// window and then box-averaged over the same length, which yields a smooth
/// gain curve that is guaranteed to be at or below what every peak needs by
/// the time that peak leaves the delay line.
pub struct Limiter {
    pub ceiling: f32,
    gain: f32,
    release: f32,
    look_l: Vec<f32>,
    look_r: Vec<f32>,
    need: Vec<f32>,
    wmin: Vec<f32>,
    sum: f64,
    idx: usize,
    pub gain_reduction_db: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        let n = ((sr * 0.0015) as usize).max(2);
        Self {
            ceiling: dsp::db_to_gain(-0.3),
            gain: 1.0,
            release: dsp::one_pole_coef(0.12, sr),
            look_l: vec![0.0; n],
            look_r: vec![0.0; n],
            need: vec![1.0; n],
            wmin: vec![1.0; n],
            sum: n as f64,
            idx: 0,
            gain_reduction_db: 0.0,
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = self.look_l.len();
        let mut min_gain: f32 = 1.0;
        for i in 0..l.len() {
            let peak = l[i].abs().max(r[i].abs());
            self.need[self.idx] = if peak > self.ceiling { self.ceiling / peak } else { 1.0 };
            let wmin = self.need.iter().fold(1.0f32, |m, &v| m.min(v));
            self.sum += (wmin - self.wmin[self.idx]) as f64;
            self.wmin[self.idx] = wmin;
            let avg = (self.sum / n as f64) as f32;
            // Release smoothly, but never above what the window demands.
            self.gain = avg.min(self.gain + (1.0 - self.gain) * self.release);
            self.look_l[self.idx] = l[i];
            self.look_r[self.idx] = r[i];
            let out = (self.idx + 1) % n; // oldest sample: delayed by n - 1
            min_gain = min_gain.min(self.gain);
            l[i] = (self.look_l[out] * self.gain).clamp(-1.0, 1.0);
            r[i] = (self.look_r[out] * self.gain).clamp(-1.0, 1.0);
            self.idx = out;
        }
        // Re-sum occasionally to keep float drift out of the running average.
        self.sum = self.wmin.iter().map(|&v| v as f64).sum();
        self.gain_reduction_db = -dsp::gain_to_db(min_gain);
    }
}

#[cfg(test)]
mod limiter_tests {
    use super::*;

    #[test]
    fn limiter_holds_the_ceiling() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr);
        let mut l: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.05).sin() * if (i / 4000) % 2 == 0 { 3.0 } else { 0.3 }).collect();
        let mut r = l.clone();
        lim.process(&mut l, &mut r);
        let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= lim.ceiling + 1e-3, "peak {peak}");
        // Quiet passages come back up to near unity after the release.
        let tail = l[44000..47900].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(tail > 0.25, "tail {tail}");
    }
}

// ---------------------------------------------------------------- Track inserts

pub mod track_param {
    pub const VOLUME: usize = 0;
    pub const PAN: usize = 1;
    pub const MUTE: usize = 2;
    pub const SOLO: usize = 3;
    pub const SEND_A: usize = 4;
    pub const SEND_B: usize = 5;
    pub const FILTER_TYPE: usize = 6;
    pub const FILTER_CUTOFF: usize = 7;
    pub const FILTER_RESO: usize = 8;
    pub const DRIVE: usize = 9;
    pub const COMP_THRESHOLD: usize = 10;
    pub const COMP_RATIO: usize = 11;
    pub const COMP_ATTACK: usize = 12;
    pub const COMP_RELEASE: usize = 13;
    pub const COMP_MAKEUP: usize = 14;
    pub const DUCK_SOURCE: usize = 15;
    pub const DUCK_AMOUNT: usize = 16;
    pub const DUCK_RELEASE: usize = 17;
    pub const COUNT: usize = 18;
}

/// Filter → drive → compressor, applied before the fader.
pub struct TrackFx {
    pub filter_type: u8,
    pub cutoff: Smoother,
    pub reso: f32,
    svf_l: Svf,
    svf_r: Svf,
    pub drive: f32,
    pub comp: Compressor,
    ctr: u32,
}

impl TrackFx {
    pub fn new(sr: f32) -> Self {
        Self {
            filter_type: 0,
            cutoff: Smoother::new(20000.0, 0.03, sr),
            reso: 0.1,
            svf_l: Svf::new(),
            svf_r: Svf::new(),
            drive: 0.0,
            comp: Compressor::new(),
            ctr: 0,
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], sr: f32) {
        let mode = match self.filter_type {
            1 => Some(FilterMode::Low),
            2 => Some(FilterMode::High),
            3 => Some(FilterMode::Band),
            _ => None,
        };
        if let Some(mode) = mode {
            for i in 0..l.len() {
                let fc = self.cutoff.next();
                if self.ctr == 0 {
                    self.svf_l.set(fc, self.reso, sr);
                    let c = self.svf_l;
                    self.svf_r.copy_coefs(&c);
                }
                self.ctr = (self.ctr + 1) & 7;
                l[i] = self.svf_l.process(l[i], mode);
                r[i] = self.svf_r.process(r[i], mode);
            }
        }
        if self.drive > 0.001 {
            let k = 1.0 + self.drive * 12.0;
            let norm = 1.0 / (1.0 + self.drive * 3.0).sqrt();
            for i in 0..l.len() {
                l[i] = dsp::soft_clip(l[i] * k) * norm;
                r[i] = dsp::soft_clip(r[i] * k) * norm;
            }
        }
        self.comp.process(l, r, sr);
    }
}
