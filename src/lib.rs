mod math;
mod mixer;
mod samples;
mod source;
mod spsc;
pub mod stream;
mod worker;

pub use samples::*;
pub use source::*;
pub use stream::stream;
pub use worker::*;

/// Type of samples making up a sound
pub type Sample = f32;

pub fn group_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}
