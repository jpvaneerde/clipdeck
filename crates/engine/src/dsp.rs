//! Small DSP building blocks shared by instruments and effects.

use core::f32::consts::PI;

pub const TAU: f32 = 2.0 * PI;

#[inline]
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * fast_exp2((note - 69.0) / 12.0)
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    fast_exp2(db * 0.166_096_4) // 10^(db/20) = 2^(db * log2(10)/20)
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

/// 2^x, accurate to well under a cent for the ranges used here.
#[inline]
pub fn fast_exp2(x: f32) -> f32 {
    x.exp2()
}

/// Rational tanh approximation, exact at 0, saturates to ±1.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x > 3.0 {
        1.0
    } else if x < -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

/// Equal-power pan law. `pan` in -1..1. Returns (left, right) gains.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (p.cos(), p.sin())
}

/// Cubic Hermite interpolation between x0 and x1.
#[inline]
pub fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

/// Flush values that would decay into denormals (slow on x86, and browsers
/// do not enable flush-to-zero for WebAssembly).
#[inline]
pub fn undenorm(x: f32) -> f32 {
    if x.abs() < 1e-15 {
        0.0
    } else {
        x
    }
}

/// One-pole parameter smoother to avoid zipper noise.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub value: f32,
    pub target: f32,
    coef: f32,
}

impl Smoother {
    pub fn new(value: f32, time_s: f32, sr: f32) -> Self {
        Self { value, target: value, coef: one_pole_coef(time_s, sr) }
    }
    #[inline]
    pub fn set(&mut self, target: f32) {
        self.target = target;
    }
    #[inline]
    pub fn jump(&mut self, v: f32) {
        self.value = v;
        self.target = v;
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coef;
        self.value
    }
}

/// Coefficient for a one-pole filter that covers ~99% of a step in `time_s`.
#[inline]
pub fn one_pole_coef(time_s: f32, sr: f32) -> f32 {
    if time_s <= 0.0 {
        1.0
    } else {
        1.0 - (-4.6 / (time_s * sr)).exp()
    }
}

/// Cheap deterministic white noise (xorshift32).
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Low,
    High,
    Band,
}

/// Topology-preserving state variable filter (Zavalishin / Simper).
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    pub fn new() -> Self {
        let mut f = Self::default();
        f.set(1000.0, 0.0, 48000.0);
        f
    }

    /// `reso` in 0..1 (1 = near self-oscillation).
    #[inline]
    pub fn set(&mut self, cutoff: f32, reso: f32, sr: f32) {
        let fc = cutoff.clamp(16.0, sr * 0.45);
        let g = (PI * fc / sr).tan();
        let k = 2.0 - 1.96 * reso.clamp(0.0, 1.0);
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Copy coefficients (not state) from another filter.
    #[inline]
    pub fn copy_coefs(&mut self, o: &Svf) {
        self.a1 = o.a1;
        self.a2 = o.a2;
        self.a3 = o.a3;
        self.k = o.k;
    }

    #[inline]
    pub fn process(&mut self, v0: f32, mode: FilterMode) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = undenorm(2.0 * v1 - self.ic1);
        self.ic2 = undenorm(2.0 * v2 - self.ic2);
        match mode {
            FilterMode::Low => v2,
            FilterMode::Band => v1,
            FilterMode::High => v0 - self.k * v1 - v2,
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR with linear attack and exponential decay/release.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    stage: Stage,
    pub level: f32,
    sustain: f32,
    attack_inc: f32,
    decay_coef: f32,
    release_coef: f32,
}

impl Adsr {
    pub fn new() -> Self {
        Self { stage: Stage::Idle, level: 0.0, sustain: 1.0, attack_inc: 1.0, decay_coef: 1.0, release_coef: 1.0 }
    }

    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) {
        self.attack_inc = 1.0 / (a.max(0.0005) * sr);
        self.decay_coef = one_pole_coef(d.max(0.001), sr);
        self.sustain = s.clamp(0.0, 1.0);
        self.release_coef = one_pole_coef(r.max(0.002), sr);
    }

    #[inline]
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    #[inline]
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    #[inline]
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline]
    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (self.sustain - self.level) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level -= self.level * self.release_coef;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Saw,
    Square,
    Triangle,
    Sine,
}

impl Wave {
    pub fn from_f32(v: f32) -> Self {
        match v.round() as i32 {
            1 => Wave::Square,
            2 => Wave::Triangle,
            3 => Wave::Sine,
            _ => Wave::Saw,
        }
    }
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Band-limited oscillator sample for phase `t` in 0..1 with increment `dt`.
#[inline]
pub fn osc(wave: Wave, t: f32, dt: f32) -> f32 {
    match wave {
        Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
        Wave::Square => {
            let mut v = if t < 0.5 { 1.0 } else { -1.0 };
            v += poly_blep(t, dt);
            let t2 = if t + 0.5 >= 1.0 { t - 0.5 } else { t + 0.5 };
            v -= poly_blep(t2, dt);
            v
        }
        Wave::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
        Wave::Sine => (TAU * t).sin(),
    }
}

#[inline]
pub fn wrap_phase(p: f32) -> f32 {
    if p >= 1.0 {
        p - p.floor()
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_reaches_sustain_and_releases() {
        let sr = 48000.0;
        let mut env = Adsr::new();
        env.set(0.01, 0.05, 0.5, 0.1, sr);
        env.gate_on();
        for _ in 0..(sr as usize / 2) {
            env.next();
        }
        assert!((env.level - 0.5).abs() < 0.01);
        env.gate_off();
        for _ in 0..(sr as usize) {
            env.next();
        }
        assert!(env.is_idle());
    }

    #[test]
    fn svf_lowpass_attenuates_highs() {
        let sr = 48000.0;
        let mut f = Svf::new();
        f.set(200.0, 0.0, sr);
        let mut peak: f32 = 0.0;
        for i in 0..48000 {
            let x = (TAU * 8000.0 * i as f32 / sr).sin();
            let y = f.process(x, FilterMode::Low);
            if i > 1000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01, "peak {peak}");
    }

    #[test]
    fn pan_is_equal_power() {
        let (l, r) = pan_gains(0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
    }
}
