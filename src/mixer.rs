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
