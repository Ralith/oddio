//! Lightweight game audio
//!
//! ```no_run
//! let (mut scene_handle, scene) = oddio::spatial();
//!
//! // In audio callback:
//! # let data = &mut [][..];
//! # let output_sample_rate = 44100;
//! let out_frames = oddio::frame_stereo(data);
//! oddio::run(&scene, output_sample_rate, out_frames);
//!
//! // In game logic:
//! # let frames = [];
//! # let sample_rate = 44100;
//! # let position = [0.0, 0.0, 0.0].into();
//! # let velocity = [0.0, 0.0, 0.0].into();
//! let frames = oddio::FramesSignal::from(oddio::Frames::from_slice(sample_rate, &frames));
//! let mut handle = scene_handle.play(frames, position, velocity);
//!
//! // When position/velocity changes:
//! handle.control::<oddio::Spatial<_>, _>().set_motion(position, velocity);
//! ```
//!
//! Key primitives:
//! - [`Frames`] stores static audio data, which can be played with a [`FramesSignal`]
//! - [`Mixer`] allows multiple signals to be played concurrently and controlled during playback
//! - [`SpatialScene`] is a mixer that spatializes its signals
//! - [`Handle`] allows manipulation of a signal while it's played on a [`SpatialScene`] or [`Mixer`]
//! - [`run`] writes frames from a [`Signal`] into an output buffer

#![warn(missing_docs)]

mod filter;
mod frame;
mod frames;
mod gain;
mod handle;
mod math;
mod mixer;
mod reinhard;
mod set;
mod signal;
mod sine;
mod spatial;
mod speed;
mod spsc;
mod stream;
mod swap;

pub use filter::*;
pub use frame::Frame;
pub use frames::*;
pub use gain::Gain;
pub use handle::*;
pub use mixer::*;
pub use reinhard::Reinhard;
use set::*;
pub use signal::*;
pub use sine::*;
pub use spatial::*;
pub use speed::Speed;
pub use stream::{stream, Receiver as StreamReceiver, Sender as StreamSender};
pub use swap::Swap;

/// Unitless instantaneous sound wave amplitude measurement
pub type Sample = f32;

/// Populate `out` with frames from `signal` at `sample_rate`
///
/// Convenience wrapper around the [`Signal`] interface.
pub fn run<S: Signal>(signal: &S, sample_rate: u32, out: &mut [S::Frame]) {
    let sample_len = 1.0 / sample_rate as f32;
    signal.sample(0.0, sample_len, out);
    signal.advance(sample_len * out.len() as f32);
}

/// Convert a slice of interleaved stereo data into a slice of stereo frames
pub fn frame_stereo(xs: &mut [Sample]) -> &mut [[Sample; 2]] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() / 2) }
}

fn flatten_stereo(xs: &mut [[Sample; 2]]) -> &mut [Sample] {
    unsafe { std::slice::from_raw_parts_mut(xs.as_mut_ptr() as _, xs.len() * 2) }
}
