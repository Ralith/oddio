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
        let a = unsafe { sample.to_int_unchecked::<usize>() };
        let fract = sample - a as f64;
        if a < self.frames.len() - 1 {
            frame::lerp(&self.frames[a], &self.frames[a + 1], fract as f32)
        } else {
            frame::lerp(&self.frames[a], &self.frames[0], fract as f32)
        }
    }
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        let ds = interval as f64 * self.frames.rate() as f64;
        let len = self.frames.len() as f64;
        let mut s = self.cursor.get();
        if s < 0.0 {
            s += len;
        }
        // Check if we can omit wraparound checks in the inner loop.
        let end = s + (ds + f64::EPSILON) * out.len() as f64;
        if end < len {
            let base = s.trunc() as usize;
            let mut offset = s.fract() as f32;
            s += ds * out.len() as f64;
            for x in out {
                let trunc = unsafe { offset.to_int_unchecked::<usize>() };
                let fract = offset - trunc as f32;
                *x = frame::lerp(&self.frames[base + trunc], &self.frames[base + trunc + 1], fract);
                offset += ds as f32;
            }
        } else {
            for x in out {
                *x = self.interpolate(s);
                s += ds;
                if s >= len {
                    s %= len;
                }
            }
        }
        self.cursor.set(s);
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
