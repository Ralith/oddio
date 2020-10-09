/// Something that produces audio
///
/// Sources are responsible for keeping track its own playback cursor.
pub trait Source {
    /// Sample rate
    fn rate(&self) -> u32;

    /// Get a sample at time `t`
    ///
    /// A listener fetching `n` samples with zero delay will sample in the range `0..n`. More
    /// distant listeners will sample in ranges that begin in the negatives.
    fn sample(&self, t: f32) -> f32;
}
