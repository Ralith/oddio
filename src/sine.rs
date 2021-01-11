use std::{cell::Cell, f32::consts::TAU};

use crate::{Sample, Source};

/// A trivial [`Source`] that produces a sine wave of a particular frequency, forever
pub struct Sine {
    /// Normalized units, i.e. [0, 1)
    phase: Cell<f32>,
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
            phase: Cell::new(phase / TAU),
            frequency: frequency_hz * TAU,
        }
    }
}

impl Source for Sine {
    type Frame = Sample;

    fn sample(&self, offset: f32, sample_length: f32, out: &mut [Sample]) {
        let start = self.phase.get() + offset;
        for (i, x) in out.iter_mut().enumerate() {
            let t = start + sample_length * i as f32;
            *x = (t * self.frequency).sin();
        }
    }

    fn advance(&self, dt: f32) {
        // Advance time, but wrap into 0-1 for numerical stability no matter how long we play for
        self.phase.set((self.phase.get() + dt).fract());
    }

    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}
