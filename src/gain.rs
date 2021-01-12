use std::sync::atomic::{AtomicU32, Ordering};

use crate::{frame, Controlled, Filter, Frame, Signal};

/// Scales amplitude by a dynamically-adjustable factor
pub struct Gain<T: ?Sized> {
    gain: AtomicU32,
    inner: T,
}

impl<T> Gain<T> {
    /// Apply dynamic gain to `signal`
    pub fn new(signal: T) -> Self {
        Self {
            gain: AtomicU32::new(1.0f32.to_bits()),
            inner: signal,
        }
    }
}

impl<T: Signal> Signal for Gain<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, offset: f32, sample_length: f32, out: &mut [T::Frame]) {
        self.inner.sample(offset, sample_length, out);
        // Should we blend from the previous value?
        let gain = f32::from_bits(self.gain.load(Ordering::Relaxed));
        for x in out {
            *x = frame::scale(x, gain);
        }
    }

    fn advance(&self, dt: f32) {
        self.inner.advance(dt);
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

pub struct GainControl<'a, T>(&'a Gain<T>);

unsafe impl<'a, T: 'a> Controlled<'a> for Gain<T> {
    type Control = GainControl<'a, T>;

    fn make_control(signal: &'a Gain<T>) -> Self::Control {
        GainControl(signal)
    }
}

impl<'a, T> GainControl<'a, T> {
    /// Get the current gain
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.0.gain.load(Ordering::Relaxed))
    }

    /// Adjust the gain
    pub fn set_gain(&mut self, factor: f32) {
        self.0.gain.store(factor.to_bits(), Ordering::Relaxed);
    }
}
