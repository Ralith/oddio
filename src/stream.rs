//! Streaming audio support

use crate::{spsc, Sample, Source};

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
            inner: recv,
            t: 0.0,
        },
    )
}

/// Handle for submitting new samples to a stream
pub struct Sender {
    inner: spsc::Sender<Sample>,
}

impl Sender {
    /// Add more samples to play
    pub fn write(&mut self, samples: &[Sample]) -> usize {
        self.inner.send_from_slice(samples)
    }
}

/// Handle for sampling from a stream
pub struct Receiver {
    rate: u32,
    past_size: usize,
    inner: spsc::Receiver<Sample>,
    /// Offset of current data from the start of the buffer, in samples
    t: f32,
}

impl Receiver {
    #[inline]
    fn get(&self, sample: isize) -> f32 {
        if sample < 0 {
            return 0.0;
        }
        let sample = sample as usize;
        if sample >= self.inner.len() {
            return 0.0;
        }
        self.inner[sample]
    }
}

impl Source for Receiver {
    #[inline]
    fn rate(&self) -> u32 {
        self.rate
    }

    #[inline]
    fn sample(&self, t: f32) -> f32 {
        let t = self.t + t;
        let x0 = t.trunc() as isize;
        let fract = t.fract() as f32;
        let x1 = x0 + 1;
        self.get(x0) * (1.0 - fract) + self.get(x1) * fract
    }

    #[inline]
    fn advance(&mut self, samples: f32) {
        // TODO: Clamp such that a configurable amount of data remains at t >= 0, to allow repeating
        // audio rather than gaps
        self.t = (self.t + samples).min((self.inner.len() + self.past_size) as f32);
        let excess = (self.t - self.past_size as f32).trunc();
        if excess > 0.0 {
            self.inner.release(excess as usize);
            self.t -= excess;
        }
    }

    #[inline]
    fn remaining(&self) -> f32 {
        f32::INFINITY
    }

    /// Fetch new data from the sender
    #[inline]
    fn prepare(&mut self) {
        self.inner.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_seq(recv: &Receiver, start: f32, seq: &[f32]) {
        for (i, &expected) in seq.iter().enumerate() {
            let actual = recv.sample(start + i as f32);
            if expected != actual {
                panic!(
                    "expected {:?} from {}, got {:?} from {}",
                    seq, start, recv.inner, -recv.t
                );
            }
        }
    }

    #[test]
    fn release_old() {
        let (mut send, mut recv) = stream(1, 4, 4);
        assert_eq!(send.write(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(send.write(&[4.0, 5.0]), 2);
        recv.prepare();
        assert_seq(&recv, -1.0, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);

        recv.advance(1.0);
        assert_seq(&recv, -2.0, &[0.0, 1.0, 2.0, 3.0]);

        // 1.0 drops off the back
        recv.advance(4.0);
        assert_seq(&recv, -5.0, &[0.0, 2.0]);

        // Only 4 slots available to the writer
        assert_eq!(send.write(&[6.0, 7.0, 8.0, 9.0, 10.0]), 4);
        recv.prepare();
        assert_seq(&recv, -1.0, &[5.0, 6.0, 7.0, 8.0, 9.0, 0.0]);
    }
}
