use alloc::sync::Arc;
use core::cell::Cell;

use crate::{frame, frame::lerp, math::Float, Frame, Frames, Seek, Signal};

/// Loops [`Frames`] end-to-end to construct a repeating signal
///
/// To avoid glitching at the loop point, the underlying `Frames` *must* be specially prepared to
/// provide a smooth transition.
#[derive(Clone)]
pub struct Cycle<T> {
    /// Current playback time, in samples
    cursor: Cell<f64>,
    frames: Arc<Frames<T>>,
}

impl<T> Cycle<T> {
    /// Construct cycle from `frames` played at `rate` Hz, smoothing the loop point over
    /// `crossfade_size` seconds
    ///
    /// Suitable for use with arbitrary audio,
    pub fn with_crossfade(crossfade_size: f32, rate: u32, frames: &[T]) -> Self
    where
        T: Frame + Copy,
    {
        let mut frames = frames.iter().copied().collect::<alloc::vec::Vec<_>>();
        let frames = apply_crossfade((crossfade_size * rate as f32) as usize, &mut frames);
        Self::new(Frames::from_slice(rate, frames))
    }

    /// Construct cycle from `frames`
    ///
    /// `frames` *must* be specially constructed to loop seamlessly. For arbitrary audio, use
    /// [`with_crossfade`](Self::with_crossfade) instead.
    pub fn new(frames: Arc<Frames<T>>) -> Self {
        Self {
            cursor: Cell::new(0.0),
            frames,
        }
    }
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
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
    fn seek(&self, seconds: f32) {
        let s = (self.cursor.get() + f64::from(seconds) * self.frames.rate() as f64)
            .rem_euclid(self.frames.len() as f64);
        self.cursor.set(s);
    }
}

/// Prepare arbitrary frames for glitch-free use in `Cycle`
fn apply_crossfade<T: Frame + Copy>(size: usize, frames: &mut [T]) -> &mut [T] {
    let end = frames.len() - size;
    for i in 0..size {
        let a = frames[end + i];
        let b = frames[i];
        let t = (i + 1) as f32 / (size + 1) as f32;
        frames[i] = lerp(&a, &b, t);
    }
    &mut frames[..end]
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

    #[test]
    fn crossfade() {
        let mut frames = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let faded = apply_crossfade(4, &mut frames);
        assert_eq!(faded.len(), 5);
        for i in 0..5 {
            assert!((faded[i] - (5 - (i + 1)) as f32 / 5.0).abs() < 1e-3);
        }
    }
}
