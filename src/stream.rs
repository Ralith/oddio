//! Streaming audio support

use std::cell::{Cell, RefCell};

use crate::{spsc, Controlled, Sample, Signal};

/// Dynamic audio from an external source
pub struct Stream {
    send: RefCell<spsc::Sender<Sample>>,
    rate: u32,
    inner: RefCell<spsc::Receiver<Sample>>,
    /// Offset of t=0 from the start of the buffer, in samples
    t: Cell<f32>,
}

impl Stream {
    /// Construct a stream of dynamic audio
    ///
    /// Samples can be appended to the stream through its [`Handle`](crate::Handle). This allows the
    /// business of obtaining streaming audio, e.g. from a streaming decoder or the network, to take
    /// place without interfering with the low-latency requirements of audio output.
    ///
    /// - `rate` is the stream's sample rate
    /// - `size` dictates the maximum number of buffered frames
    pub fn new(rate: u32, size: usize) -> Self {
        let (send, recv) = spsc::channel(size);
        Self {
            send: RefCell::new(send),
            rate,
            inner: RefCell::new(recv),
            t: Cell::new(0.0),
        }
    }

    #[inline]
    fn get(&self, sample: isize) -> f32 {
        if sample < 0 {
            return 0.0;
        }
        let sample = sample as usize;
        let inner = self.inner.borrow();
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
        let mut inner = self.inner.borrow_mut();
        let t = (self.t.get() + dt * self.rate as f32).min((inner.len()) as f32);
        inner.release(t as usize);
        self.t.set(t.fract());
    }
}

impl Signal for Stream {
    // This could be made generic if needed.
    type Frame = Sample;

    fn sample(&self, interval: f32, out: &mut [Sample]) {
        self.inner.borrow_mut().update();
        let s0 = self.t.get();
        let ds = interval * self.rate as f32;

        for (i, o) in out.iter_mut().enumerate() {
            *o = self.sample_single(s0 + ds * i as f32);
        }
        self.advance(interval * out.len() as f32);
    }

    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }
}

/// Thread-safe control for a [`Stream`]
pub struct StreamControl<'a>(&'a Stream);

unsafe impl<'a> Controlled<'a> for Stream {
    type Control = StreamControl<'a>;

    unsafe fn make_control(signal: &'a Stream) -> Self::Control {
        StreamControl(signal)
    }
}

impl<'a> StreamControl<'a> {
    /// Add more samples. Returns the number of samples read.
    pub fn write(&mut self, samples: &[Sample]) -> usize {
        self.0.send.borrow_mut().send_from_slice(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_out(stream: &Stream, expected: &[Sample]) {
        let mut output = vec![0.0; expected.len()];
        stream.sample(1.0, &mut output);
        assert_eq!(output, expected);
    }

    #[test]
    fn smoke() {
        let s = Stream::new(1, 3);
        assert_eq!(StreamControl(&s).write(&[1.0, 2.0]), 2);
        assert_eq!(StreamControl(&s).write(&[3.0, 4.0]), 1);
        assert_out(&s, &[1.0, 2.0, 3.0, 0.0, 0.0]);
        assert_eq!(StreamControl(&s).write(&[5.0, 6.0, 7.0, 8.0]), 3);
        assert_out(&s, &[5.0]);
        assert_out(&s, &[6.0, 7.0, 0.0, 0.0]);
        assert_out(&s, &[0.0, 0.0]);
    }
}
