use std::{cell::Cell, sync::Arc};

use crate::{frame, Controlled, Frame, Frames, Signal, Swap};

/// Loops [`Frames`] to construct a repeating signal
///
/// [`CycleControl::set_sample_range`] can be used to adjust the range of frames being repeated.
pub struct Cycle<T> {
    /// Current playback time, in samples
    cursor: Cell<f32>,
    range: Swap<(usize, Option<usize>)>,
    frames: Arc<Frames<T>>,
}

impl<T> Cycle<T> {
    /// Construct cycle from `frames`
    // TODO: Crossfade
    pub fn new(frames: Arc<Frames<T>>) -> Self {
        Self {
            cursor: Cell::new(0.0),
            range: Swap::new((0, Some(frames.len()))),
            frames,
        }
    }

    /// Interpolate a frame for position `sample`
    fn interpolate(&self, start: usize, end: Option<usize>, sample: f32) -> T
    where
        T: Frame,
    {
        let a = sample as usize;
        let b = match end {
            None => a + 1,
            Some(end) => start + ((a + 1).saturating_sub(start) % (end - start)),
        };
        frame::lerp(&self.frames[a], &self.frames[b], sample.fract())
    }
}

impl<T: Frame + Copy> Signal for Cycle<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        self.range.refresh();
        let (start, end) = unsafe { *self.range.received() };
        let ds = interval * self.frames.rate() as f32;
        for x in out {
            *x = self.interpolate(start, end, self.cursor.get());
            self.cursor
                .set((self.cursor.get() + ds) % self.frames.len() as f32);
        }
    }

    fn remaining(&self) -> f32 {
        let (_, end) = unsafe { *self.range.received() };
        match end {
            None => self.frames.len() as f32 / self.frames.rate() as f32 - self.cursor.get(),
            Some(_) => f32::INFINITY,
        }
    }
}

/// Thread-safe control for a [`Cycle`]
pub struct CycleControl<'a>(&'a Swap<(usize, Option<usize>)>);

unsafe impl<'a, T: 'a> Controlled<'a> for Cycle<T> {
    type Control = CycleControl<'a>;

    unsafe fn make_control(signal: &'a Cycle<T>) -> Self::Control {
        CycleControl(&signal.range)
    }
}

impl<'a> CycleControl<'a> {
    /// Adjust the range of time being cycled through, in seconds
    ///
    /// If the playback cursor is outside the range, it will be immediately moved into it.
    pub fn set_sample_range(&mut self, start: usize, end: Option<usize>) {
        unsafe { self.0.pending().write((start, end)) }
        self.0.flush();
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
