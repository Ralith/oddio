use crate::alloc::{alloc, boxed::Box, sync::Arc};
use core::{
    convert::TryFrom,
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::atomic::{AtomicIsize, Ordering},
};

use crate::{frame, math::Float, Frame, Seek, Signal};

/// A sequence of static audio frames at a particular sample rate
///
/// Used to store e.g. sound effects decoded from files on disk.
///
/// Dynamically sized type. Typically stored inside an `Arc`, allowing efficient simultaneous use by
/// multiple signals.
#[derive(Debug)]
pub struct Frames<T> {
    rate: f64,
    samples: [T],
}

impl<T> Frames<T> {
    /// Construct samples from existing memory
    pub fn from_slice(rate: u32, samples: &[T]) -> Arc<Self>
    where
        T: Copy,
    {
        let header_layout = alloc::Layout::new::<f64>();
        let (layout, payload_offset) = header_layout
            .extend(
                alloc::Layout::from_size_align(mem::size_of_val(samples), mem::align_of::<T>())
                    .unwrap(),
            )
            .unwrap();
        let layout = layout.pad_to_align();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<f64>().write(rate.into());
            let payload = mem.add(payload_offset).cast::<T>();
            for (i, &x) in samples.iter().enumerate() {
                payload.add(i).write(x);
            }
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, samples.len()) as *mut Self).into()
        }
    }

    /// Generate samples from an iterator
    pub fn from_iter<I>(rate: u32, iter: I) -> Arc<Self>
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let iter = iter.into_iter();
        let len = iter.len();
        let header_layout = alloc::Layout::new::<f64>();
        let (layout, payload_offset) = header_layout
            .extend(
                alloc::Layout::from_size_align(mem::size_of::<T>() * len, mem::align_of::<T>())
                    .unwrap(),
            )
            .unwrap();
        let layout = layout.pad_to_align();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<f64>().write(rate.into());
            let payload = mem.add(payload_offset).cast::<T>();
            let mut n = 0;
            for (i, x) in iter.enumerate() {
                payload.add(i).write(x);
                n += 1;
            }
            assert_eq!(n, len, "iterator returned incorrect length");
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, len) as *mut Self).into()
        }
    }

    /// Number of samples per second
    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    /// The runtime in seconds
    pub fn runtime(&self) -> f64 {
        self.samples.len() as f64 / self.rate
    }

    /// Interpolate a frame for position `s`
    ///
    /// Note that `s` is in samples, not seconds. Whole numbers are always an exact sample, and
    /// out-of-range positions yield 0.
    #[inline]
    pub fn interpolate(&self, s: f64) -> T
    where
        T: Frame + Copy,
    {
        let x0 = s as isize;
        let fract = (s - x0 as f64) as f32;
        let (a, b) = self.get_pair(x0);
        frame::lerp(&a, &b, fract)
    }

    #[inline]
    fn get_pair(&self, sample: isize) -> (T, T)
    where
        T: Frame + Copy,
    {
        if sample >= 0 {
            let sample = sample as usize;
            if sample < self.samples.len() - 1 {
                (self.samples[sample], self.samples[sample + 1])
            } else if sample < self.samples.len() {
                (self.samples[sample], T::ZERO)
            } else {
                (T::ZERO, T::ZERO)
            }
        } else if sample < -1 {
            (T::ZERO, T::ZERO)
        } else {
            (T::ZERO, self.samples[0])
        }
    }
}

impl<T> Deref for Frames<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.samples
    }
}

impl<T> DerefMut for Frames<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.samples
    }
}

/// An audio signal backed by a static sequence of samples
#[derive(Debug)]
pub struct FramesSignal<T> {
    /// Frames to play
    data: Arc<Frames<T>>,
    /// Playback position in seconds
    t: f64,
    /// Approximation of t in samples, for reading from the control. We could store t's bits in an
    /// AtomicU64 here, but that would sacrifice portability to platforms that don't have it,
    /// e.g. mips32.
    sample_t: Arc<AtomicIsize>,
}

impl<T> FramesSignal<T> {
    /// Create an audio signal from some samples
    ///
    /// `start_seconds` adjusts the initial playback position, and may be negative.
    pub fn new(data: Arc<Frames<T>>, start_seconds: f64) -> (FramesSignalControl, Self) {
        let samples = data.len();
        let signal = Self {
            t: start_seconds,
            sample_t: Arc::new(AtomicIsize::new((start_seconds * data.rate) as isize)),
            data,
        };
        let control = FramesSignalControl {
            samples,
            sample_position: signal.sample_t.clone(),
            rate: signal.data.rate,
        };
        (control, signal)
    }
}

impl<T: Frame + Copy> Signal for FramesSignal<T> {
    type Frame = T;

    #[inline]
    fn sample(&mut self, interval: f32, out: &mut [T]) {
        let s0 = self.t * self.data.rate;
        let ds = interval * self.data.rate as f32;
        let base = s0 as isize;
        if (ds - 1.0).abs() <= f32::EPSILON {
            // This fast-path is important for Spatial::play_buffered where we sample the signal
            // into the Ring with the interval = 1 / rate.
            let fract = (s0 - base as f64) as f32;
            for (i, o) in out.iter_mut().enumerate() {
                let (a, b) = self.data.get_pair(base + i as isize);
                *o = frame::lerp(&a, &b, fract);
            }
        } else {
            let mut offset = (s0 - base as f64) as f32;
            for o in out.iter_mut() {
                let trunc = unsafe { offset.to_int_unchecked::<isize>() };
                let (a, b) = self.data.get_pair(base + trunc);
                let fract = offset - trunc as f32;
                *o = frame::lerp(&a, &b, fract);
                offset += ds;
            }
        }
        self.t += f64::from(interval) * out.len() as f64;
        self.sample_t
            .store((self.t * self.data.rate) as isize, Ordering::Relaxed);
    }

    #[inline]
    fn is_finished(&self) -> bool {
        self.t >= (self.data.samples.len() - 1) as f64 / self.data.rate
    }
}

impl<T: Frame + Copy> Seek for FramesSignal<T> {
    #[inline]
    fn seek(&mut self, seconds: f32) {
        self.t += f64::from(seconds);
    }
}

impl<T> From<Arc<Frames<T>>> for FramesSignal<T> {
    fn from(samples: Arc<Frames<T>>) -> Self {
        Self::new(samples, 0.0).1
    }
}

/// Thread-safe control for a [`FramesSignal`], giving access to current playback location.
pub struct FramesSignalControl {
    samples: usize,
    sample_position: Arc<AtomicIsize>,
    rate: f64,
}

impl FramesSignalControl {
    /// Get the current playback position.
    ///
    /// This number may be negative if the starting time was negative,
    /// and it may be longer than the duration of the sample as well.
    ///
    /// Right now, we don't support a method to *set* the playback_position,
    /// as naively setting this variable causes audible distortions.
    #[inline]
    pub fn playback_position(&self) -> f64 {
        self.sample_position.load(Ordering::Relaxed) as f64 / self.rate
    }

    /// Whether the signal has finished playing
    #[inline]
    pub fn is_finished(&self) -> bool {
        usize::try_from(self.sample_position.load(Ordering::Relaxed))
            .map_or(false, |x| x >= self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::alloc::vec;

    fn assert_out(stream: &mut FramesSignal<f32>, interval: f32, expected: &[f32]) {
        let mut output = vec![0.0; expected.len()];
        stream.sample(interval, &mut output);
        assert_eq!(&output, expected);
    }

    #[test]
    fn from_slice() {
        const DATA: &[f32] = &[1.0, 2.0, 3.0];
        let frames = Frames::from_slice(1, DATA);
        assert_eq!(&frames[..], DATA);
    }

    #[test]
    fn sample() {
        let (_, mut signal) = FramesSignal::new(Frames::from_slice(1, &[1.0, 2.0, 3.0, 4.0]), -2.0);

        assert_out(&mut signal, 0.25, &[0.0, 0.0, 0.0, 0.0]);
        assert_out(&mut signal, 0.5, &[0.0, 0.5, 1.0]);
        assert_out(&mut signal, 1.0, &[1.5, 2.5, 3.5, 2.0, 0.0]);
    }

    #[test]
    fn playback_position() {
        let (control, mut signal) =
            FramesSignal::new(Frames::from_slice(1, &[1.0, 2.0, 3.0]), -2.0);

        // negatives are fine
        let init = control.playback_position();
        assert_eq!(init, -2.0);
        assert!(!control.is_finished());

        let mut buf = [0.0; 10];

        // get back to positive
        signal.sample(0.2, &mut buf);
        assert_eq!(0.0, control.playback_position());
        assert!(!control.is_finished());

        signal.sample(0.1, &mut buf);
        assert_eq!(1.0, control.playback_position());
        signal.sample(0.1, &mut buf);
        assert_eq!(2.0, control.playback_position());
        signal.sample(0.2, &mut buf);
        assert!(control.is_finished());
        assert_eq!(4.0, control.playback_position());
        signal.sample(0.5, &mut buf);
        assert_eq!(9.0, control.playback_position());
    }
}
