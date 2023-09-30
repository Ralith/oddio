//! Streaming audio support

use crate::{frame, math::Float, spsc, Frame, Signal};

/// Dynamic audio from an external source
pub struct Stream<T> {
    rate: u32,
    inner: spsc::Receiver<T>,
    /// Offset of t=0 from the start of the buffer, in frames
    t: f32,
    /// Whether `inner` will receive no further updates
    stopping: bool,
}

impl<T> Stream<T> {
    /// Construct a stream of dynamic audio
    ///
    /// Samples can be appended to the stream through its [`StreamControl`]. This allows the
    /// business of obtaining streaming audio, e.g. from a streaming decoder or the network, to take
    /// place without interfering with the low-latency requirements of audio output.
    ///
    /// - `rate` is the stream's sample rate
    /// - `size` dictates the maximum number of buffered frames
    pub fn new(rate: u32, size: usize) -> (StreamControl<T>, Self) {
        let (send, recv) = spsc::channel(size);
        let signal = Self {
            rate,
            inner: recv,
            t: 0.0,
            stopping: false,
        };
        let control = StreamControl(send);
        (control, signal)
    }

    #[inline]
    fn get(&self, sample: isize) -> T
    where
        T: Frame + Copy,
    {
        if sample < 0 {
            return T::ZERO;
        }
        let sample = sample as usize;
        if sample >= self.inner.len() {
            return T::ZERO;
        }
        self.inner[sample]
    }

    fn sample_single(&self, s: f32) -> T
    where
        T: Frame + Copy,
    {
        let x0 = s.trunc() as isize;
        let fract = s.fract();
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        frame::lerp(&a, &b, fract)
    }

    fn advance(&mut self, dt: f32) {
        let next = self.t + dt * self.rate as f32;
        let t = next.min(self.inner.len() as f32);
        self.inner.release(t as usize);
        self.t = t.fract();
    }
}

impl<T: Frame + Copy> Signal for Stream<T> {
    type Frame = T;

    fn sample(&mut self, interval: f32, out: &mut [T]) {
        self.inner.update();
        if self.inner.is_closed() {
            self.stopping = true;
        }
        let s0 = self.t;
        let ds = interval * self.rate as f32;

        for (i, o) in out.iter_mut().enumerate() {
            *o = self.sample_single(s0 + ds * i as f32);
        }
        self.advance(interval * out.len() as f32);
    }

    #[allow(clippy::float_cmp)]
    fn is_finished(&self) -> bool {
        self.stopping && self.t == self.inner.len() as f32
    }
}

/// Thread-safe control for a [`Stream`]
pub struct StreamControl<T>(spsc::Sender<T>);

impl<T> StreamControl<T> {
    /// Lower bound to the number of samples that the next `write` call will successfully consume
    pub fn free(&mut self) -> usize {
        self.0.free()
    }

    /// Add more samples. Returns the number of samples consumed. Remaining samples should be passed
    /// in again in a future call.
    pub fn write(&mut self, samples: &[T]) -> usize
    where
        T: Copy,
    {
        self.0.send_from_slice(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn assert_out(stream: &mut Stream<f32>, expected: &[f32]) {
        let mut output = vec![0.0; expected.len()];
        stream.sample(1.0, &mut output);
        assert_eq!(output, expected);
    }

    #[test]
    fn smoke() {
        let (mut c, mut s) = Stream::<f32>::new(1, 3);
        assert_eq!(c.write(&[1.0, 2.0]), 2);
        assert_eq!(c.write(&[3.0, 4.0]), 1);
        assert_out(&mut s, &[1.0, 2.0, 3.0, 0.0, 0.0]);
        assert_eq!(c.write(&[5.0, 6.0, 7.0, 8.0]), 3);
        assert_out(&mut s, &[5.0]);
        assert_out(&mut s, &[6.0, 7.0, 0.0, 0.0]);
        assert_out(&mut s, &[0.0, 0.0]);
    }

    #[test]
    fn cleanup() {
        let (mut c, mut s) = Stream::<f32>::new(1, 4);
        assert_eq!(c.write(&[1.0, 2.0]), 2);
        assert!(!s.is_finished());
        drop(c);
        assert!(!s.is_finished());
        s.sample(1.0, &mut [0.0]);
        assert!(!s.is_finished());
        s.sample(1.0, &mut [0.0]);
        assert!(s.is_finished());
        s.sample(1.0, &mut [0.0]);
        assert!(s.is_finished());
    }
}
