//! Streaming audio support

use std::cell::{Cell, UnsafeCell};

use crate::{spsc, Action, Sample, Sampler, Source};

/// Construct an unbounded stream of dynamic audio
///
/// Returns two handles, which can be sent to different threads. This allows the business of
/// obtaining streaming audio to take place without interfering with the low-latency requirements of
/// audio output.
///
/// - `rate` is the stream's sample rate
/// - `past_size` dictates the number of already-played samples to store. Bounds the maximum
///   distance this stream can be heard from for a particular speed of sound.
/// - `future_size` dictates how much storage is allocated for samples yet to be played. Governs the
///   maximum achievable latency. Should be at least large enough to fill one output buffer to avoid
///   constant underruns.
pub fn stream(rate: u32, past_size: usize, future_size: usize) -> (Sender, Receiver) {
    let (send, recv) = spsc::channel(past_size + future_size);
    (
        Sender { inner: send },
        Receiver {
            rate,
            past_size,
            inner: UnsafeCell::new(recv),
            t: Cell::new(0.0),
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
    past_size: usize,
    inner: UnsafeCell<spsc::Receiver<Sample>>,
    /// Offset of current data from the start of the buffer, in samples
    t: Cell<f32>,
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
}

impl Source for Receiver {
    type Sampler = StreamSampler;

    #[inline]
    fn update(&self) -> Action {
        unsafe {
            (*self.inner.get()).update();
        }
        Action::Retain
    }

    #[inline]
    fn sample(&self, t: f32, dt: f32) -> StreamSampler {
        StreamSampler {
            s0: self.t.get() + t * self.rate as f32,
            ds: dt * self.rate as f32,
        }
    }

    #[inline]
    fn advance(&self, dt: f32) {
        // TODO: Clamp such that a configurable amount of data remains at t >= 0, to allow repeating
        // audio rather than gaps
        let inner = unsafe { &mut *self.inner.get() };
        self.t
            .set((self.t.get() + dt * self.rate as f32).min((inner.len() + self.past_size) as f32));
        let excess = (self.t.get() - self.past_size as f32).trunc();
        if excess > 0.0 {
            inner.release(excess as usize);
            self.t.set(self.t.get() - excess);
        }
    }
}

/// Sampler for [`stream`]s
pub struct StreamSampler {
    s0: f32,
    ds: f32,
}

impl Sampler<Receiver> for StreamSampler {
    type Frame = Sample;

    fn get(&self, source: &Receiver, t: f32) -> Sample {
        let s = self.s0 + self.ds * t;
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        source.get(x0) * (1.0 - fract) + source.get(x1) * fract
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_seq(recv: &Receiver, start: f32, seq: &[f32]) {
        for (i, &expected) in seq.iter().enumerate() {
            let actual = recv.sample(start + i as f32, 1.0).get(&recv, 0.0);
            if expected != actual {
                panic!(
                    "expected {:?} from {}, got {:?} from {}",
                    seq,
                    start,
                    unsafe { &*recv.inner.get() },
                    -recv.t.get()
                );
            }
        }
    }

    #[test]
    fn release_old() {
        let (mut send, recv) = stream(1, 4, 4);
        assert_eq!(send.write(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(send.write(&[4.0, 5.0]), 2);
        recv.update();
        assert_seq(&recv, -1.0, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);

        recv.advance(1.0);
        assert_seq(&recv, -2.0, &[0.0, 1.0, 2.0, 3.0]);

        // 1.0 drops off the back
        recv.advance(4.0);
        assert_seq(&recv, -5.0, &[0.0, 2.0]);

        // Only 4 slots available to the writer
        assert_eq!(send.write(&[6.0, 7.0, 8.0, 9.0, 10.0]), 4);
        recv.update();
        assert_seq(&recv, -1.0, &[5.0, 6.0, 7.0, 8.0, 9.0, 0.0]);
    }
}
