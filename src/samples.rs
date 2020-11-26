use std::{
    alloc,
    cell::Cell,
    mem,
    ops::{Deref, DerefMut},
    ptr,
    sync::Arc,
};

use crate::{Sample, Sampler, Source};

/// A sequence of audio samples at a particular rate
///
/// Dynamically sized type. Typically stored inside an `Arc`, allowing efficient simultaneous reuse
/// by multiple sources.
#[derive(Debug)]
pub struct Samples {
    rate: f64,
    samples: [Sample],
}

impl Samples {
    /// Construct samples from existing memory
    pub fn from_slice(rate: u32, samples: &[Sample]) -> Arc<Self> {
        let header_layout = alloc::Layout::new::<f64>();
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
            mem.cast::<f64>().write(rate.into());
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
        let header_layout = alloc::Layout::new::<f64>();
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
            mem.cast::<f64>().write(rate.into());
            let payload = mem.add(payload_offset).cast::<Sample>();
            for (i, x) in iter.enumerate() {
                payload.add(i).write(x);
            }
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, len) as *mut Self).into()
        }
    }

    /// Number of samples per second
    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    #[inline]
    fn sample(&self, s: f64) -> f32 {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        let a = self.get(x0);
        let b = self.get(x1);
        a + fract * (b - a)
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
    type Sampler = SamplesSampler;

    #[inline]
    fn sample(&self, dt: f32) -> SamplesSampler {
        SamplesSampler {
            s0: self.t.get() * self.data.rate,
            ds: f64::from(dt) * self.data.rate,
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

/// Sampler for [`SamplesSource`]
pub struct SamplesSampler {
    s0: f64,
    ds: f64,
}

impl Sampler<SamplesSource> for SamplesSampler {
    type Frame = Sample;
    #[inline]
    fn get(&self, source: &SamplesSource, t: f32) -> Sample {
        source.data.sample(self.s0 + f64::from(t) * self.ds)
    }
}

impl From<Arc<Samples>> for SamplesSource {
    fn from(samples: Arc<Samples>) -> Self {
        Self::new(samples, 0.0)
    }
}
