use crate::Sample;

/// An audio signal
///
/// To ensure glitch-free audio, *none* of these methods should perform any operation that may
/// wait. This includes locks, memory allocation or freeing, and even unbounded compare-and-swap
/// loops.
///
/// Note that all methods take `&self`, even when side-effects might be expected. Implementers are
/// expected to rely on interior mutability. This allows `Source`s to be accessed from multiple
/// threads, permitting e.g. the use of atomics for live controls.
pub trait Source {
    /// Type of frames, e.g. `[Sample; 2]` for stereo.
    type Frame;

    /// Update internal state from controls, if any
    fn update(&self) -> Action;

    /// Pass `count` samples each covering `sample_duration` into `out`. Should be
    /// wait-free. Implicitly advances time.
    fn sample(&self, sample_duration: f32, count: usize, out: impl FnMut(usize, Self::Frame));

    //
    // Helpers
    //

    /// Convert a source from mono to stereo by duplicating its output across both channels
    fn into_stereo(self) -> MonoToStereo<Self>
    where
        Self: Sized + Source<Frame = Sample>,
    {
        MonoToStereo(self)
    }
}

/// An audio signal with a time cursor and the ability to sample behind it
pub trait Seek: Source {
    /// Like `Source::sample`, but does not advance time, and accepts an arbitrary delay subtracted
    /// from the current time.
    fn sample_at(
        &self,
        sample_duration: f32,
        count: usize,
        delay: f32,
        out: impl FnMut(usize, Self::Frame),
    );

    /// Advance time by `dt` seconds, which may be negative
    ///
    /// Future calls to `sample_at` will behave as if `dt` were added to the argument, potentially
    /// with extra precision
    fn advance(&self, dt: f32);
}

/// Action for the worker thread to take after invoking `Source::update`
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Action {
    /// Continue playing the source
    Retain,
    /// Stop the source and allow its resources to be reused
    Drop,
}

impl Default for Action {
    fn default() -> Self {
        Action::Retain
    }
}

/// Adapt a mono source to output stereo by duplicating its output
pub struct MonoToStereo<T>(pub T);

impl<T: Source<Frame = Sample>> Source for MonoToStereo<T> {
    type Frame = [Sample; 2];

    fn update(&self) -> Action {
        self.0.update()
    }

    fn sample(&self, sample_duration: f32, count: usize, mut out: impl FnMut(usize, Self::Frame)) {
        self.0.sample(sample_duration, count, |i, x| out(i, [x, x]))
    }
}

/// Type-erased source suitable for stereo mixing
pub(crate) trait Mix {
    unsafe fn mix(&self, sample_duration: f32, out: &mut [[Sample; 2]]) -> Action;
}

impl<T: Source<Frame = [Sample; 2]>> Mix for T {
    unsafe fn mix(&self, sample_duration: f32, out: &mut [[Sample; 2]]) -> Action {
        let act = self.update();
        if matches!(act, Action::Drop) {
            return act;
        }
        self.sample(sample_duration, out.len(), |i, x| {
            out[i][0] += x[0];
            out[i][1] += x[1];
        });
        act
    }
}
