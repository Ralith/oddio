use std::{
    alloc,
    cell::Cell,
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::Arc,
};

use crate::{Frame, Source};

/// A sequence of static audio frames at a particular sample rate
///
/// Used to store e.g. sound effects decoded from files on disk.
///
/// Dynamically sized type. Typically stored inside an `Arc`, allowing efficient simultaneous use by
/// multiple sources.
#[derive(Debug)]
pub struct Frames<T> {
    rate: f64,
    samples: [T],
}

impl<T: Frame + Copy> Frames<T> {
    /// Construct samples from existing memory
    pub fn from_slice(rate: u32, samples: &[T]) -> Arc<Self> {
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
    #[inline]
    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    #[inline]
    fn sample(&self, s: f64) -> T {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        a.lerp(&b, fract)
    }

    #[inline]
    fn get(&self, sample: isize) -> T {
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

/// An audio source backed by a static sequence of samples
#[derive(Debug, Clone)]
pub struct FramesSource<T> {
    /// Frames to play
    data: Arc<Frames<T>>,
    /// Position of t=0 in seconds
    t: Cell<f64>,
}

impl<T> FramesSource<T> {
    /// Create an audio source from some samples
    ///
    /// `start_seconds` adjusts the initial playback position, and may be negative.
    pub fn new(data: Arc<Frames<T>>, start_seconds: f64) -> Self {
        Self {
            t: Cell::new(start_seconds),
            data,
        }
    }
}

impl<T: Frame + Copy> Source for FramesSource<T> {
    type Frame = T;

    #[inline]
    fn sample(&self, offset: f32, dt: f32, out: &mut [T]) {
        let s0 = (self.t.get() + f64::from(offset)) * self.data.rate;
        let ds = f64::from(dt) * self.data.rate;
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.data.sample(s0 + ds * i as f64);
        }
    }

    #[inline]
    fn advance(&self, dt: f32) {
        self.t.set(self.t.get() + f64::from(dt));
    }

    #[inline]
    fn remaining(&self) -> f32 {
        (self.data.samples.len() as f64 - self.t.get() * self.data.rate) as f32
    }
}

impl<T> From<Arc<Frames<T>>> for FramesSource<T> {
    fn from(samples: Arc<Frames<T>>) -> Self {
        Self::new(samples, 0.0)
    }
}
