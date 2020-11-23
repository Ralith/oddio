use crate::Sample;

/// An audio signal with a cursor and local random access
///
/// To ensure glitch-free audio, *none* of these methods should perform any operation that may
/// wait. This includes locks, memory allocation or freeing, and even unbounded compare-and-swap
/// loops.
///
/// Note that all methods take `&self`, even when side-effects might be expected. Implementers are
/// expected to rely on interior mutability. This allows `Source`s to be accessed from multiple
/// threads, permitting e.g. the use of atomics for live controls.
pub trait Source {
    /// Helper returned by `sample` to expose a range of frames
    type Batch: Batch<Self>;

    /// Update internal state from controls, if any
    fn update(&self) -> Action;

    /// Construct a sampler around `t` relative to the internal cursor, covering `dt` seconds
    ///
    /// For best precision, `dt` should be small.
    fn sample(&self, t: f32, dt: f32) -> Self::Batch;

    /// Advance time by `dt` seconds, which may be negative
    ///
    /// Future calls to `sample` will behave as if `dt` were added to the argument, potentially with
    /// extra precision
    fn advance(&self, dt: f32);

    //
    // Helpers
    //

    /// Convert a source from mono to stereo by duplicating its output across both channels
    fn into_stereo(self) -> MonoToStereo<Self>
    where
        Self: Sized,
        Self::Batch: Batch<Self, Frame = Sample>,
    {
        MonoToStereo(self)
    }
}

/// Pseudo-iterator over a sequence of frames
///
/// A dedicated trait allows us to work around the absence of GATs.
pub trait Batch<T: ?Sized> {
    /// Type of frames yielded by `get`, e.g. `[Sample; 2]` for stereo.
    type Frame;

    /// Fetch a frame in the neighborhood of the batch
    ///
    /// `t = 0` represents the start of the batch, and `t = 1` the end, but values out of that range
    /// are permitted.
    fn get(&self, source: &T, t: f32) -> Self::Frame;
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

impl<T: Source> Source for MonoToStereo<T>
where
    T::Batch: Batch<T, Frame = Sample>,
{
    type Batch = MonoToStereoBatch<T::Batch>;

    fn update(&self) -> Action {
        self.0.update()
    }

    fn sample(&self, t: f32, dt: f32) -> MonoToStereoBatch<T::Batch> {
        MonoToStereoBatch(self.0.sample(t, dt))
    }

    fn advance(&self, dt: f32) {
        self.0.advance(dt);
    }
}

/// Batch of stereo samples produced from a mono signal
pub struct MonoToStereoBatch<T>(pub T);

impl<T> Batch<MonoToStereo<T>> for MonoToStereoBatch<T::Batch>
where
    T: Source,
    T::Batch: Batch<T, Frame = Sample>,
{
    type Frame = [Sample; 2];
    fn get(&self, source: &MonoToStereo<T>, t: f32) -> Self::Frame {
        let x = self.0.get(&source.0, t);
        [x, x]
    }
}

/// Type-erased source suitable for stereo mixing
pub(crate) trait Mix {
    unsafe fn mix(&self, sample_duration: f32, out: &mut [[Sample; 2]]) -> Action;
}

impl<T: Source> Mix for T
where
    T::Batch: Batch<T, Frame = [Sample; 2]>,
{
    unsafe fn mix(&self, sample_duration: f32, out: &mut [[Sample; 2]]) -> Action {
        let act = self.update();
        if matches!(act, Action::Drop) {
            return act;
        }
        let dt = sample_duration * out.len() as f32;
        let step = 1.0 / out.len() as f32;
        let batch = self.sample(0.0, dt);
        for (i, x) in out.iter_mut().enumerate() {
            let t = i as f32 * step;
            let frame = batch.get(self, t);
            x[0] += frame[0];
            x[1] += frame[1];
        }
        self.advance(dt);
        act
    }
}
