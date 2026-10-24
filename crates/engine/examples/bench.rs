//! Rough real-time factor for a busy 12-track session.
use clipdeck_engine::{drums, fx::track_param as tp, kind, sample::Sample, synth::param as sp, Clip, Engine, Note};
use std::time::Instant;

fn main() {
    let sr = 48000.0;
    let mut e = Engine::new(sr);
    let hit: Vec<f32> = (0..24000).map(|i| ((i as f32) * 0.07).sin() * (1.0 - i as f32 / 24000.0)).collect();
    e.load_sample(1, Sample::new(hit, 1, sr));
    for t in 0..12 {
        let k = [kind::SYNTH, kind::DRUMS, kind::AUDIO][t % 3];
        e.set_track_kind(t, k);
        e.set_track_param(t, tp::SEND_A, 0.3);
        e.set_track_param(t, tp::SEND_B, 0.2);
        e.set_track_param(t, tp::FILTER_TYPE, 1.0);
        e.set_track_param(t, tp::COMP_THRESHOLD, -18.0);
        e.set_track_param(t, tp::COMP_RATIO, 4.0);
        e.set_synth_param(t, sp::AMP_R, 1.5);
        for p in 0..16 {
            e.set_pad_param(t, p, drums::param::SAMPLE, 1.0);
        }
        let notes = (0..32)
            .map(|i| Note {
                start: i as f64 * 0.25,
                len: 1.0,
                pitch: if k == kind::DRUMS { 36 + (i % 16) as u8 } else { 48 + (i * 7 % 24) as u8 },
                vel: 0.9,
            })
            .collect();
        e.set_clip(
            t,
            0,
            Some(Clip {
                length: 8.0,
                notes,
                audio: if k == kind::AUDIO { Some(clipdeck_engine::AudioRef { sample: 1, sample_beats: 2.0, gain: 1.0 }) } else { None },
            }),
        );
    }
    e.launch_scene(0);
    let secs: f64 = 30.0;
    let frames = (sr as f64 * secs) as usize;
    let mut l = vec![0.0f32; 128];
    let mut r = vec![0.0f32; 128];
    let t0 = Instant::now();
    for _ in 0..frames / 128 {
        e.process(&mut l, &mut r);
    }
    let el = t0.elapsed().as_secs_f64();
    println!("rendered {secs}s in {el:.3}s -> {:.0}x real time", secs / el);
}
