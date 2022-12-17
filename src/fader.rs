use core::{
    cell::{Cell, UnsafeCell},
    mem,
    time::Duration,
};
use std::time::Instant;

use crate::{frame, math::Float, Controlled, Filter, Frame, Signal, Swap};

/// Cross-fades smoothly between dynamically-supplied signals
///
/// Uses constant-power fading, suitable for blending uncorrelated signals without distorting
/// perceived loudness
pub struct Fader<T> {
    progress: Cell<f32>,
    inner: UnsafeCell<T>,
    next: Swap<Option<Command<T>>>,
}

impl<T> Fader<T> {
    /// Create a fader initially wrapping `inner`
    pub fn new(inner: T) -> Self {
        Self {
            progress: Cell::new(1.0),
            inner: UnsafeCell::new(inner),
            next: Swap::new(|| None),
        }
    }
}

impl<T: Signal> Signal for Fader<T>
where
    T::Frame: Frame,
{
    type Frame = T::Frame;

    #[allow(clippy::float_cmp)]
    fn sample(&self, interval: f32, mut out: &mut [T::Frame]) {
        let inner = unsafe { &mut *self.inner.get() };

        if self.progress.get() >= 1.0 || self.progress.get() < 0.0 {
            // A fade must complete before a new one begins
            if self.next.refresh() {
                let time_diff = unsafe { (*self.next.received()).as_mut().unwrap() }
                    .begin
                    .map(|i| i.saturating_duration_since(Instant::now()).as_secs_f32())
                    .unwrap_or(0.0);
                self.progress.set(-time_diff);
            }

            if !self.next.refresh() || self.progress.get() < 0.0 {
                // Fast path
                inner.sample(interval, out);
                return;
            }
        }

        let next = unsafe { (*self.next.received()).as_mut().unwrap() };
        let increment = interval / next.duration;
        while !out.is_empty() {
            let mut buffer = [(); 1024].map(|()| T::Frame::ZERO);
            let n = buffer.len().min(out.len());
            inner.sample(interval, &mut buffer);
            next.fade_to.sample(interval, out);

            for (o, x) in out.iter_mut().zip(&buffer) {
                let fade_out = (1.0 - self.progress.get()).sqrt();
                let fade_in = self.progress.get().sqrt();
                *o = frame::mix(&frame::scale(x, fade_out), &frame::scale(o, fade_in));
                self.progress
                    .set((self.progress.get() + increment).min(1.0));
            }
            out = &mut out[n..];
        }

        if self.progress.get() >= 1.0 {
            // We've finished fading; move the new signal into `self`, and stash the old one back in
            // `next` to be dropped by a future `fade_to` call.
            mem::swap(inner, &mut next.fade_to);
        }
    }

    #[inline]
    fn is_finished(&self) -> bool {
        false
    }

    #[inline]
    fn handle_dropped(&self) {
        unsafe {
            (*self.inner.get()).handle_dropped();
        }
    }
}

impl<T> Filter for Fader<T> {
    type Inner = T;
    fn inner(&self) -> &T {
        unsafe { &*self.inner.get() }
    }
}

/// Thread-safe control for a [`Fader`] filter
pub struct FaderControl<'a, T>(&'a Swap<Option<Command<T>>>);

unsafe impl<'a, T: 'a> Controlled<'a> for Fader<T> {
    type Control = FaderControl<'a, T>;

    unsafe fn make_control(signal: &'a Fader<T>) -> Self::Control {
        FaderControl(&signal.next)
    }
}

impl<'a, T> FaderControl<'a, T> {
    /// Crossfade to `signal` over `duration`. If a fade is already in progress, it will complete
    /// before a fading to the new signal begins. If another signal is already waiting for a current
    /// fade to complete, the waiting signal is replaced.
    pub fn fade_to(&mut self, signal: T, duration: f32) {
        unsafe {
            *self.0.pending() = Some(Command {
                fade_to: signal,
                duration,
                begin: None,
            });
        }
        self.0.flush()
    }

    /// Crossfade to `signal` over `duration`, after `seconds_from_now`.
    pub fn deferred_fade_to(&mut self, signal: T, duration: f32, seconds_from_now: f32) {
        unsafe {
            *self.0.pending() = Some(Command {
                fade_to: signal,
                duration,
                begin: Some(Instant::now() + Duration::from_secs_f32(seconds_from_now)),
            });
        }
        self.0.flush()
    }
}

struct Command<T> {
    fade_to: T,
    duration: f32,
    begin: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use crate::Constant;

    use super::*;

    #[test]
    fn smoke() {
        let s = Fader::new(Constant(1.0));
        let mut buf = [42.0; 12];
        s.sample(0.1, &mut buf);
        assert_eq!(buf, [1.0; 12]);
        FaderControl(&s.next).fade_to(Constant(0.0), 1.0);
        s.sample(0.1, &mut buf);
        assert_eq!(buf[0], 1.0);
        assert_eq!(buf[11], 0.0);
        assert!((buf[5] - 0.5f32.sqrt()).abs() < 1e-6);
    }
}
