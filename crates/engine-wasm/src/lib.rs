//! Plain C-ABI wrapper around the engine for use inside an AudioWorklet.
//!
//! AudioWorkletGlobalScope has no TextDecoder and wasm-bindgen glue is
//! awkward there, so this module exports only numeric functions and owns a
//! single global engine. The module imports nothing.
//!
//! Memory protocol: the host calls `cd_alloc(n)` to get room for `n` f32
//! values in linear memory, writes into it, passes the pointer to a setter,
//! then calls `cd_free`. Setters copy what they need.

#![allow(clippy::missing_safety_doc)]

use clipdeck_engine::sample::Sample;
use clipdeck_engine::{AudioRef, Clip, Engine, Note, STATE_LEN};

struct Host {
    engine: Engine,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
}

static mut HOST: Option<Host> = None;

#[inline]
fn host() -> Option<&'static mut Host> {
    // SAFETY: the AudioWorklet is single-threaded; all calls come from it.
    unsafe { (*core::ptr::addr_of_mut!(HOST)).as_mut() }
}

macro_rules! with_engine {
    ($e:ident => $body:expr) => {
        if let Some(h) = host() {
            let $e = &mut h.engine;
            $body
        }
    };
}

#[no_mangle]
pub extern "C" fn cd_init(sample_rate: f32) {
    let host = Host { engine: Engine::new(sample_rate), out_l: vec![0.0; 4096], out_r: vec![0.0; 4096] };
    unsafe {
        *core::ptr::addr_of_mut!(HOST) = Some(host);
    }
}

#[no_mangle]
pub extern "C" fn cd_alloc(len: u32) -> *mut f32 {
    let mut v: Vec<f32> = vec![0.0; len.max(1) as usize];
    let p = v.as_mut_ptr();
    core::mem::forget(v);
    p
}

#[no_mangle]
pub unsafe extern "C" fn cd_free(ptr: *mut f32, len: u32) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, len.max(1) as usize, len.max(1) as usize));
    }
}

/// Render `frames` (≤ 4096) into the output buffers.
#[no_mangle]
pub extern "C" fn cd_process(frames: u32) {
    if let Some(h) = host() {
        let n = (frames as usize).min(h.out_l.len());
        let (l, r) = (&mut h.out_l[..n], &mut h.out_r[..n]);
        h.engine.process(l, r);
    }
}

#[no_mangle]
pub extern "C" fn cd_out_l() -> *const f32 {
    host().map_or(core::ptr::null(), |h| h.out_l.as_ptr())
}

#[no_mangle]
pub extern "C" fn cd_out_r() -> *const f32 {
    host().map_or(core::ptr::null(), |h| h.out_r.as_ptr())
}

#[no_mangle]
pub extern "C" fn cd_state() -> *const f32 {
    host().map_or(core::ptr::null(), |h| h.engine.state().as_ptr())
}

#[no_mangle]
pub extern "C" fn cd_state_len() -> u32 {
    STATE_LEN as u32
}

// ------------------------------------------------------------ transport

#[no_mangle]
pub extern "C" fn cd_set_bpm(bpm: f32) {
    with_engine!(e => e.set_bpm(bpm));
}

#[no_mangle]
pub extern "C" fn cd_play() {
    with_engine!(e => e.play());
}

#[no_mangle]
pub extern "C" fn cd_stop() {
    with_engine!(e => e.stop());
}

#[no_mangle]
pub extern "C" fn cd_launch_clip(track: u32, slot: u32) {
    with_engine!(e => e.launch_clip(track as usize, slot as usize));
}

#[no_mangle]
pub extern "C" fn cd_stop_track(track: u32) {
    with_engine!(e => e.stop_track(track as usize));
}

#[no_mangle]
pub extern "C" fn cd_launch_scene(slot: u32) {
    with_engine!(e => e.launch_scene(slot as usize));
}

#[no_mangle]
pub extern "C" fn cd_stop_all_clips() {
    with_engine!(e => e.stop_all_clips());
}

// ------------------------------------------------------------ setup

#[no_mangle]
pub extern "C" fn cd_set_track_kind(track: u32, kind: u32) {
    with_engine!(e => e.set_track_kind(track as usize, kind as u8));
}

#[no_mangle]
pub extern "C" fn cd_set_track_param(track: u32, id: u32, value: f32) {
    with_engine!(e => e.set_track_param(track as usize, id as usize, value));
}

#[no_mangle]
pub extern "C" fn cd_set_synth_param(track: u32, id: u32, value: f32) {
    with_engine!(e => e.set_synth_param(track as usize, id as usize, value));
}

#[no_mangle]
pub extern "C" fn cd_set_pad_param(track: u32, pad: u32, id: u32, value: f32) {
    with_engine!(e => e.set_pad_param(track as usize, pad as usize, id as usize, value));
}

#[no_mangle]
pub extern "C" fn cd_set_master_param(id: u32, value: f32) {
    with_engine!(e => e.set_master_param(id as usize, value));
}

#[no_mangle]
pub extern "C" fn cd_set_return_param(id: u32, value: f32) {
    with_engine!(e => e.set_return_param(id as usize, value));
}

/// `ptr` holds `count` notes as [start, len, pitch, velocity] f32 quads.
#[no_mangle]
pub unsafe extern "C" fn cd_set_clip_notes(track: u32, slot: u32, length: f64, ptr: *const f32, count: u32) {
    let raw: &[f32] = if ptr.is_null() || count == 0 { &[] } else { core::slice::from_raw_parts(ptr, count as usize * 4) };
    let notes = raw
        .chunks_exact(4)
        .map(|q| Note { start: q[0] as f64, len: q[1] as f64, pitch: q[2].clamp(0.0, 127.0) as u8, vel: q[3] })
        .collect();
    with_engine!(e => e.set_clip(track as usize, slot as usize, Some(Clip { length: length.max(0.25), notes, audio: None })));
}

#[no_mangle]
pub extern "C" fn cd_set_clip_audio(track: u32, slot: u32, length: f64, sample: i32, sample_beats: f64, gain: f32) {
    let clip = Clip { length: length.max(0.25), notes: Vec::new(), audio: Some(AudioRef { sample, sample_beats, gain }) };
    with_engine!(e => e.set_clip(track as usize, slot as usize, Some(clip)));
}

#[no_mangle]
pub extern "C" fn cd_clear_clip(track: u32, slot: u32) {
    with_engine!(e => e.set_clip(track as usize, slot as usize, None));
}

/// Copy `frames × channels` interleaved samples from `ptr` into handle `handle`.
#[no_mangle]
pub unsafe extern "C" fn cd_load_sample(handle: u32, ptr: *const f32, frames: u32, channels: u32, sample_rate: f32) {
    if ptr.is_null() {
        return;
    }
    let ch = channels.clamp(1, 2) as usize;
    let data = core::slice::from_raw_parts(ptr, frames as usize * ch).to_vec();
    with_engine!(e => e.load_sample(handle, Sample::new(data, ch, sample_rate)));
}

#[no_mangle]
pub extern "C" fn cd_free_sample(handle: u32) {
    with_engine!(e => e.free_sample(handle));
}

// ------------------------------------------------------------ live input

#[no_mangle]
pub extern "C" fn cd_note_on(track: u32, pitch: u32, velocity: f32) {
    with_engine!(e => e.note_on(track as usize, pitch.min(127) as u8, velocity));
}

#[no_mangle]
pub extern "C" fn cd_note_off(track: u32, pitch: u32) {
    with_engine!(e => e.note_off(track as usize, pitch.min(127) as u8));
}

#[no_mangle]
pub extern "C" fn cd_preview(handle: i32, gain: f32) {
    with_engine!(e => e.preview(handle, gain));
}

#[no_mangle]
pub extern "C" fn cd_stop_preview() {
    with_engine!(e => e.stop_preview());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the C ABI exactly the way the AudioWorklet does.
    #[test]
    fn abi_round_trip() {
        cd_init(48000.0);
        cd_set_bpm(124.0);
        cd_set_track_kind(0, 1);
        let notes = [0.0f32, 0.5, 48.0, 0.9, 1.0, 0.5, 55.0, 0.9];
        let p = cd_alloc(8);
        unsafe {
            core::ptr::copy_nonoverlapping(notes.as_ptr(), p, 8);
            cd_set_clip_notes(0, 0, 4.0, p, 2);
            cd_free(p, 8);
        }
        let n = 48000;
        let sp = cd_alloc(n);
        unsafe {
            for i in 0..n as usize {
                *sp.add(i) = ((i as f32) * 0.01).sin();
            }
            cd_load_sample(7, sp, n, 1, 48000.0);
            cd_free(sp, n);
        }
        cd_set_track_kind(1, 3);
        cd_set_clip_audio(1, 0, 4.0, 7, 0.0, 0.5);
        cd_launch_scene(0);
        let mut energy = 0.0f32;
        for _ in 0..400 {
            cd_process(128);
            let l = unsafe { core::slice::from_raw_parts(cd_out_l(), 128) };
            energy += l.iter().map(|x| x * x).sum::<f32>();
        }
        assert!(energy > 1.0);
        let st = unsafe { core::slice::from_raw_parts(cd_state(), cd_state_len() as usize) };
        assert_eq!(st[1], 1.0);
        assert!(st[0] > 2.0);
    }
}
