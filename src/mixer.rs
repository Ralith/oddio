use alloc::{boxed::Box, sync::Arc, vec};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{frame, set, Frame, Set, SetHandle, Signal};

/// Handle for controlling a [`Mixer`] from another thread
pub struct MixerControl<T>(SetHandle<ErasedSignal<T>>);

impl<T> MixerControl<T> {
    /// Begin playing `signal`, returning a handle that can be used to pause or stop it and access
    /// other controls
    ///
    /// Finished signals are automatically stopped, and their storage reused for future `play`
    /// calls.
    ///
    /// The type of signal given determines what additional controls can be used. See the
    /// examples for a detailed guide.
    pub fn play<S>(&mut self, signal: S) -> Mixed
    where
        S: Signal<Frame = T> + Send + 'static,
    {
        let signal = Box::new(MixedSignal::new(signal));
        let control = Mixed(signal.stop.clone());
        self.0.insert(signal);
        control
    }
}

/// Handle to a signal playing in a [`Mixer`]
pub struct Mixed(Arc<AtomicBool>);

impl Mixed {
    /// Immediately halt playback of the associated signal by its [`Mixer`]
    pub fn stop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether the signal's playback
    ///
    /// Set by both `is_stopped` and signals naturally finishing.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct MixedSignal<T: ?Sized> {
    stop: Arc<AtomicBool>,
    inner: T,
}

impl<T> MixedSignal<T> {
    fn new(signal: T) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            inner: signal,
        }
    }
}

/// A [`Signal`] that mixes a dynamic set of [`Signal`]s
pub struct Mixer<T> {
    recv: Inner<T>,
}

impl<T> Mixer<T>
where
    T: Frame + Clone,
{
    /// Construct a new mixer
    pub fn new() -> (MixerControl<T>, Self) {
        let (handle, set) = set();
        (
            MixerControl(handle),
            Self {
                recv: Inner {
                    set,
                    buffer: vec![T::ZERO; 1024].into(),
                },
            },
        )
    }
}

struct Inner<T> {
    set: Set<ErasedSignal<T>>,
    buffer: Box<[T]>,
}

impl<T: Frame> Signal for Mixer<T> {
    type Frame = T;

    fn sample(&mut self, interval: f32, out: &mut [T]) {
        let this = &mut self.recv;
        this.set.update();

        for o in out.iter_mut() {
            *o = T::ZERO;
        }

        for i in (0..this.set.len()).rev() {
            let signal = &mut this.set[i];
            if signal.stop.load(Ordering::Relaxed) || signal.inner.is_finished() {
                signal.stop.store(true, Ordering::Relaxed);
                this.set.remove(i);
                continue;
            }

            // Sample into `buffer`, then mix into `out`
            let mut iter = out.iter_mut();
            while iter.len() > 0 {
                let n = iter.len().min(this.buffer.len());
                let staging = &mut this.buffer[..n];
                signal.inner.sample(interval, staging);
                for (staged, o) in staging.iter().zip(&mut iter) {
                    *o = frame::mix(o, staged);
                }
            }
        }
    }
}

type ErasedSignal<T> = Box<MixedSignal<dyn Signal<Frame = T>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frames, FramesSignal};

    #[test]
    fn is_stopped() {
        let (mut mixer_control, mut mixer) = Mixer::new();
        let (_, signal) = FramesSignal::new(Frames::from_slice(1, &[0.0, 0.0]), 0.0);
        let handle = mixer_control.play(signal);
        assert!(!handle.is_stopped());

        let mut out = [0.0];

        mixer.sample(0.6, &mut out);
        assert!(!handle.is_stopped());

        mixer.sample(0.6, &mut out);
        // Signal is finished, but we won't actually notice until the next scan
        assert!(!handle.is_stopped());

        mixer.sample(0.0, &mut out);
        assert!(handle.is_stopped());
    }
}
