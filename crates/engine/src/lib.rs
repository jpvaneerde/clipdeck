//! Clipdeck audio engine.
//!
//! A session-view engine in the spirit of clip-launching DAWs: up to 16
//! tracks × 16 scenes of looping clips, launched in sync with a global
//! transport and quantized to the bar. Each track hosts one instrument
//! (poly synth, 16-pad drum rack, or an audio-loop player), runs through an
//! insert chain (filter, drive, compressor, sidechain duck), and feeds two
//! return buses (reverb, delay) and a limited master bus.
//!
//! The engine never allocates while processing audio once its buffers are
//! warm, has no dependencies, and is driven the same way in the browser
//! (through `clipdeck-engine-wasm` inside an AudioWorklet) and on the server
//! (offline export), so what you hear is what you export.

pub mod drums;
pub mod dsp;
pub mod fx;
pub mod sample;
pub mod synth;

use drums::DrumRack;
use dsp::Smoother;
use fx::{track_param as tp, Delay, Limiter, Reverb, TrackFx};
use sample::{Sample, SampleBank};
use synth::PolySynth;

pub const MAX_TRACKS: usize = 16;
pub const MAX_SCENES: usize = 16;
pub const MAX_BLOCK: usize = 256;

pub const STATE_HEADER: usize = 8;
pub const STATE_TRACK_STRIDE: usize = 8;
pub const STATE_LEN: usize = STATE_HEADER + MAX_TRACKS * STATE_TRACK_STRIDE;

pub mod kind {
    pub const NONE: u8 = 0;
    pub const SYNTH: u8 = 1;
    pub const DRUMS: u8 = 2;
    pub const AUDIO: u8 = 3;
}

pub mod master_param {
    pub const VOLUME: usize = 0;
    pub const SWING: usize = 1;
    pub const QUANTIZE: usize = 2;
    pub const METRONOME: usize = 3;
}

pub mod return_param {
    pub const REVERB_SIZE: usize = 0;
    pub const REVERB_DAMP: usize = 1;
    pub const REVERB_WIDTH: usize = 2;
    pub const REVERB_LEVEL: usize = 3;
    pub const DELAY_BEATS: usize = 4;
    pub const DELAY_FEEDBACK: usize = 5;
    pub const DELAY_TONE: usize = 6;
    pub const DELAY_PING_PONG: usize = 7;
    pub const DELAY_LEVEL: usize = 8;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Start in beats from the clip start.
    pub start: f64,
    /// Length in beats.
    pub len: f64,
    pub pitch: u8,
    /// 0..1
    pub vel: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioRef {
    pub sample: i32,
    /// How many beats the sample spans at its original tempo. The sample is
    /// re-pitched to fit the project tempo. 0 = play at native speed.
    pub sample_beats: f64,
    pub gain: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub length: f64,
    pub notes: Vec<Note>,
    pub audio: Option<AudioRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    Play(usize),
    Stop,
}

enum Instrument {
    None,
    Synth(Box<PolySynth>),
    Drums(Box<DrumRack>),
    Audio,
}

#[derive(Clone, Copy)]
enum EvKind {
    Launch,
    Note { pitch: u8, vel: f32, gate: u32 },
}

#[derive(Clone, Copy)]
struct Ev {
    offset: usize,
    kind: EvKind,
}

/// Short fade-out of an audio clip that was replaced or stopped (declick).
#[derive(Clone, Copy)]
struct Tail {
    sample: i32,
    pos: f64,
    rate: f64,
    gain: f32,
    fade: f32,
}

struct Ctx {
    b0: f64,
    bps: f64,
    bpm: f32,
    sr: f32,
    playing: bool,
    swing: f32,
}

pub struct Track {
    pub kind: u8,
    instrument: Instrument,
    pub clips: Vec<Option<Clip>>,
    pub playing: Option<usize>,
    clip_start: f64,
    pub queued: Option<Launch>,
    queued_at: f64,
    vol: Smoother,
    pan: Smoother,
    send_a: Smoother,
    send_b: Smoother,
    pub mute: bool,
    pub solo: bool,
    fx: TrackFx,
    duck_source: i32,
    duck_amount: f32,
    duck_release: f32,
    duck_gain: f32,
    sc_env: f32,
    sc_level: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    events: Vec<Ev>,
    peak_l: f32,
    peak_r: f32,
    audio_fade: f32,
    tail: Option<Tail>,
    params: [f32; tp::COUNT],
}

impl Track {
    fn new(sr: f32) -> Self {
        let mut params = [0.0; tp::COUNT];
        params[tp::VOLUME] = 0.8;
        params[tp::FILTER_CUTOFF] = 20000.0;
        params[tp::DUCK_SOURCE] = -1.0;
        params[tp::DUCK_RELEASE] = 180.0;
        params[tp::COMP_RATIO] = 1.0;
        Self {
            kind: kind::NONE,
            instrument: Instrument::None,
            clips: vec![None; MAX_SCENES],
            playing: None,
            clip_start: 0.0,
            queued: None,
            queued_at: 0.0,
            vol: Smoother::new(0.8, 0.02, sr),
            pan: Smoother::new(0.0, 0.02, sr),
            send_a: Smoother::new(0.0, 0.02, sr),
            send_b: Smoother::new(0.0, 0.02, sr),
            mute: false,
            solo: false,
            fx: TrackFx::new(sr),
            duck_source: -1,
            duck_amount: 0.0,
            duck_release: 180.0,
            duck_gain: 1.0,
            sc_env: 0.0,
            sc_level: 0.0,
            buf_l: vec![0.0; MAX_BLOCK],
            buf_r: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(512),
            peak_l: 0.0,
            peak_r: 0.0,
            audio_fade: 0.0,
            tail: None,
            params,
        }
    }

    fn set_kind(&mut self, k: u8, sr: f32) {
        if k == self.kind {
            return;
        }
        self.kind = k;
        self.instrument = match k {
            kind::SYNTH => Instrument::Synth(Box::new(PolySynth::new(sr))),
            kind::DRUMS => Instrument::Drums(Box::new(DrumRack::new(sr))),
            kind::AUDIO => Instrument::Audio,
            _ => Instrument::None,
        };
    }

    fn set_param(&mut self, id: usize, v: f32) {
        if id >= tp::COUNT || !v.is_finite() {
            return;
        }
        self.params[id] = v;
        match id {
            tp::VOLUME => self.vol.set(v.clamp(0.0, 2.0)),
            tp::PAN => self.pan.set(v.clamp(-1.0, 1.0)),
            tp::MUTE => self.mute = v >= 0.5,
            tp::SOLO => self.solo = v >= 0.5,
            tp::SEND_A => self.send_a.set(v.clamp(0.0, 1.0)),
            tp::SEND_B => self.send_b.set(v.clamp(0.0, 1.0)),
            tp::FILTER_TYPE => self.fx.filter_type = v.clamp(0.0, 3.0) as u8,
            tp::FILTER_CUTOFF => self.fx.cutoff.set(v.clamp(20.0, 20000.0)),
            tp::FILTER_RESO => self.fx.reso = v.clamp(0.0, 1.0),
            tp::DRIVE => self.fx.drive = v.clamp(0.0, 1.0),
            tp::COMP_THRESHOLD => self.fx.comp.threshold_db = v.clamp(-60.0, 0.0),
            tp::COMP_RATIO => self.fx.comp.ratio = v.clamp(1.0, 30.0),
            tp::COMP_ATTACK => self.fx.comp.attack_ms = v.clamp(0.1, 200.0),
            tp::COMP_RELEASE => self.fx.comp.release_ms = v.clamp(5.0, 2000.0),
            tp::COMP_MAKEUP => self.fx.comp.makeup_db = v.clamp(0.0, 24.0),
            tp::DUCK_SOURCE => self.duck_source = v.round() as i32,
            tp::DUCK_AMOUNT => self.duck_amount = v.clamp(0.0, 1.0),
            tp::DUCK_RELEASE => self.duck_release = v.clamp(10.0, 1000.0),
            _ => {}
        }
    }

    fn stop_sound(&mut self, ctx: &Ctx, bank: &SampleBank) {
        match &mut self.instrument {
            Instrument::Synth(s) => s.release_all(),
            Instrument::Drums(_) => {}
            Instrument::Audio => self.start_tail(ctx, bank),
            Instrument::None => {}
        }
    }

    fn audio_position(&self, beat: f64, ctx: &Ctx, bank: &SampleBank) -> Option<(i32, f64, f64, f32)> {
        let slot = self.playing?;
        let clip = self.clips.get(slot)?.as_ref()?;
        let audio = clip.audio?;
        let s = bank.get(audio.sample)?;
        let fpb = frames_per_beat(s, audio.sample_beats, ctx.bpm);
        let local = (beat - self.clip_start).rem_euclid(clip.length.max(0.001));
        Some((audio.sample, local * fpb, fpb * ctx.bps, audio.gain))
    }

    fn start_tail(&mut self, ctx: &Ctx, bank: &SampleBank) {
        if let Some((sample, pos, rate, gain)) = self.audio_position(ctx.b0, ctx, bank) {
            self.tail = Some(Tail { sample, pos, rate, gain: gain * self.audio_fade, fade: 1.0 });
        }
        self.audio_fade = 0.0;
    }

    fn apply_launch(&mut self, launch: Launch, at_beat: f64, ctx: &Ctx, bank: &SampleBank) {
        let had_clip = self.playing.is_some();
        match launch {
            Launch::Play(slot) => {
                if self.clips.get(slot).map_or(true, |c| c.is_none()) {
                    if had_clip {
                        self.stop_sound(ctx, bank);
                    }
                    self.playing = None;
                } else {
                    if had_clip {
                        self.stop_sound(ctx, bank);
                    }
                    self.playing = Some(slot);
                    self.clip_start = at_beat;
                    self.audio_fade = 0.0;
                }
            }
            Launch::Stop => {
                if had_clip {
                    self.stop_sound(ctx, bank);
                }
                self.playing = None;
            }
        }
        self.queued = None;
    }

    /// Push note events for `clip` in absolute beat range [x0, x1),
    /// which corresponds to sample offsets starting at `offset0`.
    fn collect_notes(events: &mut Vec<Ev>, clip: &Clip, clip_start: f64, x0: f64, x1: f64, offset0: usize, ctx: &Ctx) {
        let len = clip.length;
        if len <= 0.0 || x1 <= x0 || clip.notes.is_empty() {
            return;
        }
        let l0 = (x0 - clip_start).rem_euclid(len);
        let span = x1 - x0;
        // Window(s) in clip-local time: [l0, l0+span) possibly wrapping.
        let mut windows = [(l0, (l0 + span).min(len), 0.0f64), (0.0, 0.0, 0.0)];
        if l0 + span > len {
            windows[1] = (0.0, l0 + span - len, len - l0);
        }
        for &(w0, w1, beat_offset) in windows.iter() {
            if w1 <= w0 {
                continue;
            }
            for n in clip.notes.iter() {
                let mut start = n.start;
                if ctx.swing > 0.0 {
                    let s16 = start * 4.0;
                    let r = s16.round();
                    if (s16 - r).abs() < 0.01 && (r as i64).rem_euclid(2) == 1 {
                        start += ctx.swing as f64 / 12.0;
                    }
                }
                if start >= w0 && start < w1 {
                    let beats_in = (start - w0) + beat_offset;
                    let offset = offset0 + (beats_in / ctx.bps) as usize;
                    let gate = ((n.len.max(0.01)) / ctx.bps) as u32;
                    if events.len() < events.capacity() {
                        events.push(Ev { offset, kind: EvKind::Note { pitch: n.pitch, vel: n.vel, gate } });
                    }
                }
            }
        }
    }

    fn render_range(&mut self, from: usize, to: usize, ctx: &Ctx, bank: &SampleBank) {
        if to <= from {
            return;
        }
        match &mut self.instrument {
            Instrument::Synth(s) => s.render(&mut self.buf_l[from..to], &mut self.buf_r[from..to]),
            Instrument::Drums(d) => d.render(&mut self.buf_l[from..to], &mut self.buf_r[from..to], bank),
            Instrument::Audio => {
                let fade_step = 1.0 / (0.004 * ctx.sr);
                if ctx.playing {
                    if let Some(slot) = self.playing {
                        if let Some(Some(clip)) = self.clips.get(slot) {
                            if let Some(audio) = clip.audio {
                                if let Some(s) = bank.get(audio.sample) {
                                    let fpb = frames_per_beat(s, audio.sample_beats, ctx.bpm);
                                    let len = clip.length.max(0.001);
                                    for i in from..to {
                                        let beat = ctx.b0 + i as f64 * ctx.bps;
                                        let local = (beat - self.clip_start).rem_euclid(len);
                                        self.audio_fade = (self.audio_fade + fade_step).min(1.0);
                                        let (a, b) = s.read(local * fpb);
                                        let g = audio.gain * self.audio_fade;
                                        self.buf_l[i] += a * g;
                                        self.buf_r[i] += b * g;
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(t) = &mut self.tail {
                    if let Some(s) = bank.get(t.sample) {
                        for i in from..to {
                            t.fade -= fade_step;
                            if t.fade <= 0.0 {
                                break;
                            }
                            let (a, b) = s.read(t.pos);
                            let g = t.gain * t.fade;
                            self.buf_l[i] += a * g;
                            self.buf_r[i] += b * g;
                            t.pos += t.rate;
                        }
                    }
                    if t.fade <= 0.0 {
                        self.tail = None;
                    }
                }
            }
            Instrument::None => {}
        }
    }

    fn process(&mut self, n: usize, ctx: &Ctx, bank: &SampleBank, duck_src_level: f32) {
        self.buf_l[..n].fill(0.0);
        self.buf_r[..n].fill(0.0);

        let mut events = core::mem::take(&mut self.events);
        events.clear();
        let b1 = ctx.b0 + n as f64 * ctx.bps;
        let mut launch_at = n;
        if ctx.playing {
            if let Some(_) = self.queued {
                if self.queued_at < b1 {
                    let off = ((self.queued_at - ctx.b0) / ctx.bps).max(0.0) as usize;
                    launch_at = off.min(n - 1);
                    events.push(Ev { offset: launch_at, kind: EvKind::Launch });
                }
            }
            // Notes from the currently playing clip until the launch point.
            if let Some(slot) = self.playing {
                if let Some(Some(clip)) = self.clips.get(slot) {
                    let x1 = ctx.b0 + launch_at as f64 * ctx.bps;
                    Self::collect_notes(&mut events, clip, self.clip_start, ctx.b0, x1, 0, ctx);
                }
            }
            // Notes from the newly launched clip after the launch point.
            if launch_at < n {
                if let Some(Launch::Play(slot)) = self.queued {
                    if let Some(Some(clip)) = self.clips.get(slot) {
                        let x0 = self.queued_at.max(ctx.b0);
                        Self::collect_notes(&mut events, clip, self.queued_at, x0, b1, launch_at, ctx);
                    }
                }
            }
        }
        // Launch first at equal offsets so the new clip's first note survives.
        events.sort_by_key(|e| (e.offset, matches!(e.kind, EvKind::Note { .. }) as u8));

        let mut pos = 0;
        for ev in events.iter() {
            let off = ev.offset.min(n);
            if off > pos {
                self.render_range(pos, off, ctx, bank);
                pos = off;
            }
            match ev.kind {
                EvKind::Launch => {
                    if let Some(l) = self.queued {
                        let at = self.queued_at;
                        let sub = Ctx { b0: ctx.b0 + off as f64 * ctx.bps, ..*ctx };
                        self.apply_launch(l, at, &sub, bank);
                    }
                }
                EvKind::Note { pitch, vel, gate } => match &mut self.instrument {
                    Instrument::Synth(s) => s.note_on(pitch, vel, gate),
                    Instrument::Drums(d) => d.trigger(pitch, vel, bank),
                    _ => {}
                },
            }
        }
        self.render_range(pos, n, ctx, bank);
        self.events = events;

        // Inserts.
        self.fx.process(&mut self.buf_l[..n], &mut self.buf_r[..n], ctx.sr);

        // Sidechain envelope this track exposes to others.
        let sc_rel = dsp::one_pole_coef(0.05, ctx.sr);
        let mut sc_peak: f32 = 0.0;
        for i in 0..n {
            let x = self.buf_l[i].abs().max(self.buf_r[i].abs());
            if x > self.sc_env {
                self.sc_env = x;
            } else {
                self.sc_env -= self.sc_env * sc_rel;
            }
            sc_peak = sc_peak.max(self.sc_env);
        }
        self.sc_level = sc_peak;

        // Ducking from another track.
        if self.duck_source >= 0 && self.duck_amount > 0.0 {
            let target = 1.0 - self.duck_amount * (duck_src_level * 1.6).min(1.0);
            let att = dsp::one_pole_coef(0.003, ctx.sr);
            let rel = dsp::one_pole_coef(self.duck_release * 0.001, ctx.sr);
            for i in 0..n {
                let c = if target < self.duck_gain { att } else { rel };
                self.duck_gain += (target - self.duck_gain) * c;
                self.buf_l[i] *= self.duck_gain;
                self.buf_r[i] *= self.duck_gain;
            }
        } else {
            self.duck_gain = 1.0;
        }
    }
}

#[inline]
fn frames_per_beat(s: &Sample, sample_beats: f64, bpm: f32) -> f64 {
    if sample_beats > 0.0 {
        s.frames as f64 / sample_beats
    } else {
        s.sample_rate as f64 * 60.0 / bpm.max(1.0) as f64
    }
}

struct Click {
    phase: f32,
    freq: f32,
    env: f32,
}

pub struct Engine {
    sr: f32,
    bpm: f32,
    beat: f64,
    playing: bool,
    pub quantize: f64,
    swing: f32,
    metronome: bool,
    master_vol: Smoother,
    last_scene: usize,
    pub tracks: Vec<Track>,
    pub bank: SampleBank,
    reverb: Reverb,
    delay: Delay,
    reverb_level: f32,
    delay_level: f32,
    limiter: Limiter,
    click: Click,
    preview: Option<(i32, f64, f64, f32)>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    ra_l: Vec<f32>,
    ra_r: Vec<f32>,
    rb_l: Vec<f32>,
    rb_r: Vec<f32>,
    peak_l: f32,
    peak_r: f32,
    state: Vec<f32>,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.clamp(8000.0, 192000.0);
        Self {
            sr,
            bpm: 120.0,
            beat: 0.0,
            playing: false,
            quantize: 4.0,
            swing: 0.0,
            metronome: false,
            master_vol: Smoother::new(0.85, 0.02, sr),
            last_scene: 0,
            tracks: (0..MAX_TRACKS).map(|_| Track::new(sr)).collect(),
            bank: SampleBank::default(),
            reverb: Reverb::new(sr),
            delay: Delay::new(sr),
            reverb_level: 0.8,
            delay_level: 0.7,
            limiter: Limiter::new(sr),
            click: Click { phase: 0.0, freq: 1000.0, env: 0.0 },
            preview: None,
            mix_l: vec![0.0; MAX_BLOCK],
            mix_r: vec![0.0; MAX_BLOCK],
            ra_l: vec![0.0; MAX_BLOCK],
            ra_r: vec![0.0; MAX_BLOCK],
            rb_l: vec![0.0; MAX_BLOCK],
            rb_r: vec![0.0; MAX_BLOCK],
            peak_l: 0.0,
            peak_r: 0.0,
            state: vec![0.0; STATE_LEN],
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }
    pub fn bpm(&self) -> f32 {
        self.bpm
    }
    pub fn beat(&self) -> f64 {
        self.beat
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            b0: self.beat,
            bps: self.bpm as f64 / 60.0 / self.sr as f64,
            bpm: self.bpm,
            sr: self.sr,
            playing: self.playing,
            swing: self.swing,
        }
    }

    // ------------------------------------------------------------ transport

    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() {
            self.bpm = bpm.clamp(20.0, 999.0);
        }
    }

    /// Start the transport. If nothing is playing or queued, relaunch the
    /// last launched scene.
    pub fn play(&mut self) {
        if self.playing {
            return;
        }
        let any = self.tracks.iter().any(|t| t.playing.is_some() || t.queued.is_some());
        self.playing = true;
        self.beat = 0.0;
        if !any {
            self.launch_scene(self.last_scene);
        } else {
            let ctx = self.ctx();
            for t in self.tracks.iter_mut() {
                if t.queued.is_some() {
                    let l = t.queued.unwrap();
                    t.apply_launch(l, 0.0, &ctx, &self.bank);
                } else if t.playing.is_some() {
                    t.clip_start = 0.0;
                }
            }
        }
    }

    pub fn stop(&mut self) {
        let ctx = self.ctx();
        for t in self.tracks.iter_mut() {
            if t.playing.is_some() {
                t.stop_sound(&ctx, &self.bank);
            }
            if let Instrument::Synth(s) = &mut t.instrument {
                s.release_all();
            }
            t.playing = None;
            t.queued = None;
        }
        self.playing = false;
        self.beat = 0.0;
    }

    fn launch_beat(&self) -> f64 {
        if !self.playing {
            return 0.0;
        }
        let q = self.quantize;
        if q <= 0.0 {
            self.beat
        } else {
            ((self.beat / q) - 1e-9).ceil() * q
        }
    }

    fn queue(&mut self, track: usize, launch: Launch) {
        let at = self.launch_beat();
        if let Some(t) = self.tracks.get_mut(track) {
            t.queued = Some(launch);
            t.queued_at = at;
        }
    }

    /// Launch the clip in `slot` on `track` (quantized). Launching an empty
    /// slot stops the track, like in a session grid.
    pub fn launch_clip(&mut self, track: usize, slot: usize) {
        if track >= MAX_TRACKS || slot >= MAX_SCENES {
            return;
        }
        self.queue(track, Launch::Play(slot));
        if !self.playing {
            self.start_from_queue();
        }
    }

    pub fn stop_track(&mut self, track: usize) {
        if !self.playing {
            if let Some(t) = self.tracks.get_mut(track) {
                t.queued = None;
                t.playing = None;
            }
            return;
        }
        self.queue(track, Launch::Stop);
    }

    pub fn launch_scene(&mut self, slot: usize) {
        if slot >= MAX_SCENES {
            return;
        }
        self.last_scene = slot;
        for t in 0..MAX_TRACKS {
            let has = self.tracks[t].clips[slot].is_some();
            if has {
                self.queue(t, Launch::Play(slot));
            } else if self.tracks[t].playing.is_some() {
                self.queue(t, Launch::Stop);
            } else {
                self.tracks[t].queued = None;
            }
        }
        if !self.playing {
            self.start_from_queue();
        }
    }

    fn start_from_queue(&mut self) {
        self.playing = true;
        self.beat = 0.0;
        let ctx = self.ctx();
        for t in self.tracks.iter_mut() {
            if let Some(l) = t.queued {
                t.apply_launch(l, 0.0, &ctx, &self.bank);
            }
        }
    }

    pub fn stop_all_clips(&mut self) {
        for t in 0..MAX_TRACKS {
            if self.tracks[t].playing.is_some() {
                self.queue(t, Launch::Stop);
            }
        }
    }

    // ------------------------------------------------------------ setup

    pub fn set_track_kind(&mut self, track: usize, k: u8) {
        let sr = self.sr;
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_kind(k, sr);
        }
    }

    pub fn set_track_param(&mut self, track: usize, id: usize, value: f32) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.set_param(id, value);
        }
    }

    pub fn set_synth_param(&mut self, track: usize, id: usize, value: f32) {
        if let Some(Instrument::Synth(s)) = self.tracks.get_mut(track).map(|t| &mut t.instrument) {
            s.set_param(id, value);
        }
    }

    pub fn set_pad_param(&mut self, track: usize, pad: usize, id: usize, value: f32) {
        if let Some(Instrument::Drums(d)) = self.tracks.get_mut(track).map(|t| &mut t.instrument) {
            d.set_pad_param(pad, id, value);
        }
    }

    pub fn set_clip(&mut self, track: usize, slot: usize, clip: Option<Clip>) {
        if let Some(t) = self.tracks.get_mut(track) {
            if slot < MAX_SCENES {
                let removing = clip.is_none();
                t.clips[slot] = clip;
                if removing && t.playing == Some(slot) {
                    if let Instrument::Synth(s) = &mut t.instrument {
                        s.release_all();
                    }
                    t.playing = None;
                }
            }
        }
    }

    pub fn set_master_param(&mut self, id: usize, v: f32) {
        if !v.is_finite() {
            return;
        }
        match id {
            master_param::VOLUME => self.master_vol.set(v.clamp(0.0, 2.0)),
            master_param::SWING => self.swing = v.clamp(0.0, 1.0),
            master_param::QUANTIZE => self.quantize = v.clamp(0.0, 16.0) as f64,
            master_param::METRONOME => self.metronome = v >= 0.5,
            _ => {}
        }
    }

    pub fn set_return_param(&mut self, id: usize, v: f32) {
        if !v.is_finite() {
            return;
        }
        match id {
            return_param::REVERB_SIZE => self.reverb.size = v,
            return_param::REVERB_DAMP => self.reverb.damp = v,
            return_param::REVERB_WIDTH => self.reverb.width = v.clamp(0.0, 1.0),
            return_param::REVERB_LEVEL => self.reverb_level = v.clamp(0.0, 2.0),
            return_param::DELAY_BEATS => self.delay.beats = v,
            return_param::DELAY_FEEDBACK => self.delay.feedback = v,
            return_param::DELAY_TONE => self.delay.tone = v,
            return_param::DELAY_PING_PONG => self.delay.ping_pong = v >= 0.5,
            return_param::DELAY_LEVEL => self.delay_level = v.clamp(0.0, 2.0),
            _ => {}
        }
    }

    pub fn load_sample(&mut self, handle: u32, sample: Sample) {
        self.bank.insert(handle, sample);
    }

    pub fn free_sample(&mut self, handle: u32) {
        self.bank.remove(handle);
    }

    // ------------------------------------------------------------ live play

    pub fn note_on(&mut self, track: usize, pitch: u8, vel: f32) {
        let Some(t) = self.tracks.get_mut(track) else { return };
        match &mut t.instrument {
            Instrument::Synth(s) => s.note_on(pitch, vel, synth::GATE_HELD),
            Instrument::Drums(d) => d.trigger(pitch, vel, &self.bank),
            _ => {}
        }
    }

    pub fn note_off(&mut self, track: usize, pitch: u8) {
        if let Some(Instrument::Synth(s)) = self.tracks.get_mut(track).map(|t| &mut t.instrument) {
            s.note_off(pitch);
        }
    }

    pub fn preview(&mut self, handle: i32, gain: f32) {
        if let Some(s) = self.bank.get(handle) {
            let rate = (s.sample_rate / self.sr) as f64;
            self.preview = Some((handle, 0.0, rate, gain));
        }
    }

    pub fn stop_preview(&mut self) {
        self.preview = None;
    }

    // ------------------------------------------------------------ processing

    /// Render `out_l.len()` frames of audio.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        let mut done = 0;
        while done < n {
            let len = (n - done).min(MAX_BLOCK);
            self.process_block(len);
            out_l[done..done + len].copy_from_slice(&self.mix_l[..len]);
            out_r[done..done + len].copy_from_slice(&self.mix_r[..len]);
            done += len;
        }
        self.write_state();
    }

    fn process_block(&mut self, n: usize) {
        let ctx = self.ctx();
        for b in [&mut self.mix_l, &mut self.mix_r, &mut self.ra_l, &mut self.ra_r, &mut self.rb_l, &mut self.rb_r] {
            b[..n].fill(0.0);
        }
        let any_solo = self.tracks.iter().any(|t| t.solo && t.kind != kind::NONE);
        let peak_decay = (-(n as f32) / (self.sr * 0.35)).exp();

        for i in 0..MAX_TRACKS {
            if self.tracks[i].kind == kind::NONE {
                continue;
            }
            let src = self.tracks[i].duck_source;
            let src_level =
                if src >= 0 && (src as usize) < MAX_TRACKS && src as usize != i { self.tracks[src as usize].sc_level } else { 0.0 };
            let t = &mut self.tracks[i];
            t.process(n, &ctx, &self.bank, src_level);

            let audible = !t.mute && (!any_solo || t.solo);
            let mut pl: f32 = 0.0;
            let mut pr: f32 = 0.0;
            // Pan law evaluated at block edges and interpolated (trig per sample is costly).
            let (gl0, gr0) = dsp::pan_gains(t.pan.value);
            for _ in 0..n {
                t.pan.next();
            }
            let (gl1, gr1) = dsp::pan_gains(t.pan.value);
            let inv_n = 1.0 / n as f32;
            for j in 0..n {
                let vol = t.vol.next();
                let f = j as f32 * inv_n;
                let gl = gl0 + (gl1 - gl0) * f;
                let gr = gr0 + (gr1 - gr0) * f;
                let sa = t.send_a.next();
                let sb = t.send_b.next();
                let l = t.buf_l[j] * vol * gl * core::f32::consts::SQRT_2;
                let r = t.buf_r[j] * vol * gr * core::f32::consts::SQRT_2;
                pl = pl.max(l.abs());
                pr = pr.max(r.abs());
                if audible {
                    self.mix_l[j] += l;
                    self.mix_r[j] += r;
                    if sa > 0.0 {
                        self.ra_l[j] += l * sa;
                        self.ra_r[j] += r * sa;
                    }
                    if sb > 0.0 {
                        self.rb_l[j] += l * sb;
                        self.rb_r[j] += r * sb;
                    }
                }
            }
            if !audible {
                pl = 0.0;
                pr = 0.0;
            }
            t.peak_l = pl.max(t.peak_l * peak_decay);
            t.peak_r = pr.max(t.peak_r * peak_decay);
        }

        // Return buses.
        self.reverb.process(&self.ra_l[..n], &self.ra_r[..n], &mut self.mix_l[..n], &mut self.mix_r[..n], self.reverb_level);
        self.delay.process(&self.rb_l[..n], &self.rb_r[..n], &mut self.mix_l[..n], &mut self.mix_r[..n], self.bpm, self.delay_level);

        // Metronome.
        if self.playing && self.metronome {
            let b1 = ctx.b0 + n as f64 * ctx.bps;
            let next = ctx.b0.ceil();
            if next < b1 || ctx.b0 == next {
                let off = (((next - ctx.b0) / ctx.bps) as usize).min(n - 1);
                let down = (next as i64).rem_euclid(4) == 0;
                self.click = Click { phase: 0.0, freq: if down { 1600.0 } else { 1000.0 }, env: 0.0 };
                self.click.env = 1.0;
                self.render_click(off, n);
            } else {
                self.render_click(0, n);
            }
        }

        // Master.
        for j in 0..n {
            let v = self.master_vol.next();
            self.mix_l[j] *= v;
            self.mix_r[j] *= v;
        }
        if let Some((h, mut pos, rate, gain)) = self.preview {
            match self.bank.get(h) {
                Some(s) => {
                    for j in 0..n {
                        let (a, b) = s.read(pos);
                        self.mix_l[j] += a * gain;
                        self.mix_r[j] += b * gain;
                        pos += rate;
                    }
                    self.preview = if pos < s.frames as f64 { Some((h, pos, rate, gain)) } else { None };
                }
                None => self.preview = None,
            }
        }
        self.limiter.process(&mut self.mix_l[..n], &mut self.mix_r[..n]);

        let mut pl: f32 = 0.0;
        let mut pr: f32 = 0.0;
        for j in 0..n {
            pl = pl.max(self.mix_l[j].abs());
            pr = pr.max(self.mix_r[j].abs());
        }
        self.peak_l = pl.max(self.peak_l * peak_decay);
        self.peak_r = pr.max(self.peak_r * peak_decay);

        if self.playing {
            self.beat += n as f64 * ctx.bps;
        }
    }

    fn render_click(&mut self, from: usize, n: usize) {
        if self.click.env <= 0.0005 {
            return;
        }
        let inc = self.click.freq / self.sr;
        let decay = (-1.0 / (0.03 * self.sr)).exp();
        for j in from..n {
            let s = (dsp::TAU * self.click.phase).sin() * self.click.env * 0.35;
            self.mix_l[j] += s;
            self.mix_r[j] += s;
            self.click.phase = dsp::wrap_phase(self.click.phase + inc);
            self.click.env *= decay;
        }
    }

    fn write_state(&mut self) {
        let s = &mut self.state;
        s[0] = self.beat as f32;
        s[1] = if self.playing { 1.0 } else { 0.0 };
        s[2] = self.bpm;
        s[3] = self.peak_l;
        s[4] = self.peak_r;
        s[5] = if self.preview.is_some() { 1.0 } else { 0.0 };
        s[6] = self.limiter.gain_reduction_db;
        s[7] = self.last_scene as f32;
        for (i, t) in self.tracks.iter().enumerate() {
            let o = STATE_HEADER + i * STATE_TRACK_STRIDE;
            s[o] = t.playing.map_or(-1.0, |p| p as f32);
            s[o + 1] = match t.queued {
                Some(Launch::Play(p)) => p as f32,
                Some(Launch::Stop) => -2.0,
                None => -1.0,
            };
            let (local, len) = match t.playing.and_then(|p| t.clips[p].as_ref()) {
                Some(c) if self.playing => ((self.beat - t.clip_start).rem_euclid(c.length.max(0.001)), c.length),
                _ => (0.0, 0.0),
            };
            s[o + 2] = local as f32;
            s[o + 3] = len as f32;
            s[o + 4] = t.peak_l;
            s[o + 5] = t.peak_r;
            s[o + 6] = t.fx.comp.gain_reduction_db;
            s[o + 7] = t.duck_gain;
        }
    }

    /// Snapshot for the UI: see `STATE_*` constants for the layout.
    pub fn state(&self) -> &[f32] {
        &self.state
    }

    /// Convenience for offline rendering.
    pub fn render(&mut self, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; frames];
        let mut r = vec![0.0; frames];
        self.process(&mut l, &mut r);
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn click_sample(sr: f32) -> Sample {
        let n = (sr * 0.1) as usize;
        let data = (0..n).map(|i| (1.0 - i as f32 / n as f32) * if i % 40 < 20 { 0.8 } else { -0.8 }).collect();
        Sample::new(data, 1, sr)
    }

    #[test]
    fn synth_clip_plays_in_time() {
        let sr = 48000.0;
        let mut e = Engine::new(sr);
        e.set_bpm(120.0);
        e.set_track_kind(0, kind::SYNTH);
        e.set_clip(0, 0, Some(Clip { length: 4.0, notes: vec![Note { start: 1.0, len: 0.5, pitch: 60, vel: 1.0 }], audio: None }));
        e.launch_scene(0);
        // Beat 1 at 120 bpm = 0.5 s. First half second should be silent.
        let (l, _) = e.render((sr * 0.49) as usize);
        assert!(rms(&l) < 1e-4, "early sound: {}", rms(&l));
        let (l, _) = e.render((sr * 0.3) as usize);
        assert!(rms(&l) > 0.01);
        assert!((e.beat() - 1.58).abs() < 0.01);
    }

    #[test]
    fn drum_loop_and_quantized_launch() {
        let sr = 48000.0;
        let mut e = Engine::new(sr);
        e.load_sample(1, click_sample(sr));
        e.set_track_kind(0, kind::DRUMS);
        e.set_pad_param(0, 0, drums::param::SAMPLE, 1.0);
        let notes = (0..4).map(|b| Note { start: b as f64, len: 0.25, pitch: 36, vel: 1.0 }).collect();
        e.set_clip(0, 0, Some(Clip { length: 4.0, notes, audio: None }));
        e.set_clip(0, 1, Some(Clip { length: 4.0, notes: vec![], audio: None }));
        e.launch_clip(0, 0);
        assert!(e.is_playing());
        let _ = e.render((sr * 0.6) as usize); // beat 1.2
        e.launch_clip(0, 1);
        assert_eq!(e.tracks[0].queued, Some(Launch::Play(1)));
        let _ = e.render((sr * 1.0) as usize); // beat 3.2: still old clip
        assert_eq!(e.tracks[0].playing, Some(0));
        let _ = e.render((sr * 0.5) as usize); // beat 4.2: switched at bar line
        assert_eq!(e.tracks[0].playing, Some(1));
        assert!(e.tracks[0].queued.is_none());
    }

    #[test]
    fn audio_clip_is_tempo_synced() {
        let sr = 48000.0;
        let mut e = Engine::new(sr);
        // A 2-beat "loop" recorded at 100 bpm = 1.2 s.
        let frames = (sr * 1.2) as usize;
        let data: Vec<f32> = (0..frames).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        e.load_sample(3, Sample::new(data, 1, sr));
        e.set_bpm(120.0);
        e.set_track_kind(2, kind::AUDIO);
        e.set_clip(2, 0, Some(Clip { length: 2.0, notes: vec![], audio: Some(AudioRef { sample: 3, sample_beats: 2.0, gain: 1.0 }) }));
        e.launch_scene(0);
        let (l, _) = e.render((sr * 2.0) as usize);
        assert!(rms(&l) > 0.05);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }

    #[test]
    fn stop_resets_and_state_reports() {
        let sr = 44100.0;
        let mut e = Engine::new(sr);
        e.set_track_kind(1, kind::SYNTH);
        e.set_clip(1, 2, Some(Clip { length: 2.0, notes: vec![Note { start: 0.0, len: 1.0, pitch: 48, vel: 0.8 }], audio: None }));
        e.launch_scene(2);
        let _ = e.render(4410);
        let st = e.state();
        assert_eq!(st[1], 1.0);
        assert_eq!(st[STATE_HEADER + STATE_TRACK_STRIDE], 2.0);
        e.stop();
        let _ = e.render(128);
        assert_eq!(e.state()[1], 0.0);
        assert!(e.tracks[1].playing.is_none());
    }

    #[test]
    fn full_mix_stays_bounded() {
        let sr = 48000.0;
        let mut e = Engine::new(sr);
        e.load_sample(1, click_sample(sr));
        for t in 0..8 {
            e.set_track_kind(t, if t % 2 == 0 { kind::SYNTH } else { kind::DRUMS });
            e.set_track_param(t, tp::VOLUME, 2.0);
            e.set_track_param(t, tp::SEND_A, 0.5);
            e.set_track_param(t, tp::SEND_B, 0.5);
            e.set_pad_param(t, 0, drums::param::SAMPLE, 1.0);
            let notes = (0..16)
                .map(|i| Note { start: i as f64 * 0.25, len: 0.25, pitch: if t % 2 == 0 { 40 + i as u8 } else { 36 }, vel: 1.0 })
                .collect();
            e.set_clip(t, 0, Some(Clip { length: 4.0, notes, audio: None }));
        }
        e.set_track_param(1, tp::DUCK_SOURCE, 3.0);
        e.set_track_param(1, tp::DUCK_AMOUNT, 0.8);
        e.set_track_param(2, tp::COMP_THRESHOLD, -20.0);
        e.set_track_param(2, tp::COMP_RATIO, 4.0);
        e.set_track_param(4, tp::FILTER_TYPE, 1.0);
        e.set_track_param(4, tp::FILTER_CUTOFF, 800.0);
        e.launch_scene(0);
        let (l, r) = e.render(sr as usize * 4);
        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() <= 1.0));
        assert!(rms(&l) > 0.05);
    }
}
