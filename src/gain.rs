use std::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{frame, Controlled, Filter, Frame, Signal, Smoothed};

/// Scales amplitude by a dynamically-adjustable factor
pub struct Gain<T: ?Sized> {
    shared: AtomicU32,
    gain: RefCell<Smoothed<f32>>,
    inner: T,
}

impl<T> Gain<T> {
    /// Apply dynamic gain to `signal`
    pub fn new(signal: T) -> Self {
        Self {
            shared: AtomicU32::new(1.0f32.to_bits()),
            gain: RefCell::new(Smoothed::new(1.0)),
            inner: signal,
        }
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
        let mut gain = self.gain.borrow_mut();
        if gain.get() != shared {
            gain.set(shared);
        }
        for x in out {
            *x = frame::scale(x, gain.get());
            gain.advance(interval / SMOOTHING_PERIOD);
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
    ///
    /// `factor` is linear. Human perception of loudness is logarithmic, so user-visible
    /// configuration should use an exponential curve, e.g. `1e-3 * (6.908 * x).exp()` for `x` in
    /// [0, 1]` repreesnting a range of -60 to 0 dB.
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
