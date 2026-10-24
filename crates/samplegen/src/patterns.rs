//! Pattern generators: drums, bass lines, chord rhythms, hooks and arps.

use crate::music::{chord, voice_lead, DrumStyle, Key, Mode};
use crate::render::n;
use crate::rng::Rng;
use clipdeck_engine::Note;

// Drum rack pad notes (pad index = note - 36).
pub const KICK: i32 = 36;
pub const SNARE: i32 = 37;
pub const CLAP: i32 = 38;
pub const CH: i32 = 39;
pub const OH: i32 = 40;
pub const PERC1: i32 = 41;
pub const PERC2: i32 = 42;
pub const SHAKER: i32 = 43;
pub const TOM: i32 = 44;
pub const RIM: i32 = 45;
pub const RIDE: i32 = 46;
pub const CRASH: i32 = 47;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrumVariant {
    Full,
    Tops,
    Fill,
}

impl DrumVariant {
    pub fn slug(self) -> &'static str {
        match self {
            DrumVariant::Full => "full",
            DrumVariant::Tops => "tops",
            DrumVariant::Fill => "fill",
        }
    }
}

/// Add hits from a 16-step string ('x' accent, 'o' normal, 'g' ghost, '.' rest).
fn steps(out: &mut Vec<Note>, pattern: &str, pitch: i32, bar: u32, rng: &mut Rng, human: f32) {
    for (i, c) in pattern.chars().enumerate() {
        let vel = match c {
            'x' => 1.0,
            'o' => 0.75,
            'g' => 0.4,
            _ => continue,
        };
        let jitter = if human > 0.0 { rng.range(0.0, human) as f64 } else { 0.0 };
        let v = (vel + rng.range(-0.07, 0.07)).clamp(0.1, 1.0);
        out.push(n(bar as f64 * 4.0 + i as f64 * 0.25 + jitter, 0.2, pitch, v));
    }
}

fn pick(rng: &mut Rng, opts: &[&'static str]) -> &'static str {
    rng.pick(opts)
}

pub struct DrumPattern {
    pub notes: Vec<Note>,
    pub uses: Vec<i32>,
}

pub fn drums(style: DrumStyle, bars: u32, variant: DrumVariant, rng: &mut Rng) -> DrumPattern {
    let mut out = Vec::new();
    let human = match style {
        DrumStyle::HipHop | DrumStyle::Lofi => 0.025,
        DrumStyle::DeepHouse | DrumStyle::Afro => 0.01,
        _ => 0.0,
    };
    // Choose one pattern per instrument for the whole loop, with small per-bar variation.
    use DrumStyle::*;
    let (kick, snare, clap, ch, oh, perc, shaker, rim, ride): (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) = match style {
        House => (
            "x...x...x...x...",
            "",
            "....x.......x...",
            pick(rng, &["..x...x...x...x.", "g.x.g.x.g.x.g.x.", "..x...x...x..gx."]),
            pick(rng, &["", "..o...o...o...o.", "..........o....."]),
            pick(rng, &["", "...g......g..g..", ".g.....g.g......"]),
            pick(rng, &["", "gogggogggogggogg", "g.g.g.g.g.g.g.g."]),
            "",
            "",
        ),
        DeepHouse => (
            "x...x...x...x...",
            "",
            pick(rng, &["....o.......o...", "....x.......x..g"]),
            pick(rng, &["..o...o...o...o.", "g.o.g.o.g.o.g.o."]),
            pick(rng, &["", "..g...g...g...g."]),
            pick(rng, &["...g..g....g..g.", "......g.......g."]),
            "gogggogggogggogg",
            pick(rng, &["...g..o....g..o.", "", "......o.......o."]),
            "",
        ),
        Techno => (
            pick(rng, &["x...x...x...x...", "x...x...x...x..g"]),
            "",
            pick(rng, &["", "....o.......o...", "............o..."]),
            pick(rng, &["gxggoxggoxggoxgg", "g.o.g.o.g.o.g.o.", "xgogxgogxgogxgog"]),
            "..x...x...x...x.",
            pick(rng, &["..g..g....g..g..", "", ".g...g...g...g.."]),
            "",
            "",
            pick(rng, &["", "o.o.o.o.o.o.o.o."]),
        ),
        HipHop => (
            pick(rng, &["x.........x.....", "x......x..x.....", "x.x.......x..x..", "x......x.x......"]),
            pick(rng, &["....x.......x...", "....x.......x..g", "....x..g....x..."]),
            "",
            pick(rng, &["x.o.x.o.x.o.x.o.", "o.o.o.o.o.o.o.og", "x.g.o.g.x.g.o.g."]),
            pick(rng, &["", "..............o."]),
            pick(rng, &["", "......g......g.."]),
            "",
            "",
            "",
        ),
        Trap => (
            pick(rng, &["x......x..x.....", "x.......x.x....x", "x..x......x....."]),
            "",
            "........x.......",
            "xoxoxoxoxoxoxoxo",
            pick(rng, &["", "..............o.", "......o........."]),
            pick(rng, &["", "...........g...."]),
            "",
            pick(rng, &["", "...g.......g...."]),
            "",
        ),
        Dnb => (
            pick(rng, &["x.........x.....", "x.........x..x..", "x.x.......x....."]),
            pick(rng, &["....x.......x...", "....x..g.g..x...", "....x.g.....x..g"]),
            "",
            pick(rng, &["x.o.x.o.x.o.x.o.", "o.o.o.o.o.o.o.o."]),
            pick(rng, &["", "..o.......o....."]),
            "",
            pick(rng, &["", "gggggggggggggggg"]),
            "",
            pick(rng, &["", "o.o.o.o.o.o.o.o."]),
        ),
        Lofi => (
            pick(rng, &["x.........x.....", "x......x..x.....", "x.......x.x....."]),
            pick(rng, &["....o.......o...", ""]),
            "",
            pick(rng, &["o.g.o.g.o.g.o.g.", "o.o.o.o.o.o.o.o."]),
            "",
            "",
            pick(rng, &["", "g.g.g.g.g.g.g.g."]),
            pick(rng, &["....x.......x...", ""]),
            "",
        ),
        Garage => (
            pick(rng, &["x.........x.....", "x......x..x.....", "x.........x...g."]),
            "....x.......x...",
            pick(rng, &["", "....g.......g..."]),
            pick(rng, &["..o...o...o...o.", "g.o.g.o.ggo.g.o."]),
            pick(rng, &["", "......o.......o."]),
            pick(rng, &["...g.....g......", ""]),
            "gggggggggggggggg",
            pick(rng, &["", ".g.....g..g....."]),
            "",
        ),
        Afro => (
            "x...x...x...x...",
            "",
            pick(rng, &["", "....o.......o..."]),
            pick(rng, &["..o...o...o...o.", "g.o.g.o.g.o.g.o."]),
            "",
            pick(rng, &["x..x..x...x.x...", "x..o..x.o..x..o.", ".o..o..x.o..o..x"]),
            "goggoggogoggoggo",
            pick(rng, &["x..x..x.x..x..x.", "..x..x....x..x.."]),
            "",
        ),
        Reggaeton => (
            "x...x...x...x...",
            "...x..x....x..x.",
            "",
            pick(rng, &["x.o.x.o.x.o.x.o.", "o.o.o.o.o.o.o.o."]),
            "",
            pick(rng, &["", "......g.......g."]),
            pick(rng, &["", "g.g.g.g.g.g.g.g."]),
            "",
            "",
        ),
        Disco => (
            "x...x...x...x...",
            "",
            "....x.......x...",
            pick(rng, &["x.o.x.o.x.o.x.o.", "gogogogogogogogo"]),
            "..o...o...o...o.",
            "",
            pick(rng, &["gogggogggogggogg", "gggggggggggggggg"]),
            "",
            "",
        ),
        Synthwave => (
            pick(rng, &["x.......x.......", "x.......x.x.....", "x...x...x...x..."]),
            "....x.......x...",
            "",
            pick(rng, &["x.o.x.o.x.o.x.o.", "o.o.o.o.o.o.o.o."]),
            pick(rng, &["", "..............o."]),
            "",
            "",
            "",
            "",
        ),
    };

    let mut uses = Vec::new();
    for bar in 0..bars {
        let last = bar + 1 == bars;
        let fill = variant == DrumVariant::Fill && last;
        let lane = |s: &'static str| -> &'static str {
            if fill {
                &s[..12]
            } else {
                s
            }
        };
        if variant != DrumVariant::Tops && !kick.is_empty() {
            steps(&mut out, lane(kick), KICK, bar, rng, 0.0);
            uses.push(KICK);
        }
        if !snare.is_empty() {
            steps(&mut out, lane(snare), SNARE, bar, rng, human);
            uses.push(SNARE);
        }
        if !clap.is_empty() {
            steps(&mut out, lane(clap), CLAP, bar, rng, human * 0.5);
            uses.push(CLAP);
        }
        if !ch.is_empty() {
            if style == Trap {
                trap_hats(&mut out, bar, rng);
            } else {
                steps(&mut out, lane(ch), CH, bar, rng, human);
            }
            uses.push(CH);
        }
        if !oh.is_empty() {
            steps(&mut out, lane(oh), OH, bar, rng, human);
            uses.push(OH);
        }
        if !perc.is_empty() {
            let p = if bar % 2 == 1 && rng.chance(0.4) { PERC2 } else { PERC1 };
            steps(&mut out, lane(perc), p, bar, rng, human);
            uses.push(p);
        }
        if !shaker.is_empty() {
            steps(&mut out, lane(shaker), SHAKER, bar, rng, human);
            uses.push(SHAKER);
        }
        if !rim.is_empty() {
            steps(&mut out, lane(rim), RIM, bar, rng, human);
            uses.push(RIM);
        }
        if !ride.is_empty() {
            steps(&mut out, lane(ride), RIDE, bar, rng, 0.0);
            uses.push(RIDE);
        }
        if fill {
            // Last beat: snare/tom roll.
            let roll_pitch = if rng.chance(0.5) { TOM } else { SNARE };
            let div = *rng.pick(&[4usize, 6, 8]);
            for k in 0..div {
                let vel = 0.4 + 0.6 * k as f32 / div as f32;
                out.push(n(bar as f64 * 4.0 + 3.0 + k as f64 / div as f64, 0.1, roll_pitch, vel));
            }
            uses.push(roll_pitch);
            if variant != DrumVariant::Tops && !kick.is_empty() {
                out.push(n(bar as f64 * 4.0 + 3.0 - 0.0, 0.2, KICK, 0.9));
            }
        }
        if bar == 0 && variant == DrumVariant::Fill && rng.chance(0.5) {
            out.push(n(0.0, 0.5, CRASH, 0.8));
            uses.push(CRASH);
        }
    }
    uses.sort();
    uses.dedup();
    DrumPattern { notes: out, uses }
}

/// Trap hats: steady 8ths/16ths with triplet and 32nd rolls.
fn trap_hats(out: &mut Vec<Note>, bar: u32, rng: &mut Rng) {
    let base = bar as f64 * 4.0;
    for beat in 0..4 {
        let b = base + beat as f64;
        let roll = rng.f();
        if roll < 0.2 {
            for k in 0..6 {
                out.push(n(b + k as f64 / 6.0, 0.05, CH, 0.5 + 0.08 * k as f32));
            }
        } else if roll < 0.32 {
            for k in 0..8 {
                out.push(n(b + k as f64 / 8.0, 0.05, CH, 0.45 + 0.06 * k as f32));
            }
        } else if roll < 0.55 {
            for k in 0..2 {
                out.push(n(b + k as f64 * 0.5, 0.1, CH, if k == 0 { 0.9 } else { 0.6 }));
            }
        } else {
            for k in 0..4 {
                out.push(n(b + k as f64 * 0.25, 0.1, CH, if k % 2 == 0 { 0.9 } else { 0.55 }));
            }
        }
    }
}

// ------------------------------------------------------------ bass

#[derive(Clone, Copy, Debug)]
pub enum BassStyle {
    Offbeat,
    Groove,
    Rolling,
    Acid,
    Sustain,
    Trap808,
    Reese,
    TwoStep,
    Afro,
    Dembow,
    Octave,
    Eighths,
}

impl BassStyle {
    pub fn slug(self) -> &'static str {
        match self {
            BassStyle::Offbeat => "offbeat",
            BassStyle::Groove => "groove",
            BassStyle::Rolling => "rolling",
            BassStyle::Acid => "acid",
            BassStyle::Sustain => "sustain",
            BassStyle::Trap808 => "808",
            BassStyle::Reese => "reese",
            BassStyle::TwoStep => "2-step",
            BassStyle::Afro => "syncopated",
            BassStyle::Dembow => "dembow",
            BassStyle::Octave => "octave",
            BassStyle::Eighths => "driving",
        }
    }
}

/// Fit a pitch into [lo, lo+12).
fn fold(p: i32, lo: i32) -> i32 {
    lo + (p - lo).rem_euclid(12)
}

pub fn bass(style: BassStyle, key: &Key, prog: &[i32], bars: u32, rng: &mut Rng) -> Vec<Note> {
    let mut out = Vec::new();
    let lo = match style {
        BassStyle::Trap808 | BassStyle::Sustain | BassStyle::Reese | BassStyle::Dembow => 28,
        _ => 33,
    };
    for bar in 0..bars {
        let deg = prog[bar as usize % prog.len()];
        let root = fold(key.degree(deg, 2), lo);
        let fifth = fold(key.degree(deg + 4, 2), lo);
        let fifth = if fifth < root { fifth + 12 } else { fifth };
        let oct = root + 12;
        let b = bar as f64 * 4.0;
        let next_root = fold(key.degree(prog[(bar as usize + 1) % prog.len()], 2), lo);
        match style {
            BassStyle::Offbeat => {
                for beat in 0..4 {
                    let p = if rng.chance(0.15) { oct } else { root };
                    out.push(n(b + beat as f64 + 0.5, 0.3, p, 0.9));
                }
                if rng.chance(0.4) {
                    out.push(n(b + 3.75, 0.2, fifth, 0.6));
                }
            }
            BassStyle::Groove => {
                let pat = *rng.pick(&[&[0.0, 0.75, 1.5, 2.5, 3.0][..], &[0.5, 1.25, 2.5, 3.25][..], &[0.0, 1.5, 2.0, 3.5][..]]);
                for (i, &s) in pat.iter().enumerate() {
                    let p = if i == pat.len() - 1 && rng.chance(0.5) { fifth } else { root };
                    out.push(n(b + s, 0.45, p, if i == 0 { 1.0 } else { 0.8 }));
                }
            }
            BassStyle::Rolling => {
                for s in 0..16 {
                    if s % 4 == 0 && rng.chance(0.85) {
                        continue; // leave room for the kick
                    }
                    let p = if rng.chance(0.12) { oct } else { root };
                    out.push(n(b + s as f64 * 0.25, 0.18, p, if s % 2 == 1 { 0.95 } else { 0.7 }));
                }
            }
            BassStyle::Acid => {
                let scale_notes: Vec<i32> = (0..7).map(|d| fold(key.degree(deg + d, 2), lo)).collect();
                for s in 0..16 {
                    if rng.chance(0.25) {
                        continue;
                    }
                    let mut p = if rng.chance(0.55) { root } else { *rng.pick(&scale_notes) };
                    if rng.chance(0.2) {
                        p += 12;
                    }
                    let slide = rng.chance(0.25);
                    let accent = rng.chance(0.3);
                    out.push(n(b + s as f64 * 0.25, if slide { 0.3 } else { 0.15 }, p, if accent { 1.0 } else { 0.55 }));
                }
            }
            BassStyle::Sustain => {
                if rng.chance(0.5) {
                    out.push(n(b, 3.5, root, 0.9));
                } else {
                    out.push(n(b, 1.75, root, 0.9));
                    out.push(n(b + 2.0, 1.25, root, 0.8));
                    out.push(n(b + 3.5, 0.45, if rng.chance(0.5) { fifth } else { oct }, 0.7));
                }
            }
            BassStyle::Trap808 => {
                let hits = *rng.pick(&[&[0.0, 1.75, 2.5][..], &[0.0, 0.75, 2.5, 3.5][..], &[0.0, 2.0, 2.75][..]]);
                for (i, &s) in hits.iter().enumerate() {
                    let end = hits.get(i + 1).copied().unwrap_or(4.0);
                    let p = if i > 0 && rng.chance(0.3) { oct } else { root };
                    out.push(n(b + s, (end - s) * 0.95, p, 1.0));
                    // Slide up an octave by overlapping a note (mono glide).
                    if rng.chance(0.2) {
                        out.push(n(b + s + (end - s) * 0.6, (end - s) * 0.35, p + 12, 0.9));
                    }
                }
            }
            BassStyle::Reese => {
                out.push(n(b, 2.9, root, 0.9));
                out.push(n(b + 3.0, 0.9, if rng.chance(0.5) { root } else { next_root }, 0.8));
            }
            BassStyle::TwoStep => {
                let pat = *rng.pick(&[&[0.0, 0.75, 2.5, 3.25][..], &[0.0, 1.5, 2.75][..], &[0.5, 1.25, 2.5, 3.5][..]]);
                for &s in pat {
                    let p = if rng.chance(0.2) { oct } else { root };
                    out.push(n(b + s, 0.4, p, 0.9));
                }
            }
            BassStyle::Afro => {
                for &s in &[0.0, 0.75, 1.5, 2.25, 3.0, 3.5] {
                    if rng.chance(0.2) {
                        continue;
                    }
                    let p = if rng.chance(0.25) { fifth } else { root };
                    out.push(n(b + s, 0.35, p, 0.85));
                }
            }
            BassStyle::Dembow => {
                for &(s, l) in &[(0.0, 0.7), (0.75, 0.2), (1.5, 0.45), (2.0, 0.7), (2.75, 0.2), (3.5, 0.45)] {
                    out.push(n(b + s, l, root, 0.9));
                }
            }
            BassStyle::Octave => {
                for s in 0..8 {
                    let p = if s % 2 == 0 { root } else { oct };
                    out.push(n(b + s as f64 * 0.5, 0.25, p, if s % 2 == 0 { 0.9 } else { 0.75 }));
                }
            }
            BassStyle::Eighths => {
                for s in 0..8 {
                    let p = if s == 7 && rng.chance(0.4) { fifth } else { root };
                    out.push(n(b + s as f64 * 0.5, 0.4, p, if s % 2 == 0 { 0.9 } else { 0.7 }));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------ chords

#[derive(Clone, Copy, Debug)]
pub enum ChordRhythm {
    Sustain,
    OffbeatStabs,
    Syncopated,
    Eighths,
    Gated,
    Strum,
    TwoStep,
    Dembow,
    DubStab,
}

impl ChordRhythm {
    pub fn slug(self) -> &'static str {
        match self {
            ChordRhythm::Sustain => "sustained",
            ChordRhythm::OffbeatStabs => "offbeat-stabs",
            ChordRhythm::Syncopated => "syncopated",
            ChordRhythm::Eighths => "eighths",
            ChordRhythm::Gated => "gated",
            ChordRhythm::Strum => "strummed",
            ChordRhythm::TwoStep => "2-step",
            ChordRhythm::Dembow => "dembow",
            ChordRhythm::DubStab => "dub-stab",
        }
    }
}

pub fn chords(rhythm: ChordRhythm, key: &Key, prog: &[i32], bars: u32, sevenths: bool, rng: &mut Rng) -> Vec<Note> {
    let mut out = Vec::new();
    let mut prev: Option<Vec<i32>> = None;
    for bar in 0..bars {
        let deg = prog[bar as usize % prog.len()];
        let raw = chord(key, deg, 3, sevenths, sevenths && rng.chance(0.2));
        let v = voice_lead(&raw, prev.as_deref(), 55, 79);
        let b = bar as f64 * 4.0;
        let hits: Vec<(f64, f64, f32)> = match rhythm {
            ChordRhythm::Sustain => vec![(0.0, 3.9, 0.8)],
            ChordRhythm::OffbeatStabs => (0..4).map(|k| (k as f64 + 0.5, 0.22, 0.9)).collect(),
            ChordRhythm::Syncopated => {
                let p = *rng.pick(&[&[0.0, 0.75, 1.5, 2.5, 3.25][..], &[0.0, 1.5, 3.0][..], &[0.5, 1.75, 2.5, 3.5][..]]);
                p.iter().map(|&s| (s, 0.45, 0.85)).collect()
            }
            ChordRhythm::Eighths => (0..8).map(|k| (k as f64 * 0.5, 0.3, if k % 2 == 0 { 0.85 } else { 0.65 })).collect(),
            ChordRhythm::Gated => (0..16).filter(|_| rng.chance(0.8)).map(|k| (k as f64 * 0.25, 0.15, 0.75)).collect(),
            ChordRhythm::Strum => vec![(0.0, 2.8, 0.75), (3.0, 0.9, 0.55)],
            ChordRhythm::TwoStep => vec![(0.0, 0.4, 0.85), (0.75, 0.35, 0.7), (2.5, 0.6, 0.8)],
            ChordRhythm::Dembow => {
                vec![(0.0, 0.3, 0.85), (0.75, 0.25, 0.75), (1.5, 0.3, 0.8), (2.0, 0.3, 0.8), (2.75, 0.25, 0.75), (3.5, 0.3, 0.8)]
            }
            ChordRhythm::DubStab => {
                let s = *rng.pick(&[0.5, 1.5, 0.75]);
                vec![(s, 0.25, 0.9)]
            }
        };
        let strum = matches!(rhythm, ChordRhythm::Strum);
        for (s, l, vel) in hits {
            for (i, &p) in v.iter().enumerate() {
                let off = if strum { i as f64 * 0.025 } else { 0.0 };
                out.push(n(b + s + off, l - off, p, vel + rng.range(-0.05, 0.05)));
            }
        }
        prev = Some(v);
    }
    out
}

// ------------------------------------------------------------ hooks

/// A memorable melody: a 1-bar motif repeated over the progression with a
/// variation and a resolving ending (A A' A A'').
pub fn hook(key: &Key, prog: &[i32], bars: u32, legato: bool, rng: &mut Rng) -> Vec<Note> {
    let rhythms: &[&[f64]] = &[
        &[0.0, 0.75, 1.5, 2.0, 2.5],
        &[0.0, 0.5, 1.0, 1.75, 2.5, 3.0],
        &[0.5, 1.0, 1.5, 2.25, 3.0],
        &[0.0, 0.75, 1.5, 2.25, 3.0, 3.5],
        &[0.0, 1.0, 1.5, 2.0, 3.0],
        &[0.25, 0.75, 1.25, 1.75, 2.5],
        &[0.0, 0.5, 0.75, 1.5, 2.0, 2.75],
    ];
    let rhythm = *rng.pick(rhythms);
    let pent = key.mode.pent();
    // Pentatonic pitches across two octaves around C5.
    let base = 12 * 5 + key.tonic;
    let pool: Vec<i32> = (-1..2).flat_map(|o| pent.iter().map(move |&s| base + s + 12 * o)).filter(|p| (62..=84).contains(p)).collect();
    let start_idx = pool.len() / 2 - 1;
    // Motif as pool index steps.
    let mut motif = Vec::with_capacity(rhythm.len());
    let mut idx = start_idx as i32;
    for _ in rhythm {
        motif.push(idx);
        let step = *rng.pick(&[-2, -1, -1, 0, 1, 1, 2, 3]);
        idx = (idx + step).clamp(0, pool.len() as i32 - 1);
    }

    let mut out = Vec::new();
    for bar in 0..bars {
        let deg = prog[bar as usize % prog.len()];
        let b = bar as f64 * 4.0;
        // Shift the motif to sit on the current chord's root (in pool steps).
        let chord_root = key.degree(deg, 5);
        let nearest = pool.iter().enumerate().min_by_key(|(_, &p)| (p - chord_root).rem_euclid(12)).map(|(i, _)| i as i32).unwrap_or(0);
        let shift = if bar % 2 == 0 { 0 } else { ((nearest - start_idx as i32) % 3).clamp(-2, 2) };
        let last_bar = bar + 1 == bars;
        for (i, &s) in rhythm.iter().enumerate() {
            let mut pi = motif[i] + shift;
            if last_bar && i == rhythm.len() - 1 {
                // Resolve to the tonic nearest the line.
                pi = pool
                    .iter()
                    .enumerate()
                    .filter(|(_, &p)| (p - key.tonic).rem_euclid(12) == 0)
                    .min_by_key(|(j, _)| (*j as i32 - pi).abs())
                    .map(|(j, _)| j as i32)
                    .unwrap_or(pi);
            } else if bar % 4 == 3 && i >= rhythm.len() - 2 && rng.chance(0.5) {
                pi += 1;
            }
            let pi = pi.clamp(0, pool.len() as i32 - 1) as usize;
            let next = rhythm.get(i + 1).copied().unwrap_or(4.0);
            let len = if legato { (next - s) * 0.95 } else { ((next - s) * 0.6).min(0.4) };
            let len = if last_bar && i == rhythm.len() - 1 { (4.0 - s) * 0.9 } else { len };
            out.push(n(b + s, len, pool[pi], if i == 0 { 0.95 } else { 0.8 }));
        }
    }
    out
}

pub fn arp(key: &Key, prog: &[i32], bars: u32, rng: &mut Rng) -> Vec<Note> {
    let mode = *rng.pick(&["up", "down", "updown", "pingpong"]);
    let rate = if rng.chance(0.7) { 0.25 } else { 0.5 };
    let mut out = Vec::new();
    let mut prev: Option<Vec<i32>> = None;
    for bar in 0..bars {
        let deg = prog[bar as usize % prog.len()];
        let raw = chord(key, deg, 4, rng.chance(0.4), false);
        let mut v = voice_lead(&raw, prev.as_deref(), 60, 84);
        prev = Some(v.clone());
        v.push(v[0] + 12);
        let seq: Vec<i32> = match mode {
            "down" => v.iter().rev().copied().collect(),
            "updown" => v.iter().copied().chain(v.iter().rev().skip(1).take(v.len().saturating_sub(2)).copied()).collect(),
            "pingpong" => v.iter().enumerate().flat_map(|(i, &p)| if i % 2 == 0 { vec![p] } else { vec![p, v[0]] }).collect(),
            _ => v.clone(),
        };
        let steps = (4.0 / rate) as usize;
        for s in 0..steps {
            let p = seq[s % seq.len()];
            out.push(n(bar as f64 * 4.0 + s as f64 * rate, rate * 0.8, p, if s % 4 == 0 { 0.95 } else { 0.7 }));
        }
    }
    out
}

pub fn choose_mode(rng: &mut Rng, major_chance: f32) -> Mode {
    if rng.chance(major_chance) {
        Mode::Major
    } else if rng.chance(0.2) {
        Mode::Dorian
    } else {
        Mode::Minor
    }
}
