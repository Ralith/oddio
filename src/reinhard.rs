use crate::{Filter, Frame, Signal};

/// Smoothly maps a signal of any range into (-1, 1)
///
/// For each input sample `x`, outputs `x / (1 + |x|)`.
///
/// When many signals are combined with a [`Mixer`](crate::Mixer) or [`Spatial`](crate::Spatial), or
/// when spatial signals are very near by, audio can get arbitrarily loud. Because surprisingly loud
/// audio can be disruptive and even damaging, it can be useful to limit the output range, but
/// simple clamping introduces audible artifacts.
pub struct Reinhard<T>(T);

impl<T> Reinhard<T> {
    /// Apply the Reinhard operator to `signal`
    pub fn new(signal: T) -> Self {
        Self(signal)
    }
}

impl<T: Signal> Signal for Reinhard<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, interval: f32, out: &mut [T::Frame]) {
        self.0.sample(interval, out);
        for x in out {
            for channel in x.channels_mut() {
                *channel /= 1.0 + channel.abs();
            }
        }
    }

    fn remaining(&self) -> f32 {
        self.0.remaining()
    }

    #[inline]
    fn handle_dropped(&self) {
        self.0.handle_dropped();
    }
}

impl<T> Filter for Reinhard<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.0
    }
}
