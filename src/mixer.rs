use std::cell::RefCell;

use crate::{frame, set, Controlled, ErasedSignal, Frame, Handle, Set, SetHandle, Signal};

/// Handle for controlling a [`Mixer`] from another thread
///
/// Constructed by calling [`mixer`].
pub struct MixerControl<'a, T>(&'a Mixer<T>);

impl<T> MixerControl<'_, T> {
    /// Begin playing `signal`, returning a handle controlling its playback
    ///
    /// Finished signals are automatically stopped, and their storage reused for future `play`
    /// calls.
    pub fn play<S>(&mut self, signal: S) -> Handle<S>
    where
        S: Signal<Frame = T> + Send + 'static,
    {
        let (handle, erased) = Handle::new(signal);
        self.0.send.borrow_mut().insert(erased);
        handle
    }
}

/// A [`Signal`] that mixes a dynamic set of [`Signal`]s
///
/// Constructed by calling [`mixer`].
pub struct Mixer<T> {
    send: RefCell<SetHandle<ErasedSignal<T>>>,
    recv: RefCell<Inner<T>>,
}

impl<T> Mixer<T>
where
    T: Frame + Clone,
{
    /// Construct a new mixer
    pub fn new() -> Self {
        let (handle, set) = set();
        Self {
            send: RefCell::new(handle),
            recv: RefCell::new(Inner {
                set,
                buffer: vec![T::ZERO; 1024].into(),
            }),
        }
    }
}

impl<T> Default for Mixer<T>
where
    T: Frame + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<'a, T: 'a> Controlled<'a> for Mixer<T> {
    type Control = MixerControl<'a, T>;

    unsafe fn make_control(signal: &'a Mixer<T>) -> Self::Control {
        MixerControl(signal)
    }
}

struct Inner<T> {
    set: Set<ErasedSignal<T>>,
    buffer: Box<[T]>,
}

impl<T: Frame> Signal for Mixer<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        let this = &mut *self.recv.borrow_mut();
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
