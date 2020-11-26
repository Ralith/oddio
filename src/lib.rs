//! Lightweight game audio
//!
//! ```no_run
//! let (mut remote, mut worker) = oddio::worker();
//!
//! // In audio callback:
//! # let data = &mut [][..];
//! # let output_sample_rate = 44100;
//! let samples = oddio::frame_stereo(data);
//! for s in &mut samples[..] {
//!    *s = [0.0, 0.0];
//! }
//! worker.render(output_sample_rate, samples);
//!
//! // In game logic:
//! # let samples = [];
//! # let sample_rate = 44100;
//! # let position = [0.0, 0.0, 0.0].into();
//! # let velocity = [0.0, 0.0, 0.0].into();
//! let samples = oddio::SamplesSource::from(oddio::Samples::from_slice(sample_rate, &samples));
//! let mut handle = remote.play(oddio::Spatial::new(samples, position, velocity));
//!
//! // When position/velocity changes:
//! handle.set_motion(position, velocity);
//! ```

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
pub use spatial::{Spatial, SpatialSampler};
pub use stream::{stream, Receiver as StreamReceiver, Sender as StreamSender};
pub use swap::Swap;
pub use worker::*;

/// Unitless instantaneous sound wave amplitude measurement
pub type Sample = f32;

/// Convert a slice of interleaved stereo data into a slice of stereo frames
pub fn frame_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}
