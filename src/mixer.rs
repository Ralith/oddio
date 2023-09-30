use alloc::{boxed::Box, sync::Arc, vec};
use core::cell::RefCell;

use crate::{frame, set, Controlled, Frame, Handle, Set, SetHandle, Signal};

/// Handle for controlling a [`Mixer`] from another thread
pub struct MixerControl<'a, T>(&'a Mixer<T>);

impl<T> MixerControl<'_, T> {
    /// Begin playing `signal`, returning a handle that can be used to pause or stop it and access
    /// other controls
    ///
    /// Finished signals are automatically stopped, and their storage reused for future `play`
    /// calls.
    ///
    /// The type of signal given determines what additional controls can be used. See the
    /// examples for a detailed guide.
    pub fn play<S>(&mut self, signal: S)
    where
        S: Signal<Frame = T> + Send + 'static,
    {
        self.0.send.borrow_mut().insert(Box::new(signal));
    }
}

/// A [`Signal`] that mixes a dynamic set of [`Signal`]s
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

    fn sample(&mut self, interval: f32, out: &mut [T]) {
        let this = &mut *self.recv.borrow_mut();
        this.set.update();

        for o in out.iter_mut() {
            *o = T::ZERO;
        }

        for i in (0..this.set.len()).rev() {
            let signal = &mut this.set[i];
            if signal.is_finished() {
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

type ErasedSignal<T> = Box<dyn Signal<Frame = T>>;
