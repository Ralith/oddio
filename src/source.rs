use crate::{split_stereo, Sample, StridedMut};

/// An audio signal with a cursor
///
/// To ensure glitch-free audio, none of these methods should perform any operation that may
/// wait. This includes locks, memory allocation or freeing, and even unbounded compare-and-swap
/// loops.
pub trait Source {
    /// Type of frames yielded by `get`, e.g. `[Sample; 2]` for stereo.
    type Frame;

    /// Sample a period of `sample_length * out.len()` seconds starting at `offset` from the cursor.
    ///
    /// `sample_length` and `offset` may be negative. Output is written at intervals of `stride`.
    fn sample(&self, offset: f32, sample_length: f32, out: StridedMut<'_, Self::Frame>);

    /// Advance time by `dt` seconds
    ///
    /// Future calls to `sample` will behave as if `dt` were added to `offset`, potentially with
    /// extra precision. Typically invoked after a batch of samples have been taken, with the total
    /// period covered by those samples.
    ///
    /// Note that this method takes `&self`, even though side-effects are expected. Implementers are
    /// expected to rely on interior mutability. This allows `Source`s to be accessed while playing
    /// via [`Handle`](crate::Handle), permitting real-time control with e.g. atomics.
    fn advance(&self, dt: f32);

    /// Seconds until data runs out
    ///
    /// May be infinite for unbounded sources, or negative after advancing past the end. May change
    /// independently of calls to `advance` for sources with dynamic underlying data such as
    /// real-time streams.
    fn remaining(&self) -> f32;

    //
    // Helpers
    //

    /// Convert a source from mono to stereo by duplicating its output across both channels
    fn into_stereo(self) -> MonoToStereo<Self>
    where
        Self: Source<Frame = Sample> + Sized,
    {
        MonoToStereo(self)
    }
}

/// Adapt a mono source to output stereo by duplicating its output
pub struct MonoToStereo<T>(pub T);

impl<T: Source<Frame = Sample>> Source for MonoToStereo<T> {
    type Frame = [Sample; 2];

    fn sample(&self, dt: f32, offset: f32, mut out: StridedMut<'_, Self::Frame>) {
        let [left, _] = split_stereo(&mut out);
        self.0.sample(dt, offset, left);
        for frame in &mut out {
            frame[1] = frame[0];
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

    struct CountingSource(Cell<u32>);

    impl Source for CountingSource {
        type Frame = Sample;
        fn sample(&self, _: f32, _: f32, mut out: StridedMut<'_, Self::Frame>) {
            for x in &mut out {
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
        let source = CountingSource(Cell::new(0)).into_stereo();
        let mut buf = [[0.0; 2]; 4];
        source.sample(1.0, 0.0, (&mut buf[..]).into());
        assert_eq!(buf, [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0], [3.0, 3.0]]);
    }
}
