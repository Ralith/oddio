use crate::{flatten_stereo, Filter, Sample};

/// An audio signal
///
/// This interface is intended for use only from the code actually generating an audio signal for
/// output. For example, in a real-time application, `Signal`s will typically be owned by the
/// real-time audio thread and not directly accessible from elsewhere. Access to an active signal
/// for other purposes (e.g. to adjust parameters) is generally through [`Handle`](crate::Handle),
/// using signal-specific interfaces that implement wait-free inter-thread communication.
///
/// To ensure glitch-free audio, none of these methods should perform any operation that may
/// wait. This includes locks, memory allocation or freeing, and even unbounded compare-and-swap
/// loops.
pub trait Signal {
    /// Type of frames yielded by `sample`, e.g. `[Sample; 2]` for stereo
    type Frame;

    /// Sample frames separated by `interval` seconds each
    fn sample(&self, interval: f32, out: &mut [Self::Frame]);

    /// Whether future calls to `sample` with a nonnegative `interval` will only produce zeroes
    ///
    /// Commonly used to determine when a `Signal` can be discarded.
    #[inline]
    fn is_finished(&self) -> bool {
        false
    }

    /// Called when the signal's handle is dropped
    ///
    /// Useful for e.g. allowing [`Stream`](crate::Stream) to clean itself when no more data can be
    /// supplied
    #[inline]
    fn handle_dropped(&self) {}
}

impl<T: Signal + ?Sized> Signal for alloc::boxed::Box<T> {
    type Frame = T::Frame;

    fn sample(&self, interval: f32, out: &mut [T::Frame]) {
        (**self).sample(interval, out);
    }

    #[inline]
    fn is_finished(&self) -> bool {
        (**self).is_finished()
    }

    #[inline]
    fn handle_dropped(&self) {
        (**self).handle_dropped();
    }
}

/// Audio signals which support seeking
///
/// Should only be implemented for signals which are defined deterministically in terms of absolute
/// sample time. Nondeterministic or stateful behavior may produce audible glitches in downstream
/// code.
pub trait Seek: Signal {
    /// Shift the starting point of the next `sample` call by `seconds`
    fn seek(&self, seconds: f32);
}

impl<T: Seek + ?Sized> Seek for alloc::boxed::Box<T> {
    #[inline]
    fn seek(&self, seconds: f32) {
        (**self).seek(seconds);
    }
}

/// Adapts a mono signal to output stereo by duplicating its output
pub struct MonoToStereo<T: ?Sized>(T);

impl<T> MonoToStereo<T> {
    /// Adapt `signal` from mono to stereo
    pub fn new(signal: T) -> Self {
        Self(signal)
    }
}

impl<T: Signal<Frame = Sample>> Signal for MonoToStereo<T> {
    type Frame = [Sample; 2];

    fn sample(&self, interval: f32, out: &mut [[Sample; 2]]) {
        let n = out.len();
        let buf = flatten_stereo(out);
        self.0.sample(interval, &mut buf[..n]);
        for i in (0..buf.len()).rev() {
            buf[i] = buf[i / 2];
        }
    }

    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    #[inline]
    fn handle_dropped(&self) {
        self.0.handle_dropped();
    }
}

impl<T: ?Sized> Filter for MonoToStereo<T> {
    type Inner = T;

    fn inner(&self) -> &Self::Inner {
        &self.0
    }
}

impl<T: Seek + Signal<Frame = Sample>> Seek for MonoToStereo<T> {
    fn seek(&self, seconds: f32) {
        self.0.seek(seconds)
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use super::*;

    struct CountingSignal(Cell<u32>);

    impl Signal for CountingSignal {
        type Frame = Sample;
        fn sample(&self, _: f32, out: &mut [Sample]) {
            for x in out {
                let i = self.0.get();
                *x = i as f32;
                self.0.set(i + 1);
            }
        }
    }

    #[test]
    fn mono_to_stereo() {
        let signal = MonoToStereo::new(CountingSignal(Cell::new(0)));
        let mut buf = [[0.0; 2]; 4];
        signal.sample(1.0, (&mut buf[..]).into());
        assert_eq!(buf, [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);
    }
}
