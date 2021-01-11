use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::Source;

/// Handle for manipulating a source owned elsewhere
pub struct Handle<T: ?Sized> {
    pub(crate) shared: Arc<SourceData<T>>,
}

impl<S: Source + Send + 'static> Handle<S> {
    /// Construct a handle to `source` and erase its type
    pub fn new(source: S) -> (Self, ErasedSource<S::Frame>) {
        let shared = Arc::new(SourceData {
            stop: AtomicBool::new(false),
            source,
        });
        (
            Self {
                shared: shared.clone(),
            },
            ErasedSource(shared),
        )
    }
}

// Sound because `T` is not accessible except via `unsafe trait Controlled`
unsafe impl<T> Send for Handle<T> {}
unsafe impl<T> Sync for Handle<T> {}

impl<T> Handle<T> {
    /// Mark the source to be stopped
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the source has been stopped
    pub fn is_stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }
}

/// Type-erased source for which a [`Handle`] exists
// Future work: Allow caller-controlled degree of erasure, so e.g. `SpatialScene` doesn't have to
// rely on internal-only interfaces. Probably needs something like
// https://github.com/rust-lang/rust/issues/27732.
pub struct ErasedSource<T>(pub(crate) Arc<SourceData<dyn Source<Frame = T> + Send>>);

impl<T> ErasedSource<T> {
    /// Mark the source as stopped
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
    }

    /// Whether the source should be stopped
    pub fn is_stopped(&self) -> bool {
        self.0.stop.load(Ordering::Relaxed)
    }
}

impl<T> Deref for ErasedSource<T> {
    type Target = dyn Source<Frame = T>;
    fn deref(&self) -> &(dyn Source<Frame = T> + 'static) {
        &self.0.source
    }
}

/// State shared between [`Handle`] and [`ErasedSource`]
pub(crate) struct SourceData<S: ?Sized> {
    pub(crate) stop: AtomicBool,
    pub(crate) source: S,
}
