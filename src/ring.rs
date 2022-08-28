use crate::{frame, math::Float, Sample, Signal};
use alloc::{boxed::Box, vec};

pub struct Ring {
    buffer: Box<[Sample]>,
    write: f32,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity].into(),
            write: 0.0,
        }
    }

    /// Fill buffer from `signal`
    pub fn write<S: Signal<Frame = Sample> + ?Sized>(&mut self, signal: &S, rate: u32, dt: f32) {
        debug_assert!(
            dt * rate as f32 <= self.buffer.len() as f32,
            "output range exceeds capacity"
        );
        let end = (self.write + dt * rate as f32) % self.buffer.len() as f32;

        let start_idx = self.write.ceil() as usize;
        let end_idx = end.ceil() as usize;
        let interval = 1.0 / rate as f32;
        if end_idx > start_idx {
            signal.sample(interval, &mut self.buffer[start_idx..end_idx]);
        } else {
            signal.sample(interval, &mut self.buffer[start_idx..]);
            signal.sample(interval, &mut self.buffer[..end_idx]);
        }

        self.write = end;
    }

    /// Advance write cursor by `dt` given internal sample rate `rate`, as if writing a `Signal`
    /// that produces only zeroes
    pub fn delay(&mut self, rate: u32, dt: f32) {
        self.write = (self.write + rate as f32 * dt) % self.buffer.len() as f32;
    }

    /// Get the recorded signal at a certain range, relative to the *write* cursor. `t` must be
    /// negative.
    pub fn sample(&self, rate: u32, t: f32, interval: f32, out: &mut [Sample]) {
        debug_assert!(t < 0.0, "samples must lie in the past");
        debug_assert!(
            ((t * rate as f32).abs().ceil() as usize) < self.buffer.len(),
            "samples must lie less than a buffer period in the past"
        );
        let mut offset = (self.write + t * rate as f32).rem_euclid(self.buffer.len() as f32);
        let ds = interval * rate as f32;
        for o in out.iter_mut() {
            let trunc = unsafe { offset.to_int_unchecked::<usize>() };
            let fract = offset - trunc as f32;
            let x = trunc;
            let (a, b) = if x < self.buffer.len() - 1 {
                (self.buffer[x], self.buffer[x + 1])
            } else if x < self.buffer.len() {
                (self.buffer[x], self.buffer[0])
            } else {
                let x = x % self.buffer.len();
                offset = x as f32 + fract;
                if x < self.buffer.len() - 1 {
                    (self.buffer[x], self.buffer[x + 1])
                } else {
                    (self.buffer[x], self.buffer[0])
                }
            };
            *o = frame::lerp(&a, &b, fract);
            offset += ds;
        }
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use super::*;

    struct TimeSignal(Cell<f32>);

    impl Signal for TimeSignal {
        type Frame = Sample;
        fn sample(&self, interval: f32, out: &mut [Sample]) {
            for x in out {
                let t = self.0.get();
                *x = t as f32;
                self.0.set(t + interval);
            }
        }
    }

    fn assert_out(r: &Ring, rate: u32, t: f32, interval: f32, expected: &[f32]) {
        let mut output = vec![0.0; expected.len()];
        r.sample(rate, t, interval, &mut output);
        assert_eq!(&output, expected);
    }

    #[test]
    fn fill() {
        let mut r = Ring::new(4);
        let s = TimeSignal(Cell::new(1.0));

        r.write(&s, 1, 1.0);
        assert_eq!(r.write, 1.0);
        assert_eq!(r.buffer[..], [1.0, 0.0, 0.0, 0.0]);

        r.write(&s, 1, 2.0);
        assert_eq!(r.write, 3.0);
        assert_eq!(r.buffer[..], [1.0, 2.0, 3.0, 0.0]);

        assert_out(&r, 1, -1.5, 1.0, &[2.5, 1.5]);
        assert_out(&r, 1, -1.5, 0.25, &[2.5, 2.75, 3.0, 2.25]);
    }

    #[test]
    fn wrap() {
        let mut r = Ring::new(4);
        let s = TimeSignal(Cell::new(1.0));

        r.write(&s, 1, 3.0);
        assert_eq!(r.buffer[..], [1.0, 2.0, 3.0, 0.0]);

        r.write(&s, 1, 3.0);
        assert_eq!(r.buffer[..], [5.0, 6.0, 3.0, 4.0]);

        assert_out(&r, 1, -2.75, 0.5, &[4.25, 4.75, 5.25, 5.75, 5.25, 3.75]);
    }
}
