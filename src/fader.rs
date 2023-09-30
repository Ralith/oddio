use alloc::sync::Arc;
use core::mem;

use crate::{frame, math::Float, Frame, Signal, Swap};

/// Cross-fades smoothly between dynamically-supplied signals
///
/// Uses constant-power fading, suitable for blending uncorrelated signals without distorting
/// perceived loudness
pub struct Fader<T> {
    progress: f32,
    next: Arc<Swap<Option<Command<T>>>>,
    inner: T,
}

impl<T> Fader<T> {
    /// Create a fader initially wrapping `inner`
    pub fn new(inner: T) -> (FaderControl<T>, Self) {
        let signal = Self {
            progress: 1.0,
            next: Arc::new(Swap::new(|| None)),
            inner,
        };
        let control = FaderControl(signal.next.clone());
        (control, signal)
    }
}

impl<T: Signal> Signal for Fader<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    #[allow(clippy::float_cmp)]
    fn sample(&mut self, interval: f32, mut out: &mut [T::Frame]) {
        if self.progress >= 1.0 {
            // A fade must complete before a new one begins
            if self.next.refresh() {
                self.progress = 0.0;
            } else {
                // Fast path
                self.inner.sample(interval, out);
                return;
            }
        }

        let next = unsafe { (*self.next.received()).as_mut().unwrap() };
        let increment = interval / next.duration;
        while !out.is_empty() {
            let mut buffer = [(); 1024].map(|()| T::Frame::ZERO);
            let n = buffer.len().min(out.len());
            self.inner.sample(interval, &mut buffer);
            next.fade_to.sample(interval, out);

            for (o, x) in out.iter_mut().zip(&buffer) {
                let fade_out = (1.0 - self.progress).sqrt();
                let fade_in = self.progress.sqrt();
                *o = frame::mix(&frame::scale(x, fade_out), &frame::scale(o, fade_in));
                self.progress = (self.progress + increment).min(1.0);
            }
            out = &mut out[n..];
        }

        if self.progress >= 1.0 {
            // We've finished fading; move the new signal into `self`, and stash the old one back in
            // `next` to be dropped by a future `fade_to` call.
            mem::swap(&mut self.inner, &mut next.fade_to);
        }
    }

    #[inline]
    fn is_finished(&self) -> bool {
        false
    }
}

/// Thread-safe control for a [`Fader`] filter
pub struct FaderControl<T>(Arc<Swap<Option<Command<T>>>>);

impl<T> FaderControl<T> {
    /// Crossfade to `signal` over `duration`. If a fade is already in progress, it will complete
    /// before a fading to the new signal begins. If another signal is already waiting for a current
    /// fade to complete, the waiting signal is replaced.
    pub fn fade_to(&mut self, signal: T, duration: f32) {
        unsafe {
            *self.0.pending() = Some(Command {
                fade_to: signal,
                duration,
            });
        }
        self.0.flush()
    }
}

struct Command<T> {
    fade_to: T,
    duration: f32,
}

#[cfg(test)]
mod tests {
    use crate::Constant;

    use super::*;

    #[test]
    fn smoke() {
        let (mut c, mut s) = Fader::new(Constant(1.0));
        let mut buf = [42.0; 12];
        s.sample(0.1, &mut buf);
        assert_eq!(buf, [1.0; 12]);
        c.fade_to(Constant(0.0), 1.0);
        s.sample(0.1, &mut buf);
        assert_eq!(buf[0], 1.0);
        assert_eq!(buf[11], 0.0);
        assert!((buf[5] - 0.5f32.sqrt()).abs() < 1e-6);
    }
}
