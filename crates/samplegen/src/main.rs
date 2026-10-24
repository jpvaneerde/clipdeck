//! Generates the Clipdeck sample library.
//!
//! Usage: clipdeck-samplegen [--out DIR] [--scale F] [--seed N] [--force]
//!
//! The library is organized as construction kits: for every genre, each kit
//! shares a tempo, key and chord progression across its drum, bass, chord,
//! hook, arp and vocal loops, so loops from one kit always fit together. The
//! kit's drum sounds are exported as one-shots too. On top of that come tuned
//! 808s, vocal chops, synth hits and transition FX.
//!
//! Output: `<out>/<id>.wav` per sample plus `<out>/manifest.jsonl`, one JSON
//! object per line, which the server indexes.

mod drumsynth;
mod music;
mod patterns;
mod render;
mod rng;
mod sfx;
mod vox;

use drumsynth::SR;
use music::{progression, Genre, Key, GENRES, NOTE_NAMES};
use patterns::{BassStyle, ChordRhythm, DrumVariant};
use rayon::prelude::*;
use render::{mono_of, normalize, trim_stereo, PadDef, Session};
use rng::Rng;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

pub const LIBRARY_VERSION: u32 = 1;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Meta {
    id: String,
    path: String,
    name: String,
    category: String,
    instrument: String,
    genre: Option<String>,
    bpm: Option<f32>,
    key: Option<String>,
    bars: Option<u32>,
    beats: Option<f64>,
    kit: Option<String>,
    tags: Vec<String>,
    duration: f32,
    channels: u16,
    sample_rate: u32,
    source: String,
}

struct Out {
    id: String,
    name: String,
    category: &'static str,
    instrument: String,
    genre: Option<&'static Genre>,
    bpm: Option<f32>,
    key: Option<String>,
    bars: Option<u32>,
    kit: Option<String>,
    tags: Vec<String>,
    channels: u16,
    data: Vec<f32>,
}

impl Out {
    fn new(id: String, name: String, category: &'static str, instrument: &str, channels: u16, data: Vec<f32>) -> Self {
        Self {
            id,
            name,
            category,
            instrument: instrument.to_string(),
            genre: None,
            bpm: None,
            key: None,
            bars: None,
            kit: None,
            tags: Vec::new(),
            channels,
            data,
        }
    }
}

struct Ctx {
    out: PathBuf,
    written: AtomicUsize,
}

fn write(ctx: &Ctx, o: Out) -> Meta {
    let rel = format!("{}.wav", o.id);
    let path = ctx.out.join(&rel);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("create dir");
    }
    let spec =
        hound::WavSpec { channels: o.channels, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).expect("create wav");
    for s in &o.data {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        w.write_sample(v).expect("write");
    }
    w.finalize().expect("finalize");
    ctx.written.fetch_add(1, Ordering::Relaxed);
    let frames = o.data.len() / o.channels as usize;
    let mut tags = o.tags;
    tags.push(o.category.to_string());
    tags.push(o.instrument.clone());
    tags.extend(sound_tags(&o.instrument).iter().map(|t| t.to_string()));
    if let Some(g) = o.genre {
        tags.push(g.slug.to_string());
    }
    tags.sort();
    tags.dedup();
    Meta {
        id: o.id,
        path: rel,
        name: o.name,
        category: o.category.to_string(),
        instrument: o.instrument,
        genre: o.genre.map(|g| g.slug.to_string()),
        bpm: o.bpm,
        key: o.key,
        bars: o.bars,
        beats: o.bars.map(|b| b as f64 * 4.0),
        kit: o.kit,
        tags,
        duration: frames as f32 / SR,
        channels: o.channels,
        sample_rate: SR as u32,
        source: "generated".into(),
    }
}

// ------------------------------------------------------------ kits

struct KitSounds {
    pads: Vec<(u8, PadDef, &'static str)>,
}

fn make_kit(g: &Genre, rng: &mut Rng) -> KitSounds {
    use patterns::*;
    let mut pads = Vec::new();
    let mut add = |note: i32, sample: Vec<f32>, gain: f32, pan: f32, choke: u8, name: &'static str| {
        pads.push(((note - 36) as u8, PadDef { sample, gain, pan, choke }, name));
    };
    add(KICK, drumsynth::kick(rng, g.kick), 1.0, 0.0, 0, "kick");
    add(SNARE, drumsynth::snare(rng, g.snare), 0.85, 0.0, 0, "snare");
    add(CLAP, drumsynth::clap(rng), 0.8, 0.0, 0, "clap");
    add(CH, drumsynth::hat(rng, false), 0.5, rng.range(-0.25, 0.25), 1, "hat-closed");
    add(OH, drumsynth::hat(rng, true), 0.45, rng.range(-0.25, 0.25), 1, "hat-open");
    let (p1, _) = drumsynth::perc(rng);
    add(PERC1, p1, 0.55, rng.range(-0.5, 0.5), 0, "perc");
    let (p2, _) = drumsynth::perc(rng);
    add(PERC2, p2, 0.5, rng.range(-0.5, 0.5), 0, "perc");
    add(SHAKER, drumsynth::shaker(rng), 0.4, rng.range(-0.4, 0.4), 0, "shaker");
    add(TOM, drumsynth::tom(rng), 0.7, rng.range(-0.3, 0.3), 0, "tom");
    add(RIM, drumsynth::rim(rng), 0.55, rng.range(-0.2, 0.2), 0, "rim");
    add(RIDE, drumsynth::cymbal(rng, true), 0.35, 0.3, 0, "ride");
    add(CRASH, drumsynth::cymbal(rng, false), 0.45, -0.2, 0, "crash");
    KitSounds { pads }
}

fn bass_options(slug: &str) -> &'static [(BassStyle, &'static str)] {
    use BassStyle::*;
    match slug {
        "house" => &[(Offbeat, "pluck-bass"), (Offbeat, "organ-bass"), (Groove, "sub-bass")],
        "deep-house" => &[(Groove, "sub-bass"), (Groove, "pluck-bass"), (Sustain, "sub-bass")],
        "techno" => &[(Rolling, "pluck-bass"), (Acid, "acid"), (Rolling, "reese")],
        "hip-hop" => &[(Sustain, "sub-bass"), (Groove, "pluck-bass")],
        "trap" => &[(Trap808, "808")],
        "drum-and-bass" => &[(Reese, "reese"), (Sustain, "sub-bass")],
        "lofi" => &[(Sustain, "sub-bass"), (Groove, "sub-bass")],
        "uk-garage" => &[(TwoStep, "organ-bass"), (TwoStep, "reese"), (TwoStep, "sub-bass")],
        "afro-house" => &[(Afro, "pluck-bass"), (Afro, "sub-bass")],
        "reggaeton" => &[(Dembow, "sub-bass"), (Dembow, "808")],
        "nu-disco" => &[(Octave, "disco-bass"), (Groove, "disco-bass")],
        _ => &[(Eighths, "disco-bass"), (Eighths, "pluck-bass")],
    }
}

fn chord_options(slug: &str) -> &'static [(ChordRhythm, &'static str, bool)] {
    use ChordRhythm::*;
    match slug {
        "house" => &[(OffbeatStabs, "stab", true), (Syncopated, "keys", true), (Sustain, "pad", true), (Eighths, "keys", false)],
        "deep-house" => &[(Syncopated, "keys", true), (Sustain, "pad", true), (OffbeatStabs, "stab", true)],
        "techno" => &[(DubStab, "stab", true), (Sustain, "pad", false), (Gated, "pad", false)],
        "hip-hop" => &[(Strum, "keys", true), (Sustain, "pad", true), (Sustain, "brass", false)],
        "trap" => &[(Sustain, "pad", false), (Strum, "keys", false), (Sustain, "bell", false)],
        "drum-and-bass" => &[(Sustain, "pad", true), (Syncopated, "stab", true)],
        "lofi" => &[(Strum, "lofi-keys", true), (Sustain, "lofi-keys", true)],
        "uk-garage" => &[(TwoStep, "stab", true), (TwoStep, "keys", true), (Sustain, "pad", true)],
        "afro-house" => &[(Syncopated, "keys", true), (Syncopated, "pluck", false), (Sustain, "pad", true)],
        "reggaeton" => &[(Dembow, "pluck", false), (Dembow, "keys", false), (Sustain, "pad", false)],
        "nu-disco" => &[(Eighths, "keys", true), (OffbeatStabs, "brass", false), (Sustain, "pad", true)],
        _ => &[(Sustain, "supersaw", false), (Eighths, "brass", false), (Sustain, "pad", false)],
    }
}

fn hook_presets(slug: &str) -> &'static [&'static str] {
    match slug {
        "house" => &["lead", "pluck", "bell", "chip"],
        "deep-house" => &["bell", "pluck", "flute"],
        "techno" => &["pluck", "lead", "chip"],
        "hip-hop" => &["bell", "flute", "keys"],
        "trap" => &["bell", "flute", "pluck", "lead"],
        "drum-and-bass" => &["lead", "supersaw", "bell"],
        "lofi" => &["lofi-keys", "flute", "bell"],
        "uk-garage" => &["pluck", "lead", "bell"],
        "afro-house" => &["flute", "pluck", "bell"],
        "reggaeton" => &["pluck", "lead", "flute"],
        "nu-disco" => &["lead", "brass", "chip"],
        _ => &["lead", "supersaw", "chip"],
    }
}

/// Descriptive words so text search finds sounds by character ("dark pad", "punchy kick").
fn sound_tags(name: &str) -> &'static [&'static str] {
    match name {
        "sub-bass" => &["deep", "sub", "smooth"],
        "pluck-bass" => &["plucky", "funky"],
        "acid" => &["squelchy", "303", "resonant"],
        "reese" => &["gritty", "detuned", "wide"],
        "808" => &["boomy", "sub", "slide"],
        "organ-bass" => &["organ", "bouncy"],
        "disco-bass" => &["funky", "disco"],
        "pad" => &["warm", "atmospheric", "lush"],
        "keys" => &["piano", "electric-piano", "soft"],
        "lofi-keys" => &["dusty", "vintage", "piano", "wobbly"],
        "stab" => &["punchy", "short", "organ"],
        "pluck" => &["bright", "short", "plucky"],
        "brass" => &["brassy", "bold"],
        "lead" => &["synth", "melodic", "bold"],
        "bell" => &["glassy", "shimmer", "bright"],
        "flute" => &["airy", "breathy", "soft"],
        "chip" => &["8bit", "retro", "square"],
        "supersaw" => &["wide", "big", "anthem"],
        "kick" => &["punchy", "low"],
        "snare" => &["crack", "snappy"],
        "clap" => &["snappy"],
        "hat-closed" => &["hihat", "tight", "hat"],
        "hat-open" => &["hihat", "open", "hat"],
        "ride" => &["cymbal", "metallic"],
        "crash" => &["cymbal", "splash"],
        "shaker" => &["shaker", "groove"],
        "perc" => &["percussion", "organic"],
        "tom" => &["tom", "low"],
        "rim" => &["rimshot", "click"],
        "vox" => &["vocal", "voice", "human"],
        _ => &[],
    }
}

fn mood_tags(key: &Key) -> &'static [&'static str] {
    match key.mode {
        music::Mode::Major => &["bright", "happy", "uplifting"],
        music::Mode::Dorian => &["moody", "soulful", "dark"],
        music::Mode::Minor => &["dark", "emotional"],
    }
}

fn pumping_genre(slug: &str) -> bool {
    matches!(slug, "house" | "deep-house" | "techno" | "uk-garage" | "nu-disco" | "afro-house" | "synthwave")
}

fn title(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kit_job(g: &'static Genre, k: usize, seed: u64) -> Vec<Out> {
    use clipdeck_engine::fx::track_param as tp;
    let kit_id = format!("{}-kit{:02}", g.slug, k + 1);
    let mut rng = Rng::keyed(seed, &kit_id);
    let bpm = rng.int(g.bpm.0, g.bpm.1) as f32;
    let swing = rng.range(g.swing.0, g.swing.1);
    let key = Key { tonic: rng.int(0, 11), mode: patterns::choose_mode(&mut rng, g.major_chance) };
    let key_name = key.name();
    let prog = progression(&mut rng, key.mode);
    let kit = make_kit(g, &mut rng);
    let mut outs = Vec::new();
    let label = format!("{} Kit {:02}", g.label, k + 1);
    let base = |part: &str| format!("loops/{}/{}-{}-{}bpm-{}", g.slug, kit_id, part, bpm as i32, key_name.replace('#', "s"));

    let tag = |o: &mut Out, bars: u32, keyed: bool| {
        o.genre = Some(g);
        o.bpm = Some(bpm);
        o.bars = Some(bars);
        o.kit = Some(kit_id.clone());
        if keyed {
            o.key = Some(key_name.clone());
            o.tags.extend(mood_tags(&key).iter().map(|t| t.to_string()));
        }
    };

    // One-shots from the kit.
    let export_extra = k % 3 == 0;
    for (_, pad, name) in kit.pads.iter() {
        if matches!(*name, "tom" | "ride" | "crash") && !export_extra {
            continue;
        }
        let dup = outs.iter().filter(|o: &&Out| o.category == "drums" && o.instrument == *name).count();
        let suffix = if dup > 0 { format!("-{}", dup + 1) } else { String::new() };
        let mut o = Out::new(
            format!("one-shots/{}/{}-{}{}", name, kit_id, name, suffix),
            format!("{} · {}{}", label, title(name), if dup > 0 { format!(" {}", dup + 1) } else { String::new() }),
            "drums",
            name,
            1,
            pad.sample.clone(),
        );
        o.genre = Some(g);
        o.kit = Some(kit_id.clone());
        o.tags.push("one-shot".into());
        outs.push(o);
    }

    // Drum loops.
    for variant in [DrumVariant::Full, DrumVariant::Tops, DrumVariant::Fill] {
        let pat = patterns::drums(g.drums, g.drum_bars, variant, &mut rng);
        let mut s = Session::new(bpm, swing);
        s.e.set_return_param(clipdeck_engine::return_param::REVERB_SIZE, 0.35);
        let pads: Vec<(u8, &PadDef)> =
            kit.pads.iter().filter(|(p, _, _)| pat.uses.contains(&(*p as i32 + 36))).map(|(p, d, _)| (*p, d)).collect();
        s.drums(0, &pads);
        s.param(0, tp::COMP_THRESHOLD, -14.0);
        s.param(0, tp::COMP_RATIO, 3.0);
        s.param(0, tp::COMP_ATTACK, 8.0);
        s.param(0, tp::COMP_MAKEUP, 3.0);
        s.param(0, tp::SEND_A, 0.08);
        let beats = g.drum_bars as f64 * 4.0;
        s.clip(0, beats, pat.notes);
        let mut audio = s.render_loop(beats);
        normalize(&mut audio, 0.89);
        let mut o = Out::new(
            base(&format!("drums-{}", variant.slug())),
            format!("{label} · Drums ({}) · {} BPM", variant.slug(), bpm as i32),
            "drum-loop",
            "drums",
            2,
            audio,
        );
        o.tags.push(variant.slug().into());
        o.tags.push("loop".into());
        tag(&mut o, g.drum_bars, false);
        outs.push(o);
    }

    let bars = g.music_bars;
    let beats = bars as f64 * 4.0;

    // Bass loops.
    let opts = bass_options(g.slug);
    let mut used = Vec::new();
    for i in 0..2.min(opts.len().max(2)) {
        let (style, preset) = if opts.len() == 1 { opts[0] } else { opts[(i + rng.int(0, opts.len() as i32 - 1) as usize) % opts.len()] };
        if used.contains(&(style.slug(), preset)) && opts.len() > 1 {
            continue;
        }
        used.push((style.slug(), preset));
        let notes = patterns::bass(style, &key, &prog, bars, &mut rng);
        let mut s = Session::new(bpm, swing);
        s.synth(0, &music::preset(preset, &mut rng));
        s.clip(0, beats, notes);
        let mut audio = mono_of(&s.render_loop(beats));
        normalize(&mut audio, 0.89);
        let mut o = Out::new(
            base(&format!("bass-{}-{}{}", style.slug(), preset, if i > 0 { "-b" } else { "" })),
            format!("{label} · Bass ({}, {}) · {} · {} BPM", style.slug(), preset, key_name, bpm as i32),
            "bass",
            preset,
            1,
            audio,
        );
        o.tags.extend(["loop".to_string(), style.slug().to_string()]);
        tag(&mut o, bars, true);
        outs.push(o);
    }

    // Chord loops.
    let opts = chord_options(g.slug);
    let first = rng.int(0, opts.len() as i32 - 1) as usize;
    for i in 0..2 {
        let (rhythm, preset, sevenths) = opts[(first + i) % opts.len()];
        let notes = patterns::chords(rhythm, &key, &prog, bars, sevenths, &mut rng);
        let mut s = Session::new(bpm, swing);
        s.synth(0, &music::preset(preset, &mut rng));
        let wet = if preset == "pad" || preset == "supersaw" { 0.35 } else { 0.18 };
        s.param(0, tp::SEND_A, wet);
        if matches!(rhythm, ChordRhythm::DubStab) {
            s.param(0, tp::SEND_B, 0.45);
        }
        let pump = pumping_genre(g.slug) && rng.chance(0.5);
        if pump {
            s.ghost_sidechain(0, 1, beats, 0.65);
        }
        s.clip(0, beats, notes);
        let mut audio = s.render_loop(beats);
        normalize(&mut audio, 0.89);
        let mut o = Out::new(
            base(&format!("chords-{}-{}", rhythm.slug(), preset)),
            format!("{label} · Chords ({}, {}) · {} · {} BPM", rhythm.slug(), preset, key_name, bpm as i32),
            "chords",
            preset,
            2,
            audio,
        );
        o.tags.extend(["loop".to_string(), rhythm.slug().to_string()]);
        if pump {
            o.tags.push("sidechain".into());
        }
        tag(&mut o, bars, true);
        outs.push(o);
    }

    // Hooks.
    let presets = hook_presets(g.slug);
    let first = rng.int(0, presets.len() as i32 - 1) as usize;
    for i in 0..2 {
        let preset = presets[(first + i) % presets.len()];
        let legato = matches!(preset, "lead" | "flute" | "supersaw" | "brass");
        let notes = patterns::hook(&key, &prog, bars, legato, &mut rng);
        let mut s = Session::new(bpm, swing);
        s.synth(0, &music::preset(preset, &mut rng));
        s.param(0, tp::SEND_A, 0.2);
        s.param(0, tp::SEND_B, rng.range(0.1, 0.3));
        s.clip(0, beats, notes);
        let mut audio = s.render_loop(beats);
        normalize(&mut audio, 0.89);
        let mut o = Out::new(
            base(&format!("hook-{}-{}", preset, i + 1)),
            format!("{label} · Hook {} ({}) · {} · {} BPM", i + 1, preset, key_name, bpm as i32),
            "hook",
            preset,
            2,
            audio,
        );
        o.tags.extend(["loop".to_string(), "melody".to_string()]);
        tag(&mut o, bars, true);
        outs.push(o);
    }

    // Arp.
    {
        let notes = patterns::arp(&key, &prog, bars, &mut rng);
        let mut s = Session::new(bpm, swing.min(0.1));
        let preset = if rng.chance(0.6) { "pluck" } else { "bell" };
        s.synth(0, &music::preset(preset, &mut rng));
        s.param(0, tp::SEND_A, 0.15);
        s.param(0, tp::SEND_B, 0.3);
        s.clip(0, beats, notes);
        let mut audio = s.render_loop(beats);
        normalize(&mut audio, 0.89);
        let mut o = Out::new(
            base(&format!("arp-{preset}")),
            format!("{label} · Arp ({preset}) · {} · {} BPM", key_name, bpm as i32),
            "arp",
            preset,
            2,
            audio,
        );
        o.tags.push("loop".into());
        tag(&mut o, bars, true);
        outs.push(o);
    }

    // Vocal hook (about half the kits).
    if rng.chance(0.5) {
        let notes = patterns::hook(&key, &prog, bars, true, &mut rng);
        let spb = 60.0 / bpm;
        let lyric: Vec<(usize, usize)> = (0..notes.len())
            .map(|_| {
                let a = rng.int(0, 4) as usize;
                let b = if rng.chance(0.35) { rng.int(0, 4) as usize } else { a };
                (a, b)
            })
            .collect();
        let vn: Vec<vox::VoxNote> = notes
            .iter()
            .zip(lyric)
            .map(|(n, (a, b))| vox::VoxNote {
                start: n.start as f32 * spb,
                dur: n.len as f32 * spb,
                pitch: n.pitch as f32 - 12.0 * rng.int(0, 1) as f32,
                from: a,
                to: b,
                bend: if rng.chance(0.15) { -2.0 } else { 0.0 },
            })
            .collect();
        let total = beats as f32 * spb;
        // Render three passes; keep the second, crossfading its start from
        // the third so the loop wraps without a click.
        let mut thrice = Vec::new();
        for pass in 0..3 {
            thrice.extend(vn.iter().map(|v| vox::VoxNote { start: v.start + total * pass as f32, ..*v }));
        }
        let dry = vox::sing(&thrice, total * 3.0, &mut rng, 1.0);
        let wet = vox::space(&dry, bpm, 0.5, 0.25);
        let frames = (total * SR) as usize;
        let mut audio = wet[frames * 2..frames * 4].to_vec();
        let xf = (0.02 * SR) as usize;
        render::seam_crossfade(&mut audio, &wet[frames * 4..frames * 4 + xf * 2]);
        normalize(&mut audio, 0.89);
        let mut o = Out::new(base("vox-hook"), format!("{label} · Vox Hook · {} · {} BPM", key_name, bpm as i32), "vocal", "vox", 2, audio);
        o.tags.extend(["loop".to_string(), "hook".to_string()]);
        tag(&mut o, bars, true);
        outs.push(o);
    }
    outs
}

// ------------------------------------------------------------ extras

fn bass808_job(seed: u64) -> Vec<Out> {
    let mut outs = Vec::new();
    let names = ["clean", "dirty", "long", "distorted"];
    for timbre in 0..4 {
        for pc in 0..12 {
            let note = 24 + 12 + pc; // C2..B2 fundamentals sound right for 808s
            let mut rng = Rng::keyed(seed, &format!("808-{timbre}-{pc}"));
            let x = drumsynth::bass808(&mut rng, note as u8, timbre);
            let nn = NOTE_NAMES[pc as usize];
            let mut o = Out::new(
                format!("one-shots/808/808-{}-{}", names[timbre], nn.replace('#', "s")),
                format!("808 {} · {}", title(names[timbre]), nn),
                "drums",
                "808",
                1,
                x,
            );
            o.key = Some(nn.to_string());
            o.tags.extend(["one-shot".to_string(), "bass".to_string(), "tuned".to_string(), "trap".to_string()]);
            outs.push(o);
        }
    }
    outs
}

fn vox_chop_job(i: usize, seed: u64) -> Out {
    let mut rng = Rng::keyed(seed, &format!("vox-chop-{i}"));
    let pitch = rng.int(57, 76);
    let a = rng.int(0, 4) as usize;
    let b = if rng.chance(0.5) { rng.int(0, 4) as usize } else { a };
    let dur = rng.range(0.15, 0.7);
    let bend = if rng.chance(0.3) {
        -rng.range(2.0, 7.0)
    } else if rng.chance(0.1) {
        rng.range(1.0, 3.0)
    } else {
        0.0
    };
    let notes = [vox::VoxNote { start: 0.02, dur, pitch: pitch as f32, from: a, to: b, bend }];
    let shift = rng.range(0.9, 1.15);
    let dry = vox::sing(&notes, dur + 0.2, &mut rng, shift);
    let mut padded = dry;
    padded.extend(std::iter::repeat(0.0).take((SR * 1.2) as usize));
    let mut audio = trim_stereo(vox::space(&padded, 120.0, rng.range(0.2, 0.6), 0.0));
    normalize(&mut audio, 0.89);
    let nn = NOTE_NAMES[(pitch % 12) as usize];
    let syll = if a == b { vox::VOWELS[a].to_string() } else { format!("{}-{}", vox::VOWELS[a], vox::VOWELS[b]) };
    let mut o = Out::new(
        format!("one-shots/vox/vox-chop-{:03}-{}-{}", i + 1, syll, nn.replace('#', "s")),
        format!("Vox Chop {:03} · {} · {}", i + 1, syll, nn),
        "vocal",
        "vox",
        2,
        audio,
    );
    o.key = Some(nn.to_string());
    o.tags.extend(["one-shot".to_string(), "chop".to_string()]);
    o
}

fn fx_job(i: usize, seed: u64) -> Out {
    let mut rng = Rng::keyed(seed, &format!("fx-{i}"));
    let bpm = *rng.pick(&[90.0f32, 120.0, 124.0, 128.0, 140.0, 174.0]);
    let bar = 240.0 / bpm;
    let kinds = ["riser", "riser", "downlifter", "impact", "sweep-up", "sweep-down", "reverse-crash", "zap"];
    let kind = *rng.pick(&kinds);
    let bars = *rng.pick(&[1u32, 2, 4, 8]);
    let (mono, timed) = match kind {
        "riser" => (sfx::riser(&mut rng, bar * bars as f32), true),
        "downlifter" => (sfx::downlifter(&mut rng, bar * (bars.min(4)) as f32), true),
        "impact" => (sfx::impact(&mut rng), false),
        "sweep-up" => (sfx::sweep(&mut rng, bar * bars.min(4) as f32, true), true),
        "sweep-down" => (sfx::sweep(&mut rng, bar * bars.min(4) as f32, false), true),
        "reverse-crash" => (sfx::reverse_crash(&mut rng), false),
        _ => (sfx::zap(&mut rng), false),
    };
    let mut audio = trim_stereo(sfx::finish(mono, bpm, &mut rng));
    normalize(&mut audio, 0.89);
    let bars_eff = if kind == "riser" { bars } else { bars.min(4) };
    let mut o = Out::new(
        format!("fx/{}/{}-{:03}{}", kind, kind, i + 1, if timed { format!("-{}bars-{}bpm", bars_eff, bpm as i32) } else { String::new() }),
        format!("{} {:03}{}", title(kind), i + 1, if timed { format!(" · {} bars @ {} BPM", bars_eff, bpm as i32) } else { String::new() }),
        "fx",
        kind,
        2,
        audio,
    );
    o.tags.push("transition".into());
    if timed {
        o.bpm = Some(bpm);
    }
    o
}

fn synth_hit_job(i: usize, seed: u64) -> Out {
    let mut rng = Rng::keyed(seed, &format!("hit-{i}"));
    let kinds = [
        ("stab", "chord"),
        ("keys", "chord"),
        ("brass", "chord"),
        ("pad", "chord"),
        ("pluck", "note"),
        ("bell", "note"),
        ("pluck-bass", "bass"),
        ("supersaw", "chord"),
        ("lofi-keys", "chord"),
    ];
    let (preset, shape) = *rng.pick(&kinds);
    let tonic = rng.int(0, 11);
    let minor = rng.chance(0.6);
    let key = Key { tonic, mode: if minor { music::Mode::Minor } else { music::Mode::Major } };
    let pitches: Vec<i32> = match shape {
        "chord" => music::voice_lead(&music::chord(&key, 0, 3, rng.chance(0.5), false), None, 55, 79),
        "bass" => vec![36 + tonic],
        _ => vec![60 + tonic],
    };
    let len_beats = if preset == "pad" { 4.0 } else { rng.range(0.25, 1.0) as f64 };
    let mut s = Session::new(120.0, 0.0);
    s.synth(0, &music::preset(preset, &mut rng));
    s.param(0, clipdeck_engine::fx::track_param::SEND_A, rng.range(0.05, 0.3));
    let notes = pitches.iter().map(|&p| render::n(0.0, len_beats, p, 0.9)).collect();
    s.clip(0, len_beats.max(1.0) + 8.0, notes);
    let mut audio = trim_stereo(s.render_once(len_beats.max(1.0), 2.5));
    normalize(&mut audio, 0.89);
    let key_label = if shape == "chord" { key.name() } else { NOTE_NAMES[tonic as usize].to_string() };
    let mut o = Out::new(
        format!("one-shots/synth/{}-{:03}-{}", preset, i + 1, key_label.replace('#', "s")),
        format!("{} Hit {:03} · {}", title(preset), i + 1, key_label),
        "synth-hit",
        preset,
        2,
        audio,
    );
    o.key = Some(key_label);
    o.tags.extend(["one-shot".to_string(), shape.to_string()]);
    o
}

// ------------------------------------------------------------ main

enum Job {
    Kit(&'static Genre, usize),
    Bass808,
    VoxChop(usize),
    Fx(usize),
    Hit(usize),
}

fn main() {
    let mut out = PathBuf::from("library");
    let mut scale = 1.0f32;
    let mut seed = 1u64;
    let mut force = false;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out = PathBuf::from(&args[i + 1]);
                i += 1;
            }
            "--scale" => {
                scale = args[i + 1].parse().expect("--scale takes a number");
                i += 1;
            }
            "--seed" => {
                seed = args[i + 1].parse().expect("--seed takes an integer");
                i += 1;
            }
            "--force" => force = true,
            "-h" | "--help" => {
                println!("clipdeck-samplegen [--out DIR] [--scale F] [--seed N] [--force]\n  --scale 1.0 ≈ 2,800 samples (~1.5 GB); 0.1 for a quick test");
                return;
            }
            other => panic!("unknown argument {other}"),
        }
        i += 1;
    }

    let marker = out.join(".complete");
    let marker_content = format!("version={LIBRARY_VERSION} scale={scale} seed={seed}\n");
    if !force && fs::read_to_string(&marker).map(|s| s == marker_content).unwrap_or(false) {
        println!("library at {} is already complete ({})", out.display(), marker_content.trim());
        return;
    }
    fs::create_dir_all(&out).expect("create output dir");

    let kits = ((10.0 * scale).round() as usize).max(1);
    let mut jobs = Vec::new();
    for g in GENRES.iter() {
        for k in 0..kits {
            jobs.push(Job::Kit(g, k));
        }
    }
    jobs.push(Job::Bass808);
    for i in 0..((120.0 * scale) as usize).max(4) {
        jobs.push(Job::VoxChop(i));
    }
    for i in 0..((100.0 * scale) as usize).max(4) {
        jobs.push(Job::Fx(i));
    }
    for i in 0..((120.0 * scale) as usize).max(4) {
        jobs.push(Job::Hit(i));
    }

    let ctx = Ctx { out: out.clone(), written: AtomicUsize::new(0) };
    let t0 = Instant::now();
    let total_jobs = jobs.len();
    let done_jobs = AtomicUsize::new(0);
    let mut metas: Vec<Meta> = jobs
        .par_iter()
        .flat_map_iter(|job| {
            let outs = match job {
                Job::Kit(g, k) => kit_job(g, *k, seed),
                Job::Bass808 => bass808_job(seed),
                Job::VoxChop(i) => vec![vox_chop_job(*i, seed)],
                Job::Fx(i) => vec![fx_job(*i, seed)],
                Job::Hit(i) => vec![synth_hit_job(*i, seed)],
            };
            let metas: Vec<Meta> = outs.into_iter().map(|o| write(&ctx, o)).collect();
            let d = done_jobs.fetch_add(1, Ordering::Relaxed) + 1;
            if d % 10 == 0 || d == total_jobs {
                eprintln!("[{d}/{total_jobs}] {} samples, {:.0}s", ctx.written.load(Ordering::Relaxed), t0.elapsed().as_secs_f32());
            }
            metas
        })
        .collect();
    metas.sort_by(|a, b| a.id.cmp(&b.id));

    let mut manifest = String::new();
    for m in &metas {
        manifest.push_str(&serde_json::to_string(m).unwrap());
        manifest.push('\n');
    }
    write_atomic(&out.join("manifest.jsonl"), manifest.as_bytes());
    write_atomic(&marker, marker_content.as_bytes());
    let bytes: u64 = metas.iter().filter_map(|m| fs::metadata(out.join(&m.path)).ok()).map(|m| m.len()).sum();
    println!("wrote {} samples ({:.1} MB) to {} in {:.1}s", metas.len(), bytes as f64 / 1e6, out.display(), t0.elapsed().as_secs_f32());
}

fn write_atomic(path: &Path, data: &[u8]) {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).expect("write");
    fs::rename(&tmp, path).expect("rename");
}
