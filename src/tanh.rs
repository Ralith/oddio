use crate::{math::Float, Filter, Frame, Seek, Signal};

/// Smoothly maps a signal of any range into (-1, 1)
///
/// For each input sample `x`, outputs `x.tanh()`. Similar to [`Reinhard`](crate::Reinhard), but
/// distorts quiet sounds less, and loud sounds more.
pub struct Tanh<T>(T);

impl<T> Tanh<T> {
    /// Apply the hypberbolic tangent operator to `signal`
    pub fn new(signal: T) -> Self {
        Self(signal)
    }
}

impl<T: Signal> Signal for Tanh<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    fn sample(&self, interval: f32, out: &mut [T::Frame]) {
        self.0.sample(interval, out);
        for x in out {
            for channel in x.channels_mut() {
                *channel = channel.tanh();
            }
        }
    }

    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    #[inline]
    fn handle_dropped(&self) {
        self.0.handle_dropped();
    }
}

impl<T> Filter for Tanh<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> Seek for Tanh<T>
where
    T: Signal + Seek,
    T::Frame: Frame,
{
    fn seek(&self, seconds: f32) {
        self.0.seek(seconds);
    }
}
