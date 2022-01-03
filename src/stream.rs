//! Streaming audio support

use core::cell::{Cell, RefCell};

use crate::{frame, math::Float, spsc, Controlled, Frame, Signal};

/// Dynamic audio from an external source
pub struct Stream<T> {
    send: RefCell<spsc::Sender<T>>,
    rate: u32,
    inner: RefCell<spsc::Receiver<T>>,
    /// Offset of t=0 from the start of the buffer, in frames
    t: Cell<f32>,
    /// Whether the handle has been dropped
    closed: Cell<bool>,
}

impl<T> Stream<T> {
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
            closed: Cell::new(false),
        }
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
        let inner = self.inner.borrow();
        if sample >= inner.len() {
            return T::ZERO;
        }
        inner[sample]
    }

    fn sample_single(&self, s: f32) -> T
    where
        T: Frame + Copy,
    {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        frame::lerp(&a, &b, fract)
    }

    fn advance(&self, dt: f32) {
        let mut inner = self.inner.borrow_mut();
        let t = (self.t.get() + dt * self.rate as f32).min((inner.len()) as f32);
        inner.release(t as usize);
        self.t.set(t.fract());
    }
}

impl<T: Frame + Copy> Signal for Stream<T> {
    type Frame = T;

    fn sample(&self, interval: f32, out: &mut [T]) {
        self.inner.borrow_mut().update();
        let s0 = self.t.get();
        let ds = interval * self.rate as f32;

        for (i, o) in out.iter_mut().enumerate() {
            *o = self.sample_single(s0 + ds * i as f32);
        }
        self.advance(interval * out.len() as f32);
    }

    fn remaining(&self) -> f32 {
        if !self.closed.get() {
            return f32::INFINITY;
        }
        let t = self.t.get();
        self.inner.borrow_mut().update();
        let total_seconds = self.inner.borrow().len() as f32 / self.rate as f32;
        total_seconds - t
    }

    fn handle_dropped(&self) {
        self.closed.set(true);
    }
}

/// Thread-safe control for a [`Stream`]
pub struct StreamControl<'a, T>(&'a Stream<T>);

unsafe impl<'a, T> Controlled<'a> for Stream<T>
where
    T: 'static,
{
    type Control = StreamControl<'a, T>;

    unsafe fn make_control(signal: &'a Stream<T>) -> Self::Control {
        StreamControl(signal)
    }
}

impl<'a, T> StreamControl<'a, T> {
    /// Add more samples. Returns the number of samples read.
    pub fn write(&mut self, samples: &[T]) -> usize
    where
        T: Copy,
    {
        self.0.send.borrow_mut().send_from_slice(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn assert_out(stream: &Stream<f32>, expected: &[f32]) {
        let mut output = vec![0.0; expected.len()];
        stream.sample(1.0, &mut output);
        assert_eq!(output, expected);
    }

    #[test]
    fn smoke() {
        let s = Stream::<f32>::new(1, 3);
        assert_eq!(StreamControl(&s).write(&[1.0, 2.0]), 2);
        assert_eq!(StreamControl(&s).write(&[3.0, 4.0]), 1);
        assert_out(&s, &[1.0, 2.0, 3.0, 0.0, 0.0]);
        assert_eq!(StreamControl(&s).write(&[5.0, 6.0, 7.0, 8.0]), 3);
        assert_out(&s, &[5.0]);
        assert_out(&s, &[6.0, 7.0, 0.0, 0.0]);
        assert_out(&s, &[0.0, 0.0]);
    }

    #[test]
    fn cleanup() {
        let s = Stream::<f32>::new(1, 4);
        assert_eq!(StreamControl(&s).write(&[1.0, 2.0]), 2);
        assert_eq!(s.remaining(), f32::INFINITY);
        s.handle_dropped();
        assert_eq!(s.remaining(), 2.0);
        s.sample(1.0, &mut [0.0]);
        assert_eq!(s.remaining(), 1.0);
        s.sample(1.0, &mut [0.0]);
        assert_eq!(s.remaining(), 0.0);
        s.sample(1.0, &mut [0.0]);
        assert_eq!(s.remaining(), 0.0);
    }
}
