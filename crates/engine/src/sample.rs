//! Sample storage and interpolated reading.

use crate::dsp::hermite;

/// Audio data held by the engine. Mono or stereo, interleaved.
#[derive(Clone, Debug)]
pub struct Sample {
    pub data: Vec<f32>,
    pub channels: usize,
    pub frames: usize,
    pub sample_rate: f32,
}

impl Sample {
    pub fn new(data: Vec<f32>, channels: usize, sample_rate: f32) -> Self {
        let channels = channels.clamp(1, 2);
        let frames = data.len() / channels;
        Self { data, channels, frames, sample_rate }
    }

    #[inline]
    fn frame(&self, i: isize) -> (f32, f32) {
        if i < 0 || i as usize >= self.frames {
            return (0.0, 0.0);
        }
        let i = i as usize;
        if self.channels == 1 {
            let v = self.data[i];
            (v, v)
        } else {
            (self.data[i * 2], self.data[i * 2 + 1])
        }
    }

    /// Cubic-interpolated stereo read at fractional frame position `pos`.
    /// Positions outside the sample return silence.
    #[inline]
    pub fn read(&self, pos: f64) -> (f32, f32) {
        if pos < -1.0 || pos >= self.frames as f64 {
            return (0.0, 0.0);
        }
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;
        let (am, bm) = self.frame(i - 1);
        let (a0, b0) = self.frame(i);
        let (a1, b1) = self.frame(i + 1);
        let (a2, b2) = self.frame(i + 2);
        (hermite(am, a0, a1, a2, t), hermite(bm, b0, b1, b2, t))
    }
}

/// Samples addressed by small integer handles the host assigns.
#[derive(Default)]
pub struct SampleBank {
    slots: Vec<Option<Sample>>,
}

impl SampleBank {
    pub const MAX: usize = 4096;

    pub fn insert(&mut self, handle: u32, sample: Sample) {
        let h = handle as usize;
        if h >= Self::MAX {
            return;
        }
        if self.slots.len() <= h {
            self.slots.resize_with(h + 1, || None);
        }
        self.slots[h] = Some(sample);
    }

    pub fn remove(&mut self, handle: u32) {
        if let Some(slot) = self.slots.get_mut(handle as usize) {
            *slot = None;
        }
    }

    #[inline]
    pub fn get(&self, handle: i32) -> Option<&Sample> {
        if handle < 0 {
            return None;
        }
        self.slots.get(handle as usize).and_then(|s| s.as_ref())
    }
}
