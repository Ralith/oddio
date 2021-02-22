use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{frame, Controlled, Filter, Frame, Signal};

/// Scales amplitude by a dynamically-adjustable factor
pub struct Gain<T: ?Sized> {
    shared: AtomicU32,
    prev_gain: Cell<f32>,
    next_gain: Cell<f32>,
    time_since_changed: Cell<f32>,
    inner: T,
}

impl<T> Gain<T> {
    /// Apply dynamic gain to `signal`
    pub fn new(signal: T) -> Self {
        Self {
            shared: AtomicU32::new(1.0f32.to_bits()),
            prev_gain: Cell::new(1.0),
            next_gain: Cell::new(1.0),
            time_since_changed: Cell::new(1.0),
            inner: signal,
        }
    }

    fn gain(&self) -> f32 {
        let diff = self.next_gain.get() - self.prev_gain.get();
        let progress = ((self.time_since_changed.get()) / SMOOTHING_PERIOD).min(1.0);
        self.prev_gain.get() + progress * diff
    }
}

impl<T: Signal> Signal for Gain<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    #[allow(clippy::float_cmp)]
    fn sample(&self, interval: f32, out: &mut [T::Frame]) {
        self.inner.sample(interval, out);
        let shared = f32::from_bits(self.shared.load(Ordering::Relaxed));
        if self.next_gain.get() != shared {
            self.prev_gain.set(self.gain());
            self.next_gain.set(shared);
            self.time_since_changed.set(0.0);
        }
        for x in out {
            *x = frame::scale(x, self.gain());
            self.time_since_changed
                .set(self.time_since_changed.get() + interval);
        }
    }

    fn remaining(&self) -> f32 {
        self.inner.remaining()
    }
}

impl<T> Filter for Gain<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

/// Thread-safe control for a [`Gain`] filter
pub struct GainControl<'a>(&'a AtomicU32);

unsafe impl<'a, T: 'a> Controlled<'a> for Gain<T> {
    type Control = GainControl<'a>;

    unsafe fn make_control(signal: &'a Gain<T>) -> Self::Control {
        GainControl(&signal.shared)
    }
}

impl<'a> GainControl<'a> {
    /// Get the current gain
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Adjust the gain
    pub fn set_gain(&mut self, factor: f32) {
        self.0.store(factor.to_bits(), Ordering::Relaxed);
    }
}

/// Number of seconds over which to smooth a change in gain
const SMOOTHING_PERIOD: f32 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::Sample;

    struct Const;

    impl Signal for Const {
        type Frame = Sample;

        fn sample(&self, _: f32, out: &mut [Sample]) {
            for x in out {
                *x = 1.0;
            }
        }
    }

    #[test]
    fn smoothing() {
        let s = Gain::new(Const);
        let mut buf = [0.0; 6];
        GainControl(&s.shared).set_gain(5.0);
        s.sample(0.025, &mut buf);
        assert_eq!(buf, [1.0, 2.0, 3.0, 4.0, 5.0, 5.0]);
        s.sample(0.025, &mut buf);
        assert_eq!(buf, [5.0; 6]);
    }
}
