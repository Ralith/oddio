use std::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{frame, Controlled, Filter, Frame, Signal, Smoothed};

/// Amplifies a signal
///
/// To implement a volume control, place a gain combinator near the end of your pipeline where the
/// input amplitude is initially in the range [0, 1] and pass decibels to [`GainControl::set_gain`],
/// mapping the minimum volume to 0 decibels, and the minimum to e.g. -60.
pub struct Gain<T: ?Sized> {
    shared: AtomicU32,
    gain: RefCell<Smoothed<f32>>,
    inner: T,
}

impl<T> Gain<T> {
    /// Apply dynamic gain to `signal`, starting at `initial`
    pub fn new(signal: T, initial: f32) -> Self {
        Self {
            shared: AtomicU32::new(initial.to_bits()),
            gain: RefCell::new(Smoothed::new(initial)),
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

    #[inline]
    fn handle_dropped(&self) {
        self.inner.handle_dropped();
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

    /// Amplify the signal by `db` decibels
    ///
    /// Perceptually linear. Negative values make the signal quieter.
    ///
    /// Equivalent to `self.set_amplitude_ratio(10.0f32.powf(db / 20.0))`.
    pub fn set_gain(&mut self, db: f32) {
        self.set_amplitude_ratio(10.0f32.powf(db / 20.0));
    }

    /// Scale the amplitude of the signal directly
    ///
    /// This is nonlinear in terms of both perception and power. Most users should prefer
    /// `set_gain`. Unlike `set_gain`, this method allows a signal to be completely zeroed out if
    /// needed, or even have its phase inverted with a negative factor.
    pub fn set_amplitude_ratio(&mut self, factor: f32) {
        self.0.store(factor.to_bits(), Ordering::Relaxed);
    }
}

/// Number of seconds over which to smooth a change in gain
const SMOOTHING_PERIOD: f32 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Constant;

    #[test]
    fn smoothing() {
        let s = Gain::new(Constant(1.0), 1.0);
        let mut buf = [0.0; 6];
        GainControl(&s.shared).set_amplitude_ratio(5.0);
        s.sample(0.025, &mut buf);
        assert_eq!(buf, [1.0, 2.0, 3.0, 4.0, 5.0, 5.0]);
        s.sample(0.025, &mut buf);
        assert_eq!(buf, [5.0; 6]);
    }
}
