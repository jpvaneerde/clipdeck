//! The Clipdeck project format (JSON). `web/src/state/types.ts` mirrors it.
//!
//! `apply` pushes a project into an engine the same way the browser does,
//! which is what makes server-side export sound like the live session.

use clipdeck_engine::{
    drums::param as pad_param, fx::track_param as tp, kind, master_param, return_param, synth, AudioRef, Clip, Engine, Note, MAX_SCENES,
    MAX_TRACKS,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub scenes: Vec<Scene>,
    pub tracks: Vec<Track>,
    pub returns: Returns,
    pub master: Master,
    /// Scene chain used for export: play scene `scene` for `bars` bars, in order.
    pub arrangement: Vec<Section>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled".into(),
            bpm: 120.0,
            swing: 0.0,
            scenes: Vec::new(),
            tracks: Vec::new(),
            returns: Returns::default(),
            master: Master::default(),
            arrangement: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Scene {
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Section {
    pub scene: usize,
    pub bars: u32,
}

impl Default for Section {
    fn default() -> Self {
        Self { scene: 0, bars: 8 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    #[default]
    Synth,
    Drums,
    Audio,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub color: String,
    pub kind: TrackKind,
    pub mixer: Mixer,
    pub fx: TrackFx,
    /// Synth parameters indexed by `clipdeck_engine::synth::param`.
    pub synth: Vec<f32>,
    pub preset: Option<String>,
    pub pads: Vec<Pad>,
    pub clips: Vec<Option<ClipData>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Mixer {
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub send_a: f32,
    pub send_b: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self { volume: 0.8, pan: 0.0, mute: false, solo: false, send_a: 0.0, send_b: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackFx {
    pub filter_type: u8,
    pub cutoff: f32,
    pub reso: f32,
    pub drive: f32,
    pub comp_threshold: f32,
    pub comp_ratio: f32,
    pub comp_attack: f32,
    pub comp_release: f32,
    pub comp_makeup: f32,
    pub duck_source: i32,
    pub duck_amount: f32,
    pub duck_release: f32,
}

impl Default for TrackFx {
    fn default() -> Self {
        Self {
            filter_type: 0,
            cutoff: 20000.0,
            reso: 0.1,
            drive: 0.0,
            comp_threshold: 0.0,
            comp_ratio: 1.0,
            comp_attack: 10.0,
            comp_release: 120.0,
            comp_makeup: 0.0,
            duck_source: -1,
            duck_amount: 0.0,
            duck_release: 180.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Pad {
    pub sample: Option<String>,
    pub name: String,
    pub gain: f32,
    pub pitch: f32,
    pub pan: f32,
    pub decay: f32,
    pub choke: u8,
}

impl Default for Pad {
    fn default() -> Self {
        Self { sample: None, name: String::new(), gain: 1.0, pitch: 0.0, pan: 0.0, decay: 0.0, choke: 0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipData {
    pub name: String,
    pub color: Option<String>,
    /// Loop length in beats.
    pub length: f64,
    pub notes: Vec<NoteData>,
    pub audio: Option<AudioData>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct NoteData {
    /// start (beats)
    pub s: f64,
    /// length (beats)
    pub l: f64,
    /// MIDI pitch
    pub p: u8,
    /// velocity 0..1
    pub v: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioData {
    pub sample: String,
    /// Beats the sample spans at its own tempo; 0 = unsynced one-shot.
    pub beats: f64,
    pub gain: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Returns {
    pub reverb_size: f32,
    pub reverb_damp: f32,
    pub reverb_width: f32,
    pub reverb_level: f32,
    pub delay_beats: f32,
    pub delay_feedback: f32,
    pub delay_tone: f32,
    pub delay_ping_pong: bool,
    pub delay_level: f32,
}

impl Default for Returns {
    fn default() -> Self {
        Self {
            reverb_size: 0.75,
            reverb_damp: 0.4,
            reverb_width: 1.0,
            reverb_level: 0.8,
            delay_beats: 0.75,
            delay_feedback: 0.4,
            delay_tone: 4500.0,
            delay_ping_pong: true,
            delay_level: 0.7,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Master {
    pub volume: f32,
    pub quantize: f32,
}

impl Default for Master {
    fn default() -> Self {
        Self { volume: 0.85, quantize: 4.0 }
    }
}

impl Project {
    /// Every sample id the project references.
    pub fn sample_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for t in &self.tracks {
            for p in &t.pads {
                if let Some(s) = &p.sample {
                    ids.push(s.clone());
                }
            }
            for c in t.clips.iter().flatten() {
                if let Some(a) = &c.audio {
                    ids.push(a.sample.clone());
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }

    /// Total length of the arrangement in beats (4/4).
    pub fn arrangement_beats(&self) -> f64 {
        self.arrangement.iter().map(|s| s.bars as f64 * 4.0).sum()
    }
}

/// Push `project` into `engine`. `handle` maps a sample id to the handle it
/// was loaded under (or None if unavailable).
pub fn apply(engine: &mut Engine, project: &Project, handle: &dyn Fn(&str) -> Option<u32>) {
    engine.set_bpm(project.bpm);
    engine.set_master_param(master_param::SWING, project.swing);
    engine.set_master_param(master_param::VOLUME, project.master.volume);
    engine.set_master_param(master_param::QUANTIZE, project.master.quantize);
    let r = &project.returns;
    for (id, v) in [
        (return_param::REVERB_SIZE, r.reverb_size),
        (return_param::REVERB_DAMP, r.reverb_damp),
        (return_param::REVERB_WIDTH, r.reverb_width),
        (return_param::REVERB_LEVEL, r.reverb_level),
        (return_param::DELAY_BEATS, r.delay_beats),
        (return_param::DELAY_FEEDBACK, r.delay_feedback),
        (return_param::DELAY_TONE, r.delay_tone),
        (return_param::DELAY_PING_PONG, if r.delay_ping_pong { 1.0 } else { 0.0 }),
        (return_param::DELAY_LEVEL, r.delay_level),
    ] {
        engine.set_return_param(id, v);
    }

    for (ti, t) in project.tracks.iter().enumerate().take(MAX_TRACKS) {
        let k = match t.kind {
            TrackKind::Synth => kind::SYNTH,
            TrackKind::Drums => kind::DRUMS,
            TrackKind::Audio => kind::AUDIO,
        };
        engine.set_track_kind(ti, k);
        let m = &t.mixer;
        let f = &t.fx;
        for (id, v) in [
            (tp::VOLUME, m.volume),
            (tp::PAN, m.pan),
            (tp::MUTE, m.mute as u8 as f32),
            (tp::SOLO, m.solo as u8 as f32),
            (tp::SEND_A, m.send_a),
            (tp::SEND_B, m.send_b),
            (tp::FILTER_TYPE, f.filter_type as f32),
            (tp::FILTER_CUTOFF, f.cutoff),
            (tp::FILTER_RESO, f.reso),
            (tp::DRIVE, f.drive),
            (tp::COMP_THRESHOLD, f.comp_threshold),
            (tp::COMP_RATIO, f.comp_ratio),
            (tp::COMP_ATTACK, f.comp_attack),
            (tp::COMP_RELEASE, f.comp_release),
            (tp::COMP_MAKEUP, f.comp_makeup),
            (tp::DUCK_SOURCE, f.duck_source as f32),
            (tp::DUCK_AMOUNT, f.duck_amount),
            (tp::DUCK_RELEASE, f.duck_release),
        ] {
            engine.set_track_param(ti, id, v);
        }
        if t.kind == TrackKind::Synth {
            for (id, v) in t.synth.iter().enumerate().take(synth::param::COUNT) {
                engine.set_synth_param(ti, id, *v);
            }
        }
        if t.kind == TrackKind::Drums {
            for (pi, p) in t.pads.iter().enumerate().take(16) {
                let h = p.sample.as_deref().and_then(handle).map_or(-1.0, |h| h as f32);
                engine.set_pad_param(ti, pi, pad_param::SAMPLE, h);
                engine.set_pad_param(ti, pi, pad_param::GAIN, p.gain);
                engine.set_pad_param(ti, pi, pad_param::PITCH, p.pitch);
                engine.set_pad_param(ti, pi, pad_param::PAN, p.pan);
                engine.set_pad_param(ti, pi, pad_param::DECAY, p.decay);
                engine.set_pad_param(ti, pi, pad_param::CHOKE, p.choke as f32);
            }
        }
        for slot in 0..MAX_SCENES {
            let clip = t.clips.get(slot).cloned().flatten().map(|c| Clip {
                length: c.length.max(0.25),
                notes: c.notes.iter().map(|n| Note { start: n.s, len: n.l, pitch: n.p, vel: n.v }).collect(),
                audio: c
                    .audio
                    .as_ref()
                    .and_then(|a| handle(&a.sample).map(|h| AudioRef { sample: h as i32, sample_beats: a.beats, gain: a.gain })),
            });
            engine.set_clip(ti, slot, clip);
        }
    }
}

/// Render the project's arrangement (or `fallback_bars` of scene 0 when it
/// has none) and return interleaved stereo. Adds a short tail for reverb.
pub fn render_arrangement(engine: &mut Engine, project: &Project, fallback_bars: u32, tail_seconds: f32) -> Vec<f32> {
    let sections: Vec<Section> =
        if project.arrangement.is_empty() { vec![Section { scene: 0, bars: fallback_bars.max(1) }] } else { project.arrangement.clone() };
    let sr = engine.sample_rate();
    let bpm = project.bpm.max(20.0);
    let frames_per_beat = sr as f64 * 60.0 / bpm as f64;
    engine.set_master_param(master_param::QUANTIZE, 4.0);
    engine.stop();

    let mut out: Vec<f32> = Vec::new();
    let mut l = vec![0.0f32; 256];
    let mut r = vec![0.0f32; 256];
    let mut beat_cursor = 0.0f64;
    let mut frames_done: usize = 0;
    let total_beats: f64 = sections.iter().map(|s| s.bars as f64 * 4.0).sum();
    let total_frames = (total_beats * frames_per_beat).round() as usize;

    let mut push = |engine: &mut Engine, n: usize, out: &mut Vec<f32>| {
        engine.process(&mut l[..n], &mut r[..n]);
        for i in 0..n {
            out.push(l[i]);
            out.push(r[i]);
        }
    };

    for (i, sec) in sections.iter().enumerate() {
        if i == 0 {
            engine.launch_scene(sec.scene.min(MAX_SCENES - 1));
        }
        let end_beat = beat_cursor + sec.bars as f64 * 4.0;
        let end_frame = (end_beat * frames_per_beat).round() as usize;
        // Queue the next scene during the last bar so it lands on the bar line.
        let queue_frame = ((end_beat - 2.0).max(beat_cursor) * frames_per_beat).round() as usize;
        while frames_done < end_frame {
            let n = (end_frame - frames_done).min(256);
            if frames_done < queue_frame && frames_done + n > queue_frame {
                let n1 = queue_frame - frames_done;
                push(engine, n1, &mut out);
                frames_done += n1;
                continue;
            }
            if frames_done == queue_frame {
                match sections.get(i + 1) {
                    Some(next) => engine.launch_scene(next.scene.min(MAX_SCENES - 1)),
                    None => engine.stop_all_clips(),
                }
            }
            push(engine, n, &mut out);
            frames_done += n;
        }
        beat_cursor = end_beat;
    }
    debug_assert_eq!(frames_done, total_frames);
    // Tail: let reverb and releases ring out.
    let tail = (tail_seconds.max(0.0) * sr) as usize;
    let mut t = 0;
    while t < tail {
        let n = (tail - t).min(256);
        push(engine, n, &mut out);
        t += n;
    }
    engine.stop();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_project_with_defaults() {
        let p: Project = serde_json::from_str(
            r#"{"name":"x","bpm":128,"tracks":[{"kind":"drums","clips":[null,{"length":4,"notes":[{"s":0,"l":0.25,"p":36,"v":1}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(p.tracks[0].kind, TrackKind::Drums);
        assert_eq!(p.tracks[0].mixer.volume, 0.8);
        assert!(p.tracks[0].clips[0].is_none());
    }

    #[test]
    fn renders_arrangement_length() {
        let mut p = Project { bpm: 120.0, ..Default::default() };
        let mut t = Track { kind: TrackKind::Synth, ..Default::default() };
        t.synth = synth::DEFAULTS.to_vec();
        t.clips = vec![
            Some(ClipData { length: 4.0, notes: vec![NoteData { s: 0.0, l: 1.0, p: 60, v: 1.0 }], ..Default::default() }),
            Some(ClipData { length: 4.0, notes: vec![NoteData { s: 0.0, l: 1.0, p: 67, v: 1.0 }], ..Default::default() }),
        ];
        p.tracks.push(t);
        p.arrangement = vec![Section { scene: 0, bars: 2 }, Section { scene: 1, bars: 1 }];
        let mut e = Engine::new(48000.0);
        apply(&mut e, &p, &|_| None);
        let out = render_arrangement(&mut e, &p, 4, 0.0);
        // 3 bars at 120 bpm = 6 s.
        assert_eq!(out.len(), 48000 * 6 * 2);
        let energy: f32 = out.iter().map(|x| x * x).sum();
        assert!(energy > 10.0);
    }
}
