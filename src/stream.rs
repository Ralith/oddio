//! Streaming audio support

use std::cell::{Cell, UnsafeCell};

use crate::{spsc, Sample, Signal};

/// Construct an unbounded stream of dynamic audio
///
/// Returns two handles, which can be sent to different threads. This allows the business of
/// obtaining streaming audio to take place without interfering with the low-latency requirements of
/// audio output.
///
/// - `rate` is the stream's sample rate
/// - `size` dictates the maximum number of buffered frames
pub fn stream(rate: u32, size: usize) -> (Sender, Receiver) {
    let (send, recv) = spsc::channel(size);
    (
        Sender { inner: send },
        Receiver {
            rate,
            inner: UnsafeCell::new(recv),
            t: Cell::new(0.0),
            closed_for: Cell::new(None),
        },
    )
}

/// Handle for submitting new samples to a stream
pub struct Sender {
    inner: spsc::Sender<Sample>,
}

impl Sender {
    /// Add more samples. Returns the number of samples read.
    pub fn write(&mut self, samples: &[Sample]) -> usize {
        self.inner.send_from_slice(samples)
    }
}

/// Handle for sampling from a stream
pub struct Receiver {
    rate: u32,
    inner: UnsafeCell<spsc::Receiver<Sample>>,
    /// Offset of t=0 from the start of the buffer, in samples
    t: Cell<f32>,
    /// Seconds since the stream ended
    closed_for: Cell<Option<f32>>,
}

impl Receiver {
    #[inline]
    fn get(&self, sample: isize) -> f32 {
        if sample < 0 {
            return 0.0;
        }
        let sample = sample as usize;
        let inner = unsafe { &mut *self.inner.get() };
        if sample >= inner.len() {
            return 0.0;
        }
        inner[sample]
    }

    fn sample_single(&self, s: f32) -> Sample {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        a + fract * (b - a)
    }

    fn advance(&self, dt: f32) {
        let is_closed = unsafe { (*self.inner.get()).is_closed() };
        if is_closed {
            self.closed_for
                .set(Some(self.closed_for.get().unwrap_or(0.0) + dt));
            return;
        }
        let inner = unsafe { &mut *self.inner.get() };
        let t = (self.t.get() + dt * self.rate as f32).min((inner.len()) as f32);
        inner.release(t as usize);
        self.t.set(t.fract());
    }
}

impl Signal for Receiver {
    // This could be made generic if needed.
    type Frame = Sample;

    fn sample(&self, interval: f32, out: &mut [Sample]) {
        unsafe {
            (*self.inner.get()).update();
        }
        let s0 = self.t.get();
        let ds = interval * self.rate as f32;

        for (i, o) in out.iter_mut().enumerate() {
            *o = self.sample_single(s0 + ds * i as f32);
        }
        self.advance(interval * out.len() as f32);
    }

    #[inline]
    fn remaining(&self) -> f32 {
        self.closed_for.get().map_or(f32::INFINITY, |x| -x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_out(stream: &Receiver, expected: &[Sample]) {
        let mut output = vec![0.0; expected.len()];
        stream.sample(1.0, &mut output);
        assert_eq!(output, expected);
    }

    #[test]
    fn smoke() {
        let (mut send, recv) = stream(1, 3);
        assert_eq!(send.write(&[1.0, 2.0]), 2);
        assert_eq!(send.write(&[3.0, 4.0]), 1);
        assert_out(&recv, &[1.0, 2.0, 3.0, 0.0, 0.0]);
        assert_eq!(send.write(&[5.0, 6.0, 7.0, 8.0]), 3);
        assert_out(&recv, &[5.0]);
        assert_out(&recv, &[6.0, 7.0, 0.0, 0.0]);
        assert_out(&recv, &[0.0, 0.0]);
    }
}
