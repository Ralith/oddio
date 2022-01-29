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
    cursor: Cell<f32>,
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

    /// Interpolate a frame for position `sample`
    fn interpolate(&self, sample: f32) -> T
    where
        T: Frame,
    {
        let a = sample as usize;
        let b = (a + 1) % self.frames.len();
        frame::lerp(&self.frames[a], &self.frames[b], sample.fract())
    }
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        let ds = interval * self.frames.rate() as f32;
        for x in out {
            *x = self.interpolate(self.cursor.get());
            self.cursor
                .set((self.cursor.get() + ds) % self.frames.len() as f32);
        }
    }
}

impl<T: Frame + Copy> Seek for Cycle<T> {
    fn seek(&self, seconds: f32) {
        self.cursor.set(
            (self.cursor.get() + seconds * self.frames.rate() as f32) % self.frames.len() as f32,
        );
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
    fn crossfade() {
        let mut frames = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let faded = apply_crossfade(4, &mut frames);
        assert_eq!(faded.len(), 5);
        for i in 0..5 {
            assert!((faded[i] - (5 - (i + 1)) as f32 / 5.0).abs() < 1e-3);
        }
    }
}
