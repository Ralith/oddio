/// A random-access audio signal with a cursor
pub trait Source {
    /// Sample rate
    fn rate(&self) -> u32;

    /// Get a sample at time `t`, which may be negative or past the end
    ///
    /// A listener fetching `n` samples with zero delay will sample in the range `0..n`. More
    /// distant listeners will sample in ranges that begin in the negatives.
    fn sample(&self, t: f32) -> f32;

    /// Advance time by `dt` samples, which may be negative
    ///
    /// Future calls to `sample` will behave as if `dt` were added to the argument, potentially with
    /// extra precision
    fn advance(&mut self, dt: f32);

    /// Time, in samples, from the current instant to the end
    ///
    /// May be negative or infinite.
    fn remaining(&self) -> f32;
}
