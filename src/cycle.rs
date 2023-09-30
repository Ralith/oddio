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
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&mut self, interval: f32, out: &mut [T]) {
        let ds = interval * self.frames.rate() as f32;
        let mut base = self.cursor.get() as usize;
        let mut offset = (self.cursor.get() - base as f64) as f32;
        for o in out {
            let trunc = unsafe { offset.to_int_unchecked::<usize>() };
            let fract = offset - trunc as f32;
            let x = base + trunc;
            let (a, b) = if x < self.frames.len() - 1 {
                (self.frames[x], self.frames[x + 1])
            } else if x < self.frames.len() {
                (self.frames[x], self.frames[0])
            } else {
                base = 0;
                offset = (x % self.frames.len()) as f32 + fract;
                let x = unsafe { offset.to_int_unchecked::<usize>() };
                if x < self.frames.len() - 1 {
                    (self.frames[x], self.frames[x + 1])
                } else {
                    (self.frames[x], self.frames[0])
                }
            };

            *o = frame::lerp(&a, &b, fract);
            offset += ds;
        }
        self.cursor.set(base as f64 + offset as f64);
    }
}

impl<T: Frame + Copy> Seek for Cycle<T> {
    fn seek(&mut self, seconds: f32) {
        let s = (self.cursor.get() + f64::from(seconds) * self.frames.rate() as f64)
            .rem_euclid(self.frames.len() as f64);
        self.cursor.set(s);
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

    #[test]
    fn wrap_fract() {
        let s = Cycle::new(Frames::from_slice(1, FRAMES));
        let mut buf = [0.0; 8];
        s.sample(0.5, &mut buf[..2]);
        s.sample(0.5, &mut buf[2..]);
        assert_eq!(buf, [1.0, 1.5, 2.0, 2.5, 3.0, 2.0, 1.0, 1.5]);
    }

    #[test]
    fn wrap_fract_offset() {
        let s = Cycle::new(Frames::from_slice(1, FRAMES));
        s.seek(0.25);
        let mut buf = [0.0; 7];
        s.sample(0.5, &mut buf[..2]);
        s.sample(0.5, &mut buf[2..]);
        assert_eq!(buf, [1.25, 1.75, 2.25, 2.75, 2.5, 1.5, 1.25]);
    }

    #[test]
    fn wrap_single_frame() {
        let s = Cycle::new(Frames::from_slice(1, &[1.0]));
        s.seek(0.25);
        let mut buf = [0.0; 3];
        s.sample(1.0, &mut buf[..2]);
        s.sample(1.0, &mut buf[2..]);
        assert_eq!(buf, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn wrap_large_interval() {
        let s = Cycle::new(Frames::from_slice(1, FRAMES));
        let mut buf = [0.0; 3];
        s.sample(10.0, &mut buf[..2]);
        s.sample(10.0, &mut buf[2..]);
        assert_eq!(buf, [1.0, 2.0, 3.0]);
    }
}
