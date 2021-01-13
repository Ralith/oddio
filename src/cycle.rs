use std::cell::Cell;

use crate::{Filter, Frame, Signal};

/// Loops a signal
pub struct Cycle<T: ?Sized> {
    duration: f32,
    cursor: Cell<f32>,
    inner: T,
}

impl<T> Cycle<T> {
    /// Loop `signal` such that it starts over after `duration` seconds
    // TODO: Crossfade
    pub fn new(signal: T, duration: f32) -> Self {
        Self {
            duration,
            cursor: Cell::new(0.0),
            inner: signal,
        }
    }
}

impl<T: Signal> Signal for Cycle<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, mut offset: f32, sample_length: f32, mut out: &mut [T::Frame]) {
        offset = (offset + self.cursor.get()).rem_euclid(self.duration);
        while !out.is_empty() {
            let seconds = self.duration - offset;
            let samples = seconds / sample_length;
            let end = (1 + samples as usize).min(out.len());
            self.inner.sample(offset, sample_length, &mut out[..end]);
            offset = (1.0 - samples.fract()) * sample_length;
            out = &mut out[end..];
        }
    }

    fn advance(&self, dt: f32) {
        self.cursor
            .set((self.cursor.get() + dt).rem_euclid(self.duration));
    }

    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}

impl<T> Filter for Cycle<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn wrap() {
        // Logs the time of every sample
        struct LoggedSignal(RefCell<Vec<f32>>);

        impl Signal for LoggedSignal {
            type Frame = f32;
            fn sample(&self, offset: f32, interval: f32, out: &mut [f32]) {
                for i in 0..out.len() {
                    self.0.borrow_mut().push(offset + i as f32 * interval);
                }
            }

            fn advance(&self, _: f32) {}

            fn remaining(&self) -> f32 {
                f32::INFINITY
            }
        }

        let s = Cycle::new(LoggedSignal(RefCell::new(Vec::new())), 1.25);
        s.sample(0.0, 1.0, &mut [0.0; 5]);
        assert_eq!(s.inner.0.borrow()[..], [0.0, 1.0, 0.75, 0.5, 0.25]);
    }
}
