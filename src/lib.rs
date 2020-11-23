//! Lightweight 3D audio

#![warn(missing_docs)]

mod math;
mod samples;
mod source;
mod spatial;
mod spsc;
mod stream;
mod swap;
mod worker;

pub use samples::*;
pub use source::*;
pub use spatial::Spatial;
pub use stream::{stream, Receiver as StreamReceiver, Sender as StreamSender};
pub use swap::Swap;
pub use worker::*;

/// Unitless instantaneous sound wave amplitude measurement
pub type Sample = f32;

/// Convert a slice of interleaved stereo data into a slice of stereo frames
pub fn frame_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}
