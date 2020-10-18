mod mixer;
mod samples;
mod source;
mod spsc;
pub mod stream;

pub use mixer::*;
pub use samples::*;
pub use source::*;
pub use stream::stream;

/// Type of samples making up a sound
pub type Sample = f32;

pub fn aggregate_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}
