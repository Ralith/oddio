use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{Controlled, Filter, Frame, Signal};

/// Scales rate of playback by a dynamically-adjustable factor
///
/// Higher/lower speeds will naturally result in higher/lower pitched sound respectively.
pub struct Speed<T: ?Sized> {
    // To avoid temporal discontinuities when advancing after sampling, we only update `speed` after
    // `advance`.
    speed_shared: AtomicU32,
    speed: Cell<f32>,
    inner: T,
}

impl<T> Speed<T> {
    /// Apply dynamic speed to `signal`
    pub fn new(signal: T) -> Self {
        Self {
            speed_shared: AtomicU32::new(1.0f32.to_bits()),
            speed: Cell::new(1.0),
            inner: signal,
        }
    }
}

impl<T: Signal> Signal for Speed<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, offset: f32, sample_interval: f32, out: &mut [T::Frame]) {
        self.inner.sample(
            offset * self.speed.get(),
            sample_interval * self.speed.get(),
            out,
        );
    }

    fn advance(&self, dt: f32) {
        self.inner.advance(dt * self.speed.get());
        self.speed
            .set(f32::from_bits(self.speed_shared.load(Ordering::Relaxed)));
    }

    fn remaining(&self) -> f32 {
        self.inner.remaining() / self.speed.get()
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
        f32::from_bits(self.0.speed_shared.load(Ordering::Relaxed))
    }

    /// Adjust the speed
    pub fn set_speed(&mut self, factor: f32) {
        self.0
            .speed_shared
            .store(factor.to_bits(), Ordering::Relaxed);
    }
}
