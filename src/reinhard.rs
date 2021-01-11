use crate::{Frame, Source};

/// Smoothly maps a signal of any range into (-1, 1)
///
/// For each input sample `x`, outputs `x / (1 + |x|)`.
///
/// When many sources are combined with a [`Mixer`](crate::Mixer) or [`Spatial`](crate::Spatial), or
/// when spatial sources are very near by, audio can get arbitrarily loud. Because surprisingly loud
/// audio can be disruptive and even damaging, it can be useful to limit the output range, but
/// simple clamping introduces audible artifacts.
pub struct Reinhard<T>(T);

impl<T> Reinhard<T> {
    /// Apply the Reinhard operator to `source`
    pub fn new(source: T) -> Self {
        Self(source)
    }
}

impl<T: Source> Source for Reinhard<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, offset: f32, sample_length: f32, out: &mut [T::Frame]) {
        self.0.sample(offset, sample_length, out);
        for x in out {
            for channel in x.channels_mut() {
                *channel /= 1.0 + channel.abs();
            }
        }
    }

    fn advance(&self, dt: f32) {
        self.0.advance(dt);
    }

    fn remaining(&self) -> f32 {
        self.0.remaining()
    }
}
