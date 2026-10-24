//! Offline export: the same engine the browser runs, rendered faster than real time.

use crate::audio;
use crate::db::Db;
use anyhow::{bail, Result};
use clipdeck_engine::{sample::Sample, Engine};
use clipdeck_model::{apply, render_arrangement, Project};
use std::collections::HashMap;
use std::io::Cursor;

pub struct RenderOptions {
    pub sample_rate: u32,
    pub bits: u16,
    pub fallback_bars: u32,
    pub tail_seconds: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { sample_rate: 48000, bits: 24, fallback_bars: 8, tail_seconds: 2.0 }
    }
}

pub fn render_wav(db: &Db, project: &Project, opts: &RenderOptions) -> Result<Vec<u8>> {
    let beats = if project.arrangement.is_empty() { opts.fallback_bars as f64 * 4.0 } else { project.arrangement_beats() };
    let seconds = beats * 60.0 / project.bpm.max(20.0) as f64;
    if seconds > 30.0 * 60.0 {
        bail!("arrangement is longer than 30 minutes");
    }
    let sr = opts.sample_rate as f32;
    let mut engine = Engine::new(sr);
    let mut handles: HashMap<String, u32> = HashMap::new();
    for (i, id) in project.sample_ids().into_iter().enumerate() {
        let Some(path) = db.file_path(&id)? else {
            tracing::warn!(%id, "sample referenced by project is missing");
            continue;
        };
        match audio::decode(&path) {
            Ok(d) => {
                let h = i as u32 + 1;
                engine.load_sample(h, Sample::new(d.data, d.channels, d.sample_rate as f32));
                handles.insert(id, h);
            }
            Err(e) => tracing::warn!(%id, error = %e, "could not decode sample"),
        }
    }
    apply(&mut engine, project, &|id| handles.get(id).copied());
    let audio = render_arrangement(&mut engine, project, opts.fallback_bars, opts.tail_seconds);

    let spec =
        hound::WavSpec { channels: 2, sample_rate: opts.sample_rate, bits_per_sample: opts.bits, sample_format: hound::SampleFormat::Int };
    let mut cur = Cursor::new(Vec::with_capacity(audio.len() * opts.bits as usize / 8 + 64));
    {
        let mut w = hound::WavWriter::new(&mut cur, spec)?;
        let max = ((1i64 << (opts.bits - 1)) - 1) as f32;
        for s in audio {
            w.write_sample((s.clamp(-1.0, 1.0) * max).round() as i32)?;
        }
        w.finalize()?;
    }
    Ok(cur.into_inner())
}
