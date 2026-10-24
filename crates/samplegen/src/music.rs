//! Music theory helpers, genre definitions and synth presets.

use crate::rng::Rng;
use clipdeck_engine::synth::{param as sp, DEFAULTS};

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];
pub const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
pub const DORIAN: [i32; 7] = [0, 2, 3, 5, 7, 9, 10];
pub const MINOR_PENT: [i32; 5] = [0, 3, 5, 7, 10];
pub const MAJOR_PENT: [i32; 5] = [0, 2, 4, 7, 9];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Minor,
    Major,
    Dorian,
}

impl Mode {
    pub fn scale(self) -> &'static [i32; 7] {
        match self {
            Mode::Minor => &MINOR,
            Mode::Major => &MAJOR,
            Mode::Dorian => &DORIAN,
        }
    }
    pub fn pent(self) -> &'static [i32; 5] {
        match self {
            Mode::Major => &MAJOR_PENT,
            _ => &MINOR_PENT,
        }
    }
}

/// A key: tonic pitch class + mode.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub tonic: i32,
    pub mode: Mode,
}

impl Key {
    pub fn name(&self) -> String {
        format!("{}{}", NOTE_NAMES[self.tonic as usize], if self.mode == Mode::Major { "" } else { "m" })
    }

    /// MIDI pitch of scale degree `deg` (can be negative or > 6) above `base_octave`'s tonic.
    pub fn degree(&self, deg: i32, base_octave: i32) -> i32 {
        let s = self.mode.scale();
        let oct = deg.div_euclid(7);
        let idx = deg.rem_euclid(7) as usize;
        12 * (base_octave + 1) + self.tonic + s[idx] + 12 * oct
    }
}

/// Diatonic chord on scale degree `root`: returns MIDI pitches.
pub fn chord(key: &Key, root: i32, octave: i32, sevenths: bool, ninth: bool) -> Vec<i32> {
    let mut degs = vec![root, root + 2, root + 4];
    if sevenths {
        degs.push(root + 6);
    }
    if ninth {
        degs.push(root + 8);
    }
    degs.iter().map(|&d| key.degree(d, octave)).collect()
}

/// Re-voice `chord` close to `prev` (smooth voice leading), keeping pitches in [lo, hi].
pub fn voice_lead(chord: &[i32], prev: Option<&[i32]>, lo: i32, hi: i32) -> Vec<i32> {
    let center = match prev {
        Some(p) if !p.is_empty() => p.iter().sum::<i32>() as f32 / p.len() as f32,
        _ => (lo + hi) as f32 / 2.0,
    };
    // Try all inversions/octave shifts and keep the one closest to center.
    let pcs: Vec<i32> = chord.iter().map(|p| p.rem_euclid(12)).collect();
    let mut best: Vec<i32> = chord.to_vec();
    let mut best_score = f32::MAX;
    for inv in 0..pcs.len() {
        for base in (lo / 12 - 1)..=(hi / 12 + 1) {
            let mut v = Vec::with_capacity(pcs.len());
            let mut last = i32::MIN;
            for k in 0..pcs.len() {
                let pc = pcs[(inv + k) % pcs.len()];
                let mut p = base * 12 + pc;
                while p <= last {
                    p += 12;
                }
                v.push(p);
                last = p;
            }
            if v[0] < lo || *v.last().unwrap() > hi {
                continue;
            }
            let c = v.iter().sum::<i32>() as f32 / v.len() as f32;
            let score = (c - center).abs() + (v.last().unwrap() - v[0]) as f32 * 0.05;
            if score < best_score {
                best_score = score;
                best = v;
            }
        }
    }
    best
}

/// Chord progressions as scale degrees (0-based), one chord per bar.
pub fn progression(rng: &mut Rng, mode: Mode) -> Vec<i32> {
    let minor: &[[i32; 4]] = &[
        [0, 5, 2, 6], // i VI III VII
        [0, 3, 4, 0], // i iv v i
        [0, 6, 5, 6], // i VII VI VII
        [0, 5, 3, 4], // i VI iv v
        [0, 2, 6, 5], // i III VII VI
        [0, 3, 6, 2], // i iv VII III
        [0, 4, 5, 3], // i v VI iv
        [0, 0, 5, 6], // i i VI VII
        [5, 6, 0, 0], // VI VII i i
        [0, 3, 0, 4], // i iv i v
    ];
    let major: &[[i32; 4]] = &[
        [0, 4, 5, 3], // I V vi IV
        [0, 5, 3, 4], // I vi IV V
        [0, 3, 0, 4], // I IV I V
        [5, 3, 0, 4], // vi IV I V
        [0, 2, 3, 4], // I iii IV V
        [3, 4, 2, 5], // IV V iii vi
        [0, 3, 5, 4], // I IV vi V
    ];
    let p = match mode {
        Mode::Major => *rng.pick(major),
        _ => *rng.pick(minor),
    };
    p.to_vec()
}

// ------------------------------------------------------------ genres

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumStyle {
    House,
    DeepHouse,
    Techno,
    HipHop,
    Trap,
    Dnb,
    Lofi,
    Garage,
    Afro,
    Reggaeton,
    Disco,
    Synthwave,
}

pub struct Genre {
    pub slug: &'static str,
    pub label: &'static str,
    pub bpm: (i32, i32),
    pub swing: (f32, f32),
    pub drums: DrumStyle,
    pub kick: &'static str,
    pub snare: &'static str,
    pub major_chance: f32,
    pub drum_bars: u32,
    pub music_bars: u32,
}

pub const GENRES: [Genre; 12] = [
    Genre {
        slug: "house",
        label: "House",
        bpm: (122, 126),
        swing: (0.0, 0.15),
        drums: DrumStyle::House,
        kick: "house",
        snare: "house",
        major_chance: 0.3,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "deep-house",
        label: "Deep House",
        bpm: (118, 122),
        swing: (0.1, 0.3),
        drums: DrumStyle::DeepHouse,
        kick: "house",
        snare: "house",
        major_chance: 0.2,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "techno",
        label: "Techno",
        bpm: (128, 134),
        swing: (0.0, 0.08),
        drums: DrumStyle::Techno,
        kick: "techno",
        snare: "techno",
        major_chance: 0.0,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "hip-hop",
        label: "Hip-Hop",
        bpm: (86, 96),
        swing: (0.25, 0.5),
        drums: DrumStyle::HipHop,
        kick: "hiphop",
        snare: "hiphop",
        major_chance: 0.25,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "trap",
        label: "Trap",
        bpm: (136, 150),
        swing: (0.0, 0.05),
        drums: DrumStyle::Trap,
        kick: "trap",
        snare: "trap",
        major_chance: 0.1,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "drum-and-bass",
        label: "Drum & Bass",
        bpm: (170, 176),
        swing: (0.0, 0.05),
        drums: DrumStyle::Dnb,
        kick: "dnb",
        snare: "dnb",
        major_chance: 0.15,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "lofi",
        label: "Lo-Fi",
        bpm: (72, 86),
        swing: (0.35, 0.6),
        drums: DrumStyle::Lofi,
        kick: "lofi",
        snare: "lofi",
        major_chance: 0.5,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "uk-garage",
        label: "UK Garage",
        bpm: (130, 134),
        swing: (0.4, 0.6),
        drums: DrumStyle::Garage,
        kick: "house",
        snare: "house",
        major_chance: 0.2,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "afro-house",
        label: "Afro House",
        bpm: (118, 124),
        swing: (0.1, 0.25),
        drums: DrumStyle::Afro,
        kick: "house",
        snare: "house",
        major_chance: 0.3,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "reggaeton",
        label: "Reggaeton",
        bpm: (90, 98),
        swing: (0.0, 0.05),
        drums: DrumStyle::Reggaeton,
        kick: "hiphop",
        snare: "house",
        major_chance: 0.2,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "nu-disco",
        label: "Nu-Disco",
        bpm: (115, 122),
        swing: (0.05, 0.2),
        drums: DrumStyle::Disco,
        kick: "house",
        snare: "house",
        major_chance: 0.55,
        drum_bars: 2,
        music_bars: 4,
    },
    Genre {
        slug: "synthwave",
        label: "Synthwave",
        bpm: (100, 112),
        swing: (0.0, 0.05),
        drums: DrumStyle::Synthwave,
        kick: "techno",
        snare: "synthwave",
        major_chance: 0.35,
        drum_bars: 2,
        music_bars: 4,
    },
];

// ------------------------------------------------------------ presets

pub type Params = [f32; sp::COUNT];

fn base() -> Params {
    DEFAULTS
}

fn jitter(rng: &mut Rng, p: &mut Params, id: usize, amount: f32) {
    p[id] *= 1.0 + rng.range(-amount, amount);
}

/// Named synth presets with a little randomness so no two loops are identical.
pub fn preset(name: &str, rng: &mut Rng) -> Params {
    let mut p = base();
    match name {
        "sub-bass" => {
            p[sp::OSC1_WAVE] = 3.0;
            p[sp::OSC2_WAVE] = 2.0;
            p[sp::OSC_MIX] = 0.15;
            p[sp::OSC2_SEMI] = 12.0;
            p[sp::CUTOFF] = 400.0;
            p[sp::FILTER_ENV] = 0.5;
            p[sp::AMP_A] = 0.004;
            p[sp::AMP_D] = 0.4;
            p[sp::AMP_S] = 0.85;
            p[sp::AMP_R] = 0.06;
            p[sp::MONO] = 1.0;
            p[sp::GLIDE] = 0.02;
            p[sp::SPREAD] = 0.0;
            p[sp::DRIVE] = 0.2;
            p[sp::LEVEL] = 1.1;
            p[sp::KEY_TRACK] = 0.3;
        }
        "pluck-bass" => {
            p[sp::OSC1_WAVE] = 0.0;
            p[sp::OSC2_WAVE] = 1.0;
            p[sp::OSC2_SEMI] = -12.0;
            p[sp::OSC2_DETUNE] = 3.0;
            p[sp::CUTOFF] = rng.range(200.0, 400.0);
            p[sp::RESO] = rng.range(0.15, 0.4);
            p[sp::FILTER_ENV] = rng.range(2.5, 4.0);
            p[sp::FILT_D] = rng.range(0.1, 0.2);
            p[sp::FILT_S] = 0.0;
            p[sp::AMP_D] = 0.3;
            p[sp::AMP_S] = 0.3;
            p[sp::AMP_R] = 0.08;
            p[sp::MONO] = 1.0;
            p[sp::SPREAD] = 0.0;
            p[sp::SUB] = 0.4;
            p[sp::LEVEL] = 1.0;
        }
        "acid" => {
            p[sp::OSC1_WAVE] = if rng.chance(0.7) { 0.0 } else { 1.0 };
            p[sp::OSC_MIX] = 0.0;
            p[sp::CUTOFF] = rng.range(220.0, 420.0);
            p[sp::RESO] = rng.range(0.72, 0.86);
            p[sp::FILTER_ENV] = rng.range(3.0, 4.5);
            p[sp::FILT_D] = rng.range(0.12, 0.25);
            p[sp::FILT_S] = 0.0;
            p[sp::AMP_D] = 0.4;
            p[sp::AMP_S] = 0.6;
            p[sp::AMP_R] = 0.05;
            p[sp::MONO] = 1.0;
            p[sp::GLIDE] = 0.06;
            p[sp::SPREAD] = 0.0;
            p[sp::DRIVE] = rng.range(0.3, 0.7);
            p[sp::VEL_SENS] = 0.8;
            p[sp::LEVEL] = 0.8;
        }
        "reese" => {
            p[sp::OSC2_DETUNE] = rng.range(18.0, 32.0);
            p[sp::CUTOFF] = rng.range(500.0, 900.0);
            p[sp::RESO] = 0.25;
            p[sp::FILTER_ENV] = 0.5;
            p[sp::AMP_A] = 0.01;
            p[sp::AMP_S] = 1.0;
            p[sp::AMP_R] = 0.1;
            p[sp::SUB] = 0.6;
            p[sp::SPREAD] = 0.5;
            p[sp::LFO_RATE] = rng.range(0.2, 0.6);
            p[sp::LFO_CUTOFF] = rng.range(0.3, 0.9);
            p[sp::MONO] = 1.0;
            p[sp::GLIDE] = 0.04;
            p[sp::DRIVE] = 0.4;
            p[sp::LEVEL] = 0.8;
        }
        "808" => {
            p[sp::OSC1_WAVE] = 3.0;
            p[sp::OSC_MIX] = 0.0;
            p[sp::CUTOFF] = 3000.0;
            p[sp::FILTER_ENV] = 0.0;
            p[sp::KEY_TRACK] = 0.0;
            p[sp::AMP_A] = 0.002;
            p[sp::AMP_D] = rng.range(0.8, 1.6);
            p[sp::AMP_S] = rng.range(0.2, 0.5);
            p[sp::AMP_R] = 0.12;
            p[sp::MONO] = 1.0;
            p[sp::GLIDE] = rng.range(0.05, 0.1);
            p[sp::DRIVE] = rng.range(0.3, 0.8);
            p[sp::SPREAD] = 0.0;
            p[sp::LEVEL] = 1.3;
        }
        "organ-bass" => {
            p[sp::OSC1_WAVE] = 1.0;
            p[sp::OSC2_WAVE] = 3.0;
            p[sp::OSC2_SEMI] = 12.0;
            p[sp::CUTOFF] = 900.0;
            p[sp::FILTER_ENV] = 1.5;
            p[sp::FILT_D] = 0.15;
            p[sp::FILT_S] = 0.2;
            p[sp::AMP_D] = 0.2;
            p[sp::AMP_S] = 0.5;
            p[sp::AMP_R] = 0.06;
            p[sp::MONO] = 1.0;
            p[sp::SPREAD] = 0.0;
            p[sp::SUB] = 0.5;
        }
        "disco-bass" => {
            p[sp::CUTOFF] = rng.range(500.0, 900.0);
            p[sp::FILTER_ENV] = rng.range(2.0, 3.0);
            p[sp::FILT_D] = 0.12;
            p[sp::FILT_S] = 0.1;
            p[sp::RESO] = 0.25;
            p[sp::AMP_D] = 0.18;
            p[sp::AMP_S] = 0.35;
            p[sp::AMP_R] = 0.05;
            p[sp::MONO] = 1.0;
            p[sp::SPREAD] = 0.0;
            p[sp::SUB] = 0.3;
        }
        "pad" => {
            p[sp::OSC2_DETUNE] = rng.range(8.0, 16.0);
            p[sp::OSC1_WAVE] = if rng.chance(0.5) { 0.0 } else { 1.0 };
            p[sp::SPREAD] = 0.85;
            p[sp::CUTOFF] = rng.range(900.0, 2200.0);
            p[sp::RESO] = 0.15;
            p[sp::FILTER_ENV] = 1.0;
            p[sp::FILT_A] = 0.6;
            p[sp::FILT_D] = 1.5;
            p[sp::FILT_S] = 0.4;
            p[sp::AMP_A] = rng.range(0.25, 0.6);
            p[sp::AMP_D] = 1.0;
            p[sp::AMP_S] = 0.85;
            p[sp::AMP_R] = rng.range(0.8, 1.5);
            p[sp::LFO_RATE] = rng.range(0.15, 0.5);
            p[sp::LFO_CUTOFF] = 0.3;
            p[sp::LEVEL] = 0.55;
            p[sp::VEL_SENS] = 0.2;
        }
        "keys" => {
            p[sp::OSC1_WAVE] = 3.0;
            p[sp::OSC2_WAVE] = 2.0;
            p[sp::OSC2_SEMI] = 12.0;
            p[sp::OSC2_DETUNE] = 2.0;
            p[sp::OSC_MIX] = 0.25;
            p[sp::CUTOFF] = 3500.0;
            p[sp::FILTER_ENV] = 1.0;
            p[sp::FILT_D] = 0.6;
            p[sp::FILT_S] = 0.3;
            p[sp::KEY_TRACK] = 1.0;
            p[sp::AMP_A] = 0.002;
            p[sp::AMP_D] = rng.range(0.9, 1.6);
            p[sp::AMP_S] = 0.25;
            p[sp::AMP_R] = 0.4;
            p[sp::LFO_RATE] = 4.5;
            p[sp::LFO_PITCH] = 0.03;
            p[sp::SPREAD] = 0.4;
            p[sp::VEL_SENS] = 0.7;
            p[sp::DRIVE] = 0.15;
            p[sp::LEVEL] = 0.75;
        }
        "lofi-keys" => {
            p = preset("keys", rng);
            p[sp::CUTOFF] = rng.range(1400.0, 2200.0);
            p[sp::LFO_RATE] = rng.range(0.4, 0.8);
            p[sp::LFO_PITCH] = rng.range(0.08, 0.15);
            p[sp::NOISE] = 0.02;
            p[sp::DRIVE] = 0.3;
        }
        "stab" => {
            p[sp::OSC1_WAVE] = 1.0;
            p[sp::OSC2_SEMI] = 12.0;
            p[sp::OSC2_DETUNE] = 6.0;
            p[sp::CUTOFF] = rng.range(1200.0, 2600.0);
            p[sp::RESO] = 0.2;
            p[sp::FILTER_ENV] = 1.5;
            p[sp::FILT_D] = 0.18;
            p[sp::FILT_S] = 0.1;
            p[sp::AMP_D] = 0.25;
            p[sp::AMP_S] = 0.15;
            p[sp::AMP_R] = 0.15;
            p[sp::SPREAD] = 0.6;
            p[sp::LEVEL] = 0.6;
        }
        "pluck" => {
            p[sp::OSC1_WAVE] = 0.0;
            p[sp::OSC2_WAVE] = 1.0;
            p[sp::OSC2_DETUNE] = rng.range(5.0, 12.0);
            p[sp::CUTOFF] = rng.range(500.0, 1000.0);
            p[sp::RESO] = 0.2;
            p[sp::FILTER_ENV] = rng.range(3.0, 4.5);
            p[sp::FILT_D] = rng.range(0.1, 0.22);
            p[sp::FILT_S] = 0.0;
            p[sp::AMP_D] = rng.range(0.3, 0.5);
            p[sp::AMP_S] = 0.0;
            p[sp::AMP_R] = 0.3;
            p[sp::SPREAD] = 0.7;
            p[sp::LEVEL] = 0.7;
        }
        "brass" => {
            p[sp::OSC2_DETUNE] = 9.0;
            p[sp::CUTOFF] = 900.0;
            p[sp::RESO] = 0.15;
            p[sp::FILTER_ENV] = 2.5;
            p[sp::FILT_A] = 0.04;
            p[sp::FILT_D] = 0.4;
            p[sp::FILT_S] = 0.4;
            p[sp::AMP_A] = 0.02;
            p[sp::AMP_S] = 0.8;
            p[sp::AMP_R] = 0.3;
            p[sp::SPREAD] = 0.7;
            p[sp::LEVEL] = 0.6;
        }
        "lead" => {
            p[sp::OSC2_WAVE] = 1.0;
            p[sp::OSC2_DETUNE] = rng.range(4.0, 9.0);
            p[sp::CUTOFF] = rng.range(1800.0, 3500.0);
            p[sp::RESO] = 0.2;
            p[sp::FILTER_ENV] = 1.5;
            p[sp::FILT_D] = 0.3;
            p[sp::FILT_S] = 0.5;
            p[sp::AMP_A] = 0.005;
            p[sp::AMP_S] = 0.75;
            p[sp::AMP_R] = 0.2;
            p[sp::LFO_RATE] = 5.5;
            p[sp::LFO_PITCH] = 0.12;
            p[sp::GLIDE] = 0.03;
            p[sp::MONO] = if rng.chance(0.5) { 1.0 } else { 0.0 };
            p[sp::SPREAD] = 0.4;
            p[sp::LEVEL] = 0.6;
        }
        "bell" => {
            p[sp::OSC1_WAVE] = 3.0;
            p[sp::OSC2_WAVE] = 3.0;
            p[sp::OSC2_SEMI] = *rng.pick(&[19.0, 24.0, 28.0]);
            p[sp::OSC2_DETUNE] = 3.0;
            p[sp::OSC_MIX] = 0.35;
            p[sp::CUTOFF] = 9000.0;
            p[sp::FILTER_ENV] = 0.0;
            p[sp::AMP_A] = 0.001;
            p[sp::AMP_D] = rng.range(0.8, 1.6);
            p[sp::AMP_S] = 0.0;
            p[sp::AMP_R] = 0.8;
            p[sp::SPREAD] = 0.5;
            p[sp::LEVEL] = 0.75;
        }
        "flute" => {
            p[sp::OSC1_WAVE] = 3.0;
            p[sp::OSC2_WAVE] = 2.0;
            p[sp::OSC_MIX] = 0.2;
            p[sp::NOISE] = 0.12;
            p[sp::CUTOFF] = 3200.0;
            p[sp::FILTER_ENV] = 0.3;
            p[sp::AMP_A] = 0.06;
            p[sp::AMP_S] = 0.85;
            p[sp::AMP_R] = 0.15;
            p[sp::LFO_RATE] = 5.0;
            p[sp::LFO_PITCH] = 0.15;
            p[sp::MONO] = 1.0;
            p[sp::GLIDE] = 0.04;
            p[sp::SPREAD] = 0.0;
            p[sp::LEVEL] = 0.8;
        }
        "chip" => {
            p[sp::OSC1_WAVE] = 1.0;
            p[sp::OSC_MIX] = 0.0;
            p[sp::CUTOFF] = 7000.0;
            p[sp::FILTER_ENV] = 0.0;
            p[sp::AMP_A] = 0.001;
            p[sp::AMP_D] = 0.2;
            p[sp::AMP_S] = 0.6;
            p[sp::AMP_R] = 0.05;
            p[sp::MONO] = 1.0;
            p[sp::SPREAD] = 0.0;
            p[sp::LEVEL] = 0.45;
        }
        "supersaw" => {
            p[sp::OSC2_DETUNE] = rng.range(14.0, 24.0);
            p[sp::SPREAD] = 0.95;
            p[sp::CUTOFF] = rng.range(2500.0, 5000.0);
            p[sp::FILTER_ENV] = 1.0;
            p[sp::FILT_D] = 0.5;
            p[sp::FILT_S] = 0.6;
            p[sp::AMP_A] = 0.01;
            p[sp::AMP_S] = 0.8;
            p[sp::AMP_R] = 0.35;
            p[sp::LEVEL] = 0.45;
        }
        _ => {}
    }
    for id in [sp::CUTOFF, sp::AMP_D, sp::AMP_R] {
        jitter(rng, &mut p, id, 0.08);
    }
    p
}
