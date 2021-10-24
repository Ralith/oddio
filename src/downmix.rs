use crate::{Filter, Frame, Sample, Signal};

/// Sums all channels together
///
/// Beware that downmixing produces a maximum amplitude equal to the sum of the maximum amplitudes
/// of its inputs. However, scaling the mixed signal back down by that proportion will usually
/// produce a quieter signal than the inputs.
pub struct Downmix<T: ?Sized>(T);

impl<T> Downmix<T> {
    /// Sum together `signal`'s channels
    pub fn new(signal: T) -> Self {
        Self(signal)
    }
}

impl<T: Signal + ?Sized> Signal for Downmix<T>
where
    T::Frame: Frame,
{
    type Frame = Sample;

    fn sample(&self, interval: f32, out: &mut [Sample]) {
        const CHUNK_SIZE: usize = 256;

        let mut buf = [Frame::ZERO; CHUNK_SIZE];
        for chunk in out.chunks_mut(CHUNK_SIZE) {
            self.0.sample(interval, &mut buf);
            for (i, o) in buf.iter_mut().zip(chunk) {
                *o = i.channels().iter().copied().sum();
            }
        }
    }

    fn remaining(&self) -> f32 {
        self.0.remaining()
    }

    fn handle_dropped(&self) {
        self.0.handle_dropped();
    }
}

impl<T: ?Sized> Filter for Downmix<T> {
    type Inner = T;

    fn inner(&self) -> &Self::Inner {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Constant;

    #[test]
    fn smoke() {
        let signal = Downmix::new(Constant::new([1.0, 2.0]));
        let mut out = [0.0; 384];
        signal.sample(1.0, &mut out);
        assert_eq!(out, [3.0; 384]);
    }
}
