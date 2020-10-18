use std::{
    alloc, mem,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::{Sample, Source};

/// A finite sound
#[derive(Debug)]
pub struct Samples {
    rate: u32,
    samples: [Sample],
}

impl Samples {
    pub fn from_slice(rate: u32, samples: &[Sample]) -> Box<Self> {
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
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, samples.len()) as *mut Self)
        }
    }

    pub fn from_iter<T>(rate: u32, iter: T) -> Box<Self>
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
            Box::from_raw(ptr::slice_from_raw_parts_mut(mem, len) as *mut Self)
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn sample(&self, s: f64) -> f32 {
        let x0 = s.trunc() as isize;
        let fract = s.fract() as f32;
        let x1 = x0 + 1;
        self.get(x0) * (1.0 - fract) + self.get(x1) * fract
    }

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

pub struct SamplesSource<'a> {
    /// Samples to play
    pub data: &'a Samples,
    /// Position to begin playback at, in samples
    pub t: f64,
}

impl Source for SamplesSource<'_> {
    fn rate(&self) -> u32 {
        self.data.rate
    }

    fn sample(&self, t: f32) -> f32 {
        let s = self.t + f64::from(t);
        self.data.sample(s)
    }
}
