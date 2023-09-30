use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{Frame, Signal};

/// Scales rate of playback by a dynamically-adjustable factor
///
/// Higher/lower speeds will naturally result in higher/lower pitched sound respectively.
pub struct Speed<T: ?Sized> {
    speed: Arc<AtomicU32>,
    inner: T,
}

impl<T> Speed<T> {
    /// Apply dynamic speed to `signal`
    pub fn new(signal: T) -> (SpeedControl, Self) {
        let signal = Self {
            speed: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            inner: signal,
        };
        let control = SpeedControl(signal.speed.clone());
        (control, signal)
    }
}

impl<T: Signal> Signal for Speed<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&mut self, interval: f32, out: &mut [T::Frame]) {
        let speed = f32::from_bits(self.speed.load(Ordering::Relaxed));
        self.inner.sample(interval * speed, out);
    }

    fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

/// Thread-safe control for a [`Speed`] filter
pub struct SpeedControl(Arc<AtomicU32>);

impl SpeedControl {
    /// Get the current speed
    pub fn speed(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Adjust the speed
    pub fn set_speed(&mut self, factor: f32) {
        self.0.store(factor.to_bits(), Ordering::Relaxed);
    }
}
