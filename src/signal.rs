use crate::{flatten_stereo, Gain, Sample};

/// An audio signal with a cursor
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
    /// Type of frames yielded by `get`, e.g. `[Sample; 2]` for stereo.
    type Frame;

    /// Sample every `sample_interval` seconds starting at `offset` from the cursor.
    ///
    /// `sample_interval` and `offset` may be negative.
    fn sample(&self, offset: f32, sample_interval: f32, out: &mut [Self::Frame]);

    /// Advance time by `dt` seconds
    ///
    /// Future calls to `sample` will behave as if `dt` were added to `offset`, potentially with
    /// extra precision. Typically invoked after a batch of samples have been taken, with the total
    /// period covered by those samples.
    ///
    /// Note that this method takes `&self`, even though side-effects are expected. Implementers are
    /// expected to rely on interior mutability. This allows `Signal`s to be accessed while playing
    /// via [`Handle::control`](crate::Handle::control), permitting real-time control with
    /// e.g. atomics.
    fn advance(&self, dt: f32);

    /// Seconds until data runs out
    ///
    /// May be infinite for unbounded signals, or negative after advancing past the end. May change
    /// independently of calls to `advance` for signals with dynamic underlying data such as
    /// real-time streams.
    fn remaining(&self) -> f32;

    //
    // Helpers
    //

    /// Convert a signal from mono to stereo by duplicating its output across both channels
    fn into_stereo(self) -> MonoToStereo<Self>
    where
        Self: Signal<Frame = Sample> + Sized,
    {
        MonoToStereo(self)
    }

    /// Apply a dynamic gain control
    fn with_gain(self) -> Gain<Self>
    where
        Self: Sized,
    {
        Gain::new(self)
    }
}

/// Adapt a mono signal to output stereo by duplicating its output
pub struct MonoToStereo<T>(pub T);

impl<T: Signal<Frame = Sample>> Signal for MonoToStereo<T> {
    type Frame = [Sample; 2];

    fn sample(&self, dt: f32, offset: f32, out: &mut [[Sample; 2]]) {
        let n = out.len();
        let buf = flatten_stereo(out);
        self.0.sample(dt, offset, &mut buf[..n]);
        for i in (0..buf.len()).rev() {
            buf[i] = buf[i / 2];
        }
    }

    fn advance(&self, dt: f32) {
        self.0.advance(dt);
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
        fn sample(&self, _: f32, _: f32, out: &mut [Sample]) {
            for x in out {
                let i = self.0.get();
                *x = i as f32;
                self.0.set(i + 1);
            }
        }

        fn advance(&self, _: f32) {}

        fn remaining(&self) -> f32 {
            f32::INFINITY
        }
    }

    #[test]
    fn mono_to_stereo() {
        let signal = CountingSignal(Cell::new(0)).into_stereo();
        let mut buf = [[0.0; 2]; 4];
        signal.sample(1.0, 0.0, (&mut buf[..]).into());
        assert_eq!(buf, [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);
    }
}
