use crate::Sample;

/// A single frame of audio data, encoding one sample for each channel
pub trait Frame {
    /// A frame with zeroes in every channel
    const ZERO: Self;

    /// Access the frame's channels
    fn channels(&self) -> &[Sample];

    /// Mutably access the frame's channels
    fn channels_mut(&mut self) -> &mut [Sample];
}

#[inline]
fn map<T: Frame>(x: &T, mut f: impl FnMut(Sample) -> Sample) -> T {
    let mut out = T::ZERO;
    for (&x, o) in x.channels().iter().zip(out.channels_mut()) {
        *o = f(x);
    }
    out
}

#[inline]
fn bimap<T: Frame>(x: &T, y: &T, mut f: impl FnMut(Sample, Sample) -> Sample) -> T {
    let mut out = T::ZERO;
    for ((&x, &y), o) in x
        .channels()
        .iter()
        .zip(y.channels())
        .zip(out.channels_mut())
    {
        *o = f(x, y);
    }
    out
}

#[inline]
pub(crate) fn lerp<T: Frame>(a: &T, b: &T, t: f32) -> T {
    bimap(a, b, |a, b| a + t * (b - a))
}

#[inline]
pub(crate) fn mix<T: Frame>(a: &T, b: &T) -> T {
    bimap(a, b, |a, b| a + b)
}

#[inline]
pub(crate) fn scale<T: Frame>(x: &T, factor: f32) -> T {
    map(x, |x| x * factor)
}

impl Frame for Sample {
    const ZERO: Sample = 0.0;

    #[inline]
    fn channels(&self) -> &[Sample] {
        core::slice::from_ref(self)
    }

    #[inline]
    fn channels_mut(&mut self) -> &mut [Sample] {
        core::slice::from_mut(self)
    }
}

impl<const N: usize> Frame for [Sample; N] {
    const ZERO: Self = [0.0; N];

    #[inline]
    fn channels(&self) -> &[Sample] {
        self.as_ref()
    }

    #[inline]
    fn channels_mut(&mut self) -> &mut [Sample] {
        self.as_mut()
    }
}
