use std::sync::Arc;

use crate::resample::{Interpolator, Spline};

pub trait Source {
    /// Fill `out` with samples
    fn next(&mut self, out: &mut [f32]);
}

#[derive(Debug, Clone)]
pub struct Buffer {
    samples: Arc<[f32]>,
    cursor: usize,
}

impl Buffer {
    pub fn new(samples: Arc<[f32]>) -> Self {
        Self {
            samples,
            cursor: 0,
        }
    }
}

impl Source for Buffer {
    fn next(&mut self, out: &mut [f32]) {
        let samples_left = self.samples.len().saturating_sub(self.cursor);
        let len = samples_left.min(out.len());
        out[0..len].copy_from_slice(&self.samples[self.cursor..len]);
        for x in &mut out[len..] {
            *x = 0.0;
        }
        self.cursor += len;
    }
}

pub struct PitchShift<I: Interpolator> {
    spline: Spline<I>,
    shift: f32,
    t: f32,
}

impl<I: Interpolator> PitchShift<I> {
    pub fn new(samples: &[f32]) -> Self {
        Self {
            spline: Spline::new(samples),
            shift: 1.0,
            t: 0.0,
        }
    }

    pub fn set_shift(&mut self, shift: f32) {
        self.shift = shift;
    }
}

impl<I: Interpolator> Source for PitchShift<I> {
    fn next(&mut self, out: &mut [f32]) {
        // FIXME: Following math probably doesn't result in a uniform distribution of sampling points
        let dt = self.shift * out.len() as f32 / self.spline.len() as f32;
        let t1 = (self.t + dt).max(0.0).min(1.0);
        let max_out_len = ((self.spline.len() as f32 * (1.0 - self.t)) / self.shift).trunc() as usize;
        let len = max_out_len.min(out.len());
        self.spline.sample(&mut out[..len], self.t, t1);
        for x in &mut out[len..] {
            *x = 0.0;
        }
        self.t = t1;
    }
}
