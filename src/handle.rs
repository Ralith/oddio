use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::Signal;

/// Handle for manipulating a signal owned elsewhere
pub struct Handle<T: ?Sized> {
    pub(crate) shared: Arc<SignalData<T>>,
}

impl<S: Signal + Send + 'static> Handle<S> {
    /// Construct a handle to `signal` and erase its type
    pub fn new(signal: S) -> (Self, ErasedSignal<S::Frame>) {
        let shared = Arc::new(SignalData {
            stop: AtomicBool::new(false),
            signal,
        });
        (
            Self {
                shared: shared.clone(),
            },
            ErasedSignal(shared),
        )
    }
}

// Sound because `T` is not accessible except via `unsafe trait Controlled`
unsafe impl<T> Send for Handle<T> {}
unsafe impl<T> Sync for Handle<T> {}

impl<T> Handle<T> {
    /// Mark the signal to be stopped
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the signal has been stopped
    pub fn is_stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }
}

/// Type-erased signal for which a [`Handle`] exists
// Future work: Allow caller-controlled degree of erasure, so e.g. `SpatialScene` doesn't have to
// rely on internal-only interfaces. Probably needs something like
// https://github.com/rust-lang/rust/issues/27732.
pub struct ErasedSignal<T>(pub(crate) Arc<SignalData<dyn Signal<Frame = T> + Send>>);

impl<T> ErasedSignal<T> {
    /// Mark the signal as stopped
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the signal should be stopped
    pub fn is_stopped(&self) -> bool {
        self.0.stop.load(Ordering::Relaxed)
    }
}

impl<T> Deref for ErasedSignal<T> {
    type Target = dyn Signal<Frame = T>;
    fn deref(&self) -> &(dyn Signal<Frame = T> + 'static) {
        &self.0.signal
    }
}

/// State shared between [`Handle`] and [`ErasedSignal`]
pub(crate) struct SignalData<S: ?Sized> {
    pub(crate) stop: AtomicBool,
    pub(crate) signal: S,
}
