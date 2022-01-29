use alloc::sync::Arc;
use core::cell::Cell;

use crate::{frame, math::Float, Frame, Frames, Seek, Signal};

/// Loops [`Frames`] end-to-end to construct a repeating signal
pub struct Cycle<T> {
    /// Current playback time, in samples
    cursor: Cell<f64>,
    frames: Arc<Frames<T>>,
}

impl<T> Cycle<T> {
    /// Construct cycle from `frames`
    // TODO: Crossfade
    pub fn new(frames: Arc<Frames<T>>) -> Self {
        Self {
            cursor: Cell::new(0.0),
            frames,
        }
    }

    /// Interpolate a frame for position `sample`
    fn interpolate(&self, sample: f64) -> T
    where
        T: Frame,
    {
        let a = sample as usize;
        let b = (a + 1) % self.frames.len();
        frame::lerp(&self.frames[a], &self.frames[b], sample.fract() as f32)
    }
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        let ds = interval as f64 * self.frames.rate() as f64;
        for x in out {
            *x = self.interpolate(self.cursor.get());
            self.cursor
                .set((self.cursor.get() + ds) % self.frames.len() as f64);
        }
    }
}

impl<T: Frame + Copy> Seek for Cycle<T> {
    fn seek(&self, seconds: f32) {
        self.cursor.set(
            (self.cursor.get() + f64::from(seconds) * self.frames.rate() as f64)
                % self.frames.len() as f64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: &[f32] = &[1.0, 2.0, 3.0];

    #[test]
    fn wrap_single() {
        let s = Cycle::new(Frames::from_slice(1, FRAMES));
        let mut buf = [0.0; 5];
        s.sample(1.0, &mut buf);
        assert_eq!(buf, [1.0, 2.0, 3.0, 1.0, 2.0]);
    }

    #[test]
    fn wrap_multi() {
        let s = Cycle::new(Frames::from_slice(1, FRAMES));
        let mut buf = [0.0; 5];
        s.sample(1.0, &mut buf[..2]);
        s.sample(1.0, &mut buf[2..]);
        assert_eq!(buf, [1.0, 2.0, 3.0, 1.0, 2.0]);
    }
}
