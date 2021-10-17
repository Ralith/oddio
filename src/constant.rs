use crate::{Seek, Signal};

/// A constant signal, useful for testing
pub struct Constant<T>(pub T);

impl<T> Constant<T> {
    /// Construct a signal that always emits `frame`
    pub fn new(frame: T) -> Self {
        Self(frame)
    }
}

impl<T: Clone> Signal for Constant<T> {
    type Frame = T;

    fn sample(&self, _interval: f32, out: &mut [T]) {
        out.fill(self.0.clone());
    }
}

impl<T: Clone> Seek for Constant<T> {
    fn seek(&self, _: f32) {}
}
