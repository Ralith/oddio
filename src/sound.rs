use std::{
    alloc, mem,
    ops::{Deref, DerefMut},
    ptr,
};

/// A sound that can be played back in a scene
#[derive(Debug)]
pub struct Sound {
    rate: u32,
    samples: [Sample],
}

impl Sound {
    pub fn from_slice(rate: u32, samples: &[Sample]) -> Box<Self> {
        let align = mem::align_of::<Sample>().max(4); // Also the size of the header with padding
        let layout =
            alloc::Layout::from_size_align(align + mem::size_of::<Sample>() * samples.len(), align)
                .unwrap();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<u32>().write(rate);
            let payload = mem.add(align).cast::<Sample>();
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
        let align = mem::align_of::<Sample>().max(4); // Also the size of the header with padding
        let layout =
            alloc::Layout::from_size_align(align + mem::size_of::<Sample>() * len, align).unwrap();
        unsafe {
            let mem = alloc::alloc(layout);
            mem.cast::<u32>().write(rate);
            let payload = mem.add(align).cast::<Sample>();
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
        let x0 = s.floor() as isize;
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

impl Deref for Sound {
    type Target = [Sample];
    fn deref(&self) -> &[Sample] {
        &self.samples
    }
}

impl DerefMut for Sound {
    fn deref_mut(&mut self) -> &mut [Sample] {
        &mut self.samples
    }
}

/// Type of samples making up a sound
pub type Sample = f32;
