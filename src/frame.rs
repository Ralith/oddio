use crate::Sample;

/// A single frame of audio data, encoding one sample for each channel
pub trait Frame {
    /// The neutral value
    const ZERO: Self;

    /// Linearly interpolate the samples of two frames
    fn lerp(&self, other: &Self, t: f32) -> Self;

    /// Mix with `other`
    fn mix(&self, other: &Self) -> Self;

    /// Scale by `factor`
    fn scale(&self, factor: f32) -> Self;
}

impl Frame for Sample {
    const ZERO: Sample = 0.0;

    #[inline]
    fn lerp(&self, other: &Sample, t: f32) -> Sample {
        self + t * (other - self)
    }

    #[inline]
    fn mix(&self, other: &Sample) -> Sample {
        self + other
    }

    #[inline]
    fn scale(&self, factor: f32) -> Sample {
        self * factor
    }
}

impl Frame for [Sample; 2] {
    const ZERO: [Sample; 2] = [0.0; 2];

    #[inline]
    fn lerp(&self, other: &[Sample; 2], t: f32) -> [Sample; 2] {
        [self[0].lerp(&other[0], t), self[1].lerp(&other[1], t)]
    }

    #[inline]
    fn mix(&self, other: &[Sample; 2]) -> [Sample; 2] {
        [self[0] + other[0], self[1] + other[1]]
    }

    #[inline]
    fn scale(&self, factor: f32) -> [Sample; 2] {
        [self[0] * factor, self[1] * factor]
    }
}
