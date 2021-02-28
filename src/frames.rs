use std::{
    alloc, mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use crate::{frame, Controlled, Frame, Signal};

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
                alloc::Layout::from_size_align(
                    mem::size_of::<T>() * samples.len(),
                    mem::align_of::<T>(),
                )
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

    /// Interpolate a frame for position `s`
    ///
    /// Note that `s` is in samples, not seconds. Whole numbers are always an exact sample, and
    /// out-of-range positions yield 0.
    pub fn interpolate(&self, s: f64) -> T
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

    fn get(&self, sample: isize) -> T
    where
        T: Frame + Copy,
    {
        if sample < 0 {
            return T::ZERO;
        }
        let sample = sample as usize;
        if sample >= self.samples.len() {
            return T::ZERO;
        }
        self.samples[sample]
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
#[derive(Debug, Clone)]
pub struct FramesSignal<T> {
    /// Frames to play
    data: Arc<Frames<T>>,
    /// Playback position in seconds
    t: AtomicF64,
}

impl<T> FramesSignal<T> {
    /// Create an audio signal from some samples
    ///
    /// `start_seconds` adjusts the initial playback position, and may be negative.
    pub fn new(data: Arc<Frames<T>>, start_seconds: f64) -> Self {
        Self {
            t: AtomicF64::new(start_seconds),
            data,
        }
    }
}

impl<T: Frame + Copy> Signal for FramesSignal<T> {
    type Frame = T;

    #[inline]
    fn sample(&self, interval: f32, out: &mut [T]) {
        let s0 = self.t.get() * self.data.rate;
        let ds = f64::from(interval) * self.data.rate;
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.data.interpolate(s0 + ds * i as f64);
        }
        self.t
            .set(self.t.get() + f64::from(interval) * out.len() as f64);
    }

    #[inline]
    fn remaining(&self) -> f32 {
        (self.data.samples.len() as f64 / self.data.rate - self.t.get()) as f32
    }
}

impl<T> From<Arc<Frames<T>>> for FramesSignal<T> {
    fn from(samples: Arc<Frames<T>>) -> Self {
        Self::new(samples, 0.0)
    }
}

/// Thread-safe control for a [`FramesSignal`], giving access to current playback location.
pub struct FramesSignalControl<'a, T>(&'a FramesSignal<T>);

unsafe impl<'a, T: 'a> Controlled<'a> for FramesSignal<T> {
    type Control = FramesSignalControl<'a, T>;

    unsafe fn make_control(signal: &'a FramesSignal<T>) -> Self::Control {
        FramesSignalControl(signal)
    }
}

impl<'a, T> FramesSignalControl<'a, T> {
    /// Get the current playback position.
    ///
    /// This number may be negative if the starting time was negative,
    /// and it may be longer than the duration of the sample as well.
    pub fn playback_position(&self) -> f64 {
        self.0.t.get()
    }

    /// Sets the current playback position in seconds.
    ///
    /// This number may be negative.
    pub fn set_playback_position(&mut self, value: f64) {
        self.0.t.set(value);
    }
}

/// An f64 encoded in a u64.
#[repr(transparent)]
#[derive(Debug, Default)]
struct AtomicF64(AtomicU64);

impl AtomicF64 {
    /// Creates  a new AtomicF64
    pub fn new(input: f64) -> Self {
        AtomicF64(AtomicU64::new(input.to_bits()))
    }

    /// Loads the f64
    pub fn get(&self) -> f64 {
        let inner = self.0.load(Ordering::Relaxed);
        f64::from_bits(inner)
    }

    /// Stores an f64
    pub fn set(&self, value: f64) {
        let inner = value.to_bits();
        self.0.store(inner, Ordering::Relaxed);
    }
}

impl Clone for AtomicF64 {
    fn clone(&self) -> Self {
        let value = self.0.load(Ordering::Relaxed);
        Self(AtomicU64::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_slice() {
        const DATA: &[f32] = &[1.0, 2.0, 3.0];
        let frames = Frames::from_slice(1, DATA);
        assert_eq!(&frames[..], DATA);
    }

    #[test]
    fn playback_position() {
        let signal = FramesSignal::new(Frames::from_slice(1, &[1.0, 2.0, 3.0]), -2.5);

        // negatives are fine
        let init = FramesSignalControl(&signal).playback_position();
        assert_eq!(init, -2.5);

        let mut buf = [0.0; 10];

        // get back to positive
        signal.sample(0.25, &mut buf);
        assert_eq!(0.0, FramesSignalControl(&signal).playback_position());

        // sip the sample
        signal.sample(0.25, &mut buf);
        assert_eq!(2.5, FramesSignalControl(&signal).playback_position());
        signal.sample(0.25, &mut buf);
        assert_eq!(5.0, FramesSignalControl(&signal).playback_position());
        signal.sample(0.5, &mut buf);
        assert_eq!(10.0, FramesSignalControl(&signal).playback_position());

        // we can go over no problem too...
        signal.sample(0.5, &mut buf);
        assert_eq!(15.0, FramesSignalControl(&signal).playback_position());
    }
}
