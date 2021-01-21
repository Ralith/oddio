use std::cell::Cell;

use crate::{Filter, Frame, Seek, Signal};

/// Loops a [`Seek`]able signal
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

impl<T: Signal + Seek> Signal for Cycle<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, interval: f32, mut out: &mut [T::Frame]) {
        while !out.is_empty() {
            let seconds = self.duration - self.cursor.get();
            let samples = 1 + (seconds / interval) as usize;
            let n = samples.min(out.len());
            self.inner.sample(interval, &mut out[..n]);
            self.cursor.set(self.cursor.get() + interval * n as f32);
            if self.cursor.get() > self.duration {
                self.seek_to(self.cursor.get());
            }
            out = &mut out[n..];
        }
    }
}

impl<T> Filter for Cycle<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

impl<T: Seek> Seek for Cycle<T> {
    fn seek_to(&self, t: f32) {
        let t = t.rem_euclid(self.duration);
        self.inner.seek_to(t);
        self.cursor.set(t);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    // Logs the time of every sample
    struct LoggedSignal {
        times: RefCell<Vec<f32>>,
        t: Cell<f32>,
    }

    impl Signal for LoggedSignal {
        type Frame = f32;
        fn sample(&self, interval: f32, out: &mut [f32]) {
            for i in 0..out.len() {
                self.times
                    .borrow_mut()
                    .push(self.t.get() + i as f32 * interval);
            }
            self.t.set(self.t.get() + interval * out.len() as f32);
        }
    }

    impl Seek for LoggedSignal {
        fn seek_to(&self, t: f32) {
            self.t.set(t);
        }
    }

    #[test]
    fn wrap_single() {
        let s = Cycle::new(
            LoggedSignal {
                t: Cell::new(0.0),
                times: RefCell::new(Vec::new()),
            },
            1.25,
        );
        s.sample(1.0, &mut [0.0; 5]);
        assert_eq!(s.inner.times.borrow()[..], [0.0, 1.0, 0.75, 0.5, 0.25]);
    }

    #[test]
    fn wrap_multi() {
        let s = Cycle::new(
            LoggedSignal {
                t: Cell::new(0.0),
                times: RefCell::new(Vec::new()),
            },
            2.5,
        );
        s.sample(1.0, &mut [0.0; 2]);
        s.sample(1.0, &mut [0.0; 2]);
        assert_eq!(s.inner.times.borrow()[..], [0.0, 1.0, 2.0, 0.5]);
    }
}
