use crate::{flatten_stereo, Sample};

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

    /// Sample every `interval` seconds starting at `offset` from the cursor
    ///
    /// `interval` and `offset` may be negative.
    fn sample(&self, interval: f32, out: &mut [Self::Frame]);

    /// Seconds until data runs out
    ///
    /// May be infinite for unbounded signals, or negative after advancing past the end.
    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}

/// Adapts a mono signal to output stereo by duplicating its output
pub struct MonoToStereo<T>(T);

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

    fn remaining(&self) -> f32 {
        self.0.remaining()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

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
