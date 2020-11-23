use std::{
    alloc,
    cell::Cell,
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::Arc,
};

use crate::{Action, Sample, Seek, Source};

/// A sequence of audio samples at a particular rate
#[derive(Debug)]
pub struct Samples {
    rate: u32,
    samples: [Sample],
}

impl Samples {
    /// Construct samples from existing memory
    pub fn from_slice(rate: u32, samples: &[Sample]) -> Arc<Self> {
        let header_layout = alloc::Layout::new::<u32>();
        let (layout, payload_offset) = header_layout
            .extend(
                alloc::Layout::from_size_align(
                    mem::size_of::<Sample>() * samples.len(),
                    mem::align_of::<Sample>(),
                )
                .unwrap(),
            )
            .unwrap();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<u32>().write(rate);
            let payload = mem.add(payload_offset).cast::<Sample>();
            for (i, &x) in samples.iter().enumerate() {
                payload.add(i).write(x);
            }
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, samples.len()) as *mut Self).into()
        }
    }

    /// Generate samples from an iterator
    pub fn from_iter<T>(rate: u32, iter: T) -> Arc<Self>
    where
        T: IntoIterator<Item = Sample>,
        T::IntoIter: ExactSizeIterator,
    {
        let iter = iter.into_iter();
        let len = iter.len();
        let header_layout = alloc::Layout::new::<u32>();
        let (layout, payload_offset) = header_layout
            .extend(
                alloc::Layout::from_size_align(
                    mem::size_of::<Sample>() * len,
                    mem::align_of::<Sample>(),
                )
                .unwrap(),
            )
            .unwrap();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<u32>().write(rate);
            let payload = mem.add(payload_offset).cast::<Sample>();
            for (i, x) in iter.enumerate() {
                payload.add(i).write(x);
            }
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, len) as *mut Self).into()
        }
    }

    /// Number of samples per second
    pub fn rate(&self) -> u32 {
        self.rate
    }

    #[inline]
    fn sample(&self, s: f64) -> f32 {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        self.get(x0) * (1.0 - fract) + self.get(x1) * fract
    }

    #[inline]
    fn get(&self, sample: isize) -> f32 {
        if sample < 0 {
            return 0.0;
        }
        let sample = sample as usize;
        if sample >= self.samples.len() {
            return 0.0;
        }
        self.samples[sample]
    }
}

impl Deref for Samples {
    type Target = [Sample];
    fn deref(&self) -> &[Sample] {
        &self.samples
    }
}

impl DerefMut for Samples {
    fn deref_mut(&mut self) -> &mut [Sample] {
        &mut self.samples
    }
}

/// An audio source backed by a static sequence of samples
#[derive(Debug, Clone)]
pub struct SamplesSource {
    /// Samples to play
    data: Arc<Samples>,
    /// Position of t=0 in seconds
    t: Cell<f64>,
}

impl SamplesSource {
    /// Create an audio source from some samples
    ///
    /// `start_seconds` adjusts the initial playback position, and may be negative.
    pub fn new(data: Arc<Samples>, start_seconds: f64) -> Self {
        Self {
            t: Cell::new(start_seconds),
            data,
        }
    }
}

impl Source for SamplesSource {
    type Frame = Sample;

    #[inline]
    fn update(&self) -> Action {
        Action::Retain
    }

    #[inline]
    fn sample(&self, sample_duration: f32, count: usize, out: impl FnMut(usize, Self::Frame)) {
        self.sample_at(sample_duration, count, 0.0, out);
        self.advance(sample_duration * count as f32);
    }
}

impl Seek for SamplesSource {
    #[inline]
    fn sample_at(
        &self,
        sample_duration: f32,
        count: usize,
        delay: f32,
        mut out: impl FnMut(usize, Self::Frame),
    ) {
        let s0 = (self.t.get() - f64::from(delay)) * self.data.rate as f64;
        let ds = f64::from(sample_duration) * self.data.rate as f64;
        for i in 0..count {
            out(i, self.data.sample(s0 + (i as f64) * ds));
        }
    }

    #[inline]
    fn advance(&self, dt: f32) {
        self.t.set(self.t.get() + f64::from(dt));
    }
}

impl From<Arc<Samples>> for SamplesSource {
    fn from(samples: Arc<Samples>) -> Self {
        Self::new(samples, 0.0)
    }
}
