//! Decoding, waveform peaks, and metadata guessed from file names.

use anyhow::{anyhow, Context, Result};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct Decoded {
    /// Interleaved samples, at most 2 channels.
    pub data: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1)
    }
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.sample_rate.max(1) as f32
    }
}

/// Decode WAV quickly with hound; everything else through symphonia.
pub fn decode(path: &Path) -> Result<Decoded> {
    let is_wav = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if is_wav {
        if let Ok(d) = decode_wav(path) {
            return Ok(d);
        }
    }
    decode_any(path)
}

fn decode_wav(path: &Path) -> Result<Decoded> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(downmix(raw, ch, spec.sample_rate))
}

fn decode_any(path: &Path) -> Result<Decoded> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut sr = track.codec_params.sample_rate.unwrap_or(44100);
    let mut ch = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let mut out = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        sr = spec.rate;
        ch = spec.channels.count();
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        out.extend_from_slice(buf.samples());
    }
    if out.is_empty() {
        return Err(anyhow!("no audio decoded"));
    }
    Ok(downmix(out, ch, sr))
}

/// Keep mono and stereo as-is; fold more channels down to stereo.
fn downmix(raw: Vec<f32>, ch: usize, sr: u32) -> Decoded {
    if ch <= 2 {
        return Decoded { data: raw, channels: ch.max(1), sample_rate: sr };
    }
    let frames = raw.len() / ch;
    let mut data = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        let fr = &raw[f * ch..(f + 1) * ch];
        let l: f32 = fr.iter().step_by(2).sum::<f32>() / ch.div_ceil(2) as f32;
        let r: f32 = fr.iter().skip(1).step_by(2).sum::<f32>() / (ch / 2) as f32;
        data.push(l);
        data.push(r);
    }
    Decoded { data, channels: 2, sample_rate: sr }
}

/// `n` peak values (0..255) for waveform thumbnails.
pub fn peaks(d: &Decoded, n: usize) -> Vec<u8> {
    let frames = d.frames();
    if frames == 0 {
        return vec![0; n];
    }
    let ch = d.channels;
    let max = d.data.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    (0..n)
        .map(|i| {
            let a = i * frames / n;
            let b = ((i + 1) * frames / n).max(a + 1).min(frames);
            let mut p = 0.0f32;
            for f in a..b {
                for c in 0..ch {
                    p = p.max(d.data[f * ch + c].abs());
                }
            }
            ((p / max).sqrt() * 255.0).round() as u8
        })
        .collect()
}

pub const KEYS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Guess tempo, key and type from a file name such as
/// "Deep_Chords_124bpm_Am.wav" or "kick 03.wav".
pub struct Guess {
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub category: &'static str,
    pub instrument: String,
}

pub fn guess_from_name(name: &str) -> Guess {
    let lower = name.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphanumeric() && c != '#').filter(|t| !t.is_empty()).collect();

    let mut bpm = None;
    for (i, t) in tokens.iter().enumerate() {
        if let Some(num) = t.strip_suffix("bpm") {
            if let Ok(v) = num.parse::<f32>() {
                bpm = Some(v);
            } else if num.is_empty() && i > 0 {
                bpm = tokens[i - 1].parse::<f32>().ok();
            }
        }
    }
    if bpm.is_none() {
        // A bare number between 60 and 200 is very likely a tempo.
        bpm = tokens.iter().filter_map(|t| t.parse::<f32>().ok()).find(|v| (60.0..=200.0).contains(v) && v.fract() == 0.0);
    }

    let mut key = None;
    let orig_tokens: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric() && c != '#').filter(|t| !t.is_empty()).collect();
    for t in orig_tokens.iter().rev() {
        if let Some(k) = parse_key(t) {
            key = Some(k);
            break;
        }
    }

    let has = |w: &str| tokens.iter().any(|t| t.contains(w));
    let is_loop = has("loop") || bpm.is_some();
    let (category, instrument): (&'static str, &str) = if has("kick") || has("bd") {
        (if is_loop && has("loop") { "drum-loop" } else { "drums" }, "kick")
    } else if has("snare") {
        ("drums", "snare")
    } else if has("clap") {
        ("drums", "clap")
    } else if has("hat") || has("hh") {
        ("drums", if has("open") || has("oh") { "hat-open" } else { "hat-closed" })
    } else if has("808") {
        ("drums", "808")
    } else if has("perc") || has("shaker") || has("conga") || has("rim") {
        (if has("loop") { "drum-loop" } else { "drums" }, "perc")
    } else if has("drum") || has("beat") || has("break") || has("top") {
        ("drum-loop", "drums")
    } else if has("bass") || has("sub") {
        ("bass", "bass")
    } else if has("chord") || has("pad") || has("keys") || has("piano") || has("stab") {
        ("chords", if has("pad") { "pad" } else { "keys" })
    } else if has("vox") || has("vocal") || has("voice") || has("acapella") {
        ("vocal", "vox")
    } else if has("fx") || has("riser") || has("impact") || has("sweep") || has("uplifter") || has("downlifter") {
        ("fx", "fx")
    } else if has("arp") {
        ("arp", "arp")
    } else if has("lead") || has("melody") || has("hook") || has("synth") || has("pluck") {
        ("hook", "synth")
    } else if is_loop {
        ("hook", "loop")
    } else {
        ("synth-hit", "one-shot")
    };
    Guess { bpm, key, category, instrument: instrument.to_string() }
}

fn parse_key(t: &str) -> Option<String> {
    // Accept C, C#, Db, Cm, C#min, Ebmaj, Am, F#m, "Amin".
    let mut chars = t.chars();
    let first = chars.next()?.to_ascii_uppercase();
    if !('A'..='G').contains(&first) {
        return None;
    }
    let rest: String = chars.collect();
    let (acc, rest) = if let Some(r) = rest.strip_prefix('#') {
        (1, r.to_string())
    } else if rest.starts_with('b') && rest.len() <= 4 && !rest.starts_with("bass") {
        (-1, rest[1..].to_string())
    } else {
        (0, rest)
    };
    let minor = match rest.to_lowercase().as_str() {
        "" | "maj" | "major" => false,
        "m" | "min" | "minor" => true,
        _ => return None,
    };
    // A bare single letter is too ambiguous unless it's an explicit major marker.
    if t.len() == 1 {
        return None;
    }
    let base = KEYS.iter().position(|k| k.starts_with(first) && k.len() == 1)? as i32;
    let pc = (base + acc).rem_euclid(12) as usize;
    Some(format!("{}{}", KEYS[pc], if minor { "m" } else { "" }))
}

/// Keys that mix well with `key`: itself and its relative major/minor.
pub fn compatible_keys(key: &str) -> Vec<String> {
    let minor = key.ends_with('m');
    let root = key.trim_end_matches('m');
    let Some(pc) = KEYS.iter().position(|k| *k == root) else { return vec![key.to_string()] };
    let rel = if minor { format!("{}", KEYS[(pc + 3) % 12]) } else { format!("{}m", KEYS[(pc + 9) % 12]) };
    vec![key.to_string(), rel]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_names() {
        let g = guess_from_name("Deep_Chords_124bpm_Am.wav");
        assert_eq!(g.bpm, Some(124.0));
        assert_eq!(g.key.as_deref(), Some("Am"));
        assert_eq!(g.category, "chords");
        let g = guess_from_name("Kick 03.wav");
        assert_eq!(g.category, "drums");
        assert_eq!(g.instrument, "kick");
        let g = guess_from_name("bassline_F#min_128.wav");
        assert_eq!(g.key.as_deref(), Some("F#m"));
        assert_eq!(g.bpm, Some(128.0));
        let g = guess_from_name("Lead Ebmaj 140 BPM.flac");
        assert_eq!(g.key.as_deref(), Some("D#"));
        assert_eq!(g.bpm, Some(140.0));
    }

    #[test]
    fn relative_keys() {
        assert_eq!(compatible_keys("Am"), vec!["Am", "C"]);
        assert_eq!(compatible_keys("D#"), vec!["D#", "Cm"]);
    }
}
