use crate::{frame, Sample, Signal};
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

    /// Get the recorded signal at a certain sample, relative to the *write* cursor. `t` must be
    /// negative.
    pub fn sample(&self, rate: u32, t: f32) -> f32 {
        debug_assert!(t < 0.0, "samples must lie in the past");
        debug_assert!(
            ((t * rate as f32).abs().ceil() as usize) < self.buffer.len(),
            "samples must lie less than a buffer period in the past"
        );
        let s = (self.write + t * rate as f32).rem_euclid(self.buffer.len() as f32);
        let x0 = s.trunc() as usize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        frame::lerp(&a, &b, fract)
    }

    fn get(&self, sample: usize) -> f32 {
        if sample >= self.buffer.len() {
            return 0.0;
        }
        self.buffer[sample]
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
    }

    #[test]
    fn wrap() {
        let mut r = Ring::new(4);
        let s = TimeSignal(Cell::new(1.0));

        r.write(&s, 1, 3.0);
        assert_eq!(r.buffer[..], [1.0, 2.0, 3.0, 0.0]);

        r.write(&s, 1, 3.0);
        assert_eq!(r.buffer[..], [5.0, 6.0, 3.0, 4.0]);
    }
}
