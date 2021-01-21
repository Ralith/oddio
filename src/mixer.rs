use std::cell::RefCell;

use crate::{frame, set, ErasedSignal, Frame, Handle, Set, SetHandle, Signal};

/// Build a mixer and a handle for controlling it
pub fn mixer<T: Frame + Copy>() -> (MixerHandle<T>, Mixer<T>) {
    let (handle, set) = set();
    (
        MixerHandle(handle),
        Mixer(RefCell::new(Inner {
            set,
            buffer: vec![T::ZERO; 1024].into(),
        })),
    )
}

/// Handle for controlling a [`Mixer`] from another thread
///
/// Constructed by calling [`mixer`].
pub struct MixerHandle<T>(SetHandle<ErasedSignal<T>>);

impl<T> MixerHandle<T> {
    /// Begin playing `signal`, returning a handle controlling its playback
    ///
    /// Finished signals are automatically stopped, and their storage reused for future `play`
    /// calls.
    pub fn play<S>(&mut self, signal: S) -> Handle<S>
    where
        S: Signal<Frame = T> + Send + 'static,
    {
        let (handle, erased) = Handle::new(signal);
        self.0.insert(erased);
        handle
    }
}

/// A [`Signal`] that mixes a dynamic set of [`Signal`]s, controlled by a [`MixerHandle`]
///
/// Constructed by calling [`mixer`].
pub struct Mixer<T>(RefCell<Inner<T>>);

struct Inner<T> {
    set: Set<ErasedSignal<T>>,
    buffer: Box<[T]>,
}

impl<T: Frame> Signal for Mixer<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        let this = &mut *self.0.borrow_mut();
        this.set.update();

        for o in out.iter_mut() {
            *o = T::ZERO;
        }

        for i in (0..this.set.len()).rev() {
            let signal = &this.set[i];
            if signal.remaining() < 0.0 {
                signal.stop();
            }
            if signal.is_stopped() {
                this.set.remove(i);
                continue;
            }

            // Sample into `buffer`, then mix into `out`
            let mut iter = out.iter_mut();
            while iter.len() > 0 {
                let n = iter.len().min(this.buffer.len());
                let staging = &mut this.buffer[..n];
                signal.sample(interval, staging);
                for (staged, o) in staging.iter().zip(&mut iter) {
                    *o = frame::mix(o, staged);
                }
            }
        }
    }
}
