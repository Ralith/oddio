use std::sync::atomic::{AtomicU32, Ordering};

use crate::{Controlled, Filter, Frame, Signal};

/// Scales rate of playback by a dynamically-adjustable factor
///
/// Higher/lower speeds will naturally result in higher/lower pitched sound respectively.
pub struct Speed<T: ?Sized> {
    speed: AtomicU32,
    inner: T,
}

impl<T> Speed<T> {
    /// Apply dynamic speed to `signal`
    pub fn new(signal: T) -> Self {
        Self {
            speed: AtomicU32::new(1.0f32.to_bits()),
            inner: signal,
        }
    }
}

impl<T: Signal> Signal for Speed<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, interval: f32, out: &mut [T::Frame]) {
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        self.inner.sample(interval * speed, out);
    }

    fn remaining(&self) -> f32 {
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        self.inner.remaining() / speed
    }
}

impl<T> Filter for Speed<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.inner
    }
}

pub struct SpeedControl<'a, T>(&'a Speed<T>);

unsafe impl<'a, T: 'a> Controlled<'a> for Speed<T> {
    type Control = SpeedControl<'a, T>;

    fn make_control(signal: &'a Speed<T>) -> Self::Control {
        SpeedControl(signal)
    }
}

impl<'a, T> SpeedControl<'a, T> {
    /// Get the current speed
    pub fn speed(&self) -> f32 {
        f32::from_bits(self.0.speed.load(Ordering::Relaxed))
    }

    /// Adjust the speed
    pub fn set_speed(&mut self, factor: f32) {
        self.0.speed.store(factor.to_bits(), Ordering::Relaxed);
    }
}
