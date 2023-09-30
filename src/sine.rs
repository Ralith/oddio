use core::f32::consts::TAU;

use crate::{math::Float, Sample, Seek, Signal};

/// A trivial [`Signal`] that produces a sine wave of a particular frequency, forever
pub struct Sine {
    phase: f32,
    /// Radians per second
    frequency: f32,
}

impl Sine {
    /// Construct a sine wave that begins at `phase` radians and cycles `frequency_hz` times per
    /// second
    ///
    /// `phase` doesn't impact the sound of a single sine wave, but multiple concurrent sound waves
    /// may produce weird interference effects if their phases align.
    pub fn new(phase: f32, frequency_hz: f32) -> Self {
        Self {
            phase,
            frequency: frequency_hz * TAU,
        }
    }

    fn seek_to(&mut self, t: f32) {
        // Advance time, but wrap for numerical stability no matter how long we play for
        self.phase = (self.phase + t * self.frequency) % TAU;
    }
}

impl Signal for Sine {
    type Frame = Sample;

    fn sample(&mut self, interval: f32, out: &mut [Sample]) {
        for (i, x) in out.iter_mut().enumerate() {
            let t = interval * i as f32;
            *x = (t * self.frequency + self.phase).sin();
        }
        self.seek_to(interval * out.len() as f32);
    }
}

impl Seek for Sine {
    fn seek(&mut self, seconds: f32) {
        self.seek_to(seconds);
    }
}
