use std::cell::RefCell;

use crate::{set, ErasedSource, Frame, Handle, Set, SetHandle, Source, StridedMut};

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
pub struct MixerHandle<T>(SetHandle<ErasedSource<T>>);

impl<T> MixerHandle<T> {
    /// Begin playing `source`, returning a handle controlling its playback
    ///
    /// Finished sources are automatically stopped, and their storage reused for future `play`
    /// calls.
    pub fn play<S>(&mut self, source: S) -> Handle<S>
    where
        S: Source<Frame = T> + Send + 'static,
    {
        let (handle, erased) = Handle::new(source);
        self.0.insert(erased);
        handle
    }
}

/// A [`Source`] that mixes a dynamic set of [`Source`]s, controlled by a [`MixerHandle`]
///
/// Constructed by calling [`mixer`].
pub struct Mixer<T>(RefCell<Inner<T>>);

struct Inner<T> {
    set: Set<ErasedSource<T>>,
    buffer: Box<[T]>,
}

impl<T: Frame> Source for Mixer<T> {
    type Frame = T;

    fn sample(&self, offset: f32, sample_duration: f32, mut out: StridedMut<'_, Self::Frame>) {
        let this = &mut *self.0.borrow_mut();
        this.set.update();

        for o in &mut out {
            *o = T::ZERO;
        }

        for i in (0..this.set.len()).rev() {
            let source = &this.set[i];
            if source.remaining() < 0.0 {
                source.stop();
            }
            if source.is_stopped() {
                this.set.remove(i);
                continue;
            }

            // Sample into `buffer`, then mix into `out`
            let mut iter = out.iter_mut();
            let mut i = 0;
            while iter.len() > 0 {
                let n = iter.len().min(this.buffer.len());
                let staging = &mut this.buffer[..n];
                source.sample(
                    offset + i as f32 * sample_duration,
                    sample_duration,
                    staging.into(),
                );
                for (staged, o) in staging.iter().zip(&mut iter) {
                    *o = o.mix(staged);
                }
                i += n;
            }
        }
    }

    fn advance(&self, dt: f32) {
        let this = self.0.borrow_mut();
        for source in this.set.iter() {
            source.advance(dt);
        }
    }

    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}
